/**
 * The dial that answers "how much of this account is left".
 *
 * A 270° arc rather than a full ring, for a reason that is about reading rather
 * than taste: a full ring has no start, so a nearly-full and a nearly-empty one
 * look alike at a glance and you have to find the number to tell them apart. An
 * open arc has an obvious beginning and end, and the gap sits at the bottom
 * where the label goes.
 *
 * It draws **headroom**, not consumption. That is the number that was asked for,
 * and mixing the two — a bar that fills as you spend beside a number that
 * counts down — is how a panel ends up saying two things at once.
 */

import { useEffect, useRef, useState } from "react";

const SWEEP_DEGREES = 270;
const RADIUS = 46;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;
const ARC_LENGTH = CIRCUMFERENCE * (SWEEP_DEGREES / 360);

export interface GaugeProps {
  /** 0–100 remaining. */
  percent: number;
  /** `normal` | `warning` | `critical` | `exhausted` — drives the stroke colour. */
  band: string;
  /** Rendered inside the arc. The number, its unit, and one line under it. */
  value: string;
  unit?: string;
  caption?: string;
  /** Announced instead of the decorative parts. */
  label: string;
  size?: number;
}

/**
 * Animate a number toward its target so a refresh reads as a change rather
 * than a redraw.
 *
 * Driven by `requestAnimationFrame` rather than a CSS transition because the
 * digits and the arc have to move together — a stroke that eases while the
 * label snaps is worse than neither moving. Honours the OS reduce-motion
 * setting (§82) by arriving immediately.
 */
function useEased(target: number, duration = 620): number {
  const [value, setValue] = useState(target);
  const from = useRef(target);
  const frame = useRef<number | null>(null);

  useEffect(() => {
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
    if (reduced || from.current === target) {
      from.current = target;
      setValue(target);
      return;
    }
    const start = performance.now();
    const origin = from.current;
    const step = (at: number) => {
      const t = Math.min(1, (at - start) / duration);
      // Decelerate-only, matching --ease-out. Motion states causality here; it
      // must not overshoot past a percentage and come back (§10).
      const eased = 1 - Math.pow(1 - t, 3);
      setValue(origin + (target - origin) * eased);
      if (t < 1) {
        frame.current = requestAnimationFrame(step);
      } else {
        from.current = target;
      }
    };
    frame.current = requestAnimationFrame(step);
    return () => {
      if (frame.current !== null) cancelAnimationFrame(frame.current);
      from.current = value;
    };
    // `value` is intentionally not a dependency: reading it in cleanup captures
    // where the animation stopped, but depending on it would restart the
    // animation on every frame.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target, duration]);

  return value;
}

export function Gauge({
  percent,
  band,
  value,
  unit,
  caption,
  label,
  size = 132,
}: GaugeProps) {
  const eased = useEased(Math.max(0, Math.min(100, percent)));
  const filled = ARC_LENGTH * (eased / 100);

  return (
    <div className="gauge" style={{ width: size, height: size }} data-band={band}>
      <svg
        viewBox="0 0 120 120"
        className="gauge__svg"
        role="img"
        aria-label={label}
        focusable="false"
      >
        {/* The arc opens at the bottom: start 135° past 3 o'clock, sweep 270°.
            Rotating the whole group is simpler than computing arc endpoints and
            keeps the dash maths in one place. */}
        <g transform="rotate(135 60 60)">
          <circle
            className="gauge__track"
            cx="60"
            cy="60"
            r={RADIUS}
            strokeDasharray={`${ARC_LENGTH} ${CIRCUMFERENCE}`}
          />
          <circle
            className="gauge__fill"
            cx="60"
            cy="60"
            r={RADIUS}
            strokeDasharray={`${filled} ${CIRCUMFERENCE}`}
          />
        </g>
      </svg>
      <div className="gauge__center" aria-hidden="true">
        <span className="gauge__value">
          {value}
          {unit && <span className="gauge__unit">{unit}</span>}
        </span>
        {caption && <span className="gauge__caption">{caption}</span>}
      </div>
    </div>
  );
}

/**
 * The same reading as a slim horizontal bar, for the windows that are not
 * binding.
 *
 * Deliberately a different shape from the dial rather than a smaller dial:
 * three identical gauges of different sizes read as a ranking of importance you
 * have to decode, while one dial and a list of bars reads as "this is the one,
 * and here is everything else".
 */
export function Bar({
  percent,
  band,
  label,
}: {
  percent: number;
  band: string;
  label: string;
}) {
  const eased = useEased(Math.max(0, Math.min(100, percent)));
  return (
    <div
      className="quotabar"
      data-band={band}
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(percent)}
      aria-label={label}
    >
      {/* A hairline at zero rather than nothing: an empty track and a missing
          track look the same, and one of them means "no allowance left". */}
      <span className="quotabar__fill" style={{ width: `${Math.max(1.5, eased)}%` }} />
    </div>
  );
}
