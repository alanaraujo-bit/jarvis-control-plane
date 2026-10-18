# J.A.R.V.I.S.

A universal operating environment for software development with AI agents.
This file summarises scope; the authoritative detail lives in the documents it
links. Section numbers (§N) refer to the original product specification, which
is not stored in this repository.

## Problem and objective

Developers working with agent CLIs (Claude Code, Codex, and the ones that come
after them) juggle terminals, transcripts, Git, verification and quota by hand.
J.A.R.V.I.S. is the layer above the agents: projects, missions, agents,
sessions, code and verification in one place, so the distance between an
intention and working, *verified* software is as short as possible.

## Audience

A developer on Windows running one or more agent CLIs against local
repositories, including unattended runs with nobody watching. The quality bar
set by the owner is Cursor / Linear-class polish.

## Product principles (non-negotiable)

- **Local-first (§3).** Code, Git, terminals, credentials and history stay on
  the machine. Cloud features (relay, social, sync) are additive and opt-in.
- **A session is one append-only event log (§23).** Terminal and Conversation
  are two projections of it.
- **Completed means verified (§30).** Missions cannot complete without evidence.
- **Nothing stubbed (§81).** Unbuilt destinations are absent, not "coming soon".
- **Confidence travels with every number (§28).**
- **Providers are not equal (§26).** Capabilities are data.
- **i18n from the first string (§65):** en + pt-BR.
- **Quiet Intelligence (§6):** near-monochrome; amber means agent work.
- **Signing in is never a gate (M20).**

## What exists

The feature table in [README.md](README.md#where-the-project-actually-is) and
the per-milestone checklist in [docs/ROADMAP.md](docs/ROADMAP.md) are the
source of truth. In short: shell and design system, projects, real ConPTY
terminals, Claude Code and Codex adapters, Conversation View, missions with
evidence and verification, Mission Control, activity, analytics, guardrails,
notifications, unattended runs, files/editor/diff/Git/worktrees, Project
Brain, global search, session history, provider accounts and quota (M13/M16),
personal identity with Google sign-in and cloud sync (M20/M23), a friends
network backed by `apps/social-api` (see [PROGRESS.md](PROGRESS.md)), a local
model provider and GPU telemetry (v0.10.0).

## Out of scope / not built

Listed deliberately in README and HANDOFF §4 ("Not built — deliberately
absent"). External blockers (code-signing certificate, public update host,
Vercel OAuth, NSIS hang) are tracked in [docs/BLOCKERS.md](docs/BLOCKERS.md).

## Constraints and technical decisions

Tauri 2 + Rust core, React 19 + TypeScript + Vite UI, SQLite via `rusqlite`,
pnpm workspace on Node 22+. Cloud pieces: `apps/relay` on Vercel,
`apps/social-api` with PostgreSQL on Railway. The toolkit's Next.js/Drizzle
defaults do **not** apply and are not to be introduced. Rationale for every
stack choice: [docs/DECISIONS.md](docs/DECISIONS.md).

## Acceptance

A milestone is done only when it passes the Definition of Done in
[docs/ROADMAP.md](docs/ROADMAP.md): tests green, and the feature seen working
in the real app in both themes and both languages.

## Next delivery

Taken from [docs/ROADMAP.md](docs/ROADMAP.md) and [PROGRESS.md](PROGRESS.md)
("Next work"): automatic desktop heartbeat and metric publication for the
friends network, then profile detail and activity timeline, then integration
tests against a disposable Postgres.

## Maintenance

Update when objective, scope, principles or acceptance change. Technical
detail belongs in `docs/ARCHITECTURE.md`; evidence in `docs/setup.md`.
