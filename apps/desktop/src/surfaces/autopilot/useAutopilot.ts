import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

/** Where a driven run has got to (§32). */
export type RunState = "working" | "deciding" | "finished";

export interface RunStatus {
  sessionId: string;
  missionId: string;
  state: RunState;
  turns: number;
  budget: number;
}

interface AutopilotState {
  /** Keyed by mission id, so several missions can be driven at once. */
  runs: Record<string, RunStatus | null>;
  error: string | null;

  refresh: (missionId: string) => Promise<void>;
  start: (missionId: string) => Promise<string | null>;
  stop: (missionId: string) => Promise<void>;
}

export const useAutopilot = create<AutopilotState>((set, get) => ({
  runs: {},
  error: null,

  refresh: async (missionId) => {
    if (!isTauri()) return;
    try {
      const status = await invoke<RunStatus | null>("autopilot_status", { missionId });
      set({ runs: { ...get().runs, [missionId]: status } });
    } catch (cause) {
      set({ error: String(cause) });
    }
  },

  /**
   * Returns an error code when the run is refused.
   *
   * The refusal that matters is a mission whose autonomy is not Unattended:
   * the core rejects it, and the message is shown rather than swallowed,
   * because the answer is for the user to change a setting they own (§33).
   */
  start: async (missionId) => {
    try {
      const status = await invoke<RunStatus>("autopilot_start", { missionId });
      set({ runs: { ...get().runs, [missionId]: status }, error: null });
      return null;
    } catch (cause) {
      const message = String(cause);
      set({ error: message });
      return message;
    }
  },

  stop: async (missionId) => {
    try {
      await invoke("autopilot_stop", { missionId });
      await get().refresh(missionId);
    } catch (cause) {
      set({ error: String(cause) });
    }
  },
}));
