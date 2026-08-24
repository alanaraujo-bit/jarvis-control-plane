import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

// @ts-expect-error — a .mjs helper, deliberately untyped; only `extract` is used.
import { extract } from "../scripts/sync-protocol.mjs";

/**
 * The copy in `src/protocol.ts` must agree with the package it came from.
 *
 * A generated file that nothing checks is a file that drifts — someone edits
 * the protocol, the relay keeps compiling against yesterday's shape, and the
 * mismatch shows up as a runtime surprise on a phone. This is the check that
 * makes copying acceptable rather than merely convenient.
 */
test("the relay's copy of the protocol matches the source of truth", () => {
  const source = readFileSync(
    new URL("../../../packages/protocol/src/index.ts", import.meta.url),
    "utf8",
  );
  const copy = readFileSync(new URL("../src/protocol.ts", import.meta.url), "utf8");

  const expected = extract(source) as string;
  assert.ok(
    copy.endsWith(expected),
    "src/protocol.ts is stale — run `node scripts/sync-protocol.mjs`",
  );
});
