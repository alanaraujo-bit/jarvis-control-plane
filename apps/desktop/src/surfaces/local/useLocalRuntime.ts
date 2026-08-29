import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";
import { readGpu, type GpuMetrics } from "../../app/gpu";

/**
 * The local runtime, as the screen sees it (§92).
 *
 * Mirrors `localai::commands::LocalRuntimeReport` field for field. Nothing is
 * computed here that the core could measure: the core talks to the server and
 * to the card, and this holds what it said.
 */

export type SandboxMode = "read-only" | "workspace-write" | "danger-full-access";
export type ApprovalPolicy = "untrusted" | "on-failure" | "on-request" | "never";

export interface RuntimeConfig {
  endpoint: string;
  model: string | null;
  keepAliveMinutes: number;
  preloadOnStart: boolean;
  sandbox: SandboxMode;
  approval: ApprovalPolicy;
}

export interface InstalledModel {
  name: string;
  sizeBytes: number;
  parameterSize: string | null;
  quantization: string | null;
  /** What the model's metadata declares, not what it is loaded with. */
  maxContext: number | null;
  capabilities: string[];
  modifiedAt: string | null;
}

export interface ResidentModel {
  name: string;
  sizeBytes: number;
  /** Equal to `sizeBytes` means every layer is on the GPU. */
  sizeVramBytes: number;
  /** The window the runner was **actually** loaded with. */
  contextLength: number | null;
  expiresAt: string | null;
}

export interface ServerEnvValue {
  key: string;
  value: string | null;
  fallback: string | null;
  /** False means the value is not in force in the server that is running. */
  persisted: boolean;
}

export interface LocalRuntimeReport {
  config: RuntimeConfig;
  serverVersion: string | null;
  reachable: boolean;
  error: string | null;
  models: InstalledModel[];
  resident: ResidentModel[];
  serverEnv: ServerEnvValue[];
  codexHome: string;
  runnerInstalled: boolean;
}

interface LocalRuntimeState {
  report: LocalRuntimeReport | null;
  gpu: GpuMetrics | null;
  loading: boolean;
  /** The model a load or unload is running for, so only its button waits. */
  busyModel: string | null;
  /** A message worth showing once, such as a saved server setting. */
  notice: string | null;
  refresh: () => Promise<void>;
  save: (config: RuntimeConfig) => Promise<void>;
  load: (model: string) => Promise<void>;
  unload: (model: string) => Promise<void>;
  setServerEnv: (key: string, value: string) => Promise<void>;
  dismissNotice: () => void;
}

export const useLocalRuntime = create<LocalRuntimeState>((set, get) => ({
  report: null,
  gpu: null,
  loading: false,
  busyModel: null,
  notice: null,

  refresh: async () => {
    if (!isTauri()) return;
    set({ loading: true });
    // Together, because they are two halves of one answer: a model that is
    // resident and a card that is idle are only meaningful side by side.
    const [report, gpu] = await Promise.allSettled([
      invoke<LocalRuntimeReport>("local_runtime_report"),
      readGpu(),
    ]);
    set({
      loading: false,
      report: report.status === "fulfilled" ? report.value : get().report,
      gpu: gpu.status === "fulfilled" ? gpu.value : get().gpu,
    });
  },

  save: async (config) => {
    // Applied locally first so the control the person just moved does not
    // spring back while the round trip completes.
    const report = get().report;
    if (report) set({ report: { ...report, config } });
    await invoke("local_runtime_save", { config });
    await get().refresh();
  },

  load: async (model) => {
    set({ busyModel: model });
    try {
      await invoke("local_runtime_load", { model });
    } finally {
      set({ busyModel: null });
      await get().refresh();
    }
  },

  unload: async (model) => {
    set({ busyModel: model });
    try {
      await invoke("local_runtime_unload", { model });
    } finally {
      set({ busyModel: null });
      await get().refresh();
    }
  },

  setServerEnv: async (key, value) => {
    await invoke("local_runtime_set_server_env", { key, value });
    // Deliberately a notice rather than a silent success: this one does not
    // take effect until Ollama restarts, and a control that looks applied
    // while the running server ignores it is the failure this whole area is
    // built to avoid.
    set({ notice: "localAi.restartNeeded" });
    await get().refresh();
  },

  dismissNotice: () => set({ notice: null }),
}));

/** The runner for the configured model, when it is loaded. */
export function residentFor(report: LocalRuntimeReport | null): ResidentModel | null {
  if (!report?.config.model) return null;
  return report.resident.find((entry) => entry.name === report.config.model) ?? null;
}

/**
 * The context window a session would actually get, and how sure we are.
 *
 * The loaded runner wins over the server setting, because it is the process
 * that will serve the request. Where nothing is loaded, the server default is
 * the weaker claim and is labelled as one.
 */
export function effectiveContext(
  report: LocalRuntimeReport | null,
): { tokens: number; source: "runner" | "server" } | null {
  const resident = residentFor(report);
  if (resident?.contextLength) return { tokens: resident.contextLength, source: "runner" };

  const setting = report?.serverEnv.find((entry) => entry.key === "OLLAMA_CONTEXT_LENGTH");
  const raw = setting?.value ?? setting?.fallback;
  const tokens = raw ? Number.parseInt(raw, 10) : Number.NaN;
  return Number.isFinite(tokens) ? { tokens, source: "server" } : null;
}

export function gigabytes(bytes: number, locale: string): string {
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
    bytes / 1024 ** 3,
  )} GB`;
}
