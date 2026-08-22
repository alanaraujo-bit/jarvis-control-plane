import { useEffect, useState } from "react";
import { invoke, isTauri } from "../../app/platform";
import { useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import "./Activity.css";

type Severity = "info" | "warning" | "attention";

interface ActivityEntry {
  id: number;
  tsMs: number;
  projectId: string | null;
  sessionId: string | null;
  missionId: string | null;
  kind: string;
  severity: Severity;
  title: string;
  detail: string | null;
}

function formatWhen(tsMs: number, locale: string): string {
  const date = new Date(tsMs);
  const today = new Date();
  const sameDay = date.toDateString() === today.toDateString();
  return sameDay
    ? date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString(locale, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
}

/**
 * Activity (§48).
 *
 * A record of things worth knowing, newest first. The kind is a stable
 * identifier the UI translates, so the log reads in the user's language rather
 * than in whatever language the core happened to be written in (§65).
 */
export function Activity() {
  const t = useT();
  const [entries, setEntries] = useState<ActivityEntry[]>([]);
  const [onlyAttention, setOnlyAttention] = useState(false);

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;

    const read = () =>
      void invoke<ActivityEntry[]>("list_activity", {
        filter: onlyAttention ? { severity: "attention" } : {},
      }).then((next) => {
        if (!cancelled) setEntries(next);
      });

    read();
    // Activity accumulates while agents run, so it refreshes on a slow tick.
    const timer = window.setInterval(read, 4000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [onlyAttention]);

  return (
    <div className="act">
      <div className="act__inner">
        <header className="act__header">
          <h1 className="act__title">{t("activity.title")}</h1>
          <div className="act__filter" role="radiogroup">
            {[false, true].map((value) => (
              <button
                key={String(value)}
                type="button"
                role="radio"
                aria-checked={onlyAttention === value}
                data-active={onlyAttention === value || undefined}
                className="act__filter-option"
                onClick={() => setOnlyAttention(value)}
              >
                {value ? t("activity.attention") : t("activity.all")}
              </button>
            ))}
          </div>
        </header>

        {entries.length === 0 ? (
          <div className="act__empty">
            <p className="act__empty-title">{t("activity.empty.title")}</p>
            <p className="act__empty-body">{t("activity.empty.body")}</p>
          </div>
        ) : (
          <ul className="act__list">
            {entries.map((entry) => {
              // An unknown kind falls back to its raw identifier rather than
              // rendering an empty row.
              const label = t(`activity.kind.${entry.kind}` as MessageKey);
              const heading = label.startsWith("activity.kind.") ? entry.kind : label;

              return (
                <li key={entry.id} className="act__row" data-severity={entry.severity}>
                  <span className="act__marker" aria-hidden="true" />
                  <span className="act__kind">{heading}</span>
                  <span className="act__subject">{entry.title}</span>
                  {entry.detail && (
                    <span className="act__detail" title={entry.detail}>
                      {entry.detail}
                    </span>
                  )}
                  <time className="act__time">{formatWhen(entry.tsMs, navigator.language)}</time>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
