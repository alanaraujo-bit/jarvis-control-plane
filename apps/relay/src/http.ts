/**
 * The bits every endpoint needs, in one place.
 *
 * Kept separate from the handlers so the rules that matter — who is allowed to
 * talk, what an error is allowed to say — are stated once and cannot drift
 * between five files.
 */

import { constantTimeEqual, hashToken } from "./pairing.js";
import type { Mailbox } from "./mailbox.js";

export function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: {
      "content-type": "application/json",
      // No caching anywhere, for any relay response. Every one of them is
      // either a secret or a point-in-time reading, and neither is something
      // an intermediary should keep.
      "cache-control": "no-store",
    },
  });
}

/**
 * An error the caller is allowed to see.
 *
 * A **code**, never a sentence: the phone and the desktop each speak their own
 * language (§65), and a relay that returns English prose would either be wrong
 * for one of them or force translation of server strings. It also keeps the
 * relay from accidentally describing its own internals in an error message.
 */
export function fail(code: string, status: number): Response {
  return json({ error: code }, status);
}

/** The bearer token on a request, or null. */
export function bearer(request: Request): string | null {
  const header = request.headers.get("authorization");
  if (!header?.startsWith("Bearer ")) return null;
  const token = header.slice("Bearer ".length).trim();
  return token.length > 0 ? token : null;
}

/**
 * Whether this request carries the desktop's own token.
 *
 * Compared as hashes, in constant time. The relay never holds the token, so
 * the only thing it *can* compare is the hash — which is the point.
 */
export async function isDesktop(request: Request, mailbox: Mailbox): Promise<boolean> {
  const token = bearer(request);
  if (!token) return false;
  return constantTimeEqual(await hashToken(token), mailbox.desktopTokenHash);
}

/** Whether this request carries a token belonging to a paired phone. */
export async function isPairedDevice(request: Request, mailbox: Mailbox): Promise<boolean> {
  const token = bearer(request);
  if (!token) return false;
  const hash = await hashToken(token);
  // Every candidate is compared even after a match, so the time taken does not
  // reveal which device was recognised or how many are paired.
  let matched = false;
  for (const known of mailbox.deviceTokenHashes) {
    if (constantTimeEqual(hash, known)) matched = true;
  }
  return matched;
}

/**
 * Read a JSON body, refusing anything implausible before parsing it.
 *
 * A relay endpoint is a public URL: the size limit is what stops someone
 * posting a gigabyte at it, and it is checked before the body is read rather
 * than after.
 */
export async function readJsonBody<T>(request: Request, maxBytes = 256 * 1024): Promise<T | null> {
  const declared = Number(request.headers.get("content-length") ?? "0");
  if (declared > maxBytes) return null;
  try {
    const text = await request.text();
    if (text.length > maxBytes) return null;
    return JSON.parse(text) as T;
  } catch {
    return null;
  }
}

/**
 * CORS, scoped as tightly as this can be.
 *
 * The PWA is served from the same deployment as these endpoints, so
 * same-origin covers the real client. The allowance exists for local
 * development against a dev server on another port, and is deliberately not
 * `*` with credentials — the tokens here are bearer tokens in a header, so
 * this is not defending a cookie, but a narrow rule costs nothing.
 */
export function cors(request: Request): HeadersInit {
  const origin = request.headers.get("origin") ?? "";
  const allowed =
    origin.startsWith("http://localhost:") || origin.startsWith("http://127.0.0.1:");
  return allowed
    ? {
        "access-control-allow-origin": origin,
        "access-control-allow-headers": "authorization, content-type",
        "access-control-allow-methods": "GET, POST, OPTIONS",
      }
    : {};
}
