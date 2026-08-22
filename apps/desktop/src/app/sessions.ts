import { Channel, invoke } from "@tauri-apps/api/core";
import { isTauri } from "./platform";

/**
 * Session bridge.
 *
 * Terminal traffic never goes through JSON. Output arrives as raw bytes on an
 * IPC channel and is handed to xterm as a `Uint8Array`; decoding it to a string
 * first would break UTF-8 sequences that straddle a PTY read boundary, which is
 * exactly the corruption the Rust log takes care to avoid.
 */

export type SessionKind = "shell" | "claude-code" | "codex";

export type SessionState =
  | "starting"
  | "working"
  | "waiting"
  | "idle"
  | "completed"
  | "blocked"
  | "failed";

export interface SessionInfo {
  id: string;
  projectId: string;
  provider: string;
  title: string | null;
  cwd: string;
  state: SessionState;
  createdAt: number;
  live: boolean;
}

export async function startSession(params: {
  projectId: string;
  kind: SessionKind;
  cwd?: string;
  cols: number;
  rows: number;
}): Promise<SessionInfo> {
  return invoke<SessionInfo>("session_start", {
    projectId: params.projectId,
    kind: params.kind,
    cwd: params.cwd ?? null,
    cols: params.cols,
    rows: params.rows,
  });
}

/**
 * Attach a view and start receiving output.
 *
 * Returns a detach function. Detaching stops the stream but never stops the
 * session — closing a tab must not kill an agent mid-task (§32).
 */
export async function attachSession(
  sessionId: string,
  onOutput: (bytes: Uint8Array) => void,
): Promise<() => void> {
  const channel = new Channel<ArrayBuffer>();
  channel.onmessage = (message) => {
    onOutput(message instanceof ArrayBuffer ? new Uint8Array(message) : new Uint8Array());
  };
  await invoke("session_attach", { sessionId, channel });
  return () => {
    void invoke("session_detach", { sessionId }).catch(() => {
      // The session may already be gone; detaching is best-effort.
    });
  };
}

export async function writeSession(sessionId: string, data: string): Promise<void> {
  // Encode here rather than in Rust: the user's keystrokes are text, and the
  // pty wants bytes.
  const bytes = Array.from(new TextEncoder().encode(data));
  await invoke("session_write", { sessionId, data: bytes });
}

export async function resizeSession(
  sessionId: string,
  cols: number,
  rows: number,
): Promise<void> {
  await invoke("session_resize", { sessionId, cols, rows });
}

export async function closeSession(sessionId: string): Promise<void> {
  await invoke("session_close", { sessionId });
}

/** Terminal history for a reattaching view. */
export async function replaySession(sessionId: string): Promise<Uint8Array> {
  if (!isTauri()) return new Uint8Array();
  const body = await invoke<ArrayBuffer>("session_replay", { sessionId });
  return new Uint8Array(body);
}

export async function listSessions(projectId: string): Promise<SessionInfo[]> {
  if (!isTauri()) return [];
  return invoke<SessionInfo[]>("session_list", { projectId });
}
