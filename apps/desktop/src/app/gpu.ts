import { invoke, isTauri } from "./platform";

/**
 * GPU telemetry (§92).
 *
 * Read by two surfaces for two different questions, which is why it lives here
 * rather than inside either of them: the live HUD asks *why is this turn slow
 * right now*, and the Local model screen asks *what will this machine do*. Both
 * are answered from the same sample, so neither can contradict the other.
 *
 * Every field is nullable, and that is load-bearing. `nvidia-smi` returns
 * `[N/A]` for readings a consumer card does not expose under Windows, and the
 * core parses those to null rather than to zero — a chart of confident zeroes
 * would be worse than a gap.
 */
export interface GpuMetrics {
  name: string | null;
  driverVersion: string | null;
  powerDrawWatts: number | null;
  /** The limit in force — 375 W here — not the card's 450 W ceiling. */
  powerLimitWatts: number | null;
  powerMaxWatts: number | null;
  temperatureC: number | null;
  utilizationPercent: number | null;
  /** Memory *bandwidth*, which is what saturates first while decoding. */
  memoryUtilizationPercent: number | null;
  memoryUsedMib: number | null;
  memoryTotalMib: number | null;
  clockSmMhz: number | null;
  clockSmMaxMhz: number | null;
  clockMemoryMhz: number | null;
  fanPercent: number | null;
  performanceState: string | null;
  throttle: {
    powerCap: boolean;
    hardwareSlowdown: boolean;
    hardwareThermal: boolean;
    softwareThermal: boolean;
  };
  powerHistory: number[];
  utilizationHistory: number[];
  temperatureHistory: number[];
}

/** The current GPU state, or null when there is no NVIDIA card to ask. */
export async function readGpu(): Promise<GpuMetrics | null> {
  if (!isTauri()) return null;
  try {
    return await invoke<GpuMetrics | null>("gpu_metrics");
  } catch {
    // A driver that failed to answer is the same to every caller as no card:
    // there is nothing to show, and nothing anybody can do about it here.
    return null;
  }
}

export function throttleReason(metrics: GpuMetrics): "power" | "thermal" | "slowdown" | null {
  // Ordered by what a person can act on. A card that is both power-capped and
  // hot is, in practice, power-capped: that is the limit that was set by hand
  // and can be raised by hand.
  if (metrics.throttle.powerCap) return "power";
  if (metrics.throttle.hardwareThermal || metrics.throttle.softwareThermal) return "thermal";
  if (metrics.throttle.hardwareSlowdown) return "slowdown";
  return null;
}
