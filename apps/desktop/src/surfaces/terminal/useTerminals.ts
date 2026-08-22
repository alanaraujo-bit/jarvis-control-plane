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
  ) => Promise<void>;
  closeTerminal: (projectId: string, sessionId: string) => Promise<void>;
  setActive: (projectId: string, sessionId: string) => void;
  adopt: (projectId: string, sessions: SessionInfo[]) => void;
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

  openTerminal: async (projectId, kind, size) => {
    if (get().starting[projectId]) return;
    set((state) => ({ starting: { ...state.starting, [projectId]: true }, error: null }));

    try {
      const info = await startSession({
        projectId,
        kind,
        cols: size.cols,
        rows: size.rows,
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
}));
