import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

/**
 * Guardrails (§35).
 *
 * The operation ids are the same strings the Rust classifier uses, and they are
 * also the i18n key suffixes — `guardrail.op.<id>`. One vocabulary end to end
 * means a new operation cannot be added to the core and silently render as a
 * raw identifier in the interface.
 */
export type Operation =
  | "git.force-push"
  | "git.history-rewrite"
  | "git.branch-delete"
  | "fs.recursive-delete"
  | "secrets.access"
  | "deploy.production"
  | "package.publish"
  | "remote.execute";

export type Decision = "ask" | "allow" | "deny";
export type Scope = "global" | "project" | "default";

export interface PolicyView {
  operation: Operation;
  decision: Decision;
  /** Which scope decided it — so the UI can say why, not just what. */
  scope: Scope;
  /** What the global rule says, when looking at a project. */
  inherited: Decision | null;
}

export type GuardrailStatus = "pending" | "allowed" | "denied" | "asked";

export interface GuardrailEvent {
  id: string;
  tsMs: number;
  projectId: string | null;
  sessionId: string | null;
  missionId: string | null;
  criterionId: string | null;
  origin: "agent" | "verification";
  operation: Operation;
  /** The text that matched, verbatim. */
  fragment: string;
  command: string;
  status: GuardrailStatus;
  /** A stable code, localised through `guardrail.reason.<code>`. */
  reason: string;
  decidedAt: number | null;
  decidedBy: string | null;
}

export type Choice = "allowOnce" | "allowForProject" | "alwaysAllow" | "neverAllow";

interface GuardrailsState {
  policies: PolicyView[];
  pending: GuardrailEvent[];
  events: GuardrailEvent[];
  loading: boolean;
  error: string | null;

  loadPolicies: (projectId?: string) => Promise<void>;
  setPolicy: (
    operation: Operation,
    decision: Decision | null,
    projectId?: string,
  ) => Promise<void>;
  loadPending: (missionId?: string) => Promise<void>;
  loadEvents: (projectId?: string, missionId?: string) => Promise<void>;
  decide: (eventId: string, choice: Choice) => Promise<void>;
}

export const useGuardrails = create<GuardrailsState>((set, get) => ({
  policies: [],
  pending: [],
  events: [],
  loading: false,
  error: null,

  loadPolicies: async (projectId) => {
    if (!isTauri()) return;
    set({ loading: true, error: null });
    try {
      const policies = await invoke<PolicyView[]>("guardrail_policies", {
        projectId: projectId ?? null,
      });
      set({ policies, loading: false });
    } catch (cause) {
      set({ loading: false, error: String(cause) });
    }
  },

  setPolicy: async (operation, decision, projectId) => {
    try {
      const policies = await invoke<PolicyView[]>("set_guardrail_policy", {
        projectId: projectId ?? null,
        operation,
        decision,
      });
      set({ policies, error: null });
    } catch (cause) {
      set({ error: String(cause) });
    }
  },

  loadPending: async (missionId) => {
    if (!isTauri()) return;
    try {
      set({
        pending: await invoke<GuardrailEvent[]>("guardrail_pending", {
          missionId: missionId ?? null,
        }),
      });
    } catch (cause) {
      set({ error: String(cause) });
    }
  },

  loadEvents: async (projectId, missionId) => {
    if (!isTauri()) return;
    try {
      set({
        events: await invoke<GuardrailEvent[]>("guardrail_events", {
          projectId: projectId ?? null,
          missionId: missionId ?? null,
          limit: 100,
        }),
      });
    } catch (cause) {
      set({ error: String(cause) });
    }
  },

  decide: async (eventId, choice) => {
    try {
      await invoke("decide_guardrail", { eventId, choice, by: null });
      // Answering can change policy, the queue and the mission at once, so
      // everything on screen is re-read rather than patched in place.
      await Promise.all([get().loadPending(), get().loadEvents()]);
    } catch (cause) {
      set({ error: String(cause) });
    }
  },
}));
