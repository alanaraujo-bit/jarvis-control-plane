// End-to-end check of the community against a deployed social-api.
// node scripts/e2e-community.mjs https://<origin>
// Creates two throwaway accounts (m28-e2e-*@example.test) and deletes their
// community profiles at the end.
import assert from "node:assert/strict";

const origin = (process.argv[2] ?? "").replace(/\/$/, "");
if (!origin) throw new Error("usage: node scripts/e2e-community.mjs <origin>");
const call = async (method, path, body, token) => {
  const res = await fetch(origin + path, {
    method,
    headers: { "content-type": "application/json", ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await res.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* html */ }
  return { status: res.status, json, text };
};
const stamp = Date.now().toString(36);
async function person(name) {
  const r = await call("POST", "/v1/auth/sign-up", { displayName: name, email: `m28-e2e-${name}-${stamp}@example.test`, password: "correct horse 28" });
  assert.equal(r.status, 201, JSON.stringify(r.json));
  return r.json.token;
}
const a = await person("ana"), b = await person("bruno");
const ha = `ana_${stamp}`.slice(0, 24), hb = `bruno_${stamp}`.slice(0, 24);

// Profiles.
assert.equal((await call("GET", "/v1/social/me", undefined, a)).json.profile, null);
assert.equal((await call("GET", `/v1/social/handles/${ha}`, undefined, a)).json.available, true);
let r = await call("PUT", "/v1/social/profile", { handle: ha, displayName: "Ana <b>", bio: "agents & rust", building: "a relay", stack: ["Rust"], website: "https://ana.dev", statsVisibility: "followers", presenceVisibility: "public" }, a);
assert.equal(r.status, 200, JSON.stringify(r.json));
r = await call("PUT", "/v1/social/profile", { handle: ha, displayName: "Bruno" }, b);
assert.equal(r.status, 409, "a taken handle is refused");
r = await call("PUT", "/v1/social/profile", { handle: hb, displayName: "Bruno", statsVisibility: "public" }, b);
assert.equal(r.status, 200);
assert.equal((await call("POST", "/v1/social/posts", { kind: "post", body: "x" })).status, 401, "anonymous cannot post");

// Stats and presence, with visibility.
const snapshot = { streakCurrent: 3, streakLongest: 9, activeDays30: 18, activeDays90: 40, tokens7: 900, tokens30: 4000, agentMinutes30: 300, humanMinutes30: 120, sessions30: 22, topModels: [{ label: "claude-opus-5-5", share: 0.8 }], heatmap: [{ date: "2026-10-17", tokens: 10 }, { date: "2026-10-18", tokens: 40 }] };
assert.equal((await call("PUT", "/v1/social/stats", { snapshot }, a)).status, 200);
assert.equal((await call("POST", "/v1/social/presence", { activeSessions: 2 }, a)).status, 200);
let view = (await call("GET", `/v1/social/profiles/${ha}`, undefined, b)).json;
assert.equal(view.stats, null, "followers-only stats are hidden from a stranger");
assert.equal(view.presence.working, true, "public presence is visible");

// Follow, then the stats appear and a notification arrives.
r = await call("POST", `/v1/social/follows/${ha}`, undefined, b);
assert.equal(r.json.relation.following, true);
view = (await call("GET", `/v1/social/profiles/${ha}`, undefined, b)).json;
assert.equal(view.stats.tokens30, 4000, "followers see followers-only stats");
assert.equal(view.counts.followers, 1);

// Posts, prompts, feed, like, reply, save.
const post = (await call("POST", "/v1/social/posts", { kind: "post", body: "shipped <script>alert(1)</script>", tags: ["#Rust"] }, a)).json.post;
const prompt = (await call("POST", "/v1/social/posts", { kind: "prompt", title: "Review the diff", prompt: "Read the diff, run the tests, then summarise.", body: "my daily driver", tags: ["review"] }, a)).json.post;
assert.ok(post.id && prompt.id);
const following = (await call("GET", "/v1/social/feed?scope=following", undefined, b)).json.posts;
assert.deepEqual(following.map((p) => p.id), [prompt.id, post.id], "b's following feed has a's posts, newest first");
assert.ok((await call("GET", "/v1/social/feed?scope=discover&tag=rust", undefined, b)).json.posts.some((p) => p.id === post.id));
assert.equal((await call("POST", `/v1/social/posts/${post.id}/like`, undefined, b)).json.post.likeCount, 1);
const reply = (await call("POST", "/v1/social/posts", { kind: "post", body: "nice", replyTo: post.id }, b)).json.post;
assert.equal(reply.parent.handle, ha);
const saved = (await call("POST", `/v1/social/posts/${prompt.id}/save`, undefined, b)).json;
assert.equal(saved.prompt, "Read the diff, run the tests, then summarise.");
assert.equal(saved.post.saveCount, 1);
const thread = (await call("GET", `/v1/social/posts/${post.id}`, undefined, a)).json;
assert.equal(thread.replies.length, 1);
const notes = (await call("GET", "/v1/social/notifications", undefined, a)).json.notifications;
assert.deepEqual(notes.map((n) => n.kind).sort(), ["follow", "like", "reply", "save"]);
assert.equal((await call("GET", "/v1/social/me", undefined, a)).json.unread, 4);
await call("POST", "/v1/social/notifications/read", undefined, a);
assert.equal((await call("GET", "/v1/social/me", undefined, a)).json.unread, 0);
assert.ok((await call("GET", `/v1/social/search?q=${encodeURIComponent("diff")}`, undefined, b)).json.posts.some((p) => p.id === prompt.id));

// Public web pages: escaped, and only public data.
const page = await call("GET", `/u/${ha}`);
assert.equal(page.status, 200);
assert.ok(page.text.includes("Ana &lt;b&gt;") && !page.text.includes("<script>alert(1)"), "everything escaped");
assert.ok(!page.text.includes("How they build"), "followers-only stats never reach the public page");
assert.ok((await call("GET", `/u/${hb}`)).status === 200);
assert.equal((await call("GET", `/p/${prompt.id}`)).status, 200);
assert.equal((await call("GET", "/u/nobody_here_zz")).status, 404);

// Block: both sides stop seeing each other.
await call("POST", `/v1/social/blocks/${hb}`, undefined, a);
assert.equal((await call("GET", `/v1/social/profiles/${ha}`, undefined, b)).status, 404, "a blocked person cannot see the profile");
assert.equal((await call("POST", `/v1/social/follows/${ha}`, undefined, b)).status, 404);
assert.ok(!(await call("GET", "/v1/social/feed?scope=discover", undefined, b)).json.posts.some((p) => p.id === post.id));
await call("DELETE", `/v1/social/blocks/${hb}`, undefined, a);

// Delete own post; delete profiles.
assert.equal((await call("DELETE", `/v1/social/posts/${post.id}`, undefined, b)).status, 403, "only the author deletes");
assert.equal((await call("DELETE", `/v1/social/posts/${post.id}`, undefined, a)).status, 200);
assert.equal((await call("DELETE", "/v1/social/profile", undefined, a)).status, 200);
assert.equal((await call("DELETE", "/v1/social/profile", undefined, b)).status, 200);
assert.equal((await call("GET", `/u/${ha}`)).status, 404);
console.log(`ok: ${ha} ${hb}`);
