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
