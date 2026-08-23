import { useCallback, useEffect, useState } from "react";
import { invoke, isTauri } from "../../app/platform";

export type Autonomy = "guided" | "autonomous" | "unattended";

/**
 * The §33 chain as the core resolves it.
 *
 * `global` and `project` are nullable because "nothing chosen" is a real,
 * distinct state — a project set to Inherit is not the same as a project that
 * happens to have picked whatever the global default currently is, and the
 * difference shows the moment the global default changes.
 */
export interface AutonomyChain {
  global: Autonomy | null;
  project: Autonomy | null;
  /** What a mission with no setting of its own would run at. */
  effective: Autonomy;
}

interface UseAutonomy {
  chain: AutonomyChain | null;
  error: string | null;
  setGlobal: (autonomy: Autonomy | null) => Promise<void>;
  setProject: (autonomy: Autonomy | null) => Promise<void>;
}

/**
 * Read and change the autonomy chain, optionally scoped to one project.
 *
 * Every setter returns the recomputed chain from the core rather than patching
 * the local copy: the resolution rule lives in `resolve_autonomy` and there is
 * no second copy of it here to drift.
 */
export function useAutonomy(projectId?: string): UseAutonomy {
  const [chain, setChain] = useState<AutonomyChain | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!isTauri()) return;
    try {
      setChain(await invoke<AutonomyChain>("autonomy_chain", { projectId: projectId ?? null }));
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, [projectId]);

  useEffect(() => {
    void load();
  }, [load]);

  const setGlobal = useCallback(
    async (autonomy: Autonomy | null) => {
      try {
        const next = await invoke<AutonomyChain>("set_global_autonomy", { autonomy });
        // The core answers about the global scope, so its `project` is empty
        // by construction. In a project-scoped view that answer is missing
        // half the chain — re-read rather than render a project's own setting
        // as though it had just been cleared.
        if (projectId) await load();
        else setChain(next);
        setError(null);
      } catch (cause) {
        setError(String(cause));
      }
    },
    [projectId, load],
  );

  const setProject = useCallback(
    async (autonomy: Autonomy | null) => {
      if (!projectId) return;
      try {
        setChain(await invoke<AutonomyChain>("set_project_autonomy", { projectId, autonomy }));
        setError(null);
      } catch (cause) {
        setError(String(cause));
      }
    },
    [projectId],
  );

  return { chain, error, setGlobal, setProject };
}
