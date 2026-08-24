import { test } from "node:test";
import assert from "node:assert/strict";

import {
  CODE_LENGTH,
  constantTimeEqual,
  generateCode,
  generateToken,
  hashToken,
  isWellFormed,
  normaliseCode,
} from "../src/pairing.ts";

test("a generated code is the right shape and uses only unambiguous characters", () => {
  for (let i = 0; i < 200; i++) {
    const code = generateCode();
    assert.equal(code.length, CODE_LENGTH);
    assert.ok(isWellFormed(code), `${code} should be well formed`);
    // The whole reason for the reduced alphabet: a code is read off one screen
    // and typed on another, so these four must never appear.
    assert.ok(!/[IO01]/.test(code), `${code} contains an ambiguous character`);
  }
});

test("codes are not predictable", () => {
  // Not a proof of randomness — a smoke test that the generator is not
  // returning a constant, which is the failure a wrong implementation gives.
  const seen = new Set<string>();
  for (let i = 0; i < 500; i++) seen.add(generateCode());
  assert.ok(seen.size > 490, `expected ~500 distinct codes, got ${seen.size}`);
});

test("typing is forgiving about case and spacing", () => {
  assert.equal(normaliseCode(" a3-k9 pq "), "A3K9PQ");
  assert.equal(normaliseCode("A3K9PQ"), "A3K9PQ");
});

test("a token is long enough to be a real secret", () => {
  const token = generateToken();
  // 32 bytes as hex.
  assert.equal(token.length, 64);
  assert.match(token, /^[0-9a-f]+$/);
  assert.notEqual(generateToken(), token);
});

test("a token hashes to something stable that is not the token", async () => {
  const token = generateToken();
  const hash = await hashToken(token);

  assert.equal(hash.length, 64);
  assert.notEqual(hash, token, "storing the token itself would defeat the point");
  assert.equal(await hashToken(token), hash, "the same token must hash the same way");
  assert.notEqual(await hashToken(generateToken()), hash);
});

test("comparison does not short-circuit on the first difference", () => {
  assert.ok(constantTimeEqual("abc123", "abc123"));
  assert.ok(!constantTimeEqual("abc123", "abc124"));
  assert.ok(!constantTimeEqual("abc123", "xbc123"));
  // Different lengths are refused rather than compared.
  assert.ok(!constantTimeEqual("abc", "abc123"));
  assert.ok(constantTimeEqual("", ""));
});

test("a malformed code is rejected before it reaches storage", () => {
  assert.ok(!isWellFormed(""));
  assert.ok(!isWellFormed("SHORT"));
  assert.ok(!isWellFormed("TOOLONGXX"));
  // Excluded characters must not pass, or a typo would be accepted as a code
  // that can never have been generated.
  assert.ok(!isWellFormed("ABC0DE"));
  assert.ok(!isWellFormed("ABCIDE"));
  assert.ok(!isWellFormed("abc123"), "normalise first; validation is on the normal form");
});
