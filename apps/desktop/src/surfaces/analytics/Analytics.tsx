/**
 * Analytics (§52, §53, M22).
 *
 * The screen answers, in the order a person actually asks them:
 *
 * 1. **When have I been working?** — the calendar, which is the hero because
 *    it is the only thing here that shows a *shape* rather than a total. It is
 *    also the control: clicking a day scopes everything below it.
 * 2. **Am I keeping at it?** — days worked, current run, longest run.
 * 3. **What did I get out of it?** — the leverage figure (§53), the one number
 *    on this screen no other tool can show: minutes at the keyboard against
 *    hours of agent execution.
 * 4. **When in the day do I work?** — the rhythm histogram.
 * 5. **On what?** — project, model, provider.
 *
 * Three rules this file keeps:
 *
 * * **Nothing is a score.** §52 is explicit that metrics are information, not
 *   gamification. There is no goal, no target, no "don't break your streak".
 *   A quiet day is a day off and the screen says nothing about it.
 * * **An unknown is drawn differently from a zero.** Days before any history
 *   exists are not idle days, and rendering them the same way would invent
 *   twenty-odd days of laziness that never happened.
 * * **Magnitude is one hue.** Bars and cells are amber at varying strength.
 *   Colouring by rank would spend the only free channel restating the length.
 */

import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, CalendarDays, Clock3, Flame, Layers, X } from "lucide-react";
import { invoke, isTauri } from "../../app/platform";
import { useI18n, useT } from "../../app/i18n";
import type { MessageKey } from "@jarvis/i18n";
import "./Analytics.css";

interface UsageBucket {
  label: string;
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
  sessions: number;
  confidence: "official" | "observed" | "estimated" | "unknown";
}

interface DayCell {
  date: string;
  tokens: number;
  turns: number;
  hours: number;
  projects: number;
}

interface Streaks {
  current: number;
  longest: number;
  longestFrom: string | null;
  longestTo: string | null;
  activeDays: number;
  windowDays: number;
}

interface HourBucket {
  hour: number;
  tokens: number;
  turns: number;
}

interface AnalyticsReport {
  byProvider: UsageBucket[];
  byModel: UsageBucket[];
  byProject: UsageBucket[];
  byDay: UsageBucket[];
  calendar: DayCell[];
  streaks: Streaks;
  byHour: HourBucket[];
  leverage: {
    humanActiveMinutes: number;
    agentRuntimeMinutes: number;
    sessions: number;
    observedFrom: string | null;
  };
  filesChanged: number;
  windowDays: number;
  day: string | null;
  historyFrom: string | null;
}

const WINDOWS = [7, 30, 90] as const;

/**
 * How many strengths the calendar has.
 *
 * Five, including "worked but barely". Fewer and a heavy day looks like a light
 * one; more and the eye stops being able to rank them at a glance, which is the
 * entire job of the thing.
 */
const LEVELS = 4;

function formatTokens(value: number): string {
  if (value < 1000) return String(value);
  if (value < 1_000_000)
    return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)}k`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}

function formatDuration(minutes: number): string {
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}

/** A date the person recognises, from a `YYYY-MM-DD` in their own locale. */
function formatDay(date: string, locale: string, long = false): string {
  const [y, m, d] = date.split("-").map(Number);
  // Constructed as a *local* date rather than parsed from the string: `new
  // Date("2026-08-04")` is midnight UTC, which in a negative offset renders as
  // the third of August — the exact off-by-one this milestone removed from the
  // core, reappearing in the formatter.
  const value = new Date(y, (m ?? 1) - 1, d ?? 1);
  return value.toLocaleDateString(
    locale,
    long
      ? { day: "numeric", month: "long", year: "numeric" }
      : { day: "numeric", month: "short" },
  );
}

/**
 * Which strength band a day falls in.
 *
 * Ranked against the busiest day in view rather than against a fixed token
 * count, because "busy" only means anything relative to how this person works.
 * A square-root ramp rather than a linear one: a single 15M-token day would
 * otherwise flatten a fortnight of real work into the palest band, and the
 * corpus this was built against has exactly that shape.
 */
function levelOf(tokens: number, busiest: number): number {
  if (tokens <= 0) return 0;
  if (busiest <= 0) return 1;
  const ratio = Math.sqrt(tokens / busiest);
  return Math.min(LEVELS, Math.max(1, Math.ceil(ratio * LEVELS)));
}

/**
 * The calendar, laid out in weeks.
 *
 * Columns are weeks and rows are weekdays — the shape everyone already knows
 * how to read — with two departures from the obvious version:
 *
 * * **A day with no history is not a day with no work.** Cells before the first
 *   recorded turn are drawn as absent, so a 90-day view on a fortnight of data
 *   does not invent seventy-six idle days.
 * * **The cells are buttons.** The picture is also the filter, which is what
 *   makes this worth more than the decoration it resembles.
 */
function Calendar({
  days,
  selected,
  historyFrom,
  onSelect,
}: {
  days: DayCell[];
  selected: string | null;
  historyFrom: string | null;
  onSelect: (date: string | null) => void;
}) {
  const t = useT();
  const { locale } = useI18n();
  const busiest = useMemo(
    () => Math.max(0, ...days.map((d) => d.tokens)),
    [days],
  );

  // Pad the first week so the grid starts on the right weekday.
  const weeks = useMemo(() => {
    const cells: (DayCell | null)[] = [...days];
    if (cells.length > 0) {
      const first = cells[0] as DayCell;
      const [y, m, d] = first.date.split("-").map(Number);
      const weekday = new Date(y, (m ?? 1) - 1, d ?? 1).getDay();
      for (let i = 0; i < weekday; i += 1) cells.unshift(null);
    }
    const out: (DayCell | null)[][] = [];
    for (let i = 0; i < cells.length; i += 7) out.push(cells.slice(i, i + 7));
    return out;
  }, [days]);

  const weekdayNames = useMemo(() => {
    const format = new Intl.DateTimeFormat(locale, { weekday: "narrow" });
    // 2026-08-02 was a Sunday; seven days from it names every weekday in order.
    return Array.from({ length: 7 }, (_, i) =>
      format.format(new Date(2026, 7, 2 + i)),
    );
  }, [locale]);

  // The month a column belongs to, printed once per month rather than once per
  // week — a strip that repeats "Aug" five times is noise pretending to be an
  // axis.
  const monthLabels = useMemo(() => {
    let previous = "";
    return weeks.map((week) => {
      const first = week.find(Boolean);
      if (!first) return null;
      const [y, m] = first.date.split("-").map(Number);
      const key = `${y}-${m}`;
      if (key === previous) return null;
      previous = key;
      return new Intl.DateTimeFormat(locale, { month: "short" }).format(
        new Date(y, (m ?? 1) - 1, 1),
      );
    });
  }, [weeks, locale]);

  /**
   * Whether each cell has room to print its own date.
   *
   * The cells are sized in fractions of the card, so a short window makes them
   * large — and a large empty square is a wasted one. Past this threshold the
   * calendar stops being a heatmap with a tooltip and becomes a calendar you
   * can actually read, which is the whole of "better than the reference".
   */
  const roomy = weeks.length <= 7;

  return (
    <div className="an-cal" data-roomy={roomy || undefined}>
      <div className="an-cal__weekdays" aria-hidden="true">
        {weekdayNames.map((name, i) => (
          <span key={i}>{roomy || i % 2 === 1 ? name : ""}</span>
        ))}
      </div>
      <div className="an-cal__body">
        <div
          className="an-cal__months"
          aria-hidden="true"
          style={{ "--an-weeks": weeks.length } as React.CSSProperties}
        >
          {monthLabels.map((label, i) => (
            <span key={i}>{label}</span>
          ))}
        </div>
        <div
          className="an-cal__grid"
          role="grid"
          aria-label={t("analytics.calendar.label")}
          style={{ "--an-weeks": weeks.length } as React.CSSProperties}
        >
          {weeks.map((week, wi) => (
            <div className="an-cal__week" key={wi} role="row">
              {Array.from({ length: 7 }, (_, di) => {
                const cell = week[di] ?? null;
                if (!cell)
                  return (
                    <span className="an-cal__cell an-cal__cell--pad" key={di} />
                  );

                const noHistory =
                  historyFrom !== null && cell.date < historyFrom;
                const level = noHistory ? -1 : levelOf(cell.tokens, busiest);
                const isSelected = selected === cell.date;
                const label = noHistory
                  ? t("analytics.calendar.noHistory", {
                      date: formatDay(cell.date, locale, true),
                    })
                  : cell.turns === 0
                    ? t("analytics.calendar.idle", {
                        date: formatDay(cell.date, locale, true),
                      })
                    : t("analytics.calendar.busy", {
                        date: formatDay(cell.date, locale, true),
                        tokens: formatTokens(cell.tokens),
                        turns: cell.turns,
                      });

                return (
                  <button
                    key={di}
                    type="button"
                    role="gridcell"
                    className="an-cal__cell"
                    data-level={level}
                    data-selected={isSelected || undefined}
                    // A day with nothing in it has nothing to filter to, and a
                    // filter that yields an empty screen is a trap rather than
                    // a feature.
                    disabled={noHistory || cell.turns === 0}
                    aria-label={label}
                    title={label}
                    onClick={() => onSelect(isSelected ? null : cell.date)}
                  >
                    {roomy && <span>{Number(cell.date.split("-")[2])}</span>}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
      </div>
      {/* Outside the scrolling box on purpose: as a child of it, its full width
          made the box overflow by a hair and drew a scrollbar under a calendar
          that fitted perfectly well. */}
      <div className="an-cal__legend">
        <span>{t("analytics.calendar.less")}</span>
        {Array.from({ length: LEVELS + 1 }, (_, i) => (
          <span className="an-cal__key" data-level={i} key={i} />
        ))}
        <span>{t("analytics.calendar.more")}</span>
      </div>
    </div>
  );
}

/** The 24-hour rhythm: when in the day this person actually works. */
function Rhythm({ hours }: { hours: HourBucket[] }) {
  const t = useT();
  const peak = Math.max(1, ...hours.map((h) => h.tokens));
  return (
    <div className="an-rhythm">
      <div className="an-rhythm__bars">
        {hours.map((bucket) => (
          <div
            className="an-rhythm__slot"
            key={bucket.hour}
            title={t("analytics.rhythm.hour", {
              hour: String(bucket.hour).padStart(2, "0"),
              tokens: formatTokens(bucket.tokens),
              turns: bucket.turns,
            })}
          >
            <div
              className="an-rhythm__bar"
              data-empty={bucket.tokens === 0 || undefined}
              style={{
                height: `${Math.max(2, (bucket.tokens / peak) * 100)}%`,
              }}
            />
          </div>
        ))}
      </div>
      <div className="an-rhythm__axis" aria-hidden="true">
        {[0, 6, 12, 18].map((hour) => (
          <span key={hour} style={{ left: `${(hour / 24) * 100}%` }}>
            {String(hour).padStart(2, "0")}h
          </span>
        ))}
      </div>
    </div>
  );
}

/** A named row with a magnitude bar. Confidence travels with the number (§28). */
function BucketRows({
  buckets,
  total,
}: {
  buckets: UsageBucket[];
  total: number;
}) {
  const t = useT();
  return (
    <ul className="an-bars">
      {buckets.slice(0, 8).map((bucket) => {
        const value = bucket.input + bucket.output + bucket.cacheWrite;
        return (
          <li key={bucket.label}>
            <span className="an-bars__label" title={bucket.label}>
              {bucket.label}
            </span>
            <span className="an-bars__track">
              <span
                className="an-bars__fill"
                style={{ width: `${total > 0 ? (value / total) * 100 : 0}%` }}
              />
            </span>
            <span
              className="an-bars__value"
              data-confidence={bucket.confidence}
              title={t(
                `analytics.confidence.${bucket.confidence}` as MessageKey,
              )}
            >
              {formatTokens(value)}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

export function Analytics() {
  const t = useT();
  const { locale } = useI18n();
  const [report, setReport] = useState<AnalyticsReport | null>(null);
  const [days, setDays] = useState(30);
  const [day, setDay] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const load = useCallback(async () => {
    if (!isTauri()) return;
    setLoading(true);
    try {
      setReport(
        await invoke<AnalyticsReport>("analytics_report", {
          windowDays: days,
          day,
        }),
      );
    } finally {
      setLoading(false);
    }
  }, [days, day]);

  useEffect(() => {
    void load();
  }, [load]);

  // Changing the window while a day is selected would leave a filter pinned to
  // a date that may no longer be on screen.
  const pickWindow = (value: number) => {
    setDay(null);
    setDays(value);
  };

  if (!report) {
    return (
      <div className="an an--loading">
        {loading ? t("common.loading") : null}
      </div>
    );
  }

  const { streaks, leverage, calendar } = report;
  const tokenTotal = report.byProvider.reduce(
    (sum, b) => sum + b.input + b.output + b.cacheWrite,
    0,
  );
  const nothingMeasured = report.historyFrom === null;

  // The ratio §53 exists for. Guarded rather than clamped: dividing by zero
  // minutes of attention would print Infinity on the most quotable figure here.
  const ratio =
    leverage.humanActiveMinutes > 0
      ? leverage.agentRuntimeMinutes / leverage.humanActiveMinutes
      : null;

  return (
    <div className="an">
      <div className="an__inner">
        <header className="an__header">
          <div>
            <h1>{t("analytics.title")}</h1>
            <p>
              {report.historyFrom
                ? t("analytics.subtitle", {
                    from: formatDay(report.historyFrom, locale, true),
                  })
                : t("analytics.empty.body")}
            </p>
          </div>
          <div
            className="an__windows"
            role="radiogroup"
            aria-label={t("analytics.title")}
          >
            {WINDOWS.map((value) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={days === value}
                data-active={days === value || undefined}
                onClick={() => pickWindow(value)}
              >
                {t("analytics.windowShort", { days: value })}
              </button>
            ))}
          </div>
        </header>

        {nothingMeasured ? (
          <div className="an__empty">
            <p>{t("analytics.empty.title")}</p>
            <span>{t("analytics.empty.body")}</span>
          </div>
        ) : (
          <>
            {/* ---- The calendar, which is both the picture and the filter --- */}
            <section className="an__card an__card--hero">
              <div className="an__card-head">
                <h2>
                  <CalendarDays size={14} aria-hidden="true" />
                  {t("analytics.calendar.title")}
                </h2>
                {day ? (
                  <button
                    type="button"
                    className="an__chip"
                    onClick={() => setDay(null)}
                  >
                    {formatDay(day, locale, true)}
                    <X size={12} aria-hidden="true" />
                  </button>
                ) : (
                  <span className="an__hint">
                    {t("analytics.calendar.hint")}
                  </span>
                )}
              </div>
              {/* The calendar keeps its natural size and the figures take the
                  room beside it — a five-column month marooned in a
                  thousand-pixel card reads as a rendering fault, which is
                  exactly how the first QA pass looked. */}
              <div className="an__hero">
                <Calendar
                  days={calendar}
                  selected={day}
                  historyFrom={report.historyFrom}
                  onSelect={setDay}
                />
                <div className="an__hero-stats">
                  <div className="an__stat">
                    <span className="an__stat-label">
                      <Flame size={12} aria-hidden="true" />
                      {t("analytics.streak.current")}
                    </span>
                    <strong>
                      {t("analytics.days", { count: streaks.current })}
                    </strong>
                    <span className="an__stat-note">
                      {t("analytics.streak.currentNote")}
                    </span>
                  </div>
                  <div className="an__stat">
                    <span className="an__stat-label">
                      {t("analytics.streak.longest")}
                    </span>
                    <strong>
                      {t("analytics.days", { count: streaks.longest })}
                    </strong>
                    <span className="an__stat-note">
                      {streaks.longestFrom && streaks.longestTo
                        ? t("analytics.streak.longestRange", {
                            from: formatDay(streaks.longestFrom, locale),
                            to: formatDay(streaks.longestTo, locale),
                          })
                        : ""}
                    </span>
                  </div>
                  <div className="an__stat">
                    <span className="an__stat-label">
                      <Activity size={12} aria-hidden="true" />
                      {t("analytics.streak.active")}
                    </span>
                    <strong>
                      {t("analytics.streak.activeOf", {
                        active: streaks.activeDays,
                        total: streaks.windowDays,
                      })}
                    </strong>
                    <span className="an__stat-note">
                      {t("analytics.streak.activeNote")}
                    </span>
                  </div>
                  <div className="an__stat">
                    <span className="an__stat-label">
                      <Layers size={12} aria-hidden="true" />
                      {t("analytics.tokens")}
                    </span>
                    <strong>{formatTokens(tokenTotal)}</strong>
                    <span className="an__stat-note">
                      {t("analytics.filesChangedCount", {
                        count: report.filesChanged,
                      })}
                    </span>
                  </div>
                </div>
              </div>
            </section>

            {/* ---- The figure no other tool on the machine can show (§53) --- */}
            <section className="an__card">
              <div className="an__card-head">
                <h2>{t("analytics.leverage")}</h2>
                {leverage.observedFrom && (
                  <span className="an__hint">
                    {t("analytics.leverage.since", {
                      from: formatDay(leverage.observedFrom, locale, true),
                    })}
                  </span>
                )}
              </div>
              <div className="an__leverage">
                <div>
                  <span>{t("analytics.humanActive")}</span>
                  <strong>{formatDuration(leverage.humanActiveMinutes)}</strong>
                </div>
                <div className="an__leverage-ratio">
                  {ratio !== null ? (
                    <>
                      <strong>{`${ratio.toFixed(1)}×`}</strong>
                      <span>{t("analytics.leverage.ratio")}</span>
                    </>
                  ) : (
                    <span className="an__hint">
                      {t("analytics.leverage.notYet")}
                    </span>
                  )}
                </div>
                <div>
                  <span>{t("analytics.agentRuntime")}</span>
                  <strong data-accent="true">
                    {formatDuration(leverage.agentRuntimeMinutes)}
                  </strong>
                </div>
              </div>
              <p className="an__note">{t("analytics.leverageNote")}</p>
            </section>

            {/* ---- Rhythm and breakdowns ------------------------------------ */}
            <div className="an__columns">
              <section className="an__card">
                <div className="an__card-head">
                  <h2>
                    <Clock3 size={14} aria-hidden="true" />
                    {t("analytics.rhythm.title")}
                  </h2>
                </div>
                <Rhythm hours={report.byHour} />
                <p className="an__note">{t("analytics.rhythm.note")}</p>
              </section>

              <section className="an__card">
                <div className="an__card-head">
                  <h2>{t("analytics.byProject")}</h2>
                </div>
                <BucketRows buckets={report.byProject} total={tokenTotal} />
              </section>

              <section className="an__card">
                <div className="an__card-head">
                  <h2>{t("analytics.byModel")}</h2>
                </div>
                <BucketRows buckets={report.byModel} total={tokenTotal} />
              </section>

              <section className="an__card">
                <div className="an__card-head">
                  <h2>{t("analytics.byProvider")}</h2>
                </div>
                <BucketRows buckets={report.byProvider} total={tokenTotal} />
              </section>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
