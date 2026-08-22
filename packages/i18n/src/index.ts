import { en, type MessageKey } from "./en.ts";
import { ptBR } from "./pt-BR.ts";

export type { MessageKey };

export const LOCALES = ["en", "pt-BR"] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_NAMES: Record<Locale, string> = {
  en: "English",
  "pt-BR": "Português (Brasil)",
};

const CATALOGUES: Record<Locale, Record<MessageKey, string>> = {
  en,
  "pt-BR": ptBR,
};

export type MessageValues = Record<string, string | number>;

/**
 * Pick the plural form for a count.
 *
 * English and pt-BR share the same two-form rule, so this is deliberately
 * simple. A locale with different rules (Polish, Russian, Arabic) needs its own
 * entry here rather than a wider guess — getting plurals subtly wrong in
 * someone's language reads as carelessness.
 */
function pluralCategory(locale: Locale, count: number): "one" | "other" {
  switch (locale) {
    case "en":
      return count === 1 ? "one" : "other";
    case "pt-BR":
      // Portuguese treats 0 and 1 alike: "0 critério", "1 critério".
      return Math.abs(count) <= 1 ? "one" : "other";
  }
}

/**
 * Resolve a message, substituting `{name}` placeholders.
 *
 * Plurals: callers always pass the **base** key. The base entry carries the
 * plural ("other") wording, and an optional `<key>_one` sibling carries the
 * singular. When `values.count` selects the "one" category and that sibling
 * exists, it wins.
 *
 * Keeping the plural on the base key means a message that has not been given a
 * singular form still renders correctly, and callers never have to know which
 * messages are pluralised.
 *
 * Falls back to English rather than rendering an empty string: a missing
 * translation should degrade to a readable interface, never to a blank one.
 */
export function translate(
  locale: Locale,
  key: MessageKey,
  values?: MessageValues,
): string {
  const catalogue = CATALOGUES[locale] ?? en;
  let resolved: string | undefined;

  if (values && typeof values.count === "number") {
    if (pluralCategory(locale, values.count) === "one") {
      const singular = `${key}_one` as MessageKey;
      resolved = catalogue[singular] ?? en[singular];
    }
  }

  const template = resolved ?? catalogue[key] ?? en[key] ?? key;
  if (!values) return template;

  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in values ? String(values[name]) : match,
  );
}

/** Pick the best supported locale for a browser/OS language list. */
export function resolveLocale(preferred: readonly string[]): Locale {
  for (const candidate of preferred) {
    const exact = LOCALES.find((l) => l.toLowerCase() === candidate.toLowerCase());
    if (exact) return exact;
    const base = candidate.split("-")[0]?.toLowerCase();
    const loose = LOCALES.find((l) => l.split("-")[0].toLowerCase() === base);
    if (loose) return loose;
  }
  return "en";
}
