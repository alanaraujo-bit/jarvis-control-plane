import { create } from "zustand";
import { remember } from "./identityMemory";

export type ThemePreference = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

const STORAGE_KEY = "jarvis.theme";

function readStoredPreference(): ThemePreference {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw === "dark" || raw === "light" || raw === "system") return raw;
  } catch {
    // Private mode or blocked storage — fall through to the default.
  }
  return "dark";
}

function systemTheme(): ResolvedTheme {
  return window.matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

function resolve(preference: ThemePreference): ResolvedTheme {
  return preference === "system" ? systemTheme() : preference;
}

/**
 * Apply the theme to the document root.
 *
 * The colour transition is enabled only for *deliberate* theme changes, never
 * for the initial paint — otherwise the app visibly fades in on every launch,
 * which reads as slow (§11).
 */
function applyTheme(theme: ResolvedTheme, animate: boolean) {
  const root = document.documentElement;
  if (animate) {
    root.classList.add("theme-transition");
    window.setTimeout(() => root.classList.remove("theme-transition"), 220);
  }
  root.setAttribute("data-theme", theme);
}

interface ThemeState {
  preference: ThemePreference;
  resolved: ResolvedTheme;
  setPreference: (preference: ThemePreference) => void;
}

/**
 * Apply a theme without treating it as a choice (M20 §5).
 *
 * Signing in applies the theme the account carries. That is not the person
 * picking it, so it must not be written back — an account overwriting itself
 * with its own value is harmless today and is exactly the loop that stops
 * being harmless the moment two machines sync. It still persists to
 * `localStorage`, because the next launch has to paint the right theme before
 * anything can be asked.
 */
export function applyThemePreference(next: ThemePreference) {
  useTheme.setState((state) => {
    if (state.preference === next) return state;
    try {
      localStorage.setItem(STORAGE_KEY, next);
    } catch {
      // Persistence is a convenience; never block the change on it.
    }
    const resolved = resolve(next);
    applyTheme(resolved, true);
    return { preference: next, resolved };
  });
}

export const useTheme = create<ThemeState>((set) => {
  const preference = readStoredPreference();
  const resolved = resolve(preference);
  applyTheme(resolved, false);

  // Track the OS setting so "System" stays live rather than sampled once.
  window.matchMedia("(prefers-color-scheme: light)").addEventListener("change", () => {
    const state = useTheme.getState();
    if (state.preference !== "system") return;
    const next = systemTheme();
    applyTheme(next, true);
    set({ resolved: next });
  });

  return {
    preference,
    resolved,
    setPreference: (next) => {
      try {
        localStorage.setItem(STORAGE_KEY, next);
      } catch {
        // Persistence is a convenience; never block the change on it.
      }
      const nextResolved = resolve(next);
      applyTheme(nextResolved, true);
      set({ preference: next, resolved: nextResolved });
      // A deliberate choice, so it follows the person to the next machine.
      remember("appearance.theme", next);
    },
  };
});
