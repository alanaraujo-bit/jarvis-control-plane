import { create } from "zustand";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { invoke, isTauri } from "../../app/platform";

export interface Project {
  id: string;
  name: string;
  path: string;
  isGit: boolean;
  gitBranch: string | null;
  gitRemote: string | null;
  createdAt: number;
  lastOpenedAt: number;
  archived: boolean;
  /** False when the folder has been moved or deleted since it was added. */
  exists: boolean;
  /**
   * The project this is a worktree of (§45). A worktree is registered as its
   * own project, so without this it is a folder in the list with no
   * explanation of where it came from.
   */
  worktreeOf: string | null;
}

interface ProjectsState {
  projects: Project[];
  loading: boolean;
  error: string | null;
  refresh: () => Promise<void>;
  /** Show the folder picker and register whatever the user chooses. */
  openFolder: () => Promise<Project | null>;
  archive: (id: string) => Promise<void>;
}

export const useProjects = create<ProjectsState>((set, get) => ({
  projects: [],
  loading: false,
  error: null,

  refresh: async () => {
    if (!isTauri()) return;
    set({ loading: true, error: null });
    try {
      set({ projects: await invoke<Project[]>("list_projects"), loading: false });
    } catch (cause) {
      set({ loading: false, error: String(cause) });
    }
  },

  openFolder: async () => {
    if (!isTauri()) return null;
    const selected = await openDialog({ directory: true, multiple: false });
    // The user cancelling is not an error — say nothing and change nothing.
    if (typeof selected !== "string") return null;

    try {
      const project = await invoke<Project>("open_project", { path: selected });
      await get().refresh();
      return project;
    } catch (cause) {
      set({ error: String(cause) });
      return null;
    }
  },

  archive: async (id: string) => {
    await invoke("archive_project", { id });
    await get().refresh();
  },
}));
