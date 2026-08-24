import { Bell } from "lucide-react";
import { useT } from "../../app/i18n";
import "./NotificationBell.css";

/**
 * The entry point to the notification centre (§49).
 *
 * In the titlebar, beside the caption buttons: it belongs to the window rather
 * than to any surface, because what it reports on happens whichever surface is
 * open. The rail was the other candidate and is wrong — a rail item is a
 * *place*, and this is not somewhere you go.
 *
 * The count is drawn only when it is not zero. A permanent "0" is a control
 * reporting on its own emptiness, which §7 refuses; the bell's own presence
 * already says the feature exists.
 */
export function NotificationBell({
  count,
  open,
  onToggle,
  buttonRef,
}: {
  count: number;
  open: boolean;
  onToggle: () => void;
  buttonRef?: React.Ref<HTMLButtonElement>;
}) {
  const t = useT();
  const label = count > 0 ? `${t("notify.open")} — ${t("notify.unread", { count })}` : t("notify.open");

  return (
    <button
      ref={buttonRef}
      type="button"
      className="notify-bell"
      data-open={open || undefined}
      data-unread={count > 0 || undefined}
      onClick={onToggle}
      aria-label={label}
      // No native tooltip while the panel is open: it renders above everything
      // and lands squarely on the panel's own heading, which says the same
      // word. Seen in a real screenshot of the open centre.
      title={open ? undefined : label}
      aria-expanded={open}
    >
      <Bell size={14} strokeWidth={1.75} aria-hidden="true" />
      {count > 0 && (
        // Capped at 9+. The exact number past a handful is not information a
        // person acts on, and three digits do not fit beside a 14px icon.
        <span className="notify-bell__count">{count > 9 ? "9+" : count}</span>
      )}
    </button>
  );
}
