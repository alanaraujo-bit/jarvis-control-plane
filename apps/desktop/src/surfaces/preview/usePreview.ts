import { useCallback, useEffect, useRef, useState } from "react";
import { invoke, isTauri } from "../../app/platform";

/** What a session appears to be serving, read from its own output (§46). */
export interface Detected {
  url: string | null;
  all: string[];
}

/**
 * How often a session is re-checked for a dev server.
 *
 * A dev server takes a few seconds to print its banner, so a single check when
 * the surface opens would usually miss it — a person clicking Preview
 * immediately after `npm run dev` is the ordinary case, not the edge. Two
 * seconds is fast enough to feel like it noticed and cheap enough to ignore:
 * the scan is a bounded read of a file this process already has open.
 */
const POLL_MS = 2000;

interface UsePreview {
  detected: Detected;
  /** True while the preview window is open, so the surface can offer Reload. */
  open: boolean;
  error: string | null;
  openPreview: (url: string) => Promise<void>;
  reload: () => Promise<void>;
  close: () => Promise<void>;
}

/**
 * Watch one session for a dev server, and drive the preview window.
 *
 * Polling stops when `active` is false — the Preview area is mounted for the
 * life of the project like every other area (D24), and a hidden surface must
 * not keep reading logs for a session nobody is looking at.
 */
export function usePreview(sessionId: string | undefined, active: boolean): UsePreview {
  const [detected, setDetected] = useState<Detected>({ url: null, all: [] });
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Avoids a re-render per poll when nothing has changed, which would be every
  // two seconds for the whole time a project is open.
  const last = useRef<string>("");

  useEffect(() => {
    if (!active || !sessionId || !isTauri()) return;

    let stopped = false;
    const check = async () => {
      try {
        const next = await invoke<Detected>("preview_detect", { sessionId });
        if (stopped) return;
        const key = JSON.stringify(next);
        if (key !== last.current) {
          last.current = key;
          setDetected(next);
        }
        setOpen(await invoke<boolean>("preview_is_open"));
      } catch {
        // A session that has ended, or a log not yet written. Neither is worth
        // saying anything about — the surface simply has nothing to offer yet.
      }
    };

    void check();
    const timer = setInterval(() => void check(), POLL_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [sessionId, active]);

  const openPreview = useCallback(async (url: string) => {
    try {
      await invoke("preview_open", { url });
      setOpen(true);
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  }, []);

  const reload = useCallback(async () => {
    await invoke("preview_reload").catch(() => {
      // The window may have been closed by the user between render and click.
    });
  }, []);

  const close = useCallback(async () => {
    await invoke("preview_close").catch(() => {});
    setOpen(false);
  }, []);

  return { detected, open, error, openPreview, reload, close };
}
