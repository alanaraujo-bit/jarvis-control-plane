/**
 * Pairing a phone with a desktop, without either one having an account (§59).
 *
 * The desktop is local-first and does not make you sign in (§3). That
 * principle has to survive contact with the cloud, so pairing is deliberately
 * **not** a login: the desktop shows a short code, you type it on the phone,
 * and from then on both hold a token. No email, no password, no account.
 *
 * ## What the relay is allowed to know
 *
 * As little as possible. The relay stores a **hash** of each token, never the
 * token — so a dump of its storage does not let anyone impersonate a desktop
 * or a phone. This is the same reasoning as never storing a password: the
 * server does not need the secret to check it.
 *
 * ## Why the code is short, and why that is safe
 *
 * Six characters is short enough to read off a screen and type on a phone,
 * and far too short to be a secret on its own. Three things make it safe
 * anyway, and all three are necessary:
 *
 * 1. **It expires in minutes**, not hours.
 * 2. **It is single-use** — the first claim consumes it.
 * 3. **Attempts are counted**, and a code dies after a handful of wrong
 *    guesses rather than standing there being brute-forced.
 *
 * Take any one away and six characters is not enough. Together they make the
 * window narrow enough that guessing is not a realistic path.
 */

/** Characters a pairing code can contain. */
const ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

export const CODE_LENGTH = 6;

/** How long a pairing code is accepted, in milliseconds. */
export const CODE_TTL_MS = 5 * 60 * 1000;

/**
 * How many wrong guesses a code tolerates before it is destroyed.
 *
 * Low on purpose: a person typing a code they can see gets it right, and the
 * cost of being wrong three times is showing a new code. An attacker gets
 * three tries per code rather than as many as they can send.
 */
export const MAX_ATTEMPTS = 5;

/**
 * Generate a pairing code.
 *
 * `crypto.getRandomValues`, never `Math.random`: this is the only thing
 * standing between a stranger and a desktop for the next five minutes, and
 * `Math.random` is predictable by design.
 *
 * The alphabet omits `I`, `O`, `0` and `1` — a code is read off one screen and
 * typed on another, and "was that a zero or an O" is a failure mode worth
 * designing out rather than apologising for.
 */
export function generateCode(random: Crypto = crypto): string {
  const bytes = new Uint8Array(CODE_LENGTH);
  random.getRandomValues(bytes);
  let code = "";
  for (const byte of bytes) {
    // Modulo bias is negligible here: 256 % 32 === 0, so the mapping is exact.
    code += ALPHABET[byte % ALPHABET.length];
  }
  return code;
}

/**
 * Generate a device token.
 *
 * 32 bytes, hex — this one *is* the long-lived secret, so it is sized to never
 * be guessed rather than to be typed.
 */
export function generateToken(random: Crypto = crypto): string {
  const bytes = new Uint8Array(32);
  random.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Hash a token for storage.
 *
 * SHA-256 with no salt, and that is correct here rather than a shortcut: a
 * salt defends against precomputation over a *small* space (passwords people
 * choose). These tokens are 32 random bytes, so there is no dictionary to
 * precompute and nothing for a rainbow table to hold. What matters is that the
 * relay never stores the token itself.
 */
export async function hashToken(token: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(token));
  return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Compare two hex strings without leaking where they differ.
 *
 * A plain `===` on a secret returns as soon as it finds a difference, so how
 * long it took says how much of the value was right. That is a real attack
 * against a network service and a cheap one to close.
 */
export function constantTimeEqual(a: string, b: string): boolean {
  if (a.length !== b.length) return false;
  let diff = 0;
  for (let i = 0; i < a.length; i++) {
    diff |= a.charCodeAt(i) ^ b.charCodeAt(i);
  }
  return diff === 0;
}

/** Normalise what someone typed: case and spacing should not matter. */
export function normaliseCode(input: string): string {
  return input.trim().toUpperCase().replace(/[\s-]/g, "");
}

export function isWellFormed(code: string): boolean {
  if (code.length !== CODE_LENGTH) return false;
  return [...code].every((c) => ALPHABET.includes(c));
}
