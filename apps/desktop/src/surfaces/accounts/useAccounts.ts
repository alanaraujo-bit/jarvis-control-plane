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

export interface AccountQuota {
  accountId: string;
  health: AccountHealth;
  windows: QuotaWindow[];
  recoversAtMs: number | null;
  refusalDetail: string | null;
  tokensToday: number;
  liveSessions: number;
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
  load: (refreshIdentity?: boolean, projectId?: string | null) => Promise<void>;
  create: (provider: ProviderId, label: string, email?: string) => Promise<void>;
  rename: (accountId: string, label: string) => Promise<void>;
  pause: (accountId: string, paused: boolean) => Promise<void>;
  remove: (accountId: string) => Promise<void>;
  activate: (accountId: string) => Promise<void>;
  setAutoSwitch: (policy: AutoSwitchPolicy) => Promise<void>;
  beginSignIn: (accountId: string, email?: string) => Promise<void>;
}

async function readReport(
  refreshIdentity: boolean,
  projectId: string | null,
): Promise<AccountsReport> {
  return invoke<AccountsReport>(refreshIdentity ? "accounts_refresh" : "accounts_report", {
    projectId,
  });
}

export const useAccounts = create<AccountsState>((set, get) => ({
  report: null,
  loading: false,
  refreshing: false,
  error: null,
  projectId: null,

  load: async (refreshIdentity = false, projectId) => {
    if (!isTauri()) return;
    const context = projectId === undefined ? get().projectId : projectId;
    set(refreshIdentity ? { refreshing: true, error: null } : { loading: true, error: null });
    try {
      const report = await readReport(refreshIdentity, context);
      set({ report, projectId: context, loading: false, refreshing: false });
    } catch (cause) {
      set({ error: String(cause), loading: false, refreshing: false });
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
      await get().load(false);
    } catch (cause) {
      set({ error: String(cause) });
      throw cause;
    }
  },

  rename: async (accountId, label) => {
    await invoke("account_rename", { accountId, label });
    await get().load(false);
  },

  pause: async (accountId, paused) => {
    await invoke("account_set_paused", { accountId, paused });
    await get().load(false);
  },

  remove: async (accountId) => {
    await invoke("account_remove", { accountId });
    await get().load(false);
  },

  activate: async (accountId) => {
    await invoke("account_set_active", { accountId });
    await get().load(false);
  },

  setAutoSwitch: async (policy) => {
    await invoke("account_set_auto_switch", { policy });
    await get().load(false);
  },

  beginSignIn: async (accountId, email) => {
    await invoke("account_begin_sign_in", {
      accountId,
      email: email?.trim() || null,
    });
    await get().load(false);
  },
}));
