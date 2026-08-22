# J.A.R.V.I.S. — Architecture

## Stack (decided, see DECISIONS.md)

| Layer | Choice |
|---|---|
| Desktop shell | Tauri v2 (Rust core + WebView2) |
| Core language | Rust (PTY, git, fs, db, providers, relay) |
| UI | React 19 + TypeScript + Vite |
| Styling | CSS custom-property design tokens + plain CSS modules |
| Terminal | xterm.js + Rust `portable-pty` |
| Editor | Monaco (behind `packages/editor` boundary) |
| Persistence | SQLite (WAL) via `rusqlite` + on-disk session logs |
| Icons | Lucide |
| i18n | Custom lightweight ICU-ish catalog (`packages/i18n`), en + pt-BR |
| Cloud | Next.js on Vercel (auth, relay, push, billing) |

## The load-bearing idea: a session is an event log

> §23 — Terminal and Conversation are the SAME session.

A `Session` is **not** a terminal widget plus a chat widget. It is an
**append-only, ordered event log**. Everything else is a projection.

```
                 ┌──────────────────────────┐
   PTY bytes ───►│                          │───► Terminal View   (replay bytes → xterm)
                 │   Session Event Log      │
provider JSONL ─►│   (ordered, append-only) │───► Conversation View (project structured)
                 │                          │
   lifecycle ───►│                          │───► Usage / Activity / Evidence / Mobile
                 └──────────────────────────┘
```

Event kinds (`SessionEvent`):
- `PtyChunk`   — raw bytes written by the process (terminal truth)
- `Message`    — user / assistant / system structured message
- `ToolCall`   / `ToolResult`
- `Usage`      — token + rate-limit samples, each carrying a confidence level
- `FileChange` — files touched
- `Lifecycle`  — started / exited / state transition
- `Attachment` — images and files (first-class, §22)
- `Approval`   — request / decision

### Storage split (important)

Raw PTY bytes are **never** stored as SQLite rows — that dies on hour-3 sessions.

- `sessions/<id>/stream.log`  — append-only binary frame log (PTY + events)
- `sessions/<id>/index.bin`   — offset index for seeking / tailing
- SQLite                       — metadata, structured events, search index

## Provider observability (verified against installed CLIs)

Verified on **Claude Code 2.1.240** and **Codex CLI 0.147.0** on this machine.

The critical question was: can one session be simultaneously PTY-attached
*and* structured-observable? **Yes.** Both CLIs write structured JSONL
transcripts to disk while running interactively.

### Claude Code
- Transcript: `~/.claude/projects/<encoded-cwd>/<session-uuid>.jsonl`
- Event types: `user`, `assistant`, `system`, `attachment`, `ai-title`,
  `file-history-snapshot`, `file-history-delta`, `frame-link`, `queue-operation`
- `assistant.message.usage` gives **official** `input_tokens`, `output_tokens`,
  `cache_creation_input_tokens`, `cache_read_input_tokens`
- Messages form a DAG via `parentUuid`
- Non-interactive: `-p --output-format stream-json --include-partial-messages`
- Resume: `--resume <id>`, `--session-id <uuid>`, `--fork-session`

### Codex
- Transcript: `~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`
- Envelope: `{timestamp, ordinal, type, payload}` — strictly ordered by `ordinal`
- Types: `session_meta`, `event_msg/*`, `response_item/*`, `turn_context`, `world_state`
- `event_msg/token_count` carries **official** `rate_limits`
- Non-interactive: `codex exec --json` (JSONL events)
- Resume: `codex resume`, `codex exec resume --last`

This is why Conversation View is real structured data, not ANSI scraping.

## Crate / package layout

```
apps/desktop/src-tauri/src/
  session/     event log, frame codec, projections   ← the core
  pty/         portable-pty host, resize, lifecycle
  providers/   adapter trait + claude/ + codex/
  db/          sqlite schema + migrations
  git/         git2 integration, worktrees
  mission/     mission engine, verification, evidence
  brain/       project brain
  envscan/     environment detection
  security/    Windows credential storage
apps/desktop/src/
  design/      tokens, primitives
  surfaces/    mission-control, project, terminal, conversation, ...
packages/
  protocol/    shared TS types mirroring Rust IPC contracts
  i18n/        en + pt-BR catalogs
```

## Capabilities are data, never hardcoded booleans in UI (§26)

Each provider adapter declares a `ProviderCapabilities` struct. UI renders from
it. Adding a provider must never require touching a component.

## Distribution (§12, §62)

The product is delivered as a Windows NSIS installer built by Tauri, carrying
its own identity: generated header and sidebar artwork drawn from the same
signed-distance geometry as the application icon, and an English/Portuguese
language selector matching the app's own locales.

Install mode is **per-user**, so first run needs no administrator prompt.

### Updating

`tauri-plugin-updater` checks a `latest.json` endpoint, downloads in the
background, and verifies the download against a **minisign public key compiled
into the binary** before installing anything. An update that fails verification
is refused rather than applied.

Two separate signing concerns, often confused:

| | Purpose | State |
|---|---|---|
| **minisign keypair** | Proves an update came from us | Generated locally, working |
| **Authenticode certificate** | Stops Windows warning about an unknown publisher | Blocked — needs a purchase (B1) |

The private minisign key lives in `.keys/` and is gitignored. Losing it means no
installed copy can ever be updated again.

### Data safety across updates

User data lives in `%APPDATA%\dev.jarvis.desktop` — the SQLite database and the
per-session logs — entirely outside the installation directory. An update
replaces program files and cannot touch projects, sessions or history. Schema
changes are forward-only and additive, and a database written by a newer build
is refused rather than opened (§62).
