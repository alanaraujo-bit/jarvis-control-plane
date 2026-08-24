/**
 * Turning quota facts into the words and shapes on the Accounts screen.
 *
 * Pulled out of the components for one reason that is not tidiness: the same
 * countdown appears on the card, in the summary strip and in the tooltip, and
 * three copies of "how long until this resets" would eventually disagree with
 * each other by a minute and look broken.
 */

import type { MessageKey } from "@jarvis/i18n";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/**
 * A compact duration, floored to whole units: `47m`, `3h 54m`, `6d 7h`.
 *
 * Floored rather than rounded because this reads as a deadline. "Resets in 1h"
 * when 59 minutes remain is fine; "resets in 1h" when 61 minutes remain sends
 * somebody back to a screen that has not reset yet.
 */
export function duration(ms: number): { days: number; hours: number; minutes: number } {
  const clamped = Math.max(0, ms);
  return {
    days: Math.floor(clamped / DAY_MS),
    hours: Math.floor((clamped % DAY_MS) / HOUR_MS),
    minutes: Math.floor((clamped % HOUR_MS) / MINUTE_MS),
  };
}

export type Translate = (key: MessageKey, values?: Record<string, string | number>) => string;

/**
 * "Resets in 3h 54m" — or the "any moment now" wording when the moment has
 * arrived and the provider has not yet said otherwise.
 */
export function countdown(target: number | null, now: number, t: Translate): string | null {
  if (target === null) return null;
  const remaining = target - now;
  if (remaining <= 0) return t("accounts.reset.now");
  const { days, hours, minutes } = duration(remaining);
  if (days > 0) return t("accounts.reset.days", { days, hours });
  if (hours > 0) return t("accounts.reset.hours", { hours, minutes });
  return t("accounts.reset.minutes", { minutes });
}

/**
 * How long to wait before a countdown label would change.
 *
 * A label floored to minutes only changes on a minute boundary, and one showing
 * days only changes on the hour — so the screen wakes up at the boundary rather
 * than ticking every second behind four account cards. The `+250` lands just
 * past the boundary rather than exactly on it, where a fractional millisecond
 * would otherwise redraw the same label and schedule a zero-length timeout.
 */
export function nextTickDelay(now: number, resets: (number | null)[]): number {
  let soonest: number | null = null;
  for (const reset of resets) {
    if (reset === null || reset <= now) continue;
    const remaining = reset - now;
    const unit = remaining >= DAY_MS ? HOUR_MS : MINUTE_MS;
    const delay = (remaining % unit) + 250;
    soonest = soonest === null ? delay : Math.min(soonest, delay);
  }
  // Nothing to count down to: check back in a minute rather than never, so a
  // reset time that arrives from a background refresh starts ticking on its own.
  return soonest ?? MINUTE_MS;
}

/**
 * The exact moment a window resets, in the reader's own locale.
 *
 * Shown *beside* the countdown, never instead of it. "Resets in 4h" answers
 * "can I keep working"; "today at 20:20" answers "should I go to lunch first",
 * and Alan asked for both when he said he could not see when it resets.
 */
export function resetMoment(target: number, now: number, locale: string): string {
  const date = new Date(target);
  const sameDay = new Date(now).toDateString() === date.toDateString();
  return new Intl.DateTimeFormat(locale, {
    hour: "2-digit",
    minute: "2-digit",
    ...(sameDay ? {} : { weekday: "short", day: "numeric", month: "short" }),
  }).format(date);
}

/**
 * What is *left*, which is the number that was asked for.
 *
 * The providers state consumption; a person planning their afternoon thinks in
 * headroom. The conversion happens here, once, so the bar, the label and the
 * screen reader can never end up describing opposite halves of the same window.
 */
export function remaining(percentUsed: number): number {
  return Math.max(0, Math.min(100, 100 - percentUsed));
}

/**
 * Severity band for a used percentage, matching what the core derives.
 *
 * Duplicated here on purpose rather than only trusted from the payload: a
 * provider-supplied severity is kept verbatim in the data, and a provider that
 * invents a new band name would otherwise leave a card with no colour at all.
 */
export function bandFor(percentUsed: number): "normal" | "warning" | "critical" | "exhausted" {
  if (percentUsed >= 100) return "exhausted";
  if (percentUsed >= 90) return "critical";
  if (percentUsed >= 85) return "warning";
  return "normal";
}

/** A severity that renders, whatever the provider called it. */
export function severityBand(severity: string, percentUsed: number): string {
  return ["normal", "warning", "critical", "exhausted"].includes(severity)
    ? severity
    : bandFor(percentUsed);
}

export function formatTokens(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    notation: "compact",
    maximumFractionDigits: 1,
  }).format(value);
}

/**
 * Money in the currency the provider actually bills this account in.
 *
 * The overage block carries its own ISO code — `BRL` on this machine — so
 * rendering it as dollars would misstate a real number by a factor of five.
 * An unknown code falls back to plain digits with the code beside it rather
 * than throwing, which `Intl.NumberFormat` does for an invalid currency.
 */
export function formatMoney(
  value: number,
  currency: string,
  decimals: number,
  locale: string,
): string {
  try {
    return new Intl.NumberFormat(locale, {
      style: "currency",
      currency,
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    }).format(value);
  } catch {
    return `${value.toFixed(decimals)} ${currency}`;
  }
}

/** "just now" / "3 min ago" for the age of a live reading. */
export function readingAge(readAtMs: number, now: number, t: Translate): string {
  const age = Math.max(0, now - readAtMs);
  if (age < 45_000) return t("accounts.live.justNow");
  const { days, hours, minutes } = duration(age);
  if (days > 0) return t("accounts.live.agoDays", { days });
  if (hours > 0) return t("accounts.live.agoHours", { hours });
  return t("accounts.live.agoMinutes", { minutes });
}
