# J.A.R.V.I.S.

A local-first desktop operating environment for AI-agent software development.
Read [docs/HANDOFF.md](docs/HANDOFF.md) before working in the core, and
[docs/DECISIONS.md](docs/DECISIONS.md) before changing anything it records.

## Product and stack

- Product and scope: [PRODUCT.md](PRODUCT.md). Live plan: [docs/ROADMAP.md](docs/ROADMAP.md).
- Stack: pnpm workspace (Node 22.13+). `apps/desktop` is Tauri 2 — Rust core +
  React 19/TypeScript/Vite 7 UI. `apps/relay` (Vercel functions),
  `apps/social-api` (Node + `pg` on Railway), `apps/docs` (static site),
  `packages/{protocol,i18n,editor}`.
- Structure: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Toolkit setup and
  evidence: [docs/setup.md](docs/setup.md).

## Canonical commands

- Install: `pnpm install`
- Develop (desktop, live reload of UI + Rust rebuild): `pnpm tauri dev`
- Develop (UI only, browser, fixture data): `pnpm dev` → http://localhost:5273
- Types: `pnpm typecheck`
- Lint: `pnpm lint` (ESLint, measurement mode) · `pnpm lint:rust` (clippy)
- Tests: `pnpm test` (JS packages + `cargo test`)
- Verify everything: `pnpm verify`
- Build: `pnpm build` (UI) · `pnpm tauri build` (installer, needs the signing key)

Rust toolchain: `%USERPROFILE%\.cargo\bin` must be on `PATH`. Details and
known issues: [docs/onboarding.md](docs/onboarding.md).

## Non-negotiables (from HANDOFF §2)

- A session is one append-only event log; never add a second store for session data.
- Completed means verified: the core refuses completion without evidence.
- Nothing is stubbed to look finished — an unbuilt surface is absent.
- Every number carries its confidence (Official/Observed/Estimated/Unknown).
- Local-first: projects, sessions, Git and credentials stay on the machine.
- i18n from the first string: en + pt-BR, pt-BR typed against en.
- Signing in is never a gate on work.

## Working agreements

- Read relevant code and instructions first. Separate observation, assumption,
  decision and evidence.
- Smallest complete change; reuse existing patterns and dependencies.
- Define acceptance and checks before implementing. Fixes need a reproduction
  and a regression test when feasible. Prefer real infrastructure over mocks.
- The loop is implement → build → run the real app → look at it → fix. A UI
  change is not done until it was seen running in both themes.
- When a script patches a file, verify the patch applied.
- Update the responsible document with the change (see map below); do not
  duplicate rules across files.
- Never run test agents in the user's real projects — use a scratch repo.
- Publishing, deploys, pushes, messages, costs and destructive actions need
  explicit authorization. Installing a tool grants none of these.
- Commit messages explain *why*.

## Execution continuity

- Answer status questions and resume authorized work. A blocker stops only its
  dependents.
- Before resuming from a handoff, check files, Git state, checks and processes.
- Unfinished multi-session work keeps a short handoff (objective, verified
  state, checks, pending, next action) in the relevant milestone doc.
- Never claim background work without a verifiable process.

## Tools

- **Superpowers**: scale the process to the request; existing approvals stand.
- **Ponytail** (`full`) for code; **Caveman** for chat only — code, docs and
  commits use normal wording. `stop ponytail` / `stop caveman` disable them.
- **Context7**: current library docs, one concept per query, no secrets or
  private code. If unavailable, use official docs and say so.
- **RTK**: the Claude hook rewrites supported commands; do not prefix twice.
  `rtk proxy <cmd>` gives raw output.
- **Graphify**: `graphify-out/` (untracked) answers structural questions;
  regenerate with `graphify update .` and treat a stale graph as stale.
- **Project skills** (`.agents/skills`, `.claude/skills`): React work →
  `vercel-react-best-practices`, `vercel-composition-patterns`; UI review →
  `web-design-guidelines`; docs → `writing-guidelines`; transitions →
  `vercel-react-view-transitions`; social-api SQL → `supabase-postgres-best-practices`;
  security-sensitive diffs → `differential-review`; Vercel publish/metrics →
  `deploy-to-vercel`, `vercel-optimize` (only on request); missing capability →
  `find-skills`.
- Security review: [docs/prompts/security-review.md](docs/prompts/security-review.md),
  read-only, private report outside the repo.

## Delegation and review

- Delegate only when isolation, specialization or independent parallel work
  pays for it. Use the lightest available model that meets the acceptance
  criteria; use stronger capability for core architecture, security, data,
  concurrency and critical review.
- Each task gets goal, allowed files, acceptance and checks. One writer per
  file; the coordinator owns lockfiles, shared config and Git.
- Review nontrivial diffs by risk; stop when findings are resolved or
  dispositioned and checks pass. See [docs/quality.md](docs/quality.md).

## Git

- No Git hooks, push blockers or branch requirements. Checks are explicit
  commands.

## Where things live

| Topic | Source |
|---|---|
| Scope, users, acceptance | `PRODUCT.md` |
| Architecture, stack | `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` |
| Current state, traps, how work is done | `docs/HANDOFF.md` |
| Milestone specs | `docs/M13-*.md` … `docs/M25-*.md` |
| UI tokens and interaction | `docs/design.md`, `apps/desktop/src/design/tokens.css` |
| Checks and review | `docs/quality.md` |
| Local setup | `docs/onboarding.md` |
| Toolkit, integrations, evidence | `docs/setup.md` |
| External blockers | `docs/BLOCKERS.md` |
