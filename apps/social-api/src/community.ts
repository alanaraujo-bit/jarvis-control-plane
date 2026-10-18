/** M28: the community — profiles, follows, posts, prompts, likes, saves,
 *  replies, notifications, blocks, published stats and presence. Rules live in
 *  community-rules.ts; this module stores, fetches and enforces them.
 *  docs/M28-COMMUNITY.md is the design. */
import type { IncomingMessage, ServerResponse } from "node:http";
import { randomUUID } from "node:crypto";
import type { Pool } from "pg";
import { canSee, HANDLE, parsePost, parseProfile, parseStats, type Relation, type Visibility } from "./community-rules.ts";
import { PAGE_CSP, renderNotFound, renderPostPage, renderProfilePage, type PagePost } from "./community-pages.ts";

export interface Ctx {
  pool: Pool;
  json: (res: ServerResponse, status: number, body: unknown) => void;
  body: (req: IncomingMessage, limit?: number) => Promise<Record<string, unknown>>;
  identityMe: (req: IncomingMessage) => Promise<Record<string, unknown> | null | undefined>;
}

const PAGE = 30;
const WORKING_MS = 120_000;
const POSTS_PER_HOUR = 20;
const FOLLOWS_PER_DAY = 200;

export async function migrateCommunity(pool: Pool) {
  await pool.query(`
    CREATE TABLE IF NOT EXISTS community_profiles (
      user_id UUID PRIMARY KEY REFERENCES identity_users(id) ON DELETE CASCADE,
      handle TEXT UNIQUE NOT NULL CHECK (handle ~ '^[a-z0-9_]{3,24}$'),
      display_name TEXT NOT NULL, bio TEXT NOT NULL DEFAULT '', location TEXT NOT NULL DEFAULT '',
      website TEXT NOT NULL DEFAULT '', github TEXT NOT NULL DEFAULT '', stack TEXT[] NOT NULL DEFAULT '{}',
      building TEXT NOT NULL DEFAULT '',
      stats_visibility TEXT NOT NULL DEFAULT 'private' CHECK (stats_visibility IN ('public','followers','private')),
      presence_visibility TEXT NOT NULL DEFAULT 'private' CHECK (presence_visibility IN ('public','followers','private')),
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), updated_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS community_follows (
      follower_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      followee_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (follower_id, followee_id),
      CHECK (follower_id <> followee_id));
    CREATE INDEX IF NOT EXISTS community_follows_followee ON community_follows(followee_id);
    CREATE TABLE IF NOT EXISTS community_blocks (
      blocker_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      blocked_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (blocker_id, blocked_id));
    CREATE TABLE IF NOT EXISTS community_posts (
      id UUID PRIMARY KEY, author_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      kind TEXT NOT NULL CHECK (kind IN ('post','prompt')), body TEXT NOT NULL DEFAULT '',
      title TEXT NOT NULL DEFAULT '', prompt TEXT NOT NULL DEFAULT '', tags TEXT[] NOT NULL DEFAULT '{}',
      reply_to UUID REFERENCES community_posts(id) ON DELETE SET NULL,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), deleted_at TIMESTAMPTZ);
    CREATE INDEX IF NOT EXISTS community_posts_recent ON community_posts(created_at DESC) WHERE deleted_at IS NULL;
    CREATE INDEX IF NOT EXISTS community_posts_author ON community_posts(author_id, created_at DESC);
    CREATE INDEX IF NOT EXISTS community_posts_reply ON community_posts(reply_to);
    CREATE INDEX IF NOT EXISTS community_posts_tags ON community_posts USING GIN (tags);
    CREATE TABLE IF NOT EXISTS community_likes (
      post_id UUID NOT NULL REFERENCES community_posts(id) ON DELETE CASCADE,
      user_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (post_id, user_id));
    CREATE TABLE IF NOT EXISTS community_saves (
      post_id UUID NOT NULL REFERENCES community_posts(id) ON DELETE CASCADE,
      user_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), PRIMARY KEY (post_id, user_id));
    CREATE TABLE IF NOT EXISTS community_notifications (
      id BIGSERIAL PRIMARY KEY, user_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      kind TEXT NOT NULL CHECK (kind IN ('follow','like','reply','save')),
      actor_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      post_id UUID REFERENCES community_posts(id) ON DELETE CASCADE,
      created_at TIMESTAMPTZ NOT NULL DEFAULT now(), read_at TIMESTAMPTZ);
    CREATE INDEX IF NOT EXISTS community_notifications_user ON community_notifications(user_id, created_at DESC);
    CREATE TABLE IF NOT EXISTS community_stats (
      user_id UUID PRIMARY KEY REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      snapshot JSONB NOT NULL, observed_at TIMESTAMPTZ NOT NULL DEFAULT now());
    CREATE TABLE IF NOT EXISTS community_presence (
      user_id UUID PRIMARY KEY REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      active_sessions INTEGER NOT NULL CHECK (active_sessions BETWEEN 0 AND 100), last_seen_at TIMESTAMPTZ NOT NULL);
    CREATE TABLE IF NOT EXISTS community_reports (
      id BIGSERIAL PRIMARY KEY, reporter_id UUID NOT NULL REFERENCES community_profiles(user_id) ON DELETE CASCADE,
      post_id UUID NOT NULL REFERENCES community_posts(id) ON DELETE CASCADE,
      reason TEXT NOT NULL DEFAULT '', created_at TIMESTAMPTZ NOT NULL DEFAULT now());`);
}

type Row = Record<string, any>;

const mini = (row: Row, prefix = "") => ({
  handle: row[`${prefix}handle`],
  displayName: row[`${prefix}display_name`],
  building: row[`${prefix}building`] ?? "",
});

function profileJson(row: Row) {
  return {
    handle: row.handle, displayName: row.display_name, bio: row.bio, location: row.location,
    website: row.website, github: row.github, stack: row.stack ?? [], building: row.building,
    statsVisibility: row.stats_visibility, presenceVisibility: row.presence_visibility,
    createdAt: new Date(row.created_at).toISOString(),
  };
}

function postJson(row: Row) {
  return {
    id: row.id, kind: row.kind, body: row.body, title: row.title, prompt: row.prompt, tags: row.tags ?? [],
    replyTo: row.reply_to, createdAt: new Date(row.created_at).toISOString(),
    likeCount: Number(row.like_count ?? 0), replyCount: Number(row.reply_count ?? 0), saveCount: Number(row.save_count ?? 0),
    likedByMe: row.liked === true, savedByMe: row.saved === true,
    author: mini(row, "a_"),
    parent: row.p_handle ? { handle: row.p_handle } : null,
  };
}

/** Posts with their author and counts, hiding anything across a block. `$1`
 *  is always the viewer's id (or a nil uuid for nobody). */
const POSTS_SELECT = `
  SELECT p.*, a.handle AS a_handle, a.display_name AS a_display_name, a.building AS a_building, pa.handle AS p_handle,
    (SELECT count(*) FROM community_likes l WHERE l.post_id = p.id) AS like_count,
    (SELECT count(*) FROM community_posts r WHERE r.reply_to = p.id AND r.deleted_at IS NULL) AS reply_count,
    (SELECT count(*) FROM community_saves s WHERE s.post_id = p.id) AS save_count,
    EXISTS (SELECT 1 FROM community_likes l WHERE l.post_id = p.id AND l.user_id = $1) AS liked,
    EXISTS (SELECT 1 FROM community_saves s WHERE s.post_id = p.id AND s.user_id = $1) AS saved
  FROM community_posts p
  JOIN community_profiles a ON a.user_id = p.author_id
  LEFT JOIN community_posts pp ON pp.id = p.reply_to
  LEFT JOIN community_profiles pa ON pa.user_id = pp.author_id
  WHERE p.deleted_at IS NULL
    AND NOT EXISTS (SELECT 1 FROM community_blocks b
      WHERE (b.blocker_id = $1 AND b.blocked_id = p.author_id) OR (b.blocker_id = p.author_id AND b.blocked_id = $1))`;

const NOBODY = "00000000-0000-0000-0000-000000000000";

function cursor(url: URL): string | null {
  const before = url.searchParams.get("before");
  return before && !Number.isNaN(Date.parse(before)) ? new Date(before).toISOString() : null;
}

async function counts(pool: Pool, userId: string) {
  const row = (await pool.query(
    `SELECT (SELECT count(*) FROM community_follows WHERE followee_id = $1) AS followers,
            (SELECT count(*) FROM community_follows WHERE follower_id = $1) AS following,
            (SELECT count(*) FROM community_posts WHERE author_id = $1 AND deleted_at IS NULL AND reply_to IS NULL) AS posts`,
    [userId])).rows[0];
  return { followers: Number(row.followers), following: Number(row.following), posts: Number(row.posts) };
}

async function relation(pool: Pool, viewer: string | null, target: string) {
  if (!viewer) return { self: false, following: false, followsYou: false, blocked: false };
  if (viewer === target) return { self: true, following: false, followsYou: false, blocked: false };
  const row = (await pool.query(
    `SELECT EXISTS (SELECT 1 FROM community_follows WHERE follower_id = $1 AND followee_id = $2) AS following,
            EXISTS (SELECT 1 FROM community_follows WHERE follower_id = $2 AND followee_id = $1) AS follows_you,
            EXISTS (SELECT 1 FROM community_blocks WHERE blocker_id = $1 AND blocked_id = $2) AS blocked_by_me,
            EXISTS (SELECT 1 FROM community_blocks WHERE blocker_id = $2 AND blocked_id = $1) AS blocked_me`,
    [viewer, target])).rows[0];
  return { self: false, following: row.following, followsYou: row.follows_you, blocked: row.blocked_by_me, blockedMe: row.blocked_me };
}

/** Everything a viewer may see of one profile. `null` when it does not exist
 *  or a block stands between them. */
async function profileView(pool: Pool, viewer: string | null, handle: string) {
  const row = (await pool.query("SELECT * FROM community_profiles WHERE handle = $1", [handle.toLowerCase()])).rows[0];
  if (!row) return null;
  const rel = await relation(pool, viewer, row.user_id);
  if ((rel as { blockedMe?: boolean }).blockedMe) return null;
  const sees: Relation = { self: rel.self, following: rel.following };
  const statsRow = canSee(row.stats_visibility as Visibility, sees)
    ? (await pool.query("SELECT snapshot, observed_at FROM community_stats WHERE user_id = $1", [row.user_id])).rows[0]
    : undefined;
  const presenceRow = canSee(row.presence_visibility as Visibility, sees)
    ? (await pool.query("SELECT active_sessions, last_seen_at FROM community_presence WHERE user_id = $1", [row.user_id])).rows[0]
    : undefined;
  const fresh = presenceRow && Date.now() - new Date(presenceRow.last_seen_at).getTime() < WORKING_MS;
  return {
    userId: row.user_id as string,
    profile: profileJson(row),
    counts: await counts(pool, row.user_id),
    relation: { self: rel.self, following: rel.following, followsYou: rel.followsYou, blocked: rel.blocked },
    stats: statsRow ? { ...statsRow.snapshot, observedAt: new Date(statsRow.observed_at).toISOString() } : null,
    presence: presenceRow
      ? { working: Boolean(fresh && presenceRow.active_sessions > 0), activeSessions: fresh ? presenceRow.active_sessions : 0, lastSeenAt: new Date(presenceRow.last_seen_at).toISOString() }
      : null,
  };
}

async function notify(pool: Pool, userId: string, kind: string, actorId: string, postId: string | null) {
  if (userId === actorId) return;
  await pool.query("INSERT INTO community_notifications(user_id, kind, actor_id, post_id) VALUES ($1,$2,$3,$4)", [userId, kind, actorId, postId]);
}

async function blockedBetween(pool: Pool, a: string, b: string) {
  return (await pool.query(
    "SELECT 1 FROM community_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1)", [a, b])).rowCount! > 0;
}

/** Handle a community route. Returns false when the path is not one. */
export async function handleCommunity(req: IncomingMessage, res: ServerResponse, url: URL, ctx: Ctx): Promise<boolean> {
  const { pool, json } = ctx;
  const path = url.pathname;

  // ---- public web pages -------------------------------------------------
  if (req.method === "GET" && (path.startsWith("/u/") || path.startsWith("/p/"))) {
    const page = (status: number, html: string) => {
      res.writeHead(status, { "content-type": "text/html; charset=utf-8", "cache-control": "public, max-age=60", "content-security-policy": PAGE_CSP, "x-content-type-options": "nosniff", "referrer-policy": "no-referrer" });
      res.end(html);
    };
    if (path.startsWith("/u/")) {
      const handle = decodeURIComponent(path.slice(3)).toLowerCase();
      const view = HANDLE.test(handle) ? await profileView(pool, null, handle) : null;
      if (!view) { page(404, renderNotFound()); return true; }
      const posts = (await pool.query(`${POSTS_SELECT} AND p.author_id = $2 AND p.reply_to IS NULL ORDER BY p.created_at DESC LIMIT 20`, [NOBODY, view.userId])).rows.map(postJson);
      page(200, renderProfilePage({ profile: view.profile, counts: view.counts, stats: view.stats, working: view.presence?.working ?? null, posts: posts as PagePost[] }));
      return true;
    }
    const id = path.slice(3);
    const row = /^[0-9a-f-]{36}$/i.test(id) ? (await pool.query(`${POSTS_SELECT} AND p.id = $2`, [NOBODY, id])).rows[0] : undefined;
    if (!row) { page(404, renderNotFound()); return true; }
    const replies = (await pool.query(`${POSTS_SELECT} AND p.reply_to = $2 ORDER BY p.created_at ASC LIMIT 100`, [NOBODY, id])).rows.map(postJson);
    page(200, renderPostPage(postJson(row) as PagePost, replies as PagePost[]));
    return true;
  }

  if (!path.startsWith("/v1/social/")) return false;
  const route = path.slice("/v1/social/".length).split("/").map(decodeURIComponent);
  const user = await ctx.identityMe(req);
  const userId = user ? String(user.id) : null;
  const mine = userId ? (await pool.query("SELECT * FROM community_profiles WHERE user_id = $1", [userId])).rows[0] : undefined;
  const me = mine ? userId : null;
  const need = () => {
    if (!userId) { json(res, 401, { error: "identity.unauthorised" }); return false; }
    if (!me) { json(res, 403, { error: "social.noProfile" }); return false; }
    return true;
  };
  const viewer = me ?? NOBODY;
  const [head, arg, action] = route;

  if (req.method === "GET" && head === "me") {
    if (!userId) { json(res, 401, { error: "identity.unauthorised" }); return true; }
    if (!mine) { json(res, 200, { profile: null, counts: null, unread: 0, suggestedName: user?.display_name ?? "" }); return true; }
    const unread = Number((await pool.query("SELECT count(*) FROM community_notifications WHERE user_id=$1 AND read_at IS NULL", [me])).rows[0].count);
    json(res, 200, { profile: profileJson(mine), counts: await counts(pool, me!), unread, suggestedName: mine.display_name });
    return true;
  }
  if (req.method === "GET" && head === "handles" && arg) {
    const handle = arg.toLowerCase();
    const owner = HANDLE.test(handle) ? (await pool.query("SELECT user_id FROM community_profiles WHERE handle=$1", [handle])).rows[0] : null;
    json(res, 200, { valid: HANDLE.test(handle), available: HANDLE.test(handle) && (!owner || owner.user_id === userId) });
    return true;
  }
  if (req.method === "PUT" && head === "profile") {
    if (!userId) { json(res, 401, { error: "identity.unauthorised" }); return true; }
    const parsed = parseProfile(await ctx.body(req), !mine);
    if ("error" in parsed) { json(res, 400, parsed); return true; }
    const p = parsed.profile;
    const handle = p.handle || mine.handle;
    try {
      await pool.query(
        `INSERT INTO community_profiles(user_id, handle, display_name, bio, location, website, github, stack, building, stats_visibility, presence_visibility)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
         ON CONFLICT (user_id) DO UPDATE SET handle=excluded.handle, display_name=excluded.display_name, bio=excluded.bio,
           location=excluded.location, website=excluded.website, github=excluded.github, stack=excluded.stack,
           building=excluded.building, stats_visibility=excluded.stats_visibility, presence_visibility=excluded.presence_visibility, updated_at=now()`,
        [userId, handle, p.displayName, p.bio, p.location, p.website, p.github, p.stack, p.building, p.statsVisibility, p.presenceVisibility]);
    } catch { json(res, 409, { error: "social.handleTaken" }); return true; }
    const row = (await pool.query("SELECT * FROM community_profiles WHERE user_id=$1", [userId])).rows[0];
    json(res, 200, { profile: profileJson(row), counts: await counts(pool, userId), unread: 0 });
    return true;
  }
  if (req.method === "DELETE" && head === "profile") {
    if (!need()) return true;
    await pool.query("DELETE FROM community_profiles WHERE user_id=$1", [me]);
    json(res, 200, { ok: true });
    return true;
  }

  if (head === "profiles" && arg) {
    const view = await profileView(pool, me, arg);
    if (!view) { json(res, 404, { error: "social.profileNotFound" }); return true; }
    if (req.method === "GET" && !action) {
      const { userId: _hidden, ...visible } = view;
      json(res, 200, visible);
      return true;
    }
    if (req.method === "GET" && action === "posts") {
      const tab = url.searchParams.get("tab") ?? "posts";
      const filter = tab === "replies" ? "AND p.reply_to IS NOT NULL" : tab === "prompts" ? "AND p.kind = 'prompt' AND p.reply_to IS NULL" : "AND p.reply_to IS NULL";
      const before = cursor(url);
      const rows = (await pool.query(`${POSTS_SELECT} AND p.author_id = $2 ${filter} ${before ? "AND p.created_at < $3" : ""} ORDER BY p.created_at DESC LIMIT ${PAGE}`,
        before ? [viewer, view.userId, before] : [viewer, view.userId])).rows;
      json(res, 200, { posts: rows.map(postJson) });
      return true;
    }
    if (req.method === "GET" && (action === "followers" || action === "following")) {
      const sql = action === "followers"
        ? "SELECT p.* FROM community_follows f JOIN community_profiles p ON p.user_id=f.follower_id WHERE f.followee_id=$1 ORDER BY f.created_at DESC LIMIT 200"
        : "SELECT p.* FROM community_follows f JOIN community_profiles p ON p.user_id=f.followee_id WHERE f.follower_id=$1 ORDER BY f.created_at DESC LIMIT 200";
      json(res, 200, { people: (await pool.query(sql, [view.userId])).rows.map((r) => mini(r)) });
      return true;
    }
  }

  if ((head === "follows" || head === "blocks") && arg && (req.method === "POST" || req.method === "DELETE")) {
    if (!need()) return true;
    const target = (await pool.query("SELECT user_id FROM community_profiles WHERE handle=$1", [arg.toLowerCase()])).rows[0];
    if (!target || target.user_id === me) { json(res, 404, { error: "social.profileNotFound" }); return true; }
    if (head === "follows") {
      if (req.method === "POST") {
        // Someone who blocked you does not exist for you, as on their profile.
        const blockedMe = (await pool.query("SELECT 1 FROM community_blocks WHERE blocker_id=$1 AND blocked_id=$2", [target.user_id, me])).rowCount;
        if (blockedMe) { json(res, 404, { error: "social.profileNotFound" }); return true; }
        if (await blockedBetween(pool, me!, target.user_id)) { json(res, 403, { error: "social.blocked" }); return true; }
        const recent = Number((await pool.query("SELECT count(*) FROM community_follows WHERE follower_id=$1 AND created_at > now() - interval '1 day'", [me])).rows[0].count);
        if (recent >= FOLLOWS_PER_DAY) { json(res, 429, { error: "social.slowDown" }); return true; }
        const added = await pool.query("INSERT INTO community_follows(follower_id, followee_id) VALUES ($1,$2) ON CONFLICT DO NOTHING", [me, target.user_id]);
        if (added.rowCount) await notify(pool, target.user_id, "follow", me!, null);
      } else {
        await pool.query("DELETE FROM community_follows WHERE follower_id=$1 AND followee_id=$2", [me, target.user_id]);
      }
    } else if (req.method === "POST") {
      await pool.query("INSERT INTO community_blocks(blocker_id, blocked_id) VALUES ($1,$2) ON CONFLICT DO NOTHING", [me, target.user_id]);
      await pool.query("DELETE FROM community_follows WHERE (follower_id=$1 AND followee_id=$2) OR (follower_id=$2 AND followee_id=$1)", [me, target.user_id]);
    } else {
      await pool.query("DELETE FROM community_blocks WHERE blocker_id=$1 AND blocked_id=$2", [me, target.user_id]);
    }
    json(res, 200, { relation: (await profileView(pool, me, arg))?.relation ?? null });
    return true;
  }

  if (req.method === "GET" && head === "feed") {
    const scope = url.searchParams.get("scope") === "following" ? "following" : "discover";
    if (scope === "following" && !need()) return true;
    const tag = url.searchParams.get("tag")?.toLowerCase().replace(/^#/, "") ?? "";
    const params: unknown[] = [viewer];
    let filter = "AND p.reply_to IS NULL";
    if (scope === "following") filter += " AND (p.author_id = $1 OR p.author_id IN (SELECT followee_id FROM community_follows WHERE follower_id = $1))";
    if (tag) { params.push(tag); filter += ` AND $${params.length} = ANY(p.tags)`; }
    const before = cursor(url);
    if (before) { params.push(before); filter += ` AND p.created_at < $${params.length}`; }
    const rows = (await pool.query(`${POSTS_SELECT} ${filter} ORDER BY p.created_at DESC LIMIT ${PAGE}`, params)).rows;
    json(res, 200, { posts: rows.map(postJson) });
    return true;
  }

  if (req.method === "POST" && head === "posts" && !arg) {
    if (!need()) return true;
    const parsed = parsePost(await ctx.body(req, 64_000));
    if ("error" in parsed) { json(res, 400, parsed); return true; }
    const recent = Number((await pool.query("SELECT count(*) FROM community_posts WHERE author_id=$1 AND created_at > now() - interval '1 hour'", [me])).rows[0].count);
    if (recent >= POSTS_PER_HOUR) { json(res, 429, { error: "social.slowDown" }); return true; }
    const p = parsed.post;
    let parentAuthor: string | null = null;
    if (p.replyTo) {
      const parent = (await pool.query("SELECT author_id FROM community_posts WHERE id=$1 AND deleted_at IS NULL", [p.replyTo])).rows[0];
      if (!parent) { json(res, 404, { error: "social.postNotFound" }); return true; }
      if (await blockedBetween(pool, me!, parent.author_id)) { json(res, 403, { error: "social.blocked" }); return true; }
      parentAuthor = parent.author_id;
    }
    const id = randomUUID();
    await pool.query("INSERT INTO community_posts(id, author_id, kind, body, title, prompt, tags, reply_to) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
      [id, me, p.kind, p.body, p.title, p.prompt, p.tags, p.replyTo]);
    if (parentAuthor) await notify(pool, parentAuthor, "reply", me!, id);
    json(res, 201, { post: postJson((await pool.query(`${POSTS_SELECT} AND p.id = $2`, [viewer, id])).rows[0]) });
    return true;
  }

  if (head === "posts" && arg) {
    const row = (await pool.query(`${POSTS_SELECT} AND p.id = $2`, [viewer, arg])).rows[0];
    if (!row) { json(res, 404, { error: "social.postNotFound" }); return true; }
    if (req.method === "GET" && !action) {
      const replies = (await pool.query(`${POSTS_SELECT} AND p.reply_to = $2 ORDER BY p.created_at ASC LIMIT 200`, [viewer, arg])).rows;
      json(res, 200, { post: postJson(row), replies: replies.map(postJson) });
      return true;
    }
    if (!need()) return true;
    if (req.method === "DELETE" && !action) {
      if (row.author_id !== me) { json(res, 403, { error: "social.notYours" }); return true; }
      await pool.query("UPDATE community_posts SET deleted_at=now() WHERE id=$1", [arg]);
      json(res, 200, { ok: true });
      return true;
    }
    if (action === "like" && (req.method === "POST" || req.method === "DELETE")) {
      if (req.method === "POST") {
        const added = await pool.query("INSERT INTO community_likes(post_id, user_id) VALUES ($1,$2) ON CONFLICT DO NOTHING", [arg, me]);
        if (added.rowCount) await notify(pool, row.author_id, "like", me!, arg);
      } else {
        await pool.query("DELETE FROM community_likes WHERE post_id=$1 AND user_id=$2", [arg, me]);
      }
      json(res, 200, { post: postJson((await pool.query(`${POSTS_SELECT} AND p.id = $2`, [viewer, arg])).rows[0]) });
      return true;
    }
    if (action === "save" && req.method === "POST") {
      if (row.kind !== "prompt") { json(res, 400, { error: "social.notAPrompt" }); return true; }
      const added = await pool.query("INSERT INTO community_saves(post_id, user_id) VALUES ($1,$2) ON CONFLICT DO NOTHING", [arg, me]);
      if (added.rowCount) await notify(pool, row.author_id, "save", me!, arg);
      json(res, 200, { title: row.title, prompt: row.prompt, author: row.a_handle, post: postJson((await pool.query(`${POSTS_SELECT} AND p.id = $2`, [viewer, arg])).rows[0]) });
      return true;
    }
    if (action === "report" && req.method === "POST") {
      const reason = String((await ctx.body(req)).reason ?? "").slice(0, 500);
      await pool.query("INSERT INTO community_reports(reporter_id, post_id, reason) VALUES ($1,$2,$3)", [me, arg, reason]);
      json(res, 200, { ok: true });
      return true;
    }
  }

  if (head === "notifications") {
    if (!need()) return true;
    if (req.method === "POST" && arg === "read") {
      await pool.query("UPDATE community_notifications SET read_at=now() WHERE user_id=$1 AND read_at IS NULL", [me]);
      json(res, 200, { ok: true });
      return true;
    }
    if (req.method === "GET") {
      const rows = (await pool.query(
        `SELECT n.id, n.kind, n.created_at, n.read_at, n.post_id, a.handle, a.display_name, a.building,
                p.body AS post_body, p.title AS post_title, p.kind AS post_kind
           FROM community_notifications n JOIN community_profiles a ON a.user_id = n.actor_id
           LEFT JOIN community_posts p ON p.id = n.post_id
          WHERE n.user_id = $1 ORDER BY n.created_at DESC LIMIT 100`, [me])).rows;
      json(res, 200, { notifications: rows.map((r) => ({
        id: Number(r.id), kind: r.kind, createdAt: new Date(r.created_at).toISOString(), read: r.read_at !== null,
        actor: mini(r), postId: r.post_id, snippet: r.post_kind === "prompt" ? r.post_title : (r.post_body ?? "").slice(0, 140),
      })) });
      return true;
    }
  }

  if (req.method === "GET" && head === "search") {
    const q = (url.searchParams.get("q") ?? "").trim().slice(0, 60);
    if (!q) { json(res, 200, { people: [], posts: [] }); return true; }
    const like = `%${q.replace(/[\\%_]/g, (c) => `\\${c}`)}%`;
    const people = (await pool.query(
      `SELECT * FROM community_profiles p WHERE (p.handle ILIKE $2 OR p.display_name ILIKE $2 OR array_to_string(p.stack, ' ') ILIKE $2)
         AND NOT EXISTS (SELECT 1 FROM community_blocks b WHERE (b.blocker_id=$1 AND b.blocked_id=p.user_id) OR (b.blocker_id=p.user_id AND b.blocked_id=$1))
       ORDER BY p.updated_at DESC LIMIT 20`, [viewer, like])).rows;
    const posts = (await pool.query(`${POSTS_SELECT} AND p.reply_to IS NULL AND (p.body ILIKE $2 OR p.title ILIKE $2 OR array_to_string(p.tags, ' ') ILIKE $2) ORDER BY p.created_at DESC LIMIT 20`, [viewer, like])).rows;
    json(res, 200, { people: people.map((r) => mini(r)), posts: posts.map(postJson) });
    return true;
  }

  if (req.method === "GET" && head === "suggestions") {
    const rows = (await pool.query(
      `SELECT p.*, (SELECT count(*) FROM community_follows f WHERE f.followee_id = p.user_id) AS followers FROM community_profiles p
        WHERE p.user_id <> $1
          AND NOT EXISTS (SELECT 1 FROM community_follows f WHERE f.follower_id = $1 AND f.followee_id = p.user_id)
          AND NOT EXISTS (SELECT 1 FROM community_blocks b WHERE (b.blocker_id=$1 AND b.blocked_id=p.user_id) OR (b.blocker_id=p.user_id AND b.blocked_id=$1))
        ORDER BY GREATEST(p.updated_at, COALESCE((SELECT max(created_at) FROM community_posts x WHERE x.author_id = p.user_id), p.updated_at)) DESC LIMIT 8`,
      [viewer])).rows;
    json(res, 200, { people: rows.map((r) => ({ ...mini(r), followers: Number(r.followers) })) });
    return true;
  }

  if (req.method === "PUT" && head === "stats") {
    if (!need()) return true;
    const stats = parseStats((await ctx.body(req, 64_000)).snapshot);
    if (!stats) { json(res, 400, { error: "social.statsInvalid" }); return true; }
    await pool.query("INSERT INTO community_stats(user_id, snapshot) VALUES ($1,$2::jsonb) ON CONFLICT (user_id) DO UPDATE SET snapshot=excluded.snapshot, observed_at=now()", [me, JSON.stringify(stats)]);
    json(res, 200, { ok: true });
    return true;
  }
  if (req.method === "POST" && head === "presence") {
    if (!need()) return true;
    const active = Number((await ctx.body(req)).activeSessions);
    if (!Number.isInteger(active) || active < 0 || active > 100) { json(res, 400, { error: "social.presenceInvalid" }); return true; }
    await pool.query("INSERT INTO community_presence(user_id, active_sessions, last_seen_at) VALUES ($1,$2,now()) ON CONFLICT (user_id) DO UPDATE SET active_sessions=excluded.active_sessions, last_seen_at=now()", [me, active]);
    json(res, 200, { ok: true });
    return true;
  }

  json(res, 404, { error: "social.notFound" });
  return true;
}
