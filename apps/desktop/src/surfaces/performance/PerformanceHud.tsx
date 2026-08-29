import { useEffect, useMemo, useState } from "react";
import { Cpu, X, Zap } from "lucide-react";
import type { ConversationItem, TokenUsage } from "../conversation/ConversationView";
import { readLiveTokenProgress } from "../terminal/live";
import { invoke, isTauri } from "../../app/platform";
import { readGpu, throttleReason, type GpuMetrics } from "../../app/gpu";
import type { SessionKind } from "../../app/sessions";
import { useT } from "../../app/i18n";
import "./PerformanceHud.css";

/**
 * The context window a local session is really working inside (§92).
 *
 * Only asked for a local session, and only because the number the agent itself
 * believes can be wrong: with no metadata for the model, Codex falls back to an
 * invented window — 258400 tokens, against a runner actually loaded at 65536 on
 * this machine. Reading the runtime tells the HUD which of the two is true, so
 * "latest context" can be shown as a share of something real rather than as a
 * bare count with no ceiling.
 */
interface LocalWindow {
  tokens: number;
  model: string | null;
}

interface SystemMetrics {
  processMemoryBytes: number | null;
  systemMemoryUsedBytes: number | null;
  systemMemoryTotalBytes: number | null;
}

interface TurnRate {
  output: number;
  durationSeconds: number;
  rate: number;
}

interface DerivedStats {
  latestRate: number | null;
  averageRate: number | null;
  totalTokens: number;
  contextTokens: number | null;
  model: string | null;
  rates: number[];
  official: boolean;
  hasUsage: boolean;
}

interface LiveSpeed {
  observed: boolean;
  active: boolean;
  currentRate: number | null;
  lastRate: number | null;
  averageRate: number | null;
  rates: number[];
}

const POLL_MS = 1_500;
const LIVE_POLL_MS = 400;
const LIVE_STALE_MS = 1_800;
const INITIAL_LIVE_SPEED: LiveSpeed = {
  observed: false,
  active: false,
  currentRate: null,
  lastRate: null,
  averageRate: null,
  rates: [],
};

function usageTotal(usage: TokenUsage): number {
  return (usage.input ?? 0) + (usage.output ?? 0) + (usage.cacheRead ?? 0) + (usage.cacheWrite ?? 0);
}

/**
 * Derive effective turn throughput from provider-reported usage.
 *
 * Providers expose the exact output-token count at the end of a response, not
 * a timestamp for every streamed token. Dividing that official count by wall
 * time from the user's prompt to the last assistant sample is therefore the
 * narrowest honest live metric: it includes tool time, and never substitutes
 * terminal bytes or a tokenizer guess for provider data.
 */
export function derivePerformanceStats(items: ConversationItem[]): DerivedStats {
  const turns: TurnRate[] = [];
  let turnStartedAt: number | null = null;
  let turnEndedAt: number | null = null;
  let turnOutput = 0;
  let totalTokens = 0;
  let contextTokens: number | null = null;
  let model: string | null = null;
  let official = true;
  let hasUsage = false;

  const finishTurn = () => {
    if (turnStartedAt == null || turnEndedAt == null || turnOutput <= 0) return;
    const durationSeconds = Math.max(0.25, (turnEndedAt - turnStartedAt) / 1_000);
    turns.push({ output: turnOutput, durationSeconds, rate: turnOutput / durationSeconds });
  };

  for (const item of items) {
    if (item.kind !== "message") continue;
    if (item.role === "user") {
      finishTurn();
      turnStartedAt = item.tsMs;
      turnEndedAt = null;
      turnOutput = 0;
      continue;
    }
    if (item.role !== "assistant" || !item.usage) continue;

    const usage = item.usage;
    hasUsage = true;
    totalTokens += usageTotal(usage);
    turnOutput += usage.output ?? 0;
    turnEndedAt = item.tsMs;
    contextTokens = (usage.input ?? 0) + (usage.cacheRead ?? 0);
    model = usage.model ?? model;
    official = official && (usage.confidence == null || usage.confidence === "official");
  }
  finishTurn();

  const recent = turns.slice(-10);
  const output = recent.reduce((sum, turn) => sum + turn.output, 0);
  const seconds = recent.reduce((sum, turn) => sum + turn.durationSeconds, 0);

  return {
    latestRate: turns.at(-1)?.rate ?? null,
    averageRate: seconds > 0 ? output / seconds : null,
    totalTokens,
    contextTokens,
    model,
    rates: turns.slice(-18).map((turn) => turn.rate),
    official,
    hasUsage,
  };
}

function formatCompact(value: number, locale: string): string {
  return new Intl.NumberFormat(locale, {
    notation: value >= 1_000 ? "compact" : "standard",
    maximumFractionDigits: value < 100 ? 1 : 0,
  }).format(value);
}

function formatMemory(bytes: number | null, locale: string): string {
  if (bytes == null) return "—";
  const mib = bytes / 1024 / 1024;
  if (mib < 1_024) return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(mib)} MB`;
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(mib / 1_024)} GB`;
}

function Sparkline({ values }: { values: number[] }) {
  const points = useMemo(() => {
    if (values.length < 2) return "0,22 100,22";
    const max = Math.max(...values, 1);
    return values
      .map((value, index) => `${(index / (values.length - 1)) * 100},${24 - (value / max) * 20}`)
      .join(" ");
  }, [values]);

  return (
    <svg className="perf-hud__spark" viewBox="0 0 100 28" preserveAspectRatio="none" aria-hidden="true">
      <path d="M0 24H100" className="perf-hud__spark-base" />
      <polyline points={points} className="perf-hud__spark-line" />
    </svg>
  );
}

export function PerformanceHud({
  sessionId,
  kind,
  onClose,
}: {
  sessionId: string;
  /** Which provider this session runs on. Only a local one has a card to report. */
  kind?: SessionKind;
  onClose: () => void;
}) {
  const t = useT();
  const locale = navigator.language;
  const [items, setItems] = useState<ConversationItem[]>([]);
  const [liveSpeed, setLiveSpeed] = useState<LiveSpeed>(INITIAL_LIVE_SPEED);
  const [gpu, setGpu] = useState<GpuMetrics | null>(null);
  const [localWindow, setLocalWindow] = useState<LocalWindow | null>(null);
  const [system, setSystem] = useState<SystemMetrics>({
    processMemoryBytes: null,
    systemMemoryUsedBytes: null,
    systemMemoryTotalBytes: null,
  });

  const isLocal = kind === "local";

  useEffect(() => {
    let cancelled = false;
    const read = async () => {
      if (!isTauri()) return;
      const [conversation, machine] = await Promise.allSettled([
        invoke<ConversationItem[]>("session_conversation", { sessionId }),
        invoke<SystemMetrics>("system_metrics"),
      ]);
      if (cancelled) return;
      if (conversation.status === "fulfilled") setItems(conversation.value);
      if (machine.status === "fulfilled") setSystem(machine.value);
    };
    void read();
    const timer = window.setInterval(() => void read(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [sessionId]);

  // The card, and what the local runner was really loaded with.
  //
  // Only for a local session: when the model is in somebody else's datacentre,
  // this machine's GPU has nothing to do with how fast the answer arrives, and
  // a power meter beside a cloud agent's throughput would invite exactly the
  // wrong conclusion.
  useEffect(() => {
    if (!isLocal || !isTauri()) {
      setGpu(null);
      setLocalWindow(null);
      return;
    }
    let cancelled = false;
    const read = async () => {
      const [metrics, runtime] = await Promise.allSettled([
        readGpu(),
        invoke<{
          config: { model: string | null };
          resident: { name: string; contextLength: number | null }[];
        }>("local_runtime_report"),
      ]);
      if (cancelled) return;
      if (metrics.status === "fulfilled") setGpu(metrics.value);
      if (runtime.status === "fulfilled") {
        const model = runtime.value.config.model;
        const resident = runtime.value.resident.find((entry) => entry.name === model);
        setLocalWindow(
          resident?.contextLength ? { tokens: resident.contextLength, model } : null,
        );
      }
    };
    void read();
    const timer = window.setInterval(() => void read(), POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isLocal, sessionId]);

  useEffect(() => {
    let previous = readLiveTokenProgress(sessionId);
    let lastChangedAt = previous ? Date.now() : 0;
    let activeTokens = 0;
    let activeSeconds = 0;
    let pendingTokens = 0;
    let rates: number[] = [];
    let wasStale = false;

    const firstRate = previous ? previous.outputTokens / previous.elapsedSeconds : null;
    if (firstRate != null) {
      rates = [firstRate];
      setLiveSpeed({
        observed: true,
        active: true,
        currentRate: firstRate,
        lastRate: firstRate,
        averageRate: firstRate,
        rates,
      });
    } else {
      setLiveSpeed(INITIAL_LIVE_SPEED);
    }

    const sample = () => {
      const now = Date.now();
      const progress = readLiveTokenProgress(sessionId);

      if (!progress) {
        if (lastChangedAt && now - lastChangedAt >= LIVE_STALE_MS) {
          wasStale = true;
          setLiveSpeed((value) => value.active ? { ...value, active: false, currentRate: value.lastRate } : value);
        }
        return;
      }

      if (wasStale || !previous || progress.elapsedSeconds < previous.elapsedSeconds || progress.outputTokens < previous.outputTokens) {
        const rate = progress.outputTokens / progress.elapsedSeconds;
        previous = progress;
        lastChangedAt = now;
        activeTokens = 0;
        activeSeconds = 0;
        pendingTokens = 0;
        rates = [rate];
        wasStale = false;
        setLiveSpeed({ observed: true, active: true, currentRate: rate, lastRate: rate, averageRate: rate, rates });
        return;
      }

      const tokenDelta = progress.outputTokens - previous.outputTokens;
      const secondDelta = progress.elapsedSeconds - previous.elapsedSeconds;
      if (tokenDelta === 0 && secondDelta === 0) {
        if (now - lastChangedAt >= LIVE_STALE_MS) {
          wasStale = true;
          setLiveSpeed((value) => value.active ? { ...value, active: false, currentRate: value.lastRate } : value);
        }
        return;
      }

      previous = progress;
      lastChangedAt = now;
      wasStale = false;
      pendingTokens += Math.max(0, tokenDelta);

      if (pendingTokens > 0 && secondDelta > 0) {
        const rate = pendingTokens / secondDelta;
        activeTokens += pendingTokens;
        activeSeconds += secondDelta;
        pendingTokens = 0;
        rates = [...rates, rate].slice(-18);
        setLiveSpeed((value) => ({
          observed: true,
          active: true,
          currentRate: rate,
          lastRate: rate,
          averageRate: activeSeconds > 0 ? activeTokens / activeSeconds : value.averageRate,
          rates,
        }));
      } else if (secondDelta > 0) {
        // The provider is still advancing its clock but produced no new token:
        // typically it is using a tool or waiting on I/O. Zero is the honest
        // instantaneous rate, while `lastRate` preserves the latest generation.
        setLiveSpeed((value) => ({ ...value, observed: true, active: true, currentRate: 0 }));
      } else {
        // Token totals can refresh more often than the provider's whole-second
        // clock. Accumulate them until the next elapsed-time tick rather than
        // inventing an infinite rate or briefly flashing zero.
        setLiveSpeed((value) => ({ ...value, observed: true, active: true }));
      }
    };

    const timer = window.setInterval(sample, LIVE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [sessionId]);

  const stats = useMemo(() => derivePerformanceStats(items), [items]);
  const shownRate = liveSpeed.observed
    ? (liveSpeed.active ? liveSpeed.currentRate : liveSpeed.lastRate)
    : stats.latestRate;
  const shownAverage = liveSpeed.averageRate ?? stats.averageRate;
  const shownRates = liveSpeed.rates.length > 0 ? liveSpeed.rates : stats.rates;
  const ramPercent =
    system.systemMemoryUsedBytes != null && system.systemMemoryTotalBytes
      ? (system.systemMemoryUsedBytes / system.systemMemoryTotalBytes) * 100
      : null;

  return (
    <aside className="perf-hud" aria-label={t("performance.title")}>
      <header className="perf-hud__header">
        <span className="perf-hud__live" data-live={liveSpeed.active || undefined} aria-hidden="true" />
        <span className="perf-hud__title">{t("performance.live")}</span>
        {stats.model && <span className="perf-hud__model">{stats.model}</span>}
        <button type="button" className="perf-hud__close" onClick={onClose} aria-label={t("performance.hide")}>
          <X size={12} strokeWidth={2} aria-hidden="true" />
        </button>
      </header>

      <div className="perf-hud__hero" title={t("performance.rateHelp")}>
        <div>
          <span className="perf-hud__eyebrow">
            {liveSpeed.active
              ? t("performance.now")
              : liveSpeed.observed
                ? t("performance.lastGeneration")
                : t("performance.lastTurn")}
          </span>
          <div className="perf-hud__rate">
            <strong>{shownRate == null ? "—" : formatCompact(shownRate, locale)}</strong>
            <span>{t("performance.tokensPerSecond")}</span>
          </div>
        </div>
        <Sparkline values={shownRates} />
      </div>

      <div className="perf-hud__grid">
        <Metric
          label={liveSpeed.observed ? t("performance.generationAverage") : t("performance.average")}
          value={shownAverage == null ? "—" : `${formatCompact(shownAverage, locale)} tok/s`}
        />
        <Metric label={t("performance.tokens")} value={formatCompact(stats.totalTokens, locale)} />
        <Metric
          label={localWindow ? t("performance.contextWindow") : t("performance.context")}
          // With a measured window, the count means something: 12k of 65k is a
          // different situation from 12k of 8k, and the bare number cannot say
          // which one it is.
          value={
            stats.contextTokens == null
              ? "—"
              : localWindow
                ? t("performance.contextShare", {
                    used: formatCompact(stats.contextTokens, locale),
                    total: formatCompact(localWindow.tokens, locale),
                  })
                : formatCompact(stats.contextTokens, locale)
          }
        />
        <Metric label={t("performance.appMemory")} value={formatMemory(system.processMemoryBytes, locale)} />
      </div>

      {/* The card, only where it is the thing producing the tokens (§92). */}
      {isLocal && gpu && <GpuStrip gpu={gpu} locale={locale} />}

      <div className="perf-hud__ram">
        <span><Cpu size={11} strokeWidth={1.8} aria-hidden="true" />{t("performance.systemRam")}</span>
        <div className="perf-hud__ram-track" aria-hidden="true">
          <span style={{ width: `${Math.min(ramPercent ?? 0, 100)}%` }} />
        </div>
        <strong>{ramPercent == null ? "—" : `${Math.round(ramPercent)}%`}</strong>
      </div>

      <p
        className="perf-hud__source"
        data-live={liveSpeed.observed || undefined}
        data-official={(!liveSpeed.observed && stats.hasUsage && stats.official) || undefined}
      >
        {liveSpeed.observed
          ? t("performance.observed")
          : !stats.hasUsage
          ? t("performance.waiting")
          : stats.official
            ? t("performance.official")
            : t("performance.derived")}
      </p>
    </aside>
  );
}

/**
 * The card, compressed to what explains a number on this HUD.
 *
 * Three readings and one sentence. Power against the limit in force is first
 * because on a 375 W cap it is what a sustained generation runs into; the
 * sentence beneath it is the only part that says *why* throughput moved, and it
 * is the reason this strip exists rather than a row of numbers.
 */
function GpuStrip({ gpu, locale }: { gpu: GpuMetrics; locale: string }) {
  const t = useT();
  const reason = throttleReason(gpu);
  const percent =
    gpu.powerDrawWatts != null && gpu.powerLimitWatts
      ? Math.min(100, (gpu.powerDrawWatts / gpu.powerLimitWatts) * 100)
      : 0;

  return (
    <div className="perf-hud__gpu" data-throttled={reason ?? undefined}>
      <span className="perf-hud__gpu-head">
        <Zap size={11} strokeWidth={1.8} aria-hidden="true" />
        {t("performance.gpu")}
        <strong>
          {gpu.powerDrawWatts == null || gpu.powerLimitWatts == null
            ? "—"
            : t("gpu.powerOfLimit", {
                draw: Math.round(gpu.powerDrawWatts),
                limit: Math.round(gpu.powerLimitWatts),
              })}
        </strong>
      </span>
      <div className="perf-hud__gpu-track" aria-hidden="true">
        <span style={{ width: `${percent}%` }} />
      </div>
      <span className="perf-hud__gpu-facts">
        {gpu.temperatureC == null ? "—" : `${Math.round(gpu.temperatureC)} °C`}
        {" · "}
        {gpu.utilizationPercent == null ? "—" : `${Math.round(gpu.utilizationPercent)}%`}
        {gpu.memoryUsedMib != null && gpu.memoryTotalMib != null
          ? ` · ${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
              gpu.memoryUsedMib / 1024,
            )}/${new Intl.NumberFormat(locale, { maximumFractionDigits: 0 }).format(
              gpu.memoryTotalMib / 1024,
            )} GB`
          : ""}
      </span>
      <span className="perf-hud__gpu-reason">
        {reason === "power"
          ? t("gpu.throttlePower")
          : reason === "thermal"
            ? t("gpu.throttleThermal")
            : reason === "slowdown"
              ? t("gpu.throttleSlowdown")
              : t("gpu.throttleNone")}
      </span>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="perf-hud__metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
