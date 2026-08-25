import { create } from "zustand";
import type { Locale } from "@jarvis/i18n";
import { invoke, isTauri } from "../../app/platform";
import { applyThemePreference, type ThemePreference } from "../../app/theme";
import { refreshPreferences } from "../settings/usePreferences";

/**
 * An account, exactly as `identity::Account` serialises.
 *
 * There is no password field here and there is not one on the Rust side
 * either — see the note on that struct. If one ever appears in this interface,
 * something has gone wrong upstream.
 */
export interface Account {
  id: string;
  email: string;
  displayName: string;
  authProvider: string;
  hasPassword: boolean;
  createdAt: number;
  updatedAt: number;
  lastSignedInAt: number | null;
}

export interface KnownAccount {
  id: string;
  email: string;
  displayName: string;
  lastSignedInAt: number | null;
}

export interface IdentityReport {
  account: Account | null;
  known: KnownAccount[];
  prompted: boolean;
  googleAvailable: boolean;
}

/** The half of an account's preferences only the webview can apply. */
export interface Carried {
  theme: string | null;
  locale: string | null;
}

/**
 * The verdicts, mirroring `identity::SignInOutcome` and `SignUpOutcome`.
 *
 * The field names here are the *runtime* names — `attemptsLeft`, not
 * `attempts_left`. TypeScript cannot check that: it will happily read a field
 * that never arrives and hand back `undefined`, which is how this repository
 * twice shipped a rendered `NaN` (HANDOFF items 17 and 61). The guard is a Rust
 * test that serialises every variant, not this interface.
 */
export type SignInOutcome =
  | { status: "ok"; report: IdentityReport; carried: Carried }
  | { status: "unknownEmail" }
  | { status: "wrongPassword"; attemptsLeft: number }
  | { status: "lockedOut"; retryInMs: number }
  | { status: "noPassword" };

export type SignUpOutcome =
  | { status: "ok"; report: IdentityReport; carried: Carried }
  | { status: "nameRequired" }
  | { status: "invalidEmail" }
  | { status: "emailTaken" }
  | { status: "passwordTooShort"; minimum: number };

/** What the core enforces. Mirrored so the form can say it before asking. */
export const MIN_PASSWORD = 8;

const EMPTY: IdentityReport = {
  account: null,
  known: [],
  prompted: true,
  googleAvailable: false,
};

interface IdentityState {
  /** `null` until the core has answered — nothing renders off it before then. */
  report: IdentityReport | null;
  /**
   * Whether the auth screen has been opened deliberately, from Settings.
   *
   * Separate from `report.prompted`, which is about the *one* time the product
   * offers an account by itself. Somebody who skipped that has to be able to
   * come back, and coming back must not require pretending they were never
   * asked.
   */
  authOpen: boolean;
  load: () => Promise<void>;
  openAuth: () => void;
  closeAuth: () => void;
  signIn: (email: string, password: string) => Promise<SignInOutcome>;
  signUp: (displayName: string, email: string, password: string) => Promise<SignUpOutcome>;
  signOut: () => Promise<void>;
  skip: () => Promise<void>;
  /** Replace the report after a call that changed it (profile edits, delete). */
  put: (report: IdentityReport) => void;
}

/**
 * Apply what an account brought with it.
 *
 * The theme and the locale live in the webview — the theme has to be on the
 * document before the first paint, so a database round trip would show the
 * wrong one first — and everything else the core has already written to its own
 * settings rows by the time this runs. So this applies two values and re-reads
 * the rest.
 *
 * `applyThemePreference` and the locale setter are called in their *silent*
 * form. Applying a preference is not the person choosing it, and writing it
 * back would have the account overwrite itself with its own value on every
 * sign-in — harmless today, and exactly the kind of loop that stops being
 * harmless the moment two machines sync.
 */
async function applyCarried(carried: Carried, setLocale?: (locale: Locale) => void) {
  if (carried.theme === "dark" || carried.theme === "light" || carried.theme === "system") {
    applyThemePreference(carried.theme as ThemePreference);
  }
  if (carried.locale && setLocale) setLocale(carried.locale as Locale);
  await refreshPreferences();
}

/**
 * The locale setter, handed in by the provider that owns it.
 *
 * `useI18n` is React context and this store is not a component, so the store
 * cannot read it. Registering it once from the app shell is the smaller of two
 * evils; the alternative was moving the locale into a global store purely so
 * that signing in could reach it.
 */
let localeSetter: ((locale: Locale) => void) | null = null;
export function registerLocaleSetter(setter: (locale: Locale) => void) {
  localeSetter = setter;
}

export const useIdentity = create<IdentityState>((set, get) => ({
  report: null,
  authOpen: false,

  load: async () => {
    if (!isTauri()) {
      set({ report: EMPTY });
      return;
    }
    try {
      set({ report: await invoke<IdentityReport>("identity_report") });
    } catch {
      // Identity must never be the reason the window stays hidden — the same
      // rule `useOnboarding` follows, and for the same reason. Read as "asked
      // already, nobody signed in", which is the state in which the product
      // behaves exactly as it did before accounts existed.
      set({ report: EMPTY });
    }
  },

  openAuth: () => set({ authOpen: true }),
  closeAuth: () => set({ authOpen: false }),

  signIn: async (email, password) => {
    const outcome = await invoke<SignInOutcome>("identity_sign_in", { email, password });
    if (outcome.status === "ok") {
      set({ report: outcome.report, authOpen: false });
      await applyCarried(outcome.carried, localeSetter ?? undefined);
    }
    return outcome;
  },

  signUp: async (displayName, email, password) => {
    const outcome = await invoke<SignUpOutcome>("identity_sign_up", {
      displayName,
      email,
      password,
    });
    if (outcome.status === "ok") {
      set({ report: outcome.report, authOpen: false });
      await applyCarried(outcome.carried, localeSetter ?? undefined);
    }
    return outcome;
  },

  signOut: async () => {
    // Nothing is put back. Signing out is not a reason for the interface to
    // change appearance while somebody is looking at it (M20 §5) — the
    // account's values are still stored, waiting for the next sign-in.
    set({ report: await invoke<IdentityReport>("identity_sign_out") });
  },

  skip: async () => {
    if (!isTauri()) {
      set({ report: { ...EMPTY }, authOpen: false });
      return;
    }
    try {
      set({ report: await invoke<IdentityReport>("identity_skip"), authOpen: false });
    } catch {
      const current = get().report ?? EMPTY;
      set({ report: { ...current, prompted: true }, authOpen: false });
    }
  },

  put: (report) => set({ report }),
}));
