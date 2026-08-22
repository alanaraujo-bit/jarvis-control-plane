import "./StatusDot.css";

export type DotStatus =
  | "ready"
  | "running"
  | "verifying"
  | "waiting"
  | "blocked"
  | "failed"
  | "completed"
  | "idle";

/**
 * A single state marker.
 *
 * One dot, one meaning — no badges, no pills, no coloured chips (§7). Working
 * states pulse gently so a busy Mission Control still reads as calm; every
 * other state is still, because stillness is information too.
 */
export function StatusDot({ status, size = 6 }: { status: DotStatus; size?: number }) {
  return (
    <span
      className="status-dot"
      data-status={status}
      style={{ width: size, height: size }}
      aria-hidden="true"
    />
  );
}
