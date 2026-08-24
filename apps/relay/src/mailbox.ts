/**
 * The mailbox — what the relay is allowed to remember, and for how long (§59).
 *
 * A deliberately small surface: a desktop leaves a snapshot, a phone leaves
 * commands, each side collects what the other left. The relay never reasons
 * about any of it.
 *
 * ## Everything expires
 *
 * There is no "delete my data" flow because there is nothing to delete a week
 * later. A snapshot is stale within minutes and worthless within an hour; a
 * command not collected in that time is one the desktop was offline for, and
 * replaying it later would be worse than dropping it — imagine an approval
 * granted an hour after the thing it approved was asked about.
 *
 * TTL is enforced **on read**, not by a background sweeper. A serverless
 * relay has no process to run one in, and a value that is expired but not yet
 * swept must never be served — so the check has to be at the point of use
 * anyway. The sweeper would be an optimisation on storage cost, not on
 * correctness, and this stores kilobytes.
 */

import type { RelayCommand, RelayCommandResult, RelaySnapshot } from "./protocol.js";

/** How long a snapshot is served before the phone is told it is stale. */
export const SNAPSHOT_TTL_MS = 60 * 60 * 1000;

/**
 * How long an uncollected command waits.
 *
 * Short, and much shorter than the snapshot: a command is an instruction about
 * a situation that existed when it was sent. "Approve this force push" means
 * nothing ten minutes later — the guardrail prompt it answers has long since
 * timed out. Better to drop it and let the person ask again with current
 * information.
 */
export const COMMAND_TTL_MS = 5 * 60 * 1000;

/** A stored value with the moment it stops being true. */
export interface Expiring<T> {
  value: T;
  expiresAt: number;
}

/**
 * What the relay keeps for one paired desktop.
 *
 * Note what is *not* here: no history, no log, no list of past commands. The
 * relay is a mailbox, and a mailbox that kept copies of everything that passed
 * through it would be a second store of the user's work — the thing §23 exists
 * to prevent, and a much larger thing to secure.
 */
export interface Mailbox {
  /** SHA-256 of the desktop's token. The token itself is never stored. */
  desktopTokenHash: string;
  /** SHA-256 of each paired phone's token. */
  deviceTokenHashes: string[];
  snapshot: Expiring<RelaySnapshot> | null;
  /** Queued by the phone, waiting for the desktop to collect. */
  pending: Expiring<RelayCommand>[];
  /** Answered by the desktop, waiting for the phone to read. */
  results: Expiring<RelayCommandResult>[];
  createdAt: number;
}

export function emptyMailbox(desktopTokenHash: string, now: number): Mailbox {
  return {
    desktopTokenHash,
    deviceTokenHashes: [],
    snapshot: null,
    pending: [],
    results: [],
    createdAt: now,
  };
}

/** Whether a stored value is still worth serving. */
export function isLive<T>(entry: Expiring<T> | null, now: number): entry is Expiring<T> {
  return entry !== null && entry.expiresAt > now;
}

export function live<T>(entries: Expiring<T>[], now: number): Expiring<T>[] {
  return entries.filter((entry) => entry.expiresAt > now);
}

/**
 * How many commands may be queued at once.
 *
 * A bound rather than a guess: without one, a phone that cannot reach the
 * desktop would pile up instructions forever and the desktop would act on all
 * of them at once when it came back. Five is more than a person queues
 * deliberately and few enough to be harmless if they arrive together.
 */
export const MAX_PENDING = 5;

export function queueCommand(
  mailbox: Mailbox,
  command: RelayCommand,
  now: number,
): { ok: true; mailbox: Mailbox } | { ok: false; code: string } {
  const pending = live(mailbox.pending, now);
  if (pending.length >= MAX_PENDING) {
    return { ok: false, code: "relay.tooManyPending" };
  }
  // Same id twice is a retry, not a second instruction. The phone resends when
  // it is unsure the first attempt arrived, and acting twice on "approve" is
  // exactly the thing idempotency is for.
  if (pending.some((entry) => entry.value.id === command.id)) {
    return { ok: true, mailbox: { ...mailbox, pending } };
  }
  return {
    ok: true,
    mailbox: {
      ...mailbox,
      pending: [...pending, { value: command, expiresAt: now + COMMAND_TTL_MS }],
    },
  };
}

/**
 * Hand the desktop everything waiting, and clear it.
 *
 * Collect-and-clear in one step, deliberately: a command left in the queue
 * after being handed over would be delivered again on the next poll, and
 * "approve" is not something to do twice. The cost is that a desktop which
 * crashes between collecting and acting loses those commands — which is the
 * right trade, because the alternative is acting on them twice, and a person
 * can always ask again.
 */
export function collectCommands(
  mailbox: Mailbox,
  now: number,
): { commands: RelayCommand[]; mailbox: Mailbox } {
  const pending = live(mailbox.pending, now);
  return {
    commands: pending.map((entry) => entry.value),
    mailbox: { ...mailbox, pending: [] },
  };
}
