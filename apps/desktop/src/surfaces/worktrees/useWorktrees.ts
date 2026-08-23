import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";
import type { Choice, Operation } from "../guardrails/useGuardrails";
import type { Project } from "../projects/useProjects";

/** Mirrors `git::worktree::Worktree`, flattened into `WorktreeView`. */
export interface WorktreeView {
  path: string;
  branch: string | null;
  head: string | null;
  detached: boolean;
  /** The repository itself. It cannot be removed. */
  isMain: boolean;
  locked: boolean;
  lockReason: string | null;
  prunable: boolean;
  /**
   * `null` means Git knows about this worktree and J.A.R.V.I.S. has never
   * opened it — what a worktree created at a terminal looks like. A real state,
   * shown rather than hidden.
   */
  projectId: string | null;
  isCurrent: boolean;
}

export interface WorktreeReport {
  isRepo: boolean;
  trees: WorktreeView[];
}

export type BranchMode = "create" | "existing";

/** Mirrors `worktrees::RemoveOutcome`. */
export type RemoveOutcome =
  | { status: "done" }
  | { status: "hasWork"; command: string }
  | { status: "needsApproval"; operation: Operation; command: string }
  | { status: "refused"; operation: Operation; reason: string }
  | { status: "failed"; message: string };

/** A removal stopped mid-way, waiting on the person. */
export interface PendingRemoval {
  projectId: string;
  tree: WorktreeView;
  command: string;
  /** True once the guardrail has asked; before that we are only reporting
   *  that Git found work in the tree. */
  guarded: boolean;
}

interface WorktreesState {
  report: Record<string, WorktreeReport | undefined>;
  loading: Record<string, boolean>;
  error: Record<string, string | null>;
  busy: Record<string, boolean>;
  pending: PendingRemoval | null;

  refresh: (projectId: string) => Promise<void>;
  add: (projectId: string, branch: string, mode: BranchMode) => Promise<Project | null>;
  remove: (
    projectId: string,
    tree: WorktreeView,
    force: boolean,
    choice?: Choice,
  ) => Promise<void>;
  cancelPending: () => void;
}

export const useWorktrees = create<WorktreesState>((set, get) => ({
  report: {},
  loading: {},
  error: {},
  busy: {},
  pending: null,

  refresh: async (projectId) => {
    if (!isTauri()) return;
    set((s) => ({ loading: { ...s.loading, [projectId]: true } }));
    try {
      const report = await invoke<WorktreeReport>("worktree_report", { projectId });
      set((s) => ({
        report: { ...s.report, [projectId]: report },
        loading: { ...s.loading, [projectId]: false },
        error: { ...s.error, [projectId]: null },
      }));
    } catch (cause) {
      set((s) => ({
        loading: { ...s.loading, [projectId]: false },
        error: { ...s.error, [projectId]: String(cause) },
      }));
    }
  },

  add: async (projectId, branch, mode) => {
    if (!isTauri()) return null;
    set((s) => ({ busy: { ...s.busy, [projectId]: true }, error: { ...s.error, [projectId]: null } }));
    try {
      const project = await invoke<Project>("worktree_create", { projectId, branch, mode });
      set((s) => ({ busy: { ...s.busy, [projectId]: false } }));
      await get().refresh(projectId);
      return project;
    } catch (cause) {
      // Git's own refusals land here, and they are the useful message: a
      // branch already checked out somewhere else is exactly what worktrees
      // exist to prevent, and saying so is better than any wording of ours.
      set((s) => ({
        busy: { ...s.busy, [projectId]: false },
        error: { ...s.error, [projectId]: String(cause) },
      }));
      return null;
    }
  },

  remove: async (projectId, tree, force, choice) => {
    if (!isTauri()) return;
    set((s) => ({ busy: { ...s.busy, [projectId]: true } }));
    try {
      const outcome = await invoke<RemoveOutcome>("worktree_remove", {
        projectId,
        path: tree.path,
        force,
        choice: choice ?? null,
      });
      set((s) => ({ busy: { ...s.busy, [projectId]: false } }));

      if (outcome.status === "hasWork" || outcome.status === "needsApproval") {
        set({
          pending: {
            projectId,
            tree,
            command: outcome.command,
            guarded: outcome.status === "needsApproval",
          },
        });
        return;
      }

      set({ pending: null });
      if (outcome.status === "refused") {
        set((s) => ({ error: { ...s.error, [projectId]: `refused:${outcome.reason}` } }));
        return;
      }
      if (outcome.status === "failed") {
        set((s) => ({ error: { ...s.error, [projectId]: outcome.message } }));
        return;
      }
      await get().refresh(projectId);
    } catch (cause) {
      set((s) => ({
        busy: { ...s.busy, [projectId]: false },
        pending: null,
        error: { ...s.error, [projectId]: String(cause) },
      }));
    }
  },

  cancelPending: () => set({ pending: null }),
}));
