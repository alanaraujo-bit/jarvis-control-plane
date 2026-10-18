import { test } from "node:test";
import assert from "node:assert/strict";
import { parseDocuments } from "./documents.ts";

const doc = (over: Record<string, unknown> = {}) => ({
  kind: "projectNote", id: "n1", body: { projectPath: "C:/repo", body: "hi" }, touchedAt: 10, deletedAt: null, ...over,
});

test("accepts every carried kind", () => {
  const parsed = parseDocuments([
    doc(),
    doc({ kind: "setting", id: "accounts.rotation", body: { mode: "ask" } }),
    doc({ kind: "project", id: "p1", body: { name: "x", path: "C:/x" } }),
    doc({ kind: "projectKnowledge", id: "k1" }),
    doc({ kind: "guardrailPolicy", id: "g1", body: { operation: "git push", decision: "ask" } }),
    doc({ kind: "providerAccount", id: "a1", body: { provider: "claude-code", label: "Claude 2", email: "a@b.c", position: 1, paused: false } }),
    doc({ id: "n2", deletedAt: 20, touchedAt: 20 }),
  ]);
  assert.equal(parsed?.length, 7);
});

test("refuses a kind or a setting key outside the closed lists", () => {
  assert.equal(parseDocuments([doc({ kind: "session" })]), null);
  assert.equal(parseDocuments([doc({ kind: "setting", id: "identity.cloudSession", body: "x" })]), null);
});

test("refuses anything that looks like a provider credential", () => {
  // The registry travels; a login never does, even if a bug tried to send one.
  const leaked = { provider: "claude-code", label: "x", email: "a@b.c", position: 0, paused: false, configDir: "C:/secret" };
  assert.equal(parseDocuments([doc({ kind: "providerAccount", id: "a1", body: leaked })]), null);
  assert.equal(parseDocuments([doc({ kind: "providerAccount", id: "a1", body: { provider: "claude-code", accessToken: "t" } })]), null);
});

test("refuses malformed clocks, ids and oversized bodies", () => {
  assert.equal(parseDocuments([doc({ touchedAt: "soon" })]), null);
  assert.equal(parseDocuments([doc({ deletedAt: "never" })]), null);
  assert.equal(parseDocuments([doc({ id: "" })]), null);
  assert.equal(parseDocuments([doc({ body: { body: "x".repeat(300_000) } })]), null);
  assert.equal(parseDocuments({ not: "a list" }), null);
});
