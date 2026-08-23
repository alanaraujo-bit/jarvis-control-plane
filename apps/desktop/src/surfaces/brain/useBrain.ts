import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

/** Mirrors `brain::Kind`. The ids are also the i18n key suffixes. */
export type Kind = "what" | "convention" | "gotcha" | "glossary";

/** In the order a newcomer needs them: what it is, then how, then what hurts. */
export const KINDS: Kind[] = ["what", "convention", "gotcha", "glossary"];

/** Mirrors `brain::Source`. Kept because it changes how a claim is weighed. */
export type Source = "human" | "agent";

export interface Knowledge {
  id: string;
  projectId: string;
  kind: Kind;
  body: string;
  source: Source;
  sessionId: string | null;
  missionId: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface Note {
  id: string;
  projectId: string;
  body: string;
  pinned: boolean;
  missionId: string | null;
  sessionId: string | null;
  createdAt: number;
  updatedAt: number;
}

/**
 * Mirrors `brain::derived::Fact`.
 *
 * `derived` is the field that matters. It is carried rather than inferred from
 * where the value happens to be rendered, so a fact cannot end up displayed as
 * something a person stated (§28).
 */
export interface Fact {
  code: string;
  args: string[];
  derived: boolean;
  /**
   * The number the sentence has to agree with.
   *
   * Sent separately from `args` so the interface never has to guess which
   * argument decides the plural — guessing rendered "1 sessões".
   */
  count: number;
}

export interface Moment {
  tsMs: number;
  kind: string;
  title: string;
  severity: string;
}

export interface BrainReport {
  knowledge: Knowledge[];
  notes: Note[];
  facts: Fact[];
  timeline: Moment[];
  /** How much an agent is told, so the cost is visible rather than a surprise. */
  briefSize: number;
}

interface BrainState {
  report: Record<string, BrainReport | undefined>;
  loading: Record<string, boolean>;
  error: Record<string, string | null>;
  /**
   * The exact text an agent gets, once the person has asked to see it.
   *
   * `undefined` means "not asked for yet"; `null` means "asked, and there is
   * no briefing" — which is a real answer and reads very differently.
   */
  brief: Record<string, string | null | undefined>;

  refresh: (projectId: string) => Promise<void>;
  addKnowledge: (projectId: string, kind: Kind, body: string) => Promise<void>;
  updateKnowledge: (projectId: string, id: string, kind: Kind, body: string) => Promise<void>;
  archiveKnowledge: (projectId: string, id: string) => Promise<void>;
  addNote: (projectId: string, body: string) => Promise<void>;
  updateNote: (projectId: string, id: string, body: string) => Promise<void>;
  setNotePinned: (projectId: string, id: string, pinned: boolean) => Promise<void>;
  deleteNote: (projectId: string, id: string) => Promise<void>;
  promoteNote: (projectId: string, id: string, kind: Kind) => Promise<void>;
  loadBrief: (projectId: string) => Promise<void>;
}

export const useBrain = create<BrainState>((set) => {
  /**
   * Run a command that returns the whole report, and take what it says.
   *
   * The core returns the full report from every mutation rather than the row
   * that changed, and this keeps it that way. Promoting a note moves it between
   * two lists; archiving an entry changes what an agent would be told. Patching
   * that in the webview would be a second implementation of rules that already
   * exist in Rust, free to disagree with them.
   */
  const apply = async (projectId: string, command: string, args: Record<string, unknown>) => {
    if (!isTauri()) return;
    try {
      const report = await invoke<BrainReport>(command, { projectId, ...args });
      set((s) => ({
        report: { ...s.report, [projectId]: report },
        error: { ...s.error, [projectId]: null },
        // A briefing on screen must not outlive the knowledge behind it, so
        // anything that touches knowledge drops it back to "not asked for".
        brief: { ...s.brief, [projectId]: undefined },
      }));
    } catch (cause) {
      set((s) => ({ error: { ...s.error, [projectId]: String(cause) } }));
    }
  };

  return {
    report: {},
    loading: {},
    error: {},
    brief: {},

    refresh: async (projectId) => {
      if (!isTauri()) return;
      set((s) => ({ loading: { ...s.loading, [projectId]: true } }));
      try {
        const report = await invoke<BrainReport>("brain_report", { projectId });
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

    addKnowledge: (projectId, kind, body) =>
      apply(projectId, "brain_add_knowledge", { kind, body }),

    updateKnowledge: (projectId, id, kind, body) =>
      apply(projectId, "brain_update_knowledge", { id, kind, body }),

    archiveKnowledge: (projectId, id) =>
      apply(projectId, "brain_archive_knowledge", { id }),

    addNote: (projectId, body) => apply(projectId, "brain_add_note", { body }),

    updateNote: (projectId, id, body) => apply(projectId, "brain_update_note", { id, body }),

    setNotePinned: (projectId, id, pinned) =>
      apply(projectId, "brain_set_note_pinned", { id, pinned }),

    deleteNote: (projectId, id) => apply(projectId, "brain_delete_note", { id }),

    promoteNote: (projectId, id, kind) =>
      apply(projectId, "brain_promote_note", { id, kind }),

    loadBrief: async (projectId) => {
      if (!isTauri()) return;
      try {
        const brief = await invoke<string | null>("brain_preview_brief", { projectId });
        set((s) => ({ brief: { ...s.brief, [projectId]: brief } }));
      } catch (cause) {
        set((s) => ({ error: { ...s.error, [projectId]: String(cause) } }));
      }
    },
  };
});
