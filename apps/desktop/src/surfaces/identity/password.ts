import type { MessageKey } from "@jarvis/i18n";

/**
 * How strong a password looks, on a four-step scale.
 *
 * Length first, and by a distance: three of the four points come from getting
 * longer, and only the last from using more than one kind of character. That is
 * the opposite of the usual meter, and it is the honest ordering — a composition
 * rule produces `Password1!` across the whole world, while every extra character
 * multiplies the search space.
 *
 * This is **advice, never a gate**. The core accepts anything at or above
 * `MIN_PASSWORD`, and a meter that refuses is a meter that teaches people to
 * write their password down.
 */
export type Strength = 0 | 1 | 2 | 3 | 4;

export function strengthOf(password: string): Strength {
  if (!password) return 0;
  let score = 1;
  if (password.length >= 12) score += 1;
  if (password.length >= 16) score += 1;

  const kinds = [/[a-z]/, /[A-Z]/, /[0-9]/, /[^A-Za-z0-9]/].filter((re) =>
    re.test(password),
  ).length;
  if (kinds >= 3 && password.length >= 10) score += 1;

  // A short password never reads as anything but weak, whatever it is made of.
  if (password.length < 8) return 1;
  return Math.min(score, 4) as Strength;
}

export const STRENGTH_LABEL: Record<Exclude<Strength, 0>, MessageKey> = {
  1: "identity.password.strength.weak",
  2: "identity.password.strength.fair",
  3: "identity.password.strength.good",
  4: "identity.password.strength.strong",
};

/**
 * The domains a typo is worth catching.
 *
 * Deliberately short. A long list turns a helpful nudge into a wrong guess for
 * anybody at a company whose domain happens to be one edit from a big provider,
 * and the suggestion is only ever offered — never applied.
 */
const COMMON_DOMAINS = [
  "gmail.com",
  "outlook.com",
  "hotmail.com",
  "icloud.com",
  "yahoo.com",
  "yahoo.com.br",
  "proton.me",
  "protonmail.com",
  "live.com",
  "uol.com.br",
  "bol.com.br",
  "terra.com.br",
];

/** Ordinary Levenshtein, bounded by the fact that domains are short. */
function distance(a: string, b: string): number {
  const rows = a.length + 1;
  const cols = b.length + 1;
  let previous = Array.from({ length: cols }, (_, index) => index);
  for (let i = 1; i < rows; i += 1) {
    const current = [i];
    for (let j = 1; j < cols; j += 1) {
      const substitution = previous[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1);
      current[j] = Math.min(current[j - 1] + 1, previous[j] + 1, substitution);
    }
    previous = current;
  }
  return previous[cols - 1];
}

/**
 * "Did you mean gmail.com?" — the single most useful thing a sign-up form can
 * say, because a mistyped domain is invisible: the address is well-formed, it
 * is accepted, and nothing ever arrives.
 *
 * Only one edit away, and only when the domain is not already a real one.
 */
export function domainSuggestion(email: string): string | null {
  const at = email.lastIndexOf("@");
  if (at < 1) return null;
  const domain = email.slice(at + 1).toLowerCase();
  if (domain.length < 4 || COMMON_DOMAINS.includes(domain)) return null;

  for (const candidate of COMMON_DOMAINS) {
    if (distance(domain, candidate) <= 1) {
      return `${email.slice(0, at)}@${candidate}`;
    }
  }
  return null;
}
