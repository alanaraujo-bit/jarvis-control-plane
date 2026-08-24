import type { MessageKey } from "@jarvis/i18n";
import { agentName, type Notification } from "../../app/notifications";

type Translate = (key: MessageKey, values?: Record<string, string | number>) => string;

/**
 * The §34 stop reasons an unattended run can carry, as they arrive from the
 * core.
 *
 * A closed list, checked rather than trusted. `detailCode` crosses the Tauri
 * bridge as a plain string, and handing an arbitrary one to `t()` would render
 * the identifier itself on screen the day the core grows a sixth reason — the
 * raw-identifier failure §65 exists to prevent. An unrecognised code falls back
 * to the preview instead, which always says something true.
 */
const RUN_STOP_REASONS = [
  "autopilot.outOfTurns",
  "autopilot.needsManualCheck",
  "autopilot.awaitingApproval",
  "autopilot.notConverging",
  "autopilot.missionBlocked",
] as const;

function detailKey(code: string | null): MessageKey | null {
  if (!code) return null;
  return (RUN_STOP_REASONS as readonly string[]).includes(code)
    ? (code as MessageKey)
    : null;
}

/**
 * One notification, in words.
 *
 * The single place the sentence is composed. A row in the centre, an in-app
 * toast and a Windows toast all call this, so the three can never drift into
 * saying slightly different things about the same event — which is the way a
 * notification system starts feeling untrustworthy.
 *
 * **The title is ours and translated; the body is the agent's and is not.**
 * `preview` is what the agent wrote on its own screen or in its own reply, and
 * translating that would be inventing words nobody said. It is the only
 * untranslated string in the interface, and it is the one worth reading.
 */
export function describe(notification: Notification, t: Translate) {
  const agent = agentName(notification.provider);
  const title = t(`notify.title.${notification.reason}` as MessageKey, { agent });

  // Where it happened, so a person with four projects open knows which one.
  // The mission wins over the project: it is the more specific answer, and a
  // notification that carries one is about that mission.
  const where = notification.missionTitle ?? notification.projectName ?? null;

  // Why, when the reason alone does not say.
  //
  // "An unattended run stopped" is four different situations — out of turns,
  // not converging, waiting on an approval, or left with something only a
  // person can judge — and the one it was is exactly what somebody coming back
  // to the screen needs. The core sends the §34 cause as a stable identifier
  // that is already an i18n key, so this reads it rather than restating it.
  const cause = detailKey(notification.detailCode);

  const body = cause ? t(cause) : notification.preview?.trim() || null;

  const provenance =
    notification.confidence === "observed"
      ? t("notify.from.observed")
      : notification.confidence === "official"
        ? t("notify.from.official")
        : null;

  return { title, body, where, provenance, agent };
}

/**
 * A relative time that stays readable without a timer.
 *
 * Bucketed rather than exact: a list that says "3 minutes ago" has to re-render
 * every minute to stay true, and a notification centre is not worth a running
 * clock. The buckets are wide enough that a stale one is never *wrong*, only
 * imprecise — which is the right trade for a list you glance at.
 */
export function whenGroup(tsMs: number, now: number): "now" | "today" | "earlier" {
  const age = now - tsMs;
  if (age < 5 * 60_000) return "now";
  const then = new Date(tsMs);
  const today = new Date(now);
  if (then.toDateString() === today.toDateString()) return "today";
  return "earlier";
}

/** The clock time, for the row itself. */
export function clockTime(tsMs: number, locale: string): string {
  const date = new Date(tsMs);
  const sameDay = date.toDateString() === new Date().toDateString();
  return sameDay
    ? date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString(locale, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
}
