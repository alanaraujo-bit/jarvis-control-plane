import { create } from "zustand";
import {
  closeSession,
  startSession,
  type SessionInfo,
  type SessionKind,
} from "../../app/sessions";

export interface TerminalTab {
  sessionId: string;
  kind: SessionKind;
  title: string;
  /** Opened by Global Search (§51) against a session this window never
   * started: no PTY to attach, closing it drives no backend call, and the
   * surface renders it as conversation-only, read-only. */
  historical?: boolean;
}

/**
 * How several terminals share the screen (§20).
 *
 * `columns` and `rows` say which way the panes are laid out; `grid` is two
 * across and as many down as it takes. They are presets rather than a
 * resizable tree on purpose — see the note on `MAX_SLOTS` below.
 */
export type SplitDirection = "columns" | "rows" | "grid";

/**
 * How many terminals may share the screen at once.
 *
 * Four, and it is a real limit rather than a shrug: at this window size a
 * fifth pane is about forty columns wide, which is narrower than the prompt
 * most agent CLIs draw. A terminal too small to read is not a feature.
 */
export const MAX_SLOTS = 4;

interface TerminalsState {
  /** Tabs per project. Sessions outlive navigating away (§32). */
  tabs: Record<string, TerminalTab[]>;
  activeTab: Record<string, string | undefined>;
  /**
   * The sessions on screen together, per project, in layout order.
   *
   * Empty or absent means "just the active tab", which is the ordinary case
   * and costs no state. A session id appears here only while it is also a
   * tab; closing a tab removes it from the layout too.
   */
  slots: Record<string, string[]>;
  direction: Record<string, SplitDirection>;
  starting: Record<string, boolean>;
  error: string | null;

  openTerminal: (
    projectId: string,
    kind: SessionKind,
    size: { cols: number; rows: number },
    missionId?: string,
  ) => Promise<void>;
  closeTerminal: (projectId: string, sessionId: string) => Promise<void>;
  setActive: (projectId: string, sessionId: string) => void;
  adopt: (projectId: string, sessions: SessionInfo[]) => void;
  /** Open a past session read-only, found through Global Search (§51). Never
   * calls `startSession` — the session already ran, sometimes long ago — and
   * re-activates the existing tab rather than duplicating it. */
  openHistorical: (
    projectId: string,
    sessionId: string,
    kind: SessionKind,
    title?: string,
  ) => void;

  /** Put a session on screen beside the ones already there (§20). */
  addToSplit: (projectId: string, sessionId: string) => void;
  /** Take a session out of the split. The last one standing ends the split. */
  removeFromSplit: (projectId: string, sessionId: string) => void;
  /** Collapse back to a single terminal, keeping the active one. */
  clearSplit: (projectId: string) => void;
  setDirection: (projectId: string, direction: SplitDirection) => void;
}

const TITLES: Record<SessionKind, string> = {
  shell: "Shell",
  "claude-code": "Claude Code",
  codex: "Codex",
};

/** Number tabs of the same kind so several shells stay distinguishable. */
function nextTitle(existing: TerminalTab[], kind: SessionKind): string {
  const base = TITLES[kind];
  const used = existing.filter((tab) => tab.kind === kind).length;
  return used === 0 ? base : `${base} ${used + 1}`;
}

export const useTerminals = create<TerminalsState>((set, get) => ({
  tabs: {},
  activeTab: {},
  slots: {},
  direction: {},
  starting: {},
  error: null,

  openTerminal: async (projectId, kind, size, missionId) => {
    if (get().starting[projectId]) return;
    set((state) => ({ starting: { ...state.starting, [projectId]: true }, error: null }));

    try {
      const info = await startSession({
        projectId,
        kind,
        cols: size.cols,
        rows: size.rows,
        missionId,
      });
      set((state) => {
        const existing = state.tabs[projectId] ?? [];
        const tab: TerminalTab = {
          sessionId: info.id,
          kind,
          title: nextTitle(existing, kind),
        };
        // A new terminal opened *during* a split joins the layout, while
        // there is room for it.
        //
        // Found by running it: without this the new session became the active
        // tab but was not in the layout, so it was invisible — and being
        // active it also claimed the keyboard, so everything typed next went
        // to whichever pane had really been focused. Two shells were opened
        // and their commands landed in a third. A terminal that is focused
        // and cannot be seen is the worst of both states, so the layout
        // follows the new session rather than the session hiding behind the
        // layout.
        //
        // With the split already full it **takes the focused pane's place**
        // rather than collapsing the layout: those panes were arranged
        // deliberately, and dismantling the arrangement is a bigger loss than
        // changing what one pane shows. The displaced session is still a tab
        // and still running.
        const current = state.slots[projectId] ?? [];
        let slots = state.slots;
        if (current.length > 1) {
          if (current.length < MAX_SLOTS) {
            slots = { ...state.slots, [projectId]: [...current, info.id] };
          } else {
            const active = state.activeTab[projectId];
            const at = active ? current.indexOf(active) : -1;
            const replaced = [...current];
            replaced[at >= 0 ? at : replaced.length - 1] = info.id;
            slots = { ...state.slots, [projectId]: replaced };
          }
        }
        return {
          tabs: { ...state.tabs, [projectId]: [...existing, tab] },
          activeTab: { ...state.activeTab, [projectId]: info.id },
          slots,
          starting: { ...state.starting, [projectId]: false },
        };
      });
    } catch (cause) {
      set((state) => ({
        starting: { ...state.starting, [projectId]: false },
        error: String(cause),
      }));
    }
  },

  closeTerminal: async (projectId, sessionId) => {
    const tab = (get().tabs[projectId] ?? []).find((t) => t.sessionId === sessionId);
    // Remove the tab first: the view must not keep rendering a dying session.
    set((state) => {
      const remaining = (state.tabs[projectId] ?? []).filter((t) => t.sessionId !== sessionId);
      const wasActive = state.activeTab[projectId] === sessionId;
      // A closed session must leave the layout in the same breath. Left
      // behind, the split would keep a slot open for a terminal that no
      // longer exists — a blank rectangle holding real estate.
      const slots = (state.slots[projectId] ?? []).filter((id) => id !== sessionId);
      return {
        tabs: { ...state.tabs, [projectId]: remaining },
        activeTab: {
          ...state.activeTab,
          [projectId]: wasActive ? remaining.at(-1)?.sessionId : state.activeTab[projectId],
        },
        // One pane is not a split. Dropping to a single id collapses it, so
        // there is no state where the layout claims to be split and is not.
        slots: { ...state.slots, [projectId]: slots.length > 1 ? slots : [] },
      };
    });
    // A historical tab was never started here — nothing was ever attached,
    // and this session may have ended long before this window opened it.
    if (tab?.historical) return;
    await closeSession(sessionId).catch(() => {
      // Already gone; nothing left to do.
    });
  },

  setActive: (projectId, sessionId) =>
    set((state) => {
      const current = state.slots[projectId] ?? [];
      // The same trap as opening a terminal during a split, one gesture over:
      // clicking a tab that is not in the layout would focus a session nobody
      // can see, and every keystroke after it would land somewhere invisible.
      // Selecting a tab means "show me this one", so it takes the focused
      // pane's place — the layout keeps its shape and the click does what it
      // looks like it does.
      if (current.length > 1 && !current.includes(sessionId)) {
        const active = state.activeTab[projectId];
        const at = active ? current.indexOf(active) : -1;
        const replaced = [...current];
        replaced[at >= 0 ? at : 0] = sessionId;
        return {
          slots: { ...state.slots, [projectId]: replaced },
          activeTab: { ...state.activeTab, [projectId]: sessionId },
        };
      }
      return { activeTab: { ...state.activeTab, [projectId]: sessionId } };
    }),

  /** Rebuild tabs for sessions the core reports as still running. */
  adopt: (projectId, sessions) =>
    set((state) => {
      if (state.tabs[projectId]?.length) return state;
      const tabs = sessions
        .filter((s) => s.live)
        .map<TerminalTab>((s, index) => ({
          sessionId: s.id,
          kind: (s.provider as SessionKind) ?? "shell",
          title: s.title ?? `${TITLES[(s.provider as SessionKind) ?? "shell"]}${index ? ` ${index + 1}` : ""}`,
        }));
      if (tabs.length === 0) return state;
      return {
        tabs: { ...state.tabs, [projectId]: tabs },
        activeTab: { ...state.activeTab, [projectId]: tabs[0].sessionId },
      };
    }),

  openHistorical: (projectId, sessionId, kind, title) =>
    set((state) => {
      const existing = state.tabs[projectId] ?? [];
      if (existing.some((t) => t.sessionId === sessionId)) {
        return { activeTab: { ...state.activeTab, [projectId]: sessionId } };
      }
      const tab: TerminalTab = {
        sessionId,
        kind,
        title: title ?? nextTitle(existing, kind),
        historical: true,
      };
      return {
        tabs: { ...state.tabs, [projectId]: [...existing, tab] },
        activeTab: { ...state.activeTab, [projectId]: sessionId },
      };
    }),

  addToSplit: (projectId, sessionId) =>
    set((state) => {
      const current = state.slots[projectId] ?? [];
      const active = state.activeTab[projectId];
      // The first split is "this one *and* that one": an empty layout means
      // the active terminal is on screen alone, so it has to be carried in
      // explicitly or adding a second pane would drop the first.
      const base = current.length > 0 ? current : active ? [active] : [];
      if (base.includes(sessionId) || base.length >= MAX_SLOTS) {
        // Already showing, or full. Focus it instead of silently doing
        // nothing — the intent was "show me this one".
        return { activeTab: { ...state.activeTab, [projectId]: sessionId } };
      }
      return {
        slots: { ...state.slots, [projectId]: [...base, sessionId] },
        activeTab: { ...state.activeTab, [projectId]: sessionId },
      };
    }),

  removeFromSplit: (projectId, sessionId) =>
    set((state) => {
      const remaining = (state.slots[projectId] ?? []).filter((id) => id !== sessionId);
      const wasActive = state.activeTab[projectId] === sessionId;
      return {
        slots: { ...state.slots, [projectId]: remaining.length > 1 ? remaining : [] },
        // The session is still a tab — only its pane is gone. Move focus to
        // something still on screen rather than leaving it on a hidden tab.
        activeTab: {
          ...state.activeTab,
          [projectId]: wasActive ? (remaining[0] ?? state.activeTab[projectId]) : state.activeTab[projectId],
        },
      };
    }),

  clearSplit: (projectId) =>
    set((state) => ({ slots: { ...state.slots, [projectId]: [] } })),

  setDirection: (projectId, direction) =>
    set((state) => ({ direction: { ...state.direction, [projectId]: direction } })),
}));
