/**
 * Copy the relay's half of the shared protocol into this app.
 *
 * Vercel builds `apps/relay` on its own, so a `workspace:*` dependency on
 * `@jarvis/protocol` resolves locally and fails in the build — which is how it
 * failed the first time this was deployed. The declarations are all
 * `import type` and erased at compile time, so a copy costs nothing at runtime.
 *
 * `packages/protocol/src/index.ts` stays the source of truth. Run this after
 * changing it; `protocol.test.ts` fails if the two ever drift.
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const SOURCE = join(here, "..", "..", "..", "packages", "protocol", "src", "index.ts");
const TARGET = join(here, "..", "src", "protocol.ts");
const MARKER = "// ---- Mobile companion + cloud relay";

export function extract(source) {
  const start = source.indexOf(MARKER);
  if (start === -1) throw new Error(`marker not found in protocol source: ${MARKER}`);
  return source.slice(start);
}

const HEADER = [
  "/**",
  " * The relay's half of the shared protocol (§66).",
  " *",
  " * **Generated — do not edit here.** The source of truth is",
  " * `packages/protocol/src/index.ts`; run `node scripts/sync-protocol.mjs` to",
  " * regenerate. `protocol.test.ts` fails if the two drift.",
  " *",
  " * Why a copy and not the workspace package: Vercel builds this app from",
  " * `apps/relay` alone, so `workspace:*` resolves locally and fails in the",
  " * build. Every declaration here is `import type`, erased at compile time.",
  " */",
  "",
  "/** Mirrored from the protocol package; the relay types below refer to it. */",
  "export type MissionStatus =",
  '  | "ready"',
  '  | "running"',
  '  | "verifying"',
  '  | "waiting"',
  '  | "blocked"',
  '  | "failed"',
  '  | "completed";',
  "",
  "",
].join("\n");

// `pathToFileURL` rather than string-building a `file://` URL: on Windows the
// hand-built form does not match `import.meta.url` (drive letters and
// backslashes), so the guard silently never fired and running the script did
// nothing at all.
if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const body = extract(readFileSync(SOURCE, "utf8"));
  mkdirSync(dirname(TARGET), { recursive: true });
  writeFileSync(TARGET, HEADER + body);
  console.log(`synced ${body.length} bytes into src/protocol.ts`);
}
