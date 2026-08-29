import { useMemo } from "react";
import { Gauge, Thermometer, Zap } from "lucide-react";
import { useT } from "../../app/i18n";
import { throttleReason, type GpuMetrics } from "../../app/gpu";

/**
 * What the card is doing, and what is holding it back (§92).
 *
 * Built around one claim: with a local model, throughput is an effect and this
 * panel is the cause. So the largest thing on it is power against the limit in
 * force — a 3090 Ti generating tokens sits against a 375 W cap long before it
 * gets near a thermal one — and the loudest thing on it is the throttle line,
 * because that is the reading that turns "it got slower" into something a
 * person can act on.
 */
export function GpuPanel({ gpu, locale }: { gpu: GpuMetrics | null; locale: string }) {
  const t = useT();

  if (!gpu) {
    return (
      <section className="local__card local__card--empty">
        <h2>{t("gpu.title")}</h2>
        <p className="local__muted">{t("gpu.absent")}</p>
      </section>
    );
  }

  const reason = throttleReason(gpu);
  const powerPercent =
    gpu.powerDrawWatts != null && gpu.powerLimitWatts
      ? Math.min(100, (gpu.powerDrawWatts / gpu.powerLimitWatts) * 100)
      : null;
  const vramPercent =
    gpu.memoryUsedMib != null && gpu.memoryTotalMib
      ? Math.min(100, (gpu.memoryUsedMib / gpu.memoryTotalMib) * 100)
      : null;
  const free =
    gpu.memoryUsedMib != null && gpu.memoryTotalMib != null
      ? Math.max(0, gpu.memoryTotalMib - gpu.memoryUsedMib)
      : null;

  return (
    <section className="local__card">
      <header className="local__card-head">
        <h2>{gpu.name ?? t("gpu.title")}</h2>
        {gpu.driverVersion && (
          <span className="local__muted">{t("gpu.driver", { version: gpu.driverVersion })}</span>
        )}
      </header>

      <div
        className="local__throttle"
        data-throttled={reason ? reason : undefined}
        title={t("gpu.throttleHelp")}
      >
        {reason === "power"
          ? t("gpu.throttlePower")
          : reason === "thermal"
            ? t("gpu.throttleThermal")
            : reason === "slowdown"
              ? t("gpu.throttleSlowdown")
              : t("gpu.throttleNone")}
      </div>

      <div className="local__meters">
        <Meter
          icon={<Zap size={13} strokeWidth={1.9} aria-hidden="true" />}
          label={t("gpu.power")}
          value={
            gpu.powerDrawWatts != null && gpu.powerLimitWatts != null
              ? t("gpu.powerOfLimit", {
                  draw: format(gpu.powerDrawWatts, locale, 0),
                  limit: format(gpu.powerLimitWatts, locale, 0),
                })
              : "—"
          }
          percent={powerPercent}
          history={gpu.powerHistory}
          caption={
            gpu.powerLimitWatts != null && gpu.powerMaxWatts != null
              ? t("gpu.powerHeadroom", {
                  limit: format(gpu.powerLimitWatts, locale, 0),
                  max: format(gpu.powerMaxWatts, locale, 0),
                })
              : undefined
          }
          // The one meter that is deliberately red-lined: the point of showing
          // it is the moment it reaches the top.
          alert={gpu.throttle.powerCap}
        />

        <Meter
          icon={<Gauge size={13} strokeWidth={1.9} aria-hidden="true" />}
          label={t("gpu.utilization")}
          value={gpu.utilizationPercent == null ? "—" : `${Math.round(gpu.utilizationPercent)}%`}
          percent={gpu.utilizationPercent}
          history={gpu.utilizationHistory}
          caption={
            gpu.memoryUtilizationPercent == null
              ? undefined
              : `${t("gpu.memoryUtilization")} ${Math.round(gpu.memoryUtilizationPercent)}%`
          }
        />

        <Meter
          icon={<Thermometer size={13} strokeWidth={1.9} aria-hidden="true" />}
          label={t("gpu.temperature")}
          value={gpu.temperatureC == null ? "—" : `${Math.round(gpu.temperatureC)} °C`}
          // Scaled to 100 °C rather than to the highest sample: a temperature
          // bar that rescales itself makes 45 °C and 85 °C look identical.
          percent={gpu.temperatureC == null ? null : gpu.temperatureC}
          history={gpu.temperatureHistory}
          caption={gpu.fanPercent == null ? undefined : `${t("gpu.fan")} ${Math.round(gpu.fanPercent)}%`}
          alert={gpu.throttle.hardwareThermal || gpu.throttle.softwareThermal}
        />
      </div>

      <dl className="local__facts">
        <Fact
          label={t("gpu.vram")}
          value={
            gpu.memoryUsedMib != null && gpu.memoryTotalMib != null
              ? `${format(gpu.memoryUsedMib / 1024, locale, 1)} / ${format(
                  gpu.memoryTotalMib / 1024,
                  locale,
                  1,
                )} GB`
              : "—"
          }
          note={free == null ? undefined : t("gpu.vramFree", { free: `${format(free / 1024, locale, 1)} GB` })}
          percent={vramPercent}
        />
        <Fact
          label={t("gpu.clock")}
          value={gpu.clockSmMhz == null ? "—" : `${format(gpu.clockSmMhz, locale, 0)} MHz`}
          note={
            gpu.clockSmMaxMhz == null
              ? undefined
              : `/ ${format(gpu.clockSmMaxMhz, locale, 0)} MHz`
          }
        />
        <Fact label={t("gpu.state")} value={gpu.performanceState ?? "—"} />
      </dl>
    </section>
  );
}

function Meter({
  icon,
  label,
  value,
  percent,
  history,
  caption,
  alert,
}: {
  icon: React.ReactNode;
  label: string;
  value: string;
  percent: number | null;
  history: number[];
  caption?: string;
  alert?: boolean;
}) {
  return (
    <div className="local__meter" data-alert={alert || undefined}>
      <span className="local__meter-label">
        {icon}
        {label}
      </span>
      <strong className="local__meter-value">{value}</strong>
      <div className="local__meter-track" aria-hidden="true">
        <span style={{ width: `${Math.max(0, Math.min(100, percent ?? 0))}%` }} />
      </div>
      <Trace values={history} />
      {caption && <span className="local__meter-caption">{caption}</span>}
    </div>
  );
}

/**
 * The last minute of samples.
 *
 * Scaled to the highest sample in the window rather than to a fixed ceiling,
 * because the shape is the information here — the moment a limit started
 * biting — and a flat line at 2% of 450 W shows nothing at all.
 */
function Trace({ values }: { values: number[] }) {
  const points = useMemo(() => {
    if (values.length < 2) return null;
    const peak = Math.max(...values, 1);
    return values
      .map((value, index) => `${(index / (values.length - 1)) * 100},${20 - (value / peak) * 18}`)
      .join(" ");
  }, [values]);

  if (!points) return <div className="local__trace" />;
  return (
    <svg className="local__trace" viewBox="0 0 100 22" preserveAspectRatio="none" aria-hidden="true">
      <polyline points={points} />
    </svg>
  );
}

function Fact({
  label,
  value,
  note,
  percent,
}: {
  label: string;
  value: string;
  note?: string;
  percent?: number | null;
}) {
  return (
    <div className="local__fact">
      <dt>{label}</dt>
      <dd>
        <strong>{value}</strong>
        {note && <span>{note}</span>}
        {percent != null && (
          <div className="local__fact-track" aria-hidden="true">
            <span style={{ width: `${Math.max(0, Math.min(100, percent))}%` }} />
          </div>
        )}
      </dd>
    </div>
  );
}

function format(value: number, locale: string, digits: number): string {
  return new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
}
