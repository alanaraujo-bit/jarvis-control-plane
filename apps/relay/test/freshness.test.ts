import { test } from "node:test";
import assert from "node:assert/strict";

// @ts-expect-error — plain browser module, shipped unbuilt and deliberately untyped.
import { freshness } from "../public/freshness.js";

const NOW = 1_787_540_000_000;

function snapshotAged(seconds: number, staleAfter = 150) {
  return {
    freshness: {
      capturedAt: new Date(NOW - seconds * 1000).toISOString(),
      staleAfterSeconds: staleAfter,
    },
  };
}

test("no snapshot at all is reported as no contact, not as an empty desktop", () => {
  const result = freshness(null, NOW);
  assert.equal(result.stale, true);
  assert.match(result.text, /Sem contato/);
});

test("a recent snapshot reads as current", () => {
  assert.equal(freshness(snapshotAged(5), NOW).stale, false);
  assert.equal(freshness(snapshotAged(5), NOW).text, "Atualizado agora");
});

/**
 * The boundary is the whole point of §28 here: one missed push must not make a
 * working desktop look offline, and two in a row must.
 */
test("staleness turns over exactly at the window the desktop declared", () => {
  assert.equal(freshness(snapshotAged(150), NOW).stale, false, "at the limit it is still fresh");
  assert.equal(freshness(snapshotAged(151), NOW).stale, true, "one second past it is not");
});

test("the desktop's own window is honoured, not a constant baked in here", () => {
  // A desktop that pushes less often says so, and the phone believes it rather
  // than applying its own idea of what "recent" means.
  assert.equal(freshness(snapshotAged(200, 300), NOW).stale, false);
  assert.equal(freshness(snapshotAged(400, 300), NOW).stale, true);
});

test("a stale snapshot says how long it has been, in minutes", () => {
  assert.match(freshness(snapshotAged(600), NOW).text, /10 min/);
});

/**
 * A clock skew between the phone and the desktop can put `capturedAt` in the
 * future. Negative age must not render as "há -3 min" — clamped to zero, which
 * reads as current, which is the safe direction for a small error.
 */
test("a snapshot from the future does not produce a negative age", () => {
  const result = freshness(snapshotAged(-30), NOW);
  assert.equal(result.stale, false);
  assert.ok(!result.text.includes("-"), `got ${result.text}`);
});
