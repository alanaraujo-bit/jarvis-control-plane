import { create } from "zustand";
import { invoke, isTauri } from "./platform";

export type WorkspaceArea =
  | "sessions"
  | "files"
  | "review"
  | "preview"
  | "worktrees"
  | "brain"
  | "settings";
export type WorkspaceView = "terminal" | "conversation";
export type WorkspaceSplit = "columns" | "rows" | "grid";

export interface ProjectWorkspaceState {
  area: WorkspaceArea;
  view: WorkspaceView;
  activeSessionId?: string;
  sessionOrder: string[];
  paneSessionIds: string[];
  splitDirection: WorkspaceSplit;
}

interface WorkspaceSnapshot {
  openProjectIds: string[];
  activeProjectId: string | null;
  projects: Record<string, ProjectWorkspaceState>;
}

interface WorkspaceState extends WorkspaceSnapshot {
  hydrated: boolean;
  load: () => Promise<void>;
  openProject: (projectId: string) => void;
  showGlobal: () => void;
  closeProject: (projectId: string) => void;
  setArea: (projectId: string, area: WorkspaceArea) => void;
  setView: (projectId: string, view: WorkspaceView) => void;
  captureSessions: (
    projectId: string,
    sessionOrder: string[],
    activeSessionId: string | undefined,
    paneSessionIds: string[],
    splitDirection: WorkspaceSplit,
  ) => void;
}

const emptyProject = (): ProjectWorkspaceState => ({
  area: "sessions",
  view: "terminal",
  activeSessionId: undefined,
  sessionOrder: [],
  paneSessionIds: [],
  splitDirection: "columns",
});

let saveTimer: ReturnType<typeof setTimeout> | undefined;

function durable(state: WorkspaceState): WorkspaceSnapshot {
  return {
    openProjectIds: state.openProjectIds,
    activeProjectId: state.activeProjectId,
    projects: state.projects,
  };
}

function scheduleSave(get: () => WorkspaceState): void {
  if (!isTauri() || !get().hydrated) return;
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    saveTimer = undefined;
    void invoke<WorkspaceSnapshot>("workspace_save", { snapshot: durable(get()) }).catch(() => {
      // Persistence failure must not interrupt a running terminal. The next
      // state transition retries with a complete snapshot.
    });
  }, 180);
}

export const useWorkspace = create<WorkspaceState>((set, get) => ({
  hydrated: false,
  openProjectIds: [],
  activeProjectId: null,
  projects: {},

  load: async () => {
    if (!isTauri()) {
      set({ hydrated: true });
      return;
    }
    try {
      const snapshot = await invoke<WorkspaceSnapshot>("workspace_snapshot");
      set({ ...snapshot, hydrated: true });
    } catch {
      set({ hydrated: true });
    }
  },

  openProject: (projectId) => {
    set((state) => ({
      openProjectIds: state.openProjectIds.includes(projectId)
        ? state.openProjectIds
        : [...state.openProjectIds, projectId],
      activeProjectId: projectId,
      projects: state.projects[projectId]
        ? state.projects
        : { ...state.projects, [projectId]: emptyProject() },
    }));
    scheduleSave(get);
  },

  showGlobal: () => {
    set({ activeProjectId: null });
    scheduleSave(get);
  },

  closeProject: (projectId) => {
    set((state) => {
      const openProjectIds = state.openProjectIds.filter((id) => id !== projectId);
      const projects = { ...state.projects };
      delete projects[projectId];
      const activeProjectId =
        state.activeProjectId === projectId
          ? (openProjectIds.at(-1) ?? null)
          : state.activeProjectId;
      return { openProjectIds, activeProjectId, projects };
    });
    scheduleSave(get);
  },

  setArea: (projectId, area) => {
    set((state) => ({
      projects: {
        ...state.projects,
        [projectId]: { ...(state.projects[projectId] ?? emptyProject()), area },
      },
    }));
    scheduleSave(get);
  },

  setView: (projectId, view) => {
    set((state) => ({
      projects: {
        ...state.projects,
        [projectId]: { ...(state.projects[projectId] ?? emptyProject()), view },
      },
    }));
    scheduleSave(get);
  },

  captureSessions: (projectId, sessionOrder, activeSessionId, paneSessionIds, splitDirection) => {
    const previous = get().projects[projectId];
    if (
      previous &&
      previous.activeSessionId === activeSessionId &&
      previous.splitDirection === splitDirection &&
      previous.sessionOrder.join("\0") === sessionOrder.join("\0") &&
      previous.paneSessionIds.join("\0") === paneSessionIds.join("\0")
    ) {
      return;
    }
    set((state) => ({
      projects: {
        ...state.projects,
        [projectId]: {
          ...(state.projects[projectId] ?? emptyProject()),
          activeSessionId,
          sessionOrder,
          paneSessionIds,
          splitDirection,
        },
      },
    }));
    scheduleSave(get);
  },
}));

export function projectWorkspace(projectId: string): ProjectWorkspaceState {
  return useWorkspace.getState().projects[projectId] ?? emptyProject();
}
