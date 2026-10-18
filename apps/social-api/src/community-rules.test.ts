import { test } from "node:test";
import assert from "node:assert/strict";
import { canSee, escapeHtml, parsePost, parseProfile, parseStats } from "./community-rules.ts";

test("a profile is created with a handle and checked field by field", () => {
  const ok = parseProfile({ handle: "Alan_Dev", displayName: " Alan ", bio: "builds with agents", stack: ["Rust", "rust", "React"], website: "https://alan.dev", github: "alanaraujo-bit", statsVisibility: "public", presenceVisibility: "followers" }, true);
  assert.ok("profile" in ok, JSON.stringify(ok));
  if ("profile" in ok) {
    assert.equal(ok.profile.handle, "alan_dev");
    assert.equal(ok.profile.displayName, "Alan");
    assert.deepEqual(ok.profile.stack, ["Rust", "React"], "duplicates fold, case-insensitively");
  }
  assert.deepEqual(parseProfile({ displayName: "x" }, true), { error: "social.handleInvalid" });
  assert.deepEqual(parseProfile({ handle: "ab", displayName: "x" }, true), { error: "social.handleInvalid" });
  assert.deepEqual(parseProfile({ handle: "alan", displayName: "" }, true), { error: "social.nameRequired" });
  assert.deepEqual(parseProfile({ handle: "alan", displayName: "A", website: "javascript:alert(1)" }, true), { error: "social.websiteInvalid" });
  assert.deepEqual(parseProfile({ handle: "alan", displayName: "A", bio: "x".repeat(281) }, true), { error: "social.bioTooLong" });
  assert.deepEqual(parseProfile({ handle: "alan", displayName: "A", statsVisibility: "everyone" }, true), { error: "social.visibilityInvalid" });
});

test("a post or a prompt, with tags normalised", () => {
  const post = parsePost({ kind: "post", body: "shipped the relay", tags: ["#Rust", "agents", "rust"] });
  assert.ok("post" in post && post.post.tags.join(",") === "rust,agents");
  const prompt = parsePost({ kind: "prompt", title: "Review the diff", prompt: "Read the diff and run the tests.", body: "" });
  assert.ok("post" in prompt);
  assert.deepEqual(parsePost({ kind: "post", body: "  " }), { error: "social.bodyRequired" });
  assert.deepEqual(parsePost({ kind: "prompt", title: "", prompt: "x" }), { error: "social.titleRequired" });
  assert.deepEqual(parsePost({ kind: "post", body: "x".repeat(2001) }), { error: "social.bodyTooLong" });
  assert.deepEqual(parsePost({ kind: "post", body: "x", tags: ["a", "b", "c", "d", "e", "f"] }), { error: "social.tooManyTags" });
  assert.deepEqual(parsePost({ kind: "html", body: "x" }), { error: "social.kindInvalid" });
});

test("who sees what", () => {
  const stranger = { self: false, following: false };
  const follower = { self: false, following: true };
  const self = { self: true, following: false };
  assert.equal(canSee("public", stranger), true);
  assert.equal(canSee("followers", stranger), false);
  assert.equal(canSee("followers", follower), true);
  assert.equal(canSee("private", follower), false);
  assert.equal(canSee("private", self), true, "you always see your own");
});

test("stats are aggregate numbers only", () => {
  const stats = parseStats({
    streakCurrent: 4, streakLongest: 12, activeDays30: 20, activeDays90: 51, tokens7: 1000, tokens30: 5000,
    agentMinutes30: 600, humanMinutes30: 300, sessions30: 40,
    topModels: [{ label: "claude-opus-5-5", share: 0.7 }],
    heatmap: [{ date: "2026-10-18", tokens: 12 }],
    projects: ["secret-client"],
  });
  assert.ok(stats);
  assert.equal("projects" in (stats as object), false, "unknown fields are dropped, never stored");
  assert.equal(parseStats({ tokens30: -1 }), null);
  assert.equal(parseStats({ heatmap: [{ date: "yesterday", tokens: 1 }] }), null);
});

test("everything shown on a web page is escaped", () => {
  assert.equal(escapeHtml(`<script>alert("x")</script>&'`), "&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;&amp;&#39;");
});

test("a profile page escapes what people wrote and shows no private stats", async () => {
  const { renderProfilePage } = await import("./community-pages.ts");
  const html = renderProfilePage({
    profile: { handle: "alan", displayName: "<img src=x onerror=alert(1)>", bio: "a & b", location: "", website: "https://x.dev/\"><script>", github: "", stack: [], building: "", createdAt: "2026-10-18T00:00:00Z" },
    counts: { followers: 1, following: 2, posts: 0 },
    stats: null,
    working: null,
    posts: [],
  });
  assert.ok(!html.includes("<img src=x"), "name escaped");
  assert.ok(!html.includes("<script>"), "no script from a user ever reaches the page");
  assert.ok(html.includes("a &amp; b"));
  assert.ok(!html.includes("How they build"), "no stats section when stats are not visible");
});
