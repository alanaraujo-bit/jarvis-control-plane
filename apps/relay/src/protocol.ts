/**
 * The relay's half of the shared protocol (§66).
 *
 * **Generated — do not edit here.** The source of truth is
 * `packages/protocol/src/index.ts`; run `node scripts/sync-protocol.mjs` to
 * regenerate. `protocol.test.ts` fails if the two drift.
 *
 * Why a copy and not the workspace package: Vercel builds this app from
 * `apps/relay` alone, so `workspace:*` resolves locally and fails in the
 * build. Every declaration here is `import type`, erased at compile time.
 */

/** Mirrored from the protocol package; the relay types below refer to it. */
export type MissionStatus =
  | "ready"
  | "running"
  | "verifying"
  | "waiting"
  | "blocked"
  | "failed"
  | "completed";

// ---- Mobile companion + cloud relay (§55–§59) -------------------------------

/**
 * The relay protocol.
 *
 * The desktop is local-first (§3) and has no public address, so a phone cannot
 * reach it directly. The relay is the only thing in between — and it is
 * deliberately **a blind mailbox, not a server that knows anything**: the
 * desktop pushes a summary and pulls queued commands, the phone reads the
 * summary and queues commands, and the relay holds neither for long.
 *
 * The test of whether this respects §3: **turn the relay off and the desktop
 * is untouched.** Nothing in a project, a session, the terminal, Git or the
 * local database depends on it. Only the companion stops working.
 */

/** How the phone knows whether what it is looking at is current (§28). */
export interface Freshness {
  /** When the desktop produced this, ISO-8601. */
  capturedAt: string;
  /** Seconds after which the phone should say plainly that this is stale. */
  staleAfterSeconds: number;
}

/** One project, as much as a phone needs to know about it. */
export interface RelayProject {
  id: string;
  name: string;
  /** Missions that are running, waiting or blocked — never the whole list. */
  attention: RelayMission[];
  activeSessions: number;
}

export interface RelayMission {
  id: string;
  title: string;
  status: MissionStatus;
  /** Why it is blocked or waiting; §34 says it must be able to say. */
  reason: string | null;
  /** Present when a run is being driven right now (§32). */
  turns: number | null;
  budget: number | null;
}

/**
 * Everything the desktop tells the relay about itself.
 *
 * Deliberately a **summary and not a mirror**: what is running, what needs a
 * person. No file contents, no terminal output, no conversation text. A relay
 * holding a copy of the work would be the second store §23 exists to prevent,
 * and a much larger thing to secure.
 */
export interface RelaySnapshot {
  freshness: Freshness;
  deviceName: string;
  projects: RelayProject[];
  /** Guardrail approvals waiting for a person (§35). */
  approvals: RelayApproval[];
}

export interface RelayApproval {
  id: string;
  projectName: string;
  /** The operation class, e.g. `git.force-push`. */
  operation: string;
  /** What is about to run, already redacted by the desktop. */
  summary: string;
  requestedAt: string;
}

/**
 * Something the phone asked for, waiting for the desktop to collect it.
 *
 * A closed set on purpose. "Run anything from your phone" is a remote shell
 * with a friendly name; these are the specific things a companion is for
 * (§56), and each maps to an action the desktop already knows how to take.
 */
export type RelayCommand =
  | { kind: "approve"; id: string; approvalId: string; decision: "allow" | "deny" }
  | { kind: "startMission"; id: string; projectId: string; missionId: string }
  | { kind: "stopRun"; id: string; missionId: string };

/** What the desktop sends back after acting, so the phone can stop guessing. */
export interface RelayCommandResult {
  id: string;
  ok: boolean;
  /** A localisation code, never a sentence — the phone speaks its own language. */
  code: string | null;
}

/** How a device proves it is allowed to talk about this desktop. */
export interface RelayPairing {
  /** Short, human-readable, shown on the desktop and typed on the phone. */
  code: string;
  /** When the code stops being accepted, ISO-8601. */
  expiresAt: string;
}
