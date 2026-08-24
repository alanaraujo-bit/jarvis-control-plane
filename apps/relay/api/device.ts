/**
 * The phone's end of the mailbox (§55–§58).
 *
 * - `GET  /api/device?mailbox=…` — what the desktop last said, plus any
 *   answers to commands this phone sent.
 * - `POST /api/device?mailbox=…` — queue one command.
 *
 * Split by method rather than merged like the desktop endpoint, and for the
 * opposite reason: a phone reads far more often than it writes, and a read
 * should not have to send a body or risk being treated as a write.
 */

import type { RelayCommand } from "../src/protocol.js";

import { fail, isPairedDevice, json, readJsonBody } from "../src/http.js";
import { isLive, live, queueCommand } from "../src/mailbox.js";
import { readMailbox, writeMailbox } from "../src/store.js";

export const config = { runtime: "nodejs" };

/**
 * Commands this endpoint will accept.
 *
 * Validated against a closed list rather than trusted from the body. "Run
 * anything from your phone" is a remote shell with a friendly name; §56 is
 * specifically these three, each mapping to something the desktop already
 * knows how to do.
 */
function isKnownCommand(value: unknown): value is RelayCommand {
  if (typeof value !== "object" || value === null) return false;
  const command = value as Record<string, unknown>;
  if (typeof command.id !== "string" || command.id.length === 0) return false;

  switch (command.kind) {
    case "approve":
      return (
        typeof command.approvalId === "string" &&
        (command.decision === "allow" || command.decision === "deny")
      );
    case "startMission":
      return typeof command.projectId === "string" && typeof command.missionId === "string";
    case "stopRun":
      return typeof command.missionId === "string";
    default:
      return false;
  }
}

/** Resolve the mailbox and check the caller in one step, or explain why not. */
async function authorise(request: Request) {
  const mailboxId = new URL(request.url).searchParams.get("mailbox");
  if (!mailboxId) return { error: fail("relay.badRequest", 400) } as const;

  const mailbox = await readMailbox(mailboxId);
  if (!mailbox || !(await isPairedDevice(request, mailbox))) {
    return { error: fail("relay.unauthorised", 401) } as const;
  }
  return { mailboxId, mailbox } as const;
}

export async function GET(request: Request): Promise<Response> {
  const auth = await authorise(request);
  if ("error" in auth && auth.error) return auth.error;

  const now = Date.now();
  // An expired snapshot is reported as **absent**, not served with a note. The
  // phone's job then is to say it has not heard from the desktop, which is the
  // truth; handing over an hour-old reading and hoping the freshness stamp is
  // noticed is how a stale number gets read as current (§28).
  const snapshot = isLive(auth.mailbox.snapshot, now) ? auth.mailbox.snapshot.value : null;
  return json({
    snapshot,
    results: live(auth.mailbox.results, now).map((entry) => entry.value),
  });
}

export async function POST(request: Request): Promise<Response> {
  const auth = await authorise(request);
  if ("error" in auth && auth.error) return auth.error;

  const command = await readJsonBody<unknown>(request, 8 * 1024);
  if (!isKnownCommand(command)) return fail("relay.badCommand", 400);

  const queued = queueCommand(auth.mailbox, command, Date.now());
  if (!queued.ok) return fail(queued.code, 429);

  await writeMailbox(auth.mailboxId, queued.mailbox);
  return json({ queued: command.id });
}
