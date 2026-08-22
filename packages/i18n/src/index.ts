import { en, type MessageKey } from "./en";
import { ptBR } from "./pt-BR";

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
 * Resolve a message, substituting `{name}` placeholders.
 *
 * Falls back to English rather than rendering an empty string: a missing
 * translation should degrade to a readable interface, never to a blank one.
 */
export function translate(
  locale: Locale,
  key: MessageKey,
  values?: MessageValues,
): string {
  const template = CATALOGUES[locale]?.[key] ?? en[key] ?? key;
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
