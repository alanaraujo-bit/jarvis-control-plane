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

interface TerminalsState {
  /** Tabs per project. Sessions outlive navigating away (§32). */
  tabs: Record<string, TerminalTab[]>;
  activeTab: Record<string, string | undefined>;
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
        return {
          tabs: { ...state.tabs, [projectId]: [...existing, tab] },
          activeTab: { ...state.activeTab, [projectId]: info.id },
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
      return {
        tabs: { ...state.tabs, [projectId]: remaining },
        activeTab: {
          ...state.activeTab,
          [projectId]: wasActive ? remaining.at(-1)?.sessionId : state.activeTab[projectId],
        },
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
    set((state) => ({ activeTab: { ...state.activeTab, [projectId]: sessionId } })),

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
}));
