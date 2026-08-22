import { useEffect, useState } from "react";
import { invoke, isTauri } from "../../app/platform";
import { useT } from "../../app/i18n";
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

interface AnalyticsReport {
  byProvider: UsageBucket[];
  byModel: UsageBucket[];
  byProject: UsageBucket[];
  byDay: UsageBucket[];
  leverage: {
    humanActiveMinutes: number;
    agentRuntimeMinutes: number;
    sessions: number;
  };
  filesChanged: number;
  windowDays: number;
}

function formatTokens(value: number): string {
  if (value < 1000) return String(value);
  if (value < 1_000_000) return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)}k`;
  return `${(value / 1_000_000).toFixed(1)}M`;
}

function formatDuration(minutes: number): string {
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours}h` : `${hours}h ${rest}m`;
}

/**
 * Analytics (§52, §53).
 *
 * Every figure answers a question someone would ask. Nothing here exists to
 * fill space, and nothing is turned into a score.
 *
 * The bars are magnitude, not identity — each row is already named beside its
 * bar — so they all use one hue. Colouring them by rank would double-encode the
 * length as hue and spend the only free channel on information the bar already
 * carries.
 */
export function Analytics() {
  const t = useT();
  const [report, setReport] = useState<AnalyticsReport | null>(null);
  const [days, setDays] = useState(30);

  useEffect(() => {
    if (!isTauri()) return;
    let cancelled = false;
    void invoke<AnalyticsReport>("analytics_report", { windowDays: days }).then((next) => {
      if (!cancelled) setReport(next);
    });
    return () => {
      cancelled = true;
    };
  }, [days]);

  const totals = report?.byProvider.reduce(
    (acc, b) => ({
      input: acc.input + b.input,
      output: acc.output + b.output,
      cacheRead: acc.cacheRead + b.cacheRead,
      cacheWrite: acc.cacheWrite + b.cacheWrite,
    }),
    { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 },
  );

  const nothingMeasured =
    report && report.byProvider.length === 0 && report.leverage.agentRuntimeMinutes === 0;

  return (
    <div className="an">
      <div className="an__inner">
        <header className="an__header">
          <h1 className="an__title">{t("analytics.title")}</h1>
          <div className="an__window" role="radiogroup">
            {[7, 30, 90].map((value) => (
              <button
                key={value}
                type="button"
                role="radio"
                aria-checked={days === value}
                data-active={days === value || undefined}
                className="an__window-option"
                onClick={() => setDays(value)}
              >
                {value}d
              </button>
            ))}
          </div>
        </header>

        {nothingMeasured ? (
          <div className="an__empty">
            <p className="an__empty-title">{t("analytics.empty.title")}</p>
            <p className="an__empty-body">{t("analytics.empty.body")}</p>
          </div>
        ) : (
          report && (
            <>
              {/* §53 — the headline. Two numbers, not a chart: the story is the
                  comparison, and a two-bar chart would say less than this. */}
              <section className="an__section">
                <h2 className="an__section-title">{t("analytics.leverage")}</h2>
                <div className="an__leverage">
                  <div className="an__stat">
                    <span className="an__stat-label">{t("analytics.humanActive")}</span>
                    <span className="an__stat-value">
                      {formatDuration(report.leverage.humanActiveMinutes)}
                    </span>
                  </div>
                  <div className="an__stat an__stat--accent">
                    <span className="an__stat-label">{t("analytics.agentRuntime")}</span>
                    <span className="an__stat-value">
                      {formatDuration(report.leverage.agentRuntimeMinutes)}
                    </span>
                  </div>
                  <div className="an__stat">
                    <span className="an__stat-label">{t("analytics.sessions")}</span>
                    <span className="an__stat-value">{report.leverage.sessions}</span>
                  </div>
                  <div className="an__stat">
                    <span className="an__stat-label">{t("analytics.filesChanged")}</span>
                    <span className="an__stat-value">{report.filesChanged}</span>
                  </div>
                </div>
                <p className="an__note">{t("analytics.leverageNote")}</p>
              </section>

              {totals && (
                <section className="an__section">
                  <h2 className="an__section-title">{t("analytics.tokens")}</h2>
                  <div className="an__leverage">
                    <div className="an__stat">
                      <span className="an__stat-label">{t("analytics.input")}</span>
                      <span className="an__stat-value">{formatTokens(totals.input)}</span>
                    </div>
                    <div className="an__stat">
                      <span className="an__stat-label">{t("analytics.output")}</span>
                      <span className="an__stat-value">{formatTokens(totals.output)}</span>
                    </div>
                    <div className="an__stat">
                      <span className="an__stat-label">{t("analytics.cacheRead")}</span>
                      <span className="an__stat-value">{formatTokens(totals.cacheRead)}</span>
                    </div>
                    <div className="an__stat">
                      <span className="an__stat-label">{t("analytics.cacheWrite")}</span>
                      <span className="an__stat-value">{formatTokens(totals.cacheWrite)}</span>
                    </div>
                  </div>
                </section>
              )}

              <Breakdown title={t("analytics.byProvider")} buckets={report.byProvider} />
              <Breakdown title={t("analytics.byModel")} buckets={report.byModel} />
              <Breakdown title={t("analytics.byProject")} buckets={report.byProject} />
              <Breakdown title={t("analytics.byDay")} buckets={report.byDay} />
            </>
          )
        )}
      </div>
    </div>
  );
}

/**
 * A magnitude breakdown.
 *
 * Rows carry their own labels, so there is no legend and no per-series colour —
 * one hue, length does the work.
 */
function Breakdown({ title, buckets }: { title: string; buckets: UsageBucket[] }) {
  const t = useT();
  // An empty breakdown says nothing; it is removed rather than shown empty.
  if (buckets.length === 0) return null;

  // A single category has nothing to compare against, so a bar would just be a
  // full-width rectangle restating the number beside it. The number is the
  // chart — comparison is what earns a bar.
  const comparable = buckets.length > 1;

  const max = Math.max(...buckets.map((b) => b.input + b.output), 1);

  return (
    <section className="an__section">
      <h2 className="an__section-title">{title}</h2>
      <ul className="an__bars">
        {buckets.slice(0, 12).map((bucket) => {
          const total = bucket.input + bucket.output;
          return (
            <li key={bucket.label} className="an__bar-row" data-plain={!comparable || undefined}>
              <span className="an__bar-label" title={bucket.label}>
                {bucket.label}
              </span>
              {comparable ? (
                <span className="an__bar-track">
                  <span
                    className="an__bar-fill"
                    style={{ width: `${Math.max((total / max) * 100, 1.5)}%` }}
                  />
                </span>
              ) : (
                <span />
              )}
              <span className="an__bar-value">{formatTokens(total)}</span>
              {/* §28 — the number never appears without its provenance. */}
              <span
                className="an__bar-confidence"
                data-confidence={bucket.confidence}
                title={t(`analytics.confidence.${bucket.confidence}` as MessageKey)}
              >
                {bucket.confidence === "official" ? "" : "~"}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
