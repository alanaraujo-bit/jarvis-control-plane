/**
 * Shared IPC contracts.
 *
 * These types mirror the `serde` shapes emitted by the Rust core. They live in
 * their own package so the desktop UI, the mobile companion and any future
 * client speak exactly one protocol (§66).
 */

// ---- Environment scan (§14) -------------------------------------------------

export type ToolImportance = "required" | "recommended" | "optional";
export type ToolKind = "vcs" | "runtime" | "packageManager" | "agent" | "platform";
export type ToolState = "ready" | "missing" | "degraded";

export interface ToolReport {
  id: string;
  name: string;
  kind: ToolKind;
  importance: ToolImportance;
  state: ToolState;
  version: string | null;
  path: string | null;
  detail: string | null;
  /** Credential presence only — never the credential itself (§60/§61). */
  authenticated: boolean | null;
  installHint: string | null;
  installUrl: string | null;
}

export interface EnvironmentReport {
  tools: ToolReport[];
  scannedAt: string;
  ready: boolean;
}

// ---- Confidence (§28) -------------------------------------------------------

/**
 * How much a reported number can be trusted. Every usage figure the product
 * shows carries one of these; an estimate is never rendered as if it were
 * reported by the provider.
 */
export type Confidence = "official" | "observed" | "estimated" | "unknown";

// ---- Agent + mission states -------------------------------------------------

export type AgentState = "working" | "waiting" | "idle" | "completed" | "blocked";

export type MissionStatus =
  | "ready"
  | "running"
  | "verifying"
  | "waiting"
  | "blocked"
  | "failed"
  | "completed";
