import { create } from "zustand";
import { invoke, isTauri } from "../../app/platform";

/** Mirrors `notebook::Notebook`. */
export interface NotebookFolder {
  id: string;
  name: string;
  position: number;
  createdAt: number;
  updatedAt: number;
}

/** Mirrors `notebook::Note`. `notebookId === null` is **unfiled**. */
export interface NotebookNote {
  id: string;
  notebookId: string | null;
  title: string;
  body: string;
  pinned: boolean;
  createdAt: number;
  updatedAt: number;
}

export interface NotebookReport {
  notebooks: NotebookFolder[];
  notes: NotebookNote[];
}

/**
 * How long after the last keystroke the note is written.
 *
 * There is no Save button, so this number is the whole contract. Long enough
 * that typing a paragraph is one write rather than forty; short enough that
 * anyone glancing away and back finds their words already kept. Every exit
 * flushes regardless — see `flush`.
 */
const AUTOSAVE_MS = 500;

interface NotebookState {
  report: NotebookReport | null;
  loading: boolean;
  error: string | null;
  /** The note being edited, and the text as the person has it right now. */
  selectedId: string | null;
  draftTitle: string;
  draftBody: string;
  /** True between a keystroke and the write that follows it. */
  dirty: boolean;

  load: () => Promise<void>;
  select: (id: string | null) => Promise<void>;
  edit: (title: string, body: string) => void;
  /**
   * Write the pending draft now, and wait for it.
   *
   * Called before anything that could lose it: closing the overlay, switching
   * notes, deleting. A debounce with no flush is how a notes app drops the last
   * sentence somebody typed before pressing Escape — which is worse than the
   * Save button it replaced.
   */
  flush: () => Promise<void>;

  createNote: (notebookId: string | null) => Promise<void>;
  duplicateNote: (id: string) => Promise<void>;
  deleteNote: (id: string) => Promise<void>;
  pinNote: (id: string, pinned: boolean) => Promise<void>;
  moveNote: (id: string, notebookId: string | null) => Promise<void>;

  createNotebook: (name: string) => Promise<void>;
  renameNotebook: (id: string, name: string) => Promise<void>;
  deleteNotebook: (id: string) => Promise<void>;
}

/**
 * The Notebook's own state (M19).
 *
 * `zustand`, matching `useBrain` and `useAccounts`: this has real async CRUD
 * and several components read it. The tiny module store `usePreferences` uses
 * is right for three numbers and wrong for this.
 *
 * **The draft lives here rather than in the editor component.** The overlay can
 * be closed from three places — Escape, the close button, the backdrop — and a
 * draft held in component state would be gone before any of them could save it.
 */
let timer: number | null = null;

export const useNotebook = create<NotebookState>((set, get) => {
  /** Apply a report from the core; every mutation command returns one. */
  const apply = (report: NotebookReport) => set({ report, error: null });

  const write = async () => {
    const { selectedId, draftTitle, draftBody, dirty } = get();
    if (!dirty || !selectedId || !isTauri()) return;
    // Cleared *before* the await: a keystroke landing mid-write must mark the
    // note dirty again rather than have this write clear its flag afterwards.
    set({ dirty: false });
    try {
      apply(
        await invoke<NotebookReport>("notebook_note_update", {
          id: selectedId,
          title: draftTitle,
          body: draftBody,
        }),
      );
    } catch (cause) {
      // Put the flag back: the words are still only in the webview, and the
      // next flush is the last chance to keep them.
      set({ dirty: true, error: String(cause) });
    }
  };

  return {
    report: null,
    loading: false,
    error: null,
    selectedId: null,
    draftTitle: "",
    draftBody: "",
    dirty: false,

    load: async () => {
      if (!isTauri()) return;
      set({ loading: true });
      try {
        apply(await invoke<NotebookReport>("notebook_report"));
      } catch (cause) {
        set({ error: String(cause) });
      } finally {
        set({ loading: false });
      }
    },

    select: async (id) => {
      if (id === get().selectedId) return;
      await get().flush();
      const note = id ? (get().report?.notes.find((n) => n.id === id) ?? null) : null;
      set({
        selectedId: note?.id ?? null,
        draftTitle: note?.title ?? "",
        draftBody: note?.body ?? "",
        dirty: false,
      });
    },

    edit: (title, body) => {
      set({ draftTitle: title, draftBody: body, dirty: true });
      if (timer !== null) window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        timer = null;
        void write();
      }, AUTOSAVE_MS);
    },

    flush: async () => {
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
      await write();
    },

    createNote: async (notebookId) => {
      await get().flush();
      if (!isTauri()) return;
      try {
        const created = await invoke<{ id: string; report: NotebookReport }>(
          "notebook_note_create",
          { notebookId },
        );
        // The id comes back from the core rather than being guessed at as
        // "the newest row" — two notes created in the same millisecond would
        // make that the wrong one, and the cursor would land in a stranger.
        apply(created.report);
        set({ selectedId: created.id, draftTitle: "", draftBody: "", dirty: false });
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    duplicateNote: async (id) => {
      await get().flush();
      if (!isTauri()) return;
      try {
        const created = await invoke<{ id: string; report: NotebookReport }>(
          "notebook_note_duplicate",
          { id },
        );
        apply(created.report);
        const copy = created.report.notes.find((n) => n.id === created.id);
        set({
          selectedId: created.id,
          draftTitle: copy?.title ?? "",
          draftBody: copy?.body ?? "",
          dirty: false,
        });
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    deleteNote: async (id) => {
      if (!isTauri()) return;
      // Deliberately *not* flushed first: the pending draft belongs to a note
      // about to stop existing, and writing it would be a round trip whose only
      // effect is to make the delete race it.
      if (timer !== null) {
        window.clearTimeout(timer);
        timer = null;
      }
      try {
        const report = await invoke<NotebookReport>("notebook_note_delete", { id });
        apply(report);
        if (get().selectedId === id) {
          set({ selectedId: null, draftTitle: "", draftBody: "", dirty: false });
        }
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    pinNote: async (id, pinned) => {
      if (!isTauri()) return;
      try {
        apply(await invoke<NotebookReport>("notebook_note_pin", { id, pinned }));
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    moveNote: async (id, notebookId) => {
      if (!isTauri()) return;
      try {
        apply(await invoke<NotebookReport>("notebook_note_move", { id, notebookId }));
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    createNotebook: async (name) => {
      if (!isTauri()) return;
      try {
        apply(await invoke<NotebookReport>("notebook_create", { name }));
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    renameNotebook: async (id, name) => {
      if (!isTauri()) return;
      try {
        apply(await invoke<NotebookReport>("notebook_rename", { id, name }));
      } catch (cause) {
        set({ error: String(cause) });
      }
    },

    deleteNotebook: async (id) => {
      if (!isTauri()) return;
      try {
        apply(await invoke<NotebookReport>("notebook_delete", { id }));
      } catch (cause) {
        set({ error: String(cause) });
      }
    },
  };
});

/**
 * What a note is called in a list.
 *
 * The title when there is one, the body's first non-empty line when there is
 * not, and a translated placeholder only when there is genuinely nothing. One
 * derivation in one place — a list and an editor disagreeing about what a note
 * is called is the kind of small wrongness that reads as unfinished.
 */
export function noteName(note: NotebookNote, untitled: string): string {
  const title = note.title.trim();
  if (title) return title;
  const firstLine = note.body.split("\n").find((line) => line.trim().length > 0);
  return firstLine?.trim() ?? untitled;
}
