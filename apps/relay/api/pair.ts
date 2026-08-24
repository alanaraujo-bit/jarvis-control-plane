/**
 * Pairing — the only endpoint that creates something (§59).
 *
 * Two operations, one for each side of the handshake:
 *
 * - `POST /api/pair` with no body: **the desktop** asks for a code. It gets a
 *   code to show, and a token to keep.
 * - `POST /api/pair` with `{ code }`: **a phone** claims it, and gets its own
 *   token.
 *
 * Everything about this is shaped by one rule: the relay must never be able to
 * impersonate either side. It stores hashes, hands out the only copy of each
 * token, and forgets the code the moment it is used.
 */

import {
  CODE_TTL_MS,
  MAX_ATTEMPTS,
  generateCode,
  generateToken,
  hashToken,
  isWellFormed,
  normaliseCode,
} from "../src/pairing.js";
import { emptyMailbox } from "../src/mailbox.js";
import { fail, json, readJsonBody } from "../src/http.js";
import {
  deletePairing,
  readMailbox,
  readPairing,
  writeMailbox,
  writePairing,
} from "../src/store.js";

export const config = { runtime: "nodejs" };

interface ClaimBody {
  code?: unknown;
  deviceName?: unknown;
}

export async function POST(request: Request): Promise<Response> {
  const body = await readJsonBody<ClaimBody>(request, 4 * 1024);
  // No body at all is the desktop asking for a code; a body means a claim.
  // Distinguishing on presence rather than on a `mode` field keeps the two
  // shapes from being confusable by accident.
  if (body === null || body.code === undefined) {
    return offerCode();
  }
  return claimCode(body);
}

/** The desktop asks for a code to show on screen. */
async function offerCode(): Promise<Response> {
  const now = Date.now();
  const code = generateCode();
  const desktopToken = generateToken();
  const mailboxId = generateToken();

  const desktopTokenHash = await hashToken(desktopToken);
  await writeMailbox(mailboxId, emptyMailbox(desktopTokenHash, now));
  await writePairing(code, {
    mailboxId,
    desktopTokenHash,
    expiresAt: now + CODE_TTL_MS,
    attempts: 0,
  });

  // The only time either token is ever transmitted. The relay keeps the hash.
  return json({
    code,
    expiresAt: new Date(now + CODE_TTL_MS).toISOString(),
    mailboxId,
    desktopToken,
  });
}

/** A phone claims a code it was shown. */
async function claimCode(body: ClaimBody): Promise<Response> {
  if (typeof body.code !== "string") return fail("relay.badRequest", 400);
  const code = normaliseCode(body.code);
  // Checked before touching storage, so a malformed guess costs a read of
  // nothing and cannot be used to probe which codes exist.
  if (!isWellFormed(code)) return fail("relay.badCode", 400);

  const record = await readPairing(code);
  const now = Date.now();

  // One answer for "no such code", "expired" and "already used". Telling them
  // apart would let someone map which codes exist, and the person typing has
  // the same next step in all three cases: get a fresh code from the desktop.
  if (!record || record.expiresAt <= now) {
    if (record) await deletePairing(code);
    return fail("relay.badCode", 404);
  }

  const mailbox = await readMailbox(record.mailboxId);
  if (!mailbox) {
    await deletePairing(code);
    return fail("relay.badCode", 404);
  }

  // **Count the attempt before honouring it, not after.**
  //
  // The counter is what makes a six-character code safe (see `pairing.ts`),
  // and the first version of this file declared `attempts` and never once
  // incremented it — a limit that exists in the record, is checked on read,
  // and can never be reached. HANDOFF §5 item 29 is the same shape: a comment
  // describing intent while nothing implements it.
  //
  // What is actually being counted is *claims of a code that exists*. A
  // well-formed guess at a random code almost never gets here, so in practice
  // this bounds how many times one code can be claimed at all — which is the
  // property worth having, since a code is single-use anyway.
  const attempts = record.attempts + 1;
  if (attempts > MAX_ATTEMPTS) {
    await deletePairing(code);
    return fail("relay.badCode", 404);
  }
  await writePairing(code, { ...record, attempts });

  const deviceToken = generateToken();
  await writeMailbox(record.mailboxId, {
    ...mailbox,
    deviceTokenHashes: [...mailbox.deviceTokenHashes, await hashToken(deviceToken)],
  });

  // Single-use: consumed the moment it works, so a shoulder-surfed code is
  // worth nothing once the intended phone has used it.
  await deletePairing(code);

  return json({ mailboxId: record.mailboxId, deviceToken });
}
