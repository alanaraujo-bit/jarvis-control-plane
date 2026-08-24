import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

export type ProviderId = "claude-code" | "codex";
export type Confidence = "official" | "observed" | "estimated" | "unknown";
export type AccountHealth = "ready" | "nearing" | "exhausted" | "paused" | "signedOut";
export type AutoSwitchPolicy = "off" | "onExhaustion" | "onThreshold";

export interface Account {
  id: string;
  provider: ProviderId;
  label: string;
  configDir: string;
  adopted: boolean;
  email: string | null;
  orgId: string | null;
  orgName: string | null;
  plan: string | null;
  signedIn: boolean;
  checkedAt: number | null;
  active: boolean;
  paused: boolean;
  position: number;
  createdAt: number;
  lastUsedAt: number | null;
}

export interface QuotaWindow {
  window: string;
  percent: number | null;
  confidence: Confidence;
  resetsAtMs: number | null;
  exhausted: boolean;
  tokens: number;
  calibrationTokens: number | null;
  calibrationSamples: number;
}

/**
 * One allowance window exactly as a provider stated it a moment ago (M16).
 *
 * `percentUsed` is consumption, which is the direction both providers report
 * in. The screen shows headroom; the conversion lives in `format.remaining` so
 * there is one place it can be got wrong.
 */
export interface LiveWindow {
  kind: string;
  rawKind: string;
  group: string;
  scopeLabel: string | null;
  percentUsed: number;
  resetsAtMs: number | null;
  windowMinutes: number | null;
  /** The window actually rationing this account — "which quota am I waiting on". */
  binding: boolean;
  /** `provider` when the provider named it, `derived` when we picked it (§28). */
  bindingSource: "provider" | "derived";
  severity: string;
  severitySource: "provider" | "derived";
}

export interface LiveSpend {
  enabled: boolean;
  used: number;
  limit: number;
  currency: string;
  decimalPlaces: number;
  percentUsed: number | null;
  disabledReason: string | null;
  limitReached: boolean;
}

export interface LiveReading {
  readAtMs: number;
  source: "claudeGetUsage" | "codexAppServer";
  plan: string | null;
  windows: LiveWindow[];
  spend: LiveSpend | null;
  resetCredits: number;
}

/**
 * The outcome of asking a provider directly.
 *
 * Three states, not two, and the difference is the whole point: `unavailable`
 * means the provider answered and there is nothing to report (signed out, or a
 * plan without subscription limits), while `failed` means the question could
 * not be put. One deserves a sign-in button, the other a retry.
 */
export type LiveStatus =
  | { state: "ok"; reading: LiveReading }
  | { state: "unavailable"; reason: string; readAtMs: number }
  | { state: "failed"; reason: string; readAtMs: number };

export interface AccountQuota {
  accountId: string;
  health: AccountHealth;
  windows: QuotaWindow[];
  recoversAtMs: number | null;
  refusalDetail: string | null;
  tokensToday: number;
  liveSessions: number;
  /** `null` only before this account has ever been asked. */
  live: LiveStatus | null;
  liveStale: boolean;
}

export interface AccountCard {
  account: Account;
  quota: AccountQuota;
  folderTrusted: boolean | null;
}

export interface AccountsReport {
  accounts: AccountCard[];
  autoSwitch: AutoSwitchPolicy;
  thresholdPercent: number;
}

interface AccountsState {
  report: AccountsReport | null;
  loading: boolean;
  refreshing: boolean;
  error: string | null;
  projectId: string | null;
  /** Which single account is being re-probed, so only its card spins. */
  refreshingAccountId: string | null;
  /** When a live refresh last completed, so two callers do not both probe. */
  lastLiveAt: number;
  load: (mode?: ReadMode, projectId?: string | null) => Promise<void>;
  refreshAccount: (accountId: string) => Promise<void>;
  /** Load the cached report, then probe only if what is stored has gone stale. */
  ensureFresh: (maxAgeMs?: number) => Promise<void>;
  create: (provider: ProviderId, label: string, email?: string) => Promise<void>;
  rename: (accountId: string, label: string) => Promise<void>;
  pause: (accountId: string, paused: boolean) => Promise<void>;
  remove: (accountId: string) => Promise<void>;
  activate: (accountId: string) => Promise<void>;
  setAutoSwitch: (policy: AutoSwitchPolicy) => Promise<void>;
  beginSignIn: (accountId: string, email?: string) => Promise<void>;
}

/**
 * How much a read is allowed to cost.
 *
 * - `cached` — a database row. Instant, and what a panel paints first with.
 * - `quota` — one CLI per account, to ask the providers what is left now.
 * - `full` — that, plus re-asking each directory who it is signed in as, which
 *   doubles the process spawns for a fact that only changes around a login.
 *
 * `full` is what a person pressing "Check now" gets. The five-minute tick
 * behind the status bar runs all day, so it gets `quota` and nothing more.
 */
export type ReadMode = "cached" | "quota" | "full";

async function readReport(mode: ReadMode, projectId: string | null): Promise<AccountsReport> {
  if (mode === "cached") {
    return invoke<AccountsReport>("accounts_report", { projectId });
  }
  return invoke<AccountsReport>("accounts_refresh", {
    projectId,
    identity: mode === "full",
  });
}

export const useAccounts = create<AccountsState>((set, get) => ({
  report: null,
  loading: false,
  refreshing: false,
  refreshingAccountId: null,
  lastLiveAt: 0,
  error: null,
  projectId: null,

  load: async (mode = "cached", projectId) => {
    if (!isTauri()) return;
    const context = projectId === undefined ? get().projectId : projectId;
    const live = mode !== "cached";
    set(live ? { refreshing: true, error: null } : { loading: true, error: null });
    try {
      const report = await readReport(mode, context);
      set({
        report,
        projectId: context,
        loading: false,
        refreshing: false,
        ...(live ? { lastLiveAt: Date.now() } : {}),
      });
    } catch (cause) {
      set({ error: String(cause), loading: false, refreshing: false });
    }
  },

  /**
   * What everything that is *not* the Accounts screen should call.
   *
   * Reads the stored report immediately — that costs a database row — and only
   * then, and only if nothing has probed recently, spends the CLI startups a
   * live reading needs. Two callers arriving together (the status bar waking up
   * as the panel opens) share one probe instead of racing two.
   */
  ensureFresh: async (maxAgeMs = 300_000) => {
    if (!isTauri()) return;
    const { report, lastLiveAt, refreshing } = get();
    if (!report) await get().load("cached");
    if (refreshing || Date.now() - lastLiveAt < maxAgeMs) return;
    // Quota only: identity is re-read when a person asks, or after a sign-in.
    await get().load("quota");
  },

  /**
   * Re-probe one account.
   *
   * Separate from a whole-report refresh because that starts a CLI per account
   * and one card being stuck should not cost the wait for all of them. The
   * report that comes back is the whole report — the core has no cheaper answer
   * and a partial merge here would be a second place for cards to go stale.
   */
  refreshAccount: async (accountId) => {
    if (!isTauri()) return;
    set({ refreshingAccountId: accountId, error: null });
    try {
      const report = await invoke<AccountsReport>("account_refresh_live", {
        accountId,
        projectId: get().projectId,
      });
      set({ report, refreshingAccountId: null });
    } catch (cause) {
      set({ error: String(cause), refreshingAccountId: null });
    }
  },

  create: async (provider, label, email) => {
    set({ error: null });
    try {
      const account = await invoke<Account>("account_create", { provider, label });
      await invoke("account_begin_sign_in", {
        accountId: account.id,
        email: email?.trim() || null,
      });
      await get().load("cached");
    } catch (cause) {
      set({ error: String(cause) });
      throw cause;
    }
  },

  rename: async (accountId, label) => {
    await invoke("account_rename", { accountId, label });
    await get().load("cached");
  },

  pause: async (accountId, paused) => {
    await invoke("account_set_paused", { accountId, paused });
    await get().load("cached");
  },

  remove: async (accountId) => {
    await invoke("account_remove", { accountId });
    await get().load("cached");
  },

  activate: async (accountId) => {
    await invoke("account_set_active", { accountId });
    await get().load("cached");
  },

  setAutoSwitch: async (policy) => {
    await invoke("account_set_auto_switch", { policy });
    await get().load("cached");
  },

  beginSignIn: async (accountId, email) => {
    await invoke("account_begin_sign_in", {
      accountId,
      email: email?.trim() || null,
    });
    await get().load("cached");
  },
}));
