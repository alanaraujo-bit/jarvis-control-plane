import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";
import { remember } from "./identityMemory";
import {
  resolveLocale,
  translate,
  type Locale,
  type MessageKey,
  type MessageValues,
} from "@jarvis/i18n";

const STORAGE_KEY = "jarvis.locale";

function initialLocale(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored) return resolveLocale([stored]);
  } catch {
    // Ignore unavailable storage and fall back to the OS preference.
  }
  return resolveLocale(navigator.languages ?? [navigator.language]);
}

interface I18nContextValue {
  locale: Locale;
  /**
   * `remembered` is false when the change is the product applying what an
   * account already carries rather than the person choosing (M20 §5).
   */
  setLocale: (locale: Locale, remembered?: boolean) => void;
  t: (key: MessageKey, values?: MessageValues) => string;
}

const I18nContext = createContext<I18nContextValue | null>(null);

export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(initialLocale);

  const setLocale = useCallback((next: Locale, remembered = true) => {
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Non-fatal.
    }
    document.documentElement.lang = next;
    setLocaleState(next);
    if (remembered) remember("appearance.locale", next);
  }, []);

  const value = useMemo<I18nContextValue>(
    () => ({
      locale,
      setLocale,
      t: (key, values) => translate(locale, key, values),
    }),
    [locale, setLocale],
  );

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18nContextValue {
  const ctx = useContext(I18nContext);
  if (!ctx) throw new Error("useI18n must be used inside <I18nProvider>");
  return ctx;
}

/** The translate function itself, for components that take it as a prop. */
export type Translate = I18nContextValue["t"];

/** Convenience hook for the common case of only needing the translate function. */
export function useT() {
  return useI18n().t;
}
