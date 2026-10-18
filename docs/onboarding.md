# Local preparation

## Prerequisites

| Tool | Why | Check |
|---|---|---|
| Node 22.13+ and pnpm (version pinned in `package.json`) | workspace, Vite, scripts | `node -v`, `pnpm -v` |
| Rust stable (MSVC target) via rustup | Tauri core | `cargo -V` |
| Visual Studio 2022 Build Tools, "Desktop development with C++" | Rust MSVC linker | `vswhere -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64` |
| WebView2 runtime | Tauri window | ships with Windows 11 |
| Git | projects, tests use real repositories | `git --version` |

On Windows: `winget install Rustlang.Rustup` then `rustup default stable`, and
`winget install Microsoft.VisualStudio.2022.BuildTools` with the
`Microsoft.VisualStudio.Workload.VCTools` workload. rustup puts `cargo` in
`%USERPROFILE%\.cargo\bin`; shells opened before the install do not see it.

## First run

```bash
pnpm install
pnpm tauri dev
```

`pnpm tauri dev` starts Vite on port 5273 (fixed — `tauri.conf.json` points the
window at it) and then compiles and launches the Rust core. The first compile
takes a few minutes; later ones are incremental.

What updates live:

- **UI** (`apps/desktop/src`, `packages/*`): Vite hot-reload, instantly, in the
  desktop window.
- **Rust** (`apps/desktop/src-tauri`): Tauri watches the folder, rebuilds and
  restarts the app on save.

UI-only work can run without Rust: `pnpm dev` and open http://localhost:5273.
Outside Tauri the app runs on fixture data (status bar shows
"Preview — Fixtures"); anything that needs the core is unavailable there.

In Claude Code desktop, `.claude/launch.json` defines both: `desktop`
(`pnpm tauri dev`) and `desktop-web` (`pnpm dev`). Only one can own port 5273.

## Cloud services (optional)

`apps/social-api` reads `DATABASE_URL`, `GOOGLE_CLIENT_ID`,
`GOOGLE_CLIENT_SECRET`, `PUBLIC_ORIGIN`, `PORT`. Values live in Railway, never
in the repo. Use a disposable local Postgres for development; do not point
local work at the production database.

## Verify

```bash
pnpm verify
```

Runs typecheck, lint (measurement), JS tests, docs checks and `cargo test`.
Expected: all green; lint reports warnings without failing. Ignored Rust tests
need real CLIs, a microphone or this machine's history — see HANDOFF §4.

## Agents

Read [AGENTS.md](../AGENTS.md); [CLAUDE.md](../CLAUDE.md) imports it. Codex
reads `AGENTS.md` natively. Integrations and scopes: [setup.md](setup.md).

## Known issues

- **Running `pnpm tauri dev` marks `Cargo.toml` and `gen/schemas/*.json` as
  modified with no content diff.** The Tauri CLI rewrites them with different
  line endings. `git diff` is empty; `git checkout -- <file>` clears it.
- **`pnpm` from a PowerShell launch config fails with "execution of scripts is
  disabled".** Call `pnpm.cmd` instead (as `.claude/launch.json` does).
- **HANDOFF predates this machine.** Paths and version numbers quoted in its
  narrative sections describe the machine they were found on; verify before
  relying on them.
- **`pnpm tauri build` NSIS step hangs on this machine (B5).** Use
  `pnpm tauri build --no-bundle` or `tools/rebuild.sh`.
