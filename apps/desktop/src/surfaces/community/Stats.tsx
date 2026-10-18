import { useI18n, useT } from "../../app/i18n";
import { ago, type Stats } from "./api";
import { formatCount } from "./parts";

/** 26 weeks, one square a day, in neutral steps: activity, not brand colour. */
export function Heatmap({ cells }: { cells: Stats["heatmap"] }) {
  const t = useT();
  const { locale } = useI18n();
  if (cells.length === 0) return null;
  const max = Math.max(...cells.map((c) => c.tokens), 1);
  const level = (tokens: number) => (tokens === 0 ? 0 : Math.min(4, 1 + Math.floor((tokens / max) * 3.999)));
  const first = new Date(`${cells[0].date}T00:00:00`).getDay();
  const size = 12;
  const gap = 3;
  const step = size + gap;
  const top = 16;
  const width = Math.ceil((cells.length + first) / 7) * step;
  // A month label over the first week that starts in it.
  const months: { x: number; label: string }[] = [];
  cells.forEach((cell, index) => {
    const date = new Date(`${cell.date}T00:00:00`);
    const slot = index + first;
    if (date.getDate() <= 7 && slot % 7 === 0) {
      months.push({ x: Math.floor(slot / 7) * step, label: date.toLocaleDateString(locale, { month: "short" }).replace(".", "") });
    }
  });
  return (
    <div className="cm-heatmap-block">
      <svg className="cm-heatmap" width={width} height={top + 7 * step} viewBox={`0 0 ${width} ${top + 7 * step}`} role="img" aria-label={t("community.heatmap.label")}>
        {months.map((month) => (
          <text key={`${month.x}-${month.label}`} x={month.x} y={10} className="cm-heatmap__month">{month.label}</text>
        ))}
        {cells.map((cell, index) => {
          const slot = index + first;
          return (
            <rect key={cell.date} x={Math.floor(slot / 7) * step} y={top + (slot % 7) * step} width={size} height={size} rx={2} data-level={level(cell.tokens)}>
              <title>{t("community.heatmap.cell", { date: new Date(`${cell.date}T00:00:00`).toLocaleDateString(locale), tokens: formatCount(cell.tokens, locale) })}</title>
            </rect>
          );
        })}
      </svg>
      <div className="cm-heatmap__legend" aria-hidden="true">
        <span>{t("community.heatmap.less")}</span>
        {[0, 1, 2, 3, 4].map((l) => <i key={l} data-level={l} />)}
        <span>{t("community.heatmap.more")}</span>
      </div>
    </div>
  );
}

/** How someone builds: aggregate numbers their machine published, each marked
 *  Observed — they are counted from transcripts, not stated by a provider. */
export function StatsPanel({ stats, self }: { stats: Stats; self: boolean }) {
  const t = useT();
  const { locale } = useI18n();
  const hours = (minutes: number) => Math.round(minutes / 60);
  return (
    <section className="cm-card cm-stats" aria-label={self ? t("community.stats.titleSelf") : t("community.stats.title")}>
      <header className="cm-card__head">
        <h3 className="cm-section-title">{self ? t("community.stats.titleSelf") : t("community.stats.title")}</h3>
        <span className="cm-badge" title={t("community.stats.updated", { when: ago(stats.observedAt, locale) })}>
          {t("community.stats.observed")} · {ago(stats.observedAt, locale)}
        </span>
      </header>
      <div className="cm-stats__grid">
        <div className="cm-stat">
          <b>{stats.streakCurrent}d</b>
          <span>{t("community.stats.streak", { longest: stats.streakLongest })}</span>
        </div>
        <div className="cm-stat">
          <b>{stats.activeDays30}/30</b>
          <span>{t("community.stats.active")}</span>
        </div>
        <div className="cm-stat">
          <b>{formatCount(stats.tokens30, locale)}</b>
          <span>{t("community.stats.tokens")}</span>
        </div>
        <div className="cm-stat">
          <b>{hours(stats.agentMinutes30)}h</b>
          <span>{t("community.stats.agent", { human: hours(stats.humanMinutes30) })}</span>
        </div>
      </div>
      {stats.topModels.length > 0 && (
        <div className="cm-models">
          <span className="cm-muted">{t("community.stats.models")}</span>
          <div className="cm-models__bars">
            {stats.topModels.map((model) => (
              <div key={model.label} className="cm-model">
                <span className="cm-model__label">{model.label}</span>
                <span className="cm-model__bar"><span style={{ width: `${Math.max(2, model.share * 100)}%` }} /></span>
                <span className="cm-model__share">{model.share > 0 && model.share < 0.01 ? "<1" : Math.round(model.share * 100)}%</span>
              </div>
            ))}
          </div>
        </div>
      )}
      <div className="cm-heatmap-wrap">
        <Heatmap cells={stats.heatmap} />
      </div>
    </section>
  );
}
