/** M28: the community's queries — rows to JSON, the post query with its
 *  block filter, and the profile a viewer may see. */
import type { Pool } from "pg";
import { canSee, type Relation, type Visibility } from "./community-rules.ts";

const WORKING_MS = 120_000;

// pg returns untyped rows; their shape is the SELECT right beside each use.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export type Row = Record<string, any>;

export const mini = (row: Row, prefix = "") => ({
  handle: row[`${prefix}handle`],
  displayName: row[`${prefix}display_name`],
  building: row[`${prefix}building`] ?? "",
});

export function profileJson(row: Row) {
  return {
    handle: row.handle, displayName: row.display_name, bio: row.bio, location: row.location,
    website: row.website, github: row.github, stack: row.stack ?? [], building: row.building,
    statsVisibility: row.stats_visibility, presenceVisibility: row.presence_visibility,
    createdAt: new Date(row.created_at).toISOString(),
  };
}

export function postJson(row: Row) {
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
export const POSTS_SELECT = `
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

export const NOBODY = "00000000-0000-0000-0000-000000000000";

export function cursor(url: URL): string | null {
  const before = url.searchParams.get("before");
  return before && !Number.isNaN(Date.parse(before)) ? new Date(before).toISOString() : null;
}

export async function counts(pool: Pool, userId: string) {
  const row = (await pool.query(
    `SELECT (SELECT count(*) FROM community_follows WHERE followee_id = $1) AS followers,
            (SELECT count(*) FROM community_follows WHERE follower_id = $1) AS following,
            (SELECT count(*) FROM community_posts WHERE author_id = $1 AND deleted_at IS NULL AND reply_to IS NULL) AS posts`,
    [userId])).rows[0];
  return { followers: Number(row.followers), following: Number(row.following), posts: Number(row.posts) };
}

export async function relation(pool: Pool, viewer: string | null, target: string) {
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
export async function profileView(pool: Pool, viewer: string | null, handle: string) {
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

export async function notify(pool: Pool, userId: string, kind: string, actorId: string, postId: string | null) {
  if (userId === actorId) return;
  await pool.query("INSERT INTO community_notifications(user_id, kind, actor_id, post_id) VALUES ($1,$2,$3,$4)", [userId, kind, actorId, postId]);
}

export async function blockedBetween(pool: Pool, a: string, b: string) {
  return (await pool.query(
    "SELECT 1 FROM community_blocks WHERE (blocker_id=$1 AND blocked_id=$2) OR (blocker_id=$2 AND blocked_id=$1)", [a, b])).rowCount! > 0;
}
