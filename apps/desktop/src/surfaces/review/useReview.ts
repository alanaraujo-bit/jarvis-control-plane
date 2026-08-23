import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";
import type { Choice, Operation } from "../guardrails/useGuardrails";

export type ChangeKind =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "untracked"
  | "conflicted";

/** Mirrors `review::Attribution`. */
export interface Attribution {
  sessionId: string;
  provider: string;
  title: string | null;
  missionId: string | null;
  missionTitle: string | null;
  lastTsMs: number;
}

/** Mirrors `review::ReviewFile`. */
export interface ReviewFile {
  path: string;
  fromPath: string | null;
  kind: ChangeKind;
  /**
   * Both are sent because a file can be both at once (`MM` in porcelain):
   * part of the change staged, part of it not. The row has to be able to say
   * so, or the stage button misdescribes what it is about to do.
   */
  staged: boolean;
  unstaged: boolean;
  insertions: number;
  deletions: number;
  binary: boolean;
  tooLarge: boolean;
  sessions: Attribution[];
}

export interface ReviewReport {
  isRepo: boolean;
  hasCommits: boolean;
  branch: string | null;
  files: ReviewFile[];
}

export type LineKind = "context" | "added" | "removed";

export interface DiffLine {
  kind: LineKind;
  oldLine: number | null;
  newLine: number | null;
  text: string;
  noNewline: boolean;
}

export interface Hunk {
  oldStart: number;
  newStart: number;
  heading: string | null;
  lines: DiffLine[];
}

export interface FileDiff {
  path: string;
  fromPath: string | null;
  kind: ChangeKind;
  binary: boolean;
  tooLarge: boolean;
  insertions: number;
  deletions: number;
  hunks: Hunk[];
  truncated: boolean;
}

/** Mirrors `review::actions::GitAction`. */
export type GitAction = "stage" | "unstage" | "discard";

/**
 * Mirrors `review::actions::ActionOutcome`, which is internally tagged.
 *
 * `needsApproval` means **nothing happened**. The core recorded nothing and
 * changed nothing; it is waiting for the person to answer the §35 choices and
 * for the same call to be made again with their answer.
 */
export type ActionOutcome =
  | { status: "done" }
  | { status: "needsApproval"; operation: Operation; command: string }
  | { status: "refused"; operation: Operation; reason: string };

/** A discard the guardrail is holding until the person answers. */
export interface PendingDiscard {
  projectId: string;
  file: ReviewFile;
  operation: Operation;
  /** Verbatim, so what is approved is what runs. */
  command: string;
}

interface ReviewState {
  report: Record<string, ReviewReport | undefined>;
  loading: Record<string, boolean>;
  error: Record<string, string | null>;
  diffs: Record<string, FileDiff | undefined>;
  diffLoading: Record<string, boolean>;
  selected: Record<string, string | undefined>;
  /** At most one at a time: it is a question about the file in front of you. */
  confirming: PendingDiscard | null;
  /** A guardrail refused the last attempt, keyed by project. */
  refused: Record<string, string | undefined>;
  committing: Record<string, boolean>;

  refresh: (projectId: string) => Promise<void>;
  select: (projectId: string, file: ReviewFile) => Promise<void>;
  act: (
    projectId: string,
    action: GitAction,
    files: ReviewFile[],
    choice?: Choice,
  ) => Promise<void>;
  confirm: (choice: Choice) => Promise<void>;
  cancelConfirm: () => void;
  commit: (projectId: string, message: string) => Promise<boolean>;
}

const diffKey = (projectId: string, path: string) => `${projectId} ${path}`;

export const useReview = create<ReviewState>((set, get) => ({
  report: {},
  loading: {},
  error: {},
  diffs: {},
  diffLoading: {},
  selected: {},
  confirming: null,
  refused: {},
  committing: {},

  refresh: async (projectId) => {
    if (!isTauri()) return;
    set((state) => ({ loading: { ...state.loading, [projectId]: true } }));
    try {
      const report = await invoke<ReviewReport>("review_report", { projectId });
      set((state) => ({
        report: { ...state.report, [projectId]: report },
        loading: { ...state.loading, [projectId]: false },
        error: { ...state.error, [projectId]: null },
        // Every diff in *this* project is now potentially stale. Dropping them
        // is cheaper than reasoning about which files moved, and a review
        // surface showing a diff that no longer matches the file is the one
        // thing it must never do. Other projects' diffs are left alone —
        // refreshing one project says nothing about another.
        diffs: Object.fromEntries(
          Object.entries(state.diffs).filter(([id]) => !id.startsWith(`${projectId} `)),
        ),
      }));

      // Land on something rather than an empty pane: the first row is the
      // most recently agent-touched file, which is what this surface is for.
      const first = report.files[0];
      const current = get().selected[projectId];
      const stillThere = report.files.some((f) => f.path === current);
      if (first && !stillThere) await get().select(projectId, first);
      else if (current && stillThere) {
        const file = report.files.find((f) => f.path === current);
        if (file) await get().select(projectId, file);
      }
    } catch (cause) {
      set((state) => ({
        loading: { ...state.loading, [projectId]: false },
        error: { ...state.error, [projectId]: String(cause) },
      }));
    }
  },

  select: async (projectId, file) => {
    const id = diffKey(projectId, file.path);
    set((state) => ({ selected: { ...state.selected, [projectId]: file.path } }));
    if (get().diffs[id] || get().diffLoading[id] || !isTauri()) return;

    set((state) => ({ diffLoading: { ...state.diffLoading, [id]: true } }));
    try {
      const diff = await invoke<FileDiff>("review_file_diff", {
        projectId,
        path: file.path,
        // The core needs to know whether Git has ever seen this file: an
        // untracked one has no `HEAD` side to diff against.
        kind: file.kind,
        // And a renamed file has to carry its old name, or Git cannot pair the
        // two sides and reports the move as a file created from nothing.
        fromPath: file.fromPath,
      });
      set((state) => ({
        diffs: { ...state.diffs, [id]: diff },
        diffLoading: { ...state.diffLoading, [id]: false },
      }));
    } catch (cause) {
      set((state) => ({
        diffLoading: { ...state.diffLoading, [id]: false },
        error: { ...state.error, [projectId]: String(cause) },
      }));
    }
  },
  /**
   * Perform a Git action (§44).
   *
   * `choice` is the person's answer to a guardrail question, passed straight
   * back to the core — which resolves policy again and decides for itself. The
   * webview never carries permission, only an answer.
   */
  act: async (projectId, action, files, choice) => {
    if (!isTauri() || files.length === 0) return;
    set((state) => ({ refused: { ...state.refused, [projectId]: undefined } }));

    try {
      const outcome = await invoke<ActionOutcome>("review_git_action", {
        projectId,
        action,
        targets: files.map((file) => ({
          path: file.path,
          // A rename is one change with two names and both have to travel, or
          // discarding it leaves the original deleted.
          fromPath: file.fromPath,
          kind: file.kind,
        })),
        choice: choice ?? null,
      });

      if (outcome.status === "needsApproval") {
        // Nothing has happened yet. Ask, then call back with the answer.
        set({
          confirming: {
            projectId,
            file: files[0],
            operation: outcome.operation,
            command: outcome.command,
          },
        });
        return;
      }

      set({ confirming: null });
      if (outcome.status === "refused") {
        set((state) => ({
          refused: { ...state.refused, [projectId]: outcome.reason },
        }));
        return;
      }

      // The working tree moved, so everything read from it is stale.
      await get().refresh(projectId);
    } catch (cause) {
      set((state) => ({
        confirming: null,
        error: { ...state.error, [projectId]: String(cause) },
      }));
    }
  },

  confirm: async (choice) => {
    const pending = get().confirming;
    if (!pending) return;
    await get().act(pending.projectId, "discard", [pending.file], choice);
  },

  cancelConfirm: () => set({ confirming: null }),

  commit: async (projectId, message) => {
    if (!isTauri()) return false;
    set((state) => ({ committing: { ...state.committing, [projectId]: true } }));
    try {
      await invoke("review_commit", { projectId, message });
      set((state) => ({
        committing: { ...state.committing, [projectId]: false },
        error: { ...state.error, [projectId]: null },
      }));
      await get().refresh(projectId);
      return true;
    } catch (cause) {
      // A failing `pre-commit` hook lands here, and its text is the useful
      // part — it is the user's own hook telling them what is wrong.
      set((state) => ({
        committing: { ...state.committing, [projectId]: false },
        error: { ...state.error, [projectId]: String(cause) },
      }));
      return false;
    }
  },
}));

export const reviewDiffKey = diffKey;
