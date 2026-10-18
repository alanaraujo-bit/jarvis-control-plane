// End-to-end check of document sync against a deployed social-api.
// node scripts/e2e-documents.mjs https://<origin>
// Creates a throwaway account named m27-e2e-<time>@example.test.
import assert from "node:assert/strict";

const origin = (process.argv[2] ?? "").replace(/\/$/, "");
if (!origin) throw new Error("usage: node scripts/e2e-documents.mjs <origin>");
const call = async (method, path, body, token) => {
  const res = await fetch(origin + path, {
    method,
    headers: { "content-type": "application/json", ...(token ? { authorization: `Bearer ${token}` } : {}) },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  return { status: res.status, json: await res.json() };
};

const email = `m27-e2e-${Date.now()}@example.test`;
const up = await call("POST", "/v1/auth/sign-up", { displayName: "M27 e2e", email, password: "correct horse 27" });
assert.equal(up.status, 201, JSON.stringify(up.json));
const token = up.json.token;
assert.deepEqual(up.json.documents, [], "a new account starts empty");

const docs = [
  { kind: "setting", id: "accounts.rotation", body: { mode: "ask", trigger: { kind: "nearLimit", percent: 80 } }, touchedAt: 100, deletedAt: null },
  { kind: "project", id: "p1", body: { name: "repo", path: "C:/repo", autonomy: null, archived: false }, touchedAt: 100, deletedAt: null },
  { kind: "projectNote", id: "n1", body: { projectPath: "C:/repo", body: "remember the migration", pinned: true }, touchedAt: 100, deletedAt: null },
  { kind: "providerAccount", id: "a1", body: { provider: "claude-code", label: "Claude 2", email: "x@y.z", position: 1, paused: false }, touchedAt: 100, deletedAt: null },
];
const put = await call("PUT", "/v1/sync/documents", { documents: docs }, token);
assert.equal(put.status, 200, JSON.stringify(put.json));
assert.equal(put.json.documents.length, 4);

// An older write does not overwrite a newer one; a newer tombstone wins.
await call("PUT", "/v1/sync/documents", { documents: [{ ...docs[2], body: { projectPath: "C:/repo", body: "stale" }, touchedAt: 50 }] }, token);
await call("PUT", "/v1/sync/documents", { documents: [{ ...docs[1], touchedAt: 200, deletedAt: 200 }] }, token);
const refused = await call("PUT", "/v1/sync/documents", { documents: [{ ...docs[3], body: { ...docs[3].body, configDir: "C:/secret" } }] }, token);
assert.equal(refused.status, 400, "a credential-shaped field is refused");

const state = await call("GET", "/v1/sync/state", undefined, token);
assert.equal(state.status, 200);
const byId = Object.fromEntries(state.json.documents.map((d) => [d.id, d]));
assert.equal(byId.n1.body.body, "remember the migration", "stale write ignored");
assert.equal(byId.p1.deletedAt, 200, "tombstone kept");
assert.equal(byId.a1.body.label, "Claude 2");
assert.equal(byId["accounts.rotation"].body.trigger.percent, 80);

// Signing in again from "another machine" returns the same documents.
const again = await call("POST", "/v1/auth/sign-in", { email, password: "correct horse 27" });
assert.equal(again.status, 200);
assert.equal(again.json.documents.length, 4);
console.log(`ok: ${email}`);
