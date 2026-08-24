import type { MessageKey } from "@jarvis/i18n";
import type { Translate } from "../../app/i18n";
import type { DotStatus } from "../../design/StatusDot";
import type { HistoryEntry } from "../../app/history";

/**
 * How a session is described (§88).
 *
 * Shared by the list and the preview deliberately. These are the same facts
 * about the same session, and two copies would be two things free to disagree
 * about what a row costs or how long it ran — the reader would have no way to
 * tell which one was lying.
 */

/** Session states, mapped onto the shared state vocabulary. */
export function dotFor(entry: HistoryEntry): DotStatus {
  if (entry.live) return entry.state === "waiting" ? "waiting" : "working";
  switch (entry.state) {
    case "completed":
      return "completed";
    case "blocked":
      return "blocked";
    case "failed":
      return "failed";
    default:
      return "idle";
  }
}

/**
 * Bytes as a person reads them.
 *
 * Binary units, because this is disk and the operating system reporting it uses
 * them too — a figure here that disagreed with Explorer would be a figure
 * nobody trusts.
 */
export function formatBytes(bytes: number, locale: string): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value < 10 ? 1 : 0;
  return `${value.toLocaleString(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  })} ${units[unit]}`;
}

/**
 * How long a session ran, or has been running.
 *
 * `null` for one that never got past a minute — "0m" on every row is noise, and
 * a session that short has nothing to report about how long it took.
 */
export function formatDuration(entry: HistoryEntry, now: number): string | null {
  const end = entry.endedAt ?? (entry.live ? now : null);
  if (end === null) return null;
  const ms = end - entry.createdAt;
  if (ms < 60_000) return null;
  const minutes = Math.round(ms / 60_000);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}

/**
 * A provider's display name, falling back to its raw id.
 *
 * An unknown provider renders as itself rather than as a missing-translation
 * key — the same fallback Activity uses for an unknown kind.
 */
export function providerLabel(provider: string, t: Translate): string {
  const label = t(`history.provider.${provider}` as MessageKey);
  return label.startsWith("history.provider.") ? provider : label;
}

/** Which day-bucket a session falls in, by when it started. */
export type Bucket = "today" | "yesterday" | "week" | "month" | "earlier";

export function bucketOf(tsMs: number, now: number): Bucket {
  const day = (value: number) => {
    const date = new Date(value);
    return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  };
  const days = Math.round((day(now) - day(tsMs)) / 86_400_000);
  if (days <= 0) return "today";
  if (days === 1) return "yesterday";
  if (days < 7) return "week";
  if (days < 31) return "month";
  return "earlier";
}

export const BUCKET_ORDER: Bucket[] = ["today", "yesterday", "week", "month", "earlier"];

/**
 * A short relative time, the way a list of recent things wants one.
 *
 * The exact instant belongs on the element's `title`: a relative time is quick
 * to scan and useless the moment somebody actually needs to know when.
 */
export function relative(tsMs: number, now: number, t: Translate): string {
  const seconds = Math.max(0, Math.round((now - tsMs) / 1000));
  if (seconds < 60) return t("history.now");
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.round(hours / 24);
  if (days < 31) return `${days}d`;
  const months = Math.round(days / 30);
  if (months < 12) return `${months}mo`;
  return `${Math.round(months / 12)}y`;
}

/** The session kind a history row reopens as (§51). */
export function kindOf(entry: HistoryEntry): "shell" | "claude-code" | "codex" {
  return entry.provider === "claude-code" || entry.provider === "codex"
    ? entry.provider
    : "shell";
}
