import { useEffect, useState } from "react";
import { isTauri } from "./platform";

/**
 * What version this actually is.
 *
 * ## Why this is not a constant
 *
 * It was one — in two places, `StatusBar` and `Updates`, and by the time
 * anybody looked they disagreed with each other *and* with the build: the
 * status bar said 0.2.0 and the updater said 0.1.0 while the installed binary
 * reported 0.3.0. Nobody had done anything wrong; a number written by hand in
 * a file nobody edits when they bump a version is a number that drifts, and
 * the only surprising thing is that it took two releases.
 *
 * A version the product states about itself has to come from the build, or it
 * is decoration that happens to look like a fact. `getVersion()` reads the one
 * in `tauri.conf.json`, which is the same number the installer and the updater
 * use, so the three cannot disagree again.
 *
 * `null` while it is being fetched, and in browser preview where there is no
 * build to ask. Callers render nothing rather than a placeholder: a wrong
 * version is worse than no version, which is the whole point of this file.
 */
export function useAppVersion(): string | null {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    void (async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        const found = await getVersion();
        if (!cancelled) setVersion(found);
      } catch {
        // Nothing to say, so nothing is said.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return version;
}
