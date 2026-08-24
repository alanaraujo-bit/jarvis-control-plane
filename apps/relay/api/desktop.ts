/**
 * The desktop's end of the mailbox (§59).
 *
 * `POST /api/desktop?mailbox=…` — leave a snapshot, take whatever the phone
 * queued, in **one round trip**. Two endpoints would mean two calls on a
 * timer, twice the invocations and twice the chance of one half succeeding.
 *
 * The desktop is the only writer of the snapshot and the only reader of the
 * queue, which is what makes a non-transactional store safe here — see the
 * note in `store.ts`.
 */

import type { RelayCommandResult, RelaySnapshot } from "../src/protocol.js";

import { fail, isDesktop, json, readJsonBody } from "../src/http.js";
import { SNAPSHOT_TTL_MS, collectCommands, live } from "../src/mailbox.js";
import { readMailbox, writeMailbox } from "../src/store.js";

export const config = { runtime: "nodejs" };

interface Body {
  snapshot?: RelaySnapshot;
  /** Answers to commands collected on a previous call. */
  results?: RelayCommandResult[];
}

export async function POST(request: Request): Promise<Response> {
  const mailboxId = new URL(request.url).searchParams.get("mailbox");
  if (!mailboxId) return fail("relay.badRequest", 400);

  const mailbox = await readMailbox(mailboxId);
  // Same answer for "no such mailbox" and "wrong token": distinguishing them
  // would confirm that a mailbox id exists, which is the one thing an id is
  // supposed to keep quiet about.
  if (!mailbox || !(await isDesktop(request, mailbox))) {
    return fail("relay.unauthorised", 401);
  }

  const body = await readJsonBody<Body>(request);
  if (!body) return fail("relay.badRequest", 400);

  const now = Date.now();
  const collected = collectCommands(mailbox, now);

  await writeMailbox(mailboxId, {
    ...collected.mailbox,
    snapshot: body.snapshot
      ? { value: body.snapshot, expiresAt: now + SNAPSHOT_TTL_MS }
      : collected.mailbox.snapshot,
    // Results are appended for the phone to read, and share the command TTL:
    // an answer nobody collected is as stale as the question was.
    results: [
      ...live(collected.mailbox.results, now),
      ...(body.results ?? []).map((value) => ({ value, expiresAt: now + SNAPSHOT_TTL_MS })),
    ],
  });

  return json({ commands: collected.commands });
}
