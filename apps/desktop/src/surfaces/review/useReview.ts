import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

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

interface ReviewState {
  report: Record<string, ReviewReport | undefined>;
  loading: Record<string, boolean>;
  error: Record<string, string | null>;
  diffs: Record<string, FileDiff | undefined>;
  diffLoading: Record<string, boolean>;
  selected: Record<string, string | undefined>;

  refresh: (projectId: string) => Promise<void>;
  select: (projectId: string, file: ReviewFile) => Promise<void>;
}

const diffKey = (projectId: string, path: string) => `${projectId} ${path}`;

export const useReview = create<ReviewState>((set, get) => ({
  report: {},
  loading: {},
  error: {},
  diffs: {},
  diffLoading: {},
  selected: {},

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
}));

export const reviewDiffKey = diffKey;
