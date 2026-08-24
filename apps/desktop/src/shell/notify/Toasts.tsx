import { useCallback, useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import { StatusDot } from "../../design/StatusDot";
import { useT } from "../../app/i18n";
import { dotFor, type Notification } from "../../app/notifications";
import { describe } from "./describe";
import "./Toasts.css";

/**
 * The in-app toast stack (§49).
 *
 * For the case a Windows toast cannot serve: the window is in front, the person
 * is working in it, and something they are *not* looking at has stopped. A
 * desktop notification there would be redundant with a window they can already
 * see; nothing at all would be a missed agent.
 *
 * Three rules, and each is the difference between this being useful and being
 * the thing people turn off first:
 *
 * * **A question stays; an announcement leaves.** Anything still asking for a
 *   decision sits there until it is dismissed, opened, or the person comes back
 *   to the window — coming back is the answer to "did they see it". A finished
 *   turn fades after `LINGER` on its own: it was information, and information
 *   that has been read has done its job. Nothing is lost either way; the badge
 *   and the centre still hold everything.
 * * **Never more than `MAX_VISIBLE`.** Four agents finishing at once is an
 *   ordinary Tuesday here, and a stack that grows without limit covers the work
 *   it is reporting on. The overflow is counted, not drawn.
 * * **Bottom-right, above nothing.** It never covers the terminal prompt or the
 *   editor's cursor, which are the two places a person is actually typing.
 */

/** How long an announcement stays before it fades. */
const LINGER_MS = 7_000;

/** How many are drawn at once; the rest are counted. */
const MAX_VISIBLE = 3;

export function Toasts({
  items,
  onOpen,
  onDismiss,
}: {
  items: Notification[];
  onOpen: (notification: Notification) => void;
  onDismiss: (id: number) => void;
}) {
  const t = useT();
  const visible = items.slice(0, MAX_VISIBLE);
  const overflow = items.length - visible.length;

  if (items.length === 0) return null;

  return (
    <div className="toasts" role="region" aria-label={t("notify.title")} aria-live="polite">
      {overflow > 0 && (
        <div className="toasts__overflow">{t("notify.more", { count: overflow })}</div>
      )}
      {visible.map((item) => (
        <Toast
          key={item.id}
          notification={item}
          onOpen={() => onOpen(item)}
          onDismiss={() => onDismiss(item.id)}
        />
      ))}
    </div>
  );
}

function Toast({
  notification,
  onOpen,
  onDismiss,
}: {
  notification: Notification;
  onOpen: () => void;
  onDismiss: () => void;
}) {
  const t = useT();
  const { title, body, where } = describe(notification, t);
  const asking = notification.kind === "needsApproval";
  const [leaving, setLeaving] = useState(false);
  const hovered = useRef(false);

  const dismiss = useCallback(() => {
    setLeaving(true);
    // Let the exit animation finish before the row leaves the list, so it
    // slides out rather than vanishing mid-motion.
    window.setTimeout(onDismiss, 160);
  }, [onDismiss]);

  useEffect(() => {
    if (asking) return;
    const timer = window.setInterval(() => {
      // Reaching for a toast must not make it leave. The timer keeps running
      // and simply refuses to fire while the pointer is on it.
      if (!hovered.current) {
        window.clearInterval(timer);
        dismiss();
      }
    }, LINGER_MS);
    return () => window.clearInterval(timer);
  }, [asking, dismiss]);

  return (
    <div
      className="toast"
      data-kind={notification.kind}
      data-leaving={leaving || undefined}
      onMouseEnter={() => {
        hovered.current = true;
      }}
      onMouseLeave={() => {
        hovered.current = false;
      }}
    >
      <button type="button" className="toast__hit" onClick={onOpen}>
        <StatusDot status={dotFor(notification.kind)} size={6} />
        <span className="toast__text">
          <span className="toast__title">{title}</span>
          {body && <span className="toast__body">{body}</span>}
          {where && <span className="toast__where">{where}</span>}
        </span>
      </button>
      <button
        type="button"
        className="toast__close"
        onClick={dismiss}
        aria-label={t("common.dismiss")}
        title={t("common.dismiss")}
      >
        <X size={12} strokeWidth={2} aria-hidden="true" />
      </button>
    </div>
  );
}
