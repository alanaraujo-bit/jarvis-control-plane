/**
 * Where a mailbox actually lives.
 *
 * Vercel Blob, private. Chosen over a database because the shape is
 * key-to-small-JSON with nothing to query across — a relational store would be
 * machinery in exchange for nothing, and the Postgres integration already on
 * this account belongs to an unrelated project.
 *
 * **Private, not public.** A mailbox holds what is running on someone's
 * machine and which approvals are waiting; a public URL is guessable given
 * enough time and this is not the sort of thing to leave on one.
 *
 * ## The consistency this does and does not give
 *
 * Blob is not transactional. Two writers to one mailbox can interleave and the
 * later write wins wholesale — so this is only safe because the writers are
 * disjoint by construction: **the desktop owns the snapshot, the phone owns
 * the queue**, and they never write the same field. The one place they meet is
 * `collectCommands`, which is the desktop clearing a queue only it clears.
 *
 * A phone queueing a command at the same instant the desktop collects can lose
 * that command. It is a real race and the honest answer is that the phone can
 * see the queue did not shrink and send again — which the retry-by-id rule in
 * `mailbox.ts` already makes safe. Fixing it properly needs compare-and-set,
 * which Blob does not offer; a database would, at a cost this does not
 * justify. Written down rather than left to be discovered.
 */

import { del, get, list, put } from "@vercel/blob";

import type { Mailbox } from "./mailbox.js";

/** Where a desktop's mailbox is filed. Never derived from anything guessable. */
function mailboxPath(id: string): string {
  return `mailboxes/${id}.json`;
}

/** Where an unclaimed pairing code waits. */
function pairingPath(code: string): string {
  return `pairings/${code}.json`;
}

export interface PairingRecord {
  /** The mailbox this code will hand over. */
  mailboxId: string;
  desktopTokenHash: string;
  expiresAt: number;
  attempts: number;
}

async function readJson<T>(path: string): Promise<T | null> {
  try {
    const result = await get(path, {
      access: "private",
      // **Straight from origin, never the CDN.** A snapshot served from a
      // cache after the desktop has moved on is precisely the stale reading
      // §28 forbids presenting as current — and the phone would have no way
      // to tell. The freshness stamp inside the payload is the second line of
      // defence, not the first.
      useCache: false,
    });
    if (!result) return null;
    return (await new Response(result.stream).json()) as T;
  } catch {
    // Not found, or unreadable. Both mean "there is nothing here", which is a
    // normal answer for a store where everything expires — not an error worth
    // propagating to a caller that would only turn it back into null.
    return null;
  }
}

async function writeJson(path: string, value: unknown): Promise<void> {
  await put(path, JSON.stringify(value), {
    // Private, matching the store. A mailbox says what is running on someone's
    // machine and which approvals are waiting; that does not belong on a URL
    // whose only protection is being long.
    access: "private",
    // The path *is* the identifier, so a random suffix would make it
    // unfindable. Overwriting is the intended behaviour here.
    addRandomSuffix: false,
    allowOverwrite: true,
    cacheControlMaxAge: 0,
  });
}

export async function readMailbox(id: string): Promise<Mailbox | null> {
  return readJson<Mailbox>(mailboxPath(id));
}

export async function writeMailbox(id: string, mailbox: Mailbox): Promise<void> {
  await writeJson(mailboxPath(id), mailbox);
}

export async function readPairing(code: string): Promise<PairingRecord | null> {
  return readJson<PairingRecord>(pairingPath(code));
}

export async function writePairing(code: string, record: PairingRecord): Promise<void> {
  await writeJson(pairingPath(code), record);
}

/** Consume a pairing code, so it can never be claimed twice. */
export async function deletePairing(code: string): Promise<void> {
  try {
    await del(pairingPath(code));
  } catch {
    // Already gone. Deleting an absent code is the outcome we wanted.
  }
}

/**
 * Remove mailboxes nobody has touched in a long time.
 *
 * Not a correctness measure — TTL is enforced on read (see `mailbox.ts`) — but
 * storage does accumulate, and a mailbox whose desktop was uninstalled would
 * otherwise sit there forever. Called from the cron entry point.
 */
export async function sweep(olderThanMs: number, now: number): Promise<number> {
  const { blobs } = await list({ prefix: "mailboxes/" });
  let removed = 0;
  for (const blob of blobs) {
    if (now - blob.uploadedAt.getTime() > olderThanMs) {
      await del(blob.url);
      removed++;
    }
  }
  return removed;
}
