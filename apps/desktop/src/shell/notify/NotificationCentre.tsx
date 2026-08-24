import { useEffect, useMemo, useRef, useState } from "react";
import { Bell, Check, Trash2 } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { StatusDot } from "../../design/StatusDot";
import { useI18n, useT } from "../../app/i18n";
import { dotFor, useNotifications, type Notification } from "../../app/notifications";
import { clockTime, describe, whenGroup } from "./describe";
import "./NotificationCentre.css";

/**
 * What you missed (§49).
 *
 * A panel under the bell, not a surface in the rail. This is a **queue**, not a
 * record: everything in it is either still asking for something or was over the
 * moment it arrived, and a queue you have to navigate away to read is a queue
 * you stop reading. What happened, permanently, is Activity (§48).
 *
 * Opening it marks everything read. That is the point of opening it, and a
 * separate "mark as read" gesture for something you have visibly just read is
 * the kind of bookkeeping §7 exists to refuse. The button is still there for
 * clearing the badge without opening — from the keyboard, or from habit.
 *
 * Clicking a row is the **only** click-through in the feature, because it is
 * the only one that can be: a Windows toast has no activation callback on the
 * desktop (see `present`). So this panel is not a nicety beside the toast; it
 * is where the toast points.
 */
export function NotificationCentre({
  open,
  onClose,
  onOpenNotification,
}: {
  open: boolean;
  onClose: () => void;
  onOpenNotification: (notification: Notification) => void;
}) {
  const t = useT();
  const { locale } = useI18n();
  const items = useNotifications((state) => state.items);
  const enabled = useNotifications((state) => state.enabled);
  const markAllSeen = useNotifications((state) => state.markAllSeen);
  const clear = useNotifications((state) => state.clear);
  const markActed = useNotifications((state) => state.markActed);
  const panel = useRef<HTMLDivElement>(null);

  // Frozen when the panel opens. A list that reorders and re-groups under the
  // pointer while somebody is reaching for a row is how you click the wrong
  // thing; arrivals go to the top on the next open.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (open) setNow(Date.now());
  }, [open]);

  /**
   * Which rows were new when the panel opened.
   *
   * Opening marks everything read — the badge has to fall the moment you look,
   * or it is a counter of nothing. But that also erased the marker showing
   * *which* rows were the new ones, which is the first thing anybody wants
   * from a list they have opened because a badge appeared. Found by opening
   * the panel in the real app and watching two genuinely-new rows render
   * exactly like two old ones.
   *
   * So the marker is drawn from a snapshot taken at the moment of opening,
   * and the database is updated immediately regardless.
   */
  const [wasNew, setWasNew] = useState<Set<number>>(() => new Set());
  useEffect(() => {
    if (!open) return;
    setWasNew(new Set(items.filter((item) => item.seenAt === null).map((item) => item.id)));
    void markAllSeen();
    // Deliberately not keyed on `items`: the snapshot is of the moment the
    // panel opened, and re-taking it as notifications arrive would mark
    // everything new again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, markAllSeen]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [open, onClose]);

  const groups = useMemo(() => {
    const buckets: Record<string, Notification[]> = { now: [], today: [], earlier: [] };
    for (const item of items) buckets[whenGroup(item.tsMs, now)].push(item);
    return (["now", "today", "earlier"] as const)
      .map((id) => ({ id, items: buckets[id] }))
      .filter((group) => group.items.length > 0);
  }, [items, now]);

  if (!open) return null;

  return (
    <div className="notify-centre" ref={panel} role="dialog" aria-label={t("notify.title")}>
      <header className="notify-centre__head">
        <h2 className="notify-centre__title">{t("notify.title")}</h2>
        {items.length > 0 && (
          <div className="notify-centre__actions">
            <button
              type="button"
              className="notify-centre__action"
              onClick={() => void markAllSeen()}
              title={t("notify.markAllSeen")}
              aria-label={t("notify.markAllSeen")}
            >
              <Check size={13} strokeWidth={1.75} aria-hidden="true" />
            </button>
            <button
              type="button"
              className="notify-centre__action"
              onClick={() => void clear()}
              title={t("notify.clear")}
              aria-label={t("notify.clear")}
            >
              <Trash2 size={13} strokeWidth={1.75} aria-hidden="true" />
            </button>
          </div>
        )}
      </header>

      {items.length === 0 ? (
        // Two sentences, not one: the first says the state, the second says
        // what would put something here. An empty queue with no explanation
        // reads as a feature that is not working.
        <div className="notify-centre__empty">
          <Bell size={18} strokeWidth={1.5} aria-hidden="true" />
          <p className="notify-centre__empty-title">
            {enabled ? t("notify.empty") : t("notify.disabled")}
          </p>
          <p className="notify-centre__empty-hint">
            {enabled ? t("notify.emptyHint") : t("notify.disabledHint")}
          </p>
        </div>
      ) : (
        <div className="notify-centre__scroll">
          {groups.map((group) => (
            <section key={group.id} className="notify-centre__group">
              <h3 className="notify-centre__group-title">
                {t(`notify.group.${group.id}` as MessageKey)}
              </h3>
              <ul className="notify-centre__list">
                {group.items.map((item) => (
                  <NotificationRow
                    key={item.id}
                    notification={item}
                    isNew={wasNew.has(item.id)}
                    locale={locale}
                    onOpen={() => {
                      void markActed(item.id);
                      onOpenNotification(item);
                      onClose();
                    }}
                  />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

function NotificationRow({
  notification,
  isNew,
  locale,
  onOpen,
}: {
  notification: Notification;
  /** New when this panel was opened — see the snapshot in the parent. */
  isNew: boolean;
  locale: string;
  onOpen: () => void;
}) {
  const t = useT();
  const { title, body, where, provenance } = describe(notification, t);
  // Only somewhere to go if there is somewhere to go. A row for a mission with
  // no session, or a session that has since been closed, is still worth
  // reading — it just is not a link, and must not pretend to be one (§81).
  const navigable = notification.sessionId !== null || notification.projectId !== null;

  const content = (
    <>
      <StatusDot status={dotFor(notification.kind)} size={6} />
      <div className="notify-row__text">
        <div className="notify-row__head">
          <span className="notify-row__title">{title}</span>
          <time className="notify-row__time">{clockTime(notification.tsMs, locale)}</time>
        </div>
        {/* The agent's own words. Kept as a quotation rather than styled as our
            prose, because they are not ours. */}
        {body && <p className="notify-row__body">{body}</p>}
        <div className="notify-row__meta">
          {where && <span className="notify-row__where">{where}</span>}
          {provenance && <span className="notify-row__provenance">{provenance}</span>}
        </div>
      </div>
    </>
  );

  return (
    <li className="notify-row" data-unseen={isNew || undefined}>
      {navigable ? (
        <button type="button" className="notify-row__hit" onClick={onOpen}>
          {content}
        </button>
      ) : (
        <div className="notify-row__hit notify-row__hit--static">{content}</div>
      )}
    </li>
  );
}
