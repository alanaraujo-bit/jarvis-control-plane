import { useEffect, useState } from "react";
import { Bookmark, Heart, MessageCircle, UserPlus } from "lucide-react";
import type { MessageKey } from "@jarvis/i18n";
import { useI18n, useT } from "../../app/i18n";
import { ago, social, useCommunity, type Notification } from "./api";
import { Avatar, Empty } from "./parts";

const ICON = { follow: UserPlus, like: Heart, reply: MessageCircle, save: Bookmark };

/** Who followed, liked, replied or saved. Opening it marks everything read. */
export function Activity() {
  const t = useT();
  const { locale } = useI18n();
  const go = useCommunity((s) => s.go);
  const loadMe = useCommunity((s) => s.load);
  const [items, setItems] = useState<Notification[] | null>(null);

  useEffect(() => {
    void social<{ notifications: Notification[] }>("GET", "notifications")
      .then(async (r) => {
        setItems(r.notifications);
        if (r.notifications.some((n) => !n.read)) {
          await social("POST", "notifications/read");
          void loadMe();
        }
      })
      .catch(() => setItems([]));
  }, [loadMe]);

  if (items === null) return <div className="cm-skeleton" aria-busy="true" />;
  if (items.length === 0) return <Empty>{t("community.activity.empty")}</Empty>;
  return (
    <ul className="cm-activity">
      {items.map((item) => {
        const Icon = ICON[item.kind];
        return (
          <li key={item.id} data-unread={!item.read || undefined}>
            <button type="button" className="cm-activity__row" onClick={() => go(item.postId ? { kind: "thread", id: item.postId } : { kind: "profile", handle: item.actor.handle })}>
              <span className="cm-activity__icon"><Icon size={13} aria-hidden="true" /></span>
              <Avatar name={item.actor.displayName} size={28} />
              <span className="cm-activity__text">
                <span>{t(`community.activity.${item.kind}` as MessageKey, { name: item.actor.displayName })}</span>
                {item.snippet && <span className="cm-muted cm-activity__snippet">{item.snippet}</span>}
              </span>
              <span className="cm-muted">{ago(item.createdAt, locale)}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
