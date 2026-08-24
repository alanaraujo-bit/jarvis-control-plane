import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "../../app/platform";

/** Every preference §64 exposes, as the product will actually use them. */
export interface Preferences {
  terminalFontSize: number;
  terminalScrollback: number;
  autopilotTurnBudget: number;
}

/**
 * The keys the core accepts. Exported so a control names its own preference
 * rather than a caller passing a string that only the core will reject.
 */
export const PREF = {
  fontSize: "terminal.fontSize",
  scrollback: "terminal.scrollback",
  turnBudget: "autopilot.turnBudget",
} as const;

/** The bounds and defaults, mirrored from the core so a slider can be drawn. */
export const BOUNDS = {
  [PREF.fontSize]: { min: 10, max: 20, step: 1, default: 13 },
  [PREF.scrollback]: { min: 1_000, max: 100_000, step: 1_000, default: 20_000 },
  [PREF.turnBudget]: { min: 4, max: 100, step: 1, default: 24 },
} as const;

/**
 * Values the terminal needs before the core has answered.
 *
 * The same numbers as `BOUNDS.default`, and deliberately so: a terminal built
 * with a placeholder size and re-created a moment later would tear down its
 * own scrollback on every mount. Starting at the default means the common
 * case — nothing configured — needs no second build at all.
 */
const INITIAL: Preferences = {
  terminalFontSize: BOUNDS[PREF.fontSize].default,
  terminalScrollback: BOUNDS[PREF.scrollback].default,
  autopilotTurnBudget: BOUNDS[PREF.turnBudget].default,
};

let cache: Preferences = INITIAL;
const listeners = new Set<(prefs: Preferences) => void>();

/**
 * One shared copy, not one fetch per component.
 *
 * The terminal reads these on every mount and a split can hold four; a
 * per-component fetch would mean four round trips for one answer, and four
 * chances to disagree about what the font size is. A tiny store rather than
 * `zustand` because the whole state is three numbers and one setter — the
 * other stores here earn their machinery with async lifecycles this does not
 * have.
 */
function publish(next: Preferences) {
  cache = next;
  for (const listener of listeners) listener(next);
}

let loaded = false;

export function usePreferences() {
  const [prefs, setPrefs] = useState<Preferences>(cache);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    listeners.add(setPrefs);
    // Fetched once for the life of the process. Preferences change only
    // through the setter below, which publishes to every listener, so there is
    // nothing to poll for.
    if (!loaded && isTauri()) {
      loaded = true;
      void invoke<Preferences>("settings_preferences")
        .then(publish)
        .catch(() => {
          // The defaults are already in place and are the right answer when
          // the core cannot be asked — a settings screen that fails to render
          // is worse than one showing what the product is actually using.
          loaded = false;
        });
    }
    return () => {
      listeners.delete(setPrefs);
    };
  }, []);

  const set = useCallback(async (key: string, value: number | null) => {
    try {
      publish(await invoke<Preferences>("settings_set_preference", { key, value }));
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  return { prefs, error, set };
}

/** Read-only access, for surfaces that consume a preference but never set it. */
export function usePreferenceValues(): Preferences {
  return usePreferences().prefs;
}
