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
import { deleteMailbox, readMailbox, writeMailbox } from "../src/store.js";

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

/**
 * Unpair: delete the mailbox, revoking every device at once.
 *
 * **Found by testing the real thing.** Disconnecting on the desktop cleared
 * the local pairing and left the mailbox standing, so a phone that had already
 * paired kept reading snapshots from a desktop that believed it had cut them
 * off. A switch marked "disconnect" that leaves a live token working is worse
 * than no switch — it is one that lies.
 *
 * Deleting the mailbox rather than removing device tokens one by one: the
 * desktop is discarding its own token in the same breath, so nothing is left
 * that could use the mailbox anyway, and "revoke everything" is the only
 * meaning "disconnect" can honestly have here.
 */
export async function DELETE(request: Request): Promise<Response> {
  const mailboxId = new URL(request.url).searchParams.get("mailbox");
  if (!mailboxId) return fail("relay.badRequest", 400);

  const mailbox = await readMailbox(mailboxId);
  // Answering the same way for "gone" and "not yours" keeps an id from being
  // probed — and a mailbox that is already gone is the state being asked for.
  if (!mailbox) return json({ ok: true });
  if (!(await isDesktop(request, mailbox))) return fail("relay.unauthorised", 401);

  await deleteMailbox(mailboxId);
  return json({ ok: true });
}
