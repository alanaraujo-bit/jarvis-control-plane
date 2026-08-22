import "./StatusDot.css";

/**
 * Every state a mission or a session can be in.
 *
 * Deliberately one vocabulary for both: a mission that is running and an agent
 * that is working mean the same thing to someone glancing at the screen, and
 * should look the same.
 */
export type DotStatus =
  // Mission states (§29)
  | "ready"
  | "running"
  | "verifying"
  | "waiting"
  | "blocked"
  | "failed"
  | "completed"
  // Session states (§21)
  | "starting"
  | "working"
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
