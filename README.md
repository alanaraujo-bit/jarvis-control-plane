# J.A.R.V.I.S.

A universal operating environment for software development with AI agents.

Claude Code, Codex and the agents that come after them are execution providers
*inside* J.A.R.V.I.S. The product is the layer above them: projects, missions,
agents, sessions, code and verification in one place, so the distance between
an intention and working software is as short as possible.

Local-first. Your code, your Git, your terminals, your credentials and your
history stay on your machine.

---

## Where the project actually is

Working and verified against real agents on a real machine:

| Area | State |
|---|---|
| Application shell, design system (dark + light), i18n (en, pt-BR) | Working |
| Command palette | Working |
| Environment scan — Git, Node, pnpm, Claude Code, Codex, GitHub CLI | Working |
| Projects — open a folder, Git detection, recents | Working |
| Session event log — append-only, crash-safe, byte-exact | Working |
| Terminal — real ConPTY, tabs, restore, both themes | Working |
| Providers — Claude Code and Codex adapters, capability model | Working |
| Conversation View — structured projection with official token usage | Working |
| Windows installer + updater | Working (unsigned — see `docs/BLOCKERS.md`) |

Not built yet — deliberately absent rather than stubbed (§81):
Missions, Project Brain, Notes, Files/Editor/Diff, Preview, Analytics,
Activity, onboarding, mobile companion, cloud sync, voice.

`docs/ROADMAP.md` is the live plan. `docs/DECISIONS.md` records why things are
the way they are, including several findings that only showed up by running the
product rather than reasoning about it.

---

## Building it

Requires Node 22+, pnpm, and a Rust toolchain with the MSVC build tools.

```bash
pnpm install
pnpm dev          # run the desktop app against Vite
pnpm tauri build  # produce the Windows installer
```

Rust tests, including ones that drive a real PTY and a real Git repository:

```bash
cd apps/desktop/src-tauri
cargo test
cargo test -- --ignored   # also parses every Claude Code transcript on this machine
```

---

## How it is put together

The load-bearing idea is that **a session is one append-only event log**.

```
                 ┌──────────────────────────┐
   PTY bytes ───►│                          │───► Terminal View   (replay bytes)
                 │   Session Event Log      │
provider JSONL ─►│   (ordered, append-only) │───► Conversation View (structured)
                 │                          │
   lifecycle ───►│                          │───► Usage · Activity · Evidence
                 └──────────────────────────┘
```

Terminal and Conversation are not two sessions and not two stores — they are two
projections of one stream, written by a single writer so their order is a fact
rather than an agreement. Everything downstream (history, activity, evidence,
mobile sync) reads the same log.

See `docs/ARCHITECTURE.md`.

---

## Repository layout

```
apps/desktop/          Tauri v2 application
  src/                   React UI — shell, surfaces, design system
  src-tauri/src/
    session/             event log, live runtime, transcript following
    pty/                 pseudo-terminals, Windows job containment
    providers/           adapter trait, Claude Code, Codex
    db/                  SQLite schema and migrations
    git/                 Git integration
    envscan/             environment detection
packages/protocol/     shared IPC types
packages/i18n/         en + pt-BR catalogues
brand/                 mark and installer artwork generators
tools/                 window capture and UI automation for visual review
docs/                  architecture, roadmap, decisions, blockers
```
