import { strict as assert } from "node:assert";
import { test } from "node:test";

import { buildThread, reconcile, type ChatItem, type Entry } from "./thread.ts";

const user = (text: string, tsMs = 1): ChatItem => ({ kind: "message", role: "user", text, tsMs, usage: null });
const agent = (text: string, tsMs = 1): ChatItem => ({ kind: "message", role: "assistant", text, tsMs, usage: { model: "claude-opus-5-5" } });

test("work between two replies folds into one activity line", () => {
  const thread = buildThread([
    user("make hello.txt"),
    { kind: "thinking", text: "", tsMs: 2 },
    { kind: "thinking", text: "", tsMs: 2 },
    { kind: "toolCall", id: "t1", name: "Write", summary: "C:/p/hello.txt", tsMs: 3 },
    { kind: "toolResult", id: "t1", ok: true, summary: "ok", tsMs: 4 },
    { kind: "toolCall", id: "t2", name: "Bash", summary: "cat hello.txt", tsMs: 5 },
    agent("Done.", 6),
    { kind: "turnEnded", reason: "end_turn", tsMs: 6 },
  ]);
  assert.deepEqual(
    thread.map((entry) => entry.type),
    ["user", "activity", "assistant"],
  );
  const activity = thread[1] as Extract<Entry, { type: "activity" }>;
  assert.equal(activity.steps.length, 3, "two thoughts in a row are one step");
  assert.equal(activity.steps[0].thinking, true);
  assert.equal(activity.steps[1].ok, true, "a result attaches to its call");
  assert.equal(activity.steps[2].ok, null, "no result yet");
  assert.equal((thread[2] as Extract<Entry, { type: "assistant" }>).model, "claude-opus-5-5");
});

test("a turn boundary starts a new activity line", () => {
  const thread = buildThread([
    { kind: "toolCall", id: "a", name: "Read", summary: "x", tsMs: 1 },
    { kind: "turnEnded", reason: "end_turn", tsMs: 2 },
    { kind: "toolCall", id: "b", name: "Read", summary: "y", tsMs: 3 },
  ]);
  assert.equal(thread.filter((entry) => entry.type === "activity").length, 2);
});

test("empty and system messages are not bubbles; errors are shown", () => {
  const thread = buildThread([
    { kind: "message", role: "system", text: "init", tsMs: 1, usage: null },
    agent("   "),
    { kind: "error", message: "You've hit your usage limit.", tsMs: 2 },
  ]);
  assert.deepEqual(thread, [{ type: "error", message: "You've hit your usage limit.", tsMs: 2 }]);
});

test("the draft goes once the log has the same reply", () => {
  const entries = buildThread([user("hi", 1000), agent("Hello!", 1500)]);
  const draft = { text: "Hello!", startedAt: 1200, doneAt: 1400 };
  assert.equal(reconcile(entries, draft, [], 1600).draft, null);
  const different = { text: "Hello! And", startedAt: 1200, doneAt: null };
  assert.deepEqual(reconcile(entries, different, [], 1600).draft, different, "still writing");
});

test("an older identical reply does not swallow a new draft", () => {
  const entries = buildThread([agent("Done.", 1000)]);
  const draft = { text: "Done.", startedAt: 60_000, doneAt: null };
  assert.deepEqual(reconcile(entries, draft, [], 61_000).draft, draft);
});

test("a finished draft the log never matched is let go after a grace period", () => {
  const draft = { text: "x", startedAt: 0, doneAt: 1000 };
  assert.deepEqual(reconcile([], draft, [], 5000).draft, draft);
  assert.equal(reconcile([], draft, [], 12_000).draft, null);
});

test("each optimistic send goes once its own message appears", () => {
  const entries = buildThread([user("again", 2000)]);
  const sent = [
    { text: "again", at: 1900 },
    { text: "again", at: 2100 },
  ];
  const { sent: left } = reconcile(entries, null, sent, 2500);
  assert.equal(left.length, 1, "one log message settles one send, not both");
});
