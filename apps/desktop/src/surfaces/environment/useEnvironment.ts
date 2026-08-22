import { useEffect } from "react";
import { create } from "zustand";
import { invoke } from "../../app/platform";
import type { EnvironmentReport } from "@jarvis/protocol";

interface EnvironmentState {
  report: EnvironmentReport | null;
  loading: boolean;
  error: string | null;
  /** Guards against several mounted consumers each triggering a scan. */
  started: boolean;
  scan: (force?: boolean) => Promise<void>;
}

const useEnvironmentStore = create<EnvironmentState>((set, get) => ({
  report: null,
  loading: false,
  error: null,
  started: false,

  scan: async (force = false) => {
    if (get().loading) return;
    if (get().started && !force) return;
    set({ loading: true, started: true, error: null });
    try {
      const report = await invoke<EnvironmentReport>("scan_environment");
      set({ report, loading: false });
    } catch (cause) {
      set({
        loading: false,
        error: cause instanceof Error ? cause.message : String(cause),
      });
    }
  },
}));

/**
 * Shared environment scan.
 *
 * A single scan backs every consumer — the status bar, settings and onboarding
 * all read the same result rather than each spawning its own process probes.
 */
export function useEnvironment() {
  const state = useEnvironmentStore();
  useEffect(() => {
    void state.scan();
    // Intentionally once per app session; `scan()` de-duplicates internally.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  return state;
}

export { useEnvironmentStore };
