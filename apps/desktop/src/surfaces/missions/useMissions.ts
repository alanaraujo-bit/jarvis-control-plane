import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

export type MissionStatus =
  | "ready"
  | "running"
  | "verifying"
  | "waiting"
  | "blocked"
  | "failed"
  | "completed";

export type Autonomy = "guided" | "autonomous" | "unattended";
export type CriterionStatus = "pending" | "verified" | "failed";

export type Verification =
  | { type: "command"; command: string; cwd: string | null; expectExit: number }
  | { type: "fileExists"; path: string }
  | { type: "fileContains"; path: string; text: string }
  | { type: "manual" };

export interface Mission {
  id: string;
  projectId: string;
  title: string;
  goal: string | null;
  description: string | null;
  status: MissionStatus;
  autonomy: Autonomy | null;
  blockedReason: string | null;
  createdAt: number;
  updatedAt: number;
  startedAt: number | null;
  completedAt: number | null;
}

export interface MissionSummary extends Mission {
  projectName: string;
  taskCount: number;
  tasksDone: number;
  /** Required, active criteria that are not yet verified. */
  openCriteria: number;
}

export interface AcceptanceCriterion {
  id: string;
  missionId: string;
  description: string;
  required: boolean;
  verification: Verification;
  status: CriterionStatus;
  position: number;
  removedAt: number | null;
  removedReason: string | null;
  removedBy: string | null;
}

export interface Evidence {
  id: string;
  missionId: string;
  criterionId: string | null;
  sessionId: string | null;
  kind: "command" | "file" | "commit" | "screenshot" | "url" | "manual";
  ok: boolean;
  /** English, always present — the fallback when there is no code (§65). */
  summary: string;
  /** A message key the UI translates, when the sentence is ours to write. */
  code: string | null;
  /** JSON arguments for that message. */
  codeArgs: string | null;
  detail: string | null;
  tsMs: number;
}

export interface MissionTask {
  id: string;
  missionId: string;
  description: string;
  done: boolean;
  position: number;
}

export interface MissionDetail extends Mission {
  tasks: MissionTask[];
  criteria: AcceptanceCriterion[];
  evidence: Evidence[];
  effectiveAutonomy: Autonomy;
}

export interface NewCriterionInput {
  description: string;
  required: boolean;
  verification: Verification;
}

interface MissionsState {
  summaries: MissionSummary[];
  loading: boolean;
  error: string | null;

  refresh: () => Promise<void>;
  createMission: (input: {
    projectId: string;
    title: string;
    goal?: string;
    tasks?: string[];
    criteria?: NewCriterionInput[];
    autonomy?: Autonomy | null;
  }) => Promise<Mission | null>;
  detail: (missionId: string) => Promise<MissionDetail | null>;
  verify: (missionId: string) => Promise<MissionDetail | null>;
  setStatus: (
    missionId: string,
    status: MissionStatus,
    reason?: string,
  ) => Promise<string | null>;
  confirmCriterion: (criterionId: string, by: string) => Promise<void>;
  setTaskDone: (taskId: string, done: boolean) => Promise<void>;
}

export const useMissions = create<MissionsState>((set, get) => ({
  summaries: [],
  loading: false,
  error: null,

  refresh: async () => {
    if (!isTauri()) return;
    set({ loading: true, error: null });
    try {
      set({ summaries: await invoke<MissionSummary[]>("mission_summaries"), loading: false });
    } catch (cause) {
      set({ loading: false, error: String(cause) });
    }
  },

  createMission: async (input) => {
    try {
      const mission = await invoke<Mission>("create_mission", {
        mission: {
          projectId: input.projectId,
          title: input.title,
          goal: input.goal ?? null,
          description: null,
          tasks: input.tasks ?? [],
          criteria: input.criteria ?? [],
          autonomy: input.autonomy ?? null,
        },
      });
      await get().refresh();
      return mission;
    } catch (cause) {
      set({ error: String(cause) });
      return null;
    }
  },

  detail: async (missionId) => {
    if (!isTauri()) return null;
    try {
      return await invoke<MissionDetail>("mission_detail", { missionId });
    } catch (cause) {
      set({ error: String(cause) });
      return null;
    }
  },

  verify: async (missionId) => {
    try {
      const detail = await invoke<MissionDetail>("verify_mission_now", { missionId });
      await get().refresh();
      return detail;
    } catch (cause) {
      set({ error: String(cause) });
      return null;
    }
  },

  /**
   * Returns an error message when the transition is refused.
   *
   * The refusal that matters is completing a mission whose required criteria
   * are unverified — the core rejects it, and the message is shown rather than
   * swallowed, because the user needs to know *why* (§30).
   */
  setStatus: async (missionId, status, reason) => {
    try {
      await invoke<Mission>("set_mission_status", {
        missionId,
        status,
        reason: reason ?? null,
      });
      await get().refresh();
      return null;
    } catch (cause) {
      const message = String(cause);
      set({ error: message });
      return message;
    }
  },

  confirmCriterion: async (criterionId, by) => {
    await invoke("confirm_criterion", { criterionId, by });
  },

  setTaskDone: async (taskId, done) => {
    await invoke("set_mission_task_done", { taskId, done });
  },
}));
