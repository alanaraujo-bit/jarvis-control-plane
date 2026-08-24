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
  /** Set when the session was started to work on a mission (§86). */
  missionId?: string;
  /// A past session to continue (§88). The new session is a new process with
  /// its own log; this is what makes it a continuation rather than a fresh
  /// start that happens to be in the same folder.
  resumeFrom?: string;
}): Promise<SessionInfo> {
  return invoke<SessionInfo>("session_start", {
    projectId: params.projectId,
    kind: params.kind,
    cwd: params.cwd ?? null,
    cols: params.cols,
    rows: params.rows,
    missionId: params.missionId ?? null,
    resumeFrom: params.resumeFrom ?? null,
  });
}

/** Sessions that are working, or worked, on a mission. */
export async function missionSessions(missionId: string): Promise<SessionInfo[]> {
  if (!isTauri()) return [];
  return invoke<SessionInfo[]>("mission_sessions", { missionId });
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

/** An image pasted into a session (§22). */
export interface Attachment {
  /** Absolute path on disk — this is what gets typed at the prompt. */
  path: string;
  name: string;
  bytes: number;
  mime: string;
}

/**
 * Take the image on the clipboard and attach it to a session (§22).
 *
 * The **core** reads the clipboard, not the webview: WebView2 raises no
 * `paste` event for image data at all, so there is nothing for a browser-side
 * handler to read. See `session::attachment::from_clipboard`.
 *
 * Rejects with `attachment.noImage` when the clipboard holds no image, which
 * is the ordinary case for a text paste rather than a failure.
 */
export async function pasteImage(sessionId: string): Promise<Attachment> {
  return invoke<Attachment>("session_paste_image", { sessionId });
}

/** Read a pasted image back, for the preview. */
export async function readAttachment(sessionId: string, path: string): Promise<Uint8Array> {
  const body = await invoke<ArrayBuffer>("session_read_attachment", { sessionId, path });
  return new Uint8Array(body);
}
