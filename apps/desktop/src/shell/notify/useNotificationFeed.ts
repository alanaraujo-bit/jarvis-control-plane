import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useT } from "../../app/i18n";
import { isTauri } from "../../app/platform";
import {
  isWindowFocused,
  onVisibleSessions,
  useNotifications,
  type Notification,
} from "../../app/notifications";
import { describe } from "./describe";
import { chime, flashTaskbar, systemToast } from "./present";

/** The event the core emits, one per notification. Matches `notify::store::EVENT`. */
const EVENT = "jarvis://notification";

/**
 * The live feed (§49): one subscription, and what to do with what arrives.
 *
 * This is the seam between the two halves of the feature. The core has already
 * decided that this notification is worth making — it dropped everything the
 * person was watching — so nothing here re-litigates that. What is left is a
 * presentation choice, and it turns on one thing the core cannot see:
 *
 * > **Is the window in front?**
 *
 * If it is, the person is here and an in-app toast is the least intrusive way
 * to reach them. If it is not, they are somewhere else and only the desktop can
 * carry it — plus a taskbar flash, because a Windows toast can be missed,
 * dismissed by the OS, or switched off in Settings we do not control.
 *
 * Both together would be a duplicate for somebody already looking at the app.
 */
export function useNotificationFeed() {
  const t = useT();
  const receive = useNotifications((state) => state.receive);
  const load = useNotifications((state) => state.load);
  const systemEnabled = useRef(true);
  const soundEnabled = useRef(true);

  /**
   * The translator, held in a ref rather than closed over.
   *
   * The listener is built once and must stay built: rebuilding it whenever the
   * language changes would unsubscribe and resubscribe, and a notification
   * arriving in that gap is simply lost. But a listener that closed over `t`
   * from its first render would compose every desktop toast, forever, in
   * whatever language was chosen at launch — and changing the language is
   * exactly the moment somebody would notice.
   */
  const translate = useRef(t);
  translate.current = t;

  /** What is on the in-app stack right now. Separate from the stored list: a
      dismissed toast is still a notification, it is just no longer on screen. */
  const [toasts, setToasts] = useState<Notification[]>([]);

  const dismiss = useCallback((id: number) => {
    setToasts((current) => current.filter((item) => item.id !== id));
  }, []);

  const clearToasts = useCallback(() => setToasts([]), []);

  useEffect(() => {
    void load();
  }, [load]);

  // A toast about a session somebody has just gone to has said what it had to
  // say. See `onVisibleSessions`.
  useEffect(
    () =>
      onVisibleSessions((ids) => {
        if (ids.length === 0) return;
        setToasts((current) =>
          current.filter((item) => !(item.sessionId && ids.includes(item.sessionId))),
        );
      }),
    [],
  );

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const start = async () => {
      const { listen } = await import("@tauri-apps/api/event");
      const stop = await listen<Notification>(EVENT, async (event) => {
        const notification = event.payload;
        receive(notification);

        // Two signals, and neither alone is enough. The tracked focus state is
        // what the core was told, so using it keeps both halves agreeing; the
        // minimised check is there because a minimised window is not where
        // anybody is looking whatever it claims about focus. See
        // `isWindowFocused` for what asking afresh actually returned.
        const minimised = await getCurrentWindow()
          .isMinimized()
          .catch(() => false);
        const here = isWindowFocused() && !minimised;

        if (here) {
          setToasts((current) =>
            current.some((item) => item.id === notification.id)
              ? current
              : [notification, ...current],
          );
        } else {
          const { title, body } = describe(notification, translate.current);
          if (systemEnabled.current) {
            await systemToast({ title, body: body ?? (title || "") });
          }
          await flashTaskbar();
        }

        if (soundEnabled.current) {
          chime(notification.kind === "needsApproval" ? "waiting" : "done");
        }
      });
      if (cancelled) stop();
      else unlisten = stop;
    };

    void start();
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [receive]);

  /** Keep the presentation switches current without rebuilding the listener. */
  const setChannels = useCallback((system: boolean, sound: boolean) => {
    systemEnabled.current = system;
    soundEnabled.current = sound;
  }, []);

  return { toasts, dismiss, clearToasts, setChannels };
}
