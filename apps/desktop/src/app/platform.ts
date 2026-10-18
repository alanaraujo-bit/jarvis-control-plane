import { invoke as tauriInvoke } from "@tauri-apps/api/core";

/**
 * Platform bridge.
 *
 * The UI normally runs inside the Tauri webview and talks to the Rust core. It
 * can also be opened in a plain browser for fast visual iteration and automated
 * screenshots (§76).
 *
 * IMPORTANT: browser mode is a *rendering* harness only. Its fixtures exist so
 * surfaces can be inspected visually; they are never evidence that an
 * integration works (§80). Anything running against fixtures reports itself as
 * such through `isTauri()`, and the status bar shows a "Preview" marker.
 */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Fixtures used only when rendering outside the desktop shell. */
const BROWSER_FIXTURES: Record<string, unknown> = {
  // Previewing other surfaces should not land on the welcome screen every
  // time; flip this to `false` locally to preview onboarding itself.
  onboarding_status: true,
  voice_model_status: { present: true },
  // Notifications (§49). Enough shapes to inspect the surface: a question with
  // a long command, a finished turn with the agent's own reply, and a run that
  // stopped — the three kinds, so the colour vocabulary can be checked at a
  // glance in both themes.
  notifications_centre: {
    outstanding: 2,
    enabled: true,
    notifications: [
      {
        id: 3, tsMs: Date.now() - 40_000, kind: "needsApproval", reason: "providerPrompt",
        confidence: "observed", projectId: "p1", projectName: "casco-api", sessionId: "s1",
        missionId: null, missionTitle: null, provider: "claude-code",
        preview: "Do you want to proceed? — Bash command · git push --force origin main",
        detailCode: null, seenAt: null, actedAt: null,
      },
      {
        id: 2, tsMs: Date.now() - 9 * 60_000, kind: "finished", reason: "turnEnded",
        confidence: "official", projectId: "p1", projectName: "casco-api", sessionId: "s2",
        missionId: "m1", missionTitle: "Rate limit the public API",
        provider: "claude-code",
        preview: "Added the token bucket and its tests. All 34 pass; the middleware is wired into the public router only.",
        detailCode: null, seenAt: null, actedAt: null,
      },
      {
        id: 1, tsMs: Date.now() - 3 * 3_600_000, kind: "stopped", reason: "runStopped",
        confidence: "official", projectId: "p2", projectName: "jarvis-relay", sessionId: "s3",
        missionId: "m2", missionTitle: "Move the relay to Node 22",
        provider: "codex",
        preview: "Autopilot ran out of turns",
        detailCode: "autopilot.outOfTurns", seenAt: Date.now() - 3 * 3_600_000, actedAt: null,
      },
    ],
  },
  notifications_attention: null,
  notifications_mark_seen: 0,
  notifications_mark_all_seen: 0,
  notifications_mark_acted: 0,
  notifications_clear: null,
  scan_environment: {
    scannedAt: new Date().toISOString(),
    ready: true,
    tools: [
      { id: "git", name: "Git", kind: "vcs", importance: "required", state: "ready", version: "2.55.0.windows.3", path: "C:\\Program Files\\Git\\cmd\\git.exe", detail: null, authenticated: null, installHint: null, installUrl: null },
      { id: "node", name: "Node.js", kind: "runtime", importance: "recommended", state: "ready", version: "24.19.0", path: "C:\\Program Files\\nodejs\\node.exe", detail: null, authenticated: null, installHint: null, installUrl: null },
      { id: "claude", name: "Claude Code", kind: "agent", importance: "recommended", state: "ready", version: "2.1.240", path: "C:\\Users\\dev\\.local\\bin\\claude.exe", detail: null, authenticated: true, installHint: null, installUrl: null },
      { id: "codex", name: "Codex", kind: "agent", importance: "recommended", state: "ready", version: "0.147.0", path: "C:\\Users\\dev\\AppData\\Local\\Programs\\OpenAI\\Codex\\bin\\codex.exe", detail: null, authenticated: true, installHint: null, installUrl: null },
      { id: "pnpm", name: "pnpm", kind: "packageManager", importance: "optional", state: "ready", version: "11.20.0", path: "C:\\Users\\dev\\AppData\\Roaming\\npm\\pnpm.cmd", detail: null, authenticated: null, installHint: null, installUrl: null },
      { id: "gh", name: "GitHub CLI", kind: "platform", importance: "optional", state: "missing", version: null, path: null, detail: "GitHub CLI was not found on PATH.", authenticated: null, installHint: "winget install --id GitHub.cli", installUrl: "https://cli.github.com" },
    ],
  },
};

/** Call a Rust command, or resolve a fixture when running in browser preview. */
export async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (isTauri()) return tauriInvoke<T>(command, args);

  if (command in BROWSER_FIXTURES) {
    // Small delay so loading states are exercised rather than skipped.
    await new Promise((resolve) => setTimeout(resolve, 180));
    return BROWSER_FIXTURES[command] as T;
  }
  throw new Error(`No browser fixture for command "${command}".`);
}
