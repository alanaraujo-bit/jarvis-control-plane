# Setup and evidence

## Source and context

- Date: 2026-10-18. Toolkit: [vibe-coding-toolkit](https://github.com/soumatheusgomes/vibe-coding-toolkit)
  at `de66fe371a49f5a4d3b521759e5c401f390b6424` (2026-10-17), cloned outside the
  repository; every template and guide below comes from that revision.
- Applied with prompt 12 ("Set up or update an existing project").
- Machine: Windows 11, Node 24.20, pnpm 11.20 (pinned), Rust 1.99.1 (MSVC),
  VS 2022 Build Tools, Python 3.14.
- Clients: Claude Code 2.1.302 (main model Opus 5.5, unchanged), Codex CLI
  0.163.0 (installed; not exercised in this setup).
- Delegation policy: subagents allowed, lightest capable model. The harness
  cases ran on Sonnet in fresh subagent contexts (not a restarted client).
  Token and cost figures were not measured.

## Decisions

- **Keep the stack.** Tauri 2 + Rust + React 19/Vite, pnpm workspace,
  `rusqlite`. The toolkit's Next.js/Drizzle/Zod/Pino defaults do not apply.
  Nothing was scaffolded and no framework was introduced.
- **Existing documents stay canonical.** `docs/ARCHITECTURE.md` maps to the
  manifest's `docs/architecture.md`; on Windows the filesystem is
  case-insensitive, so no second file was created. `docs/HANDOFF.md`,
  `DECISIONS.md`, `ROADMAP.md` and the milestone docs are unchanged.
- **Lint arrives in measurement mode.** Rules with legacy violations start at
  `warn` with recorded counts (docs/quality.md). Nothing was weakened.
- **Clients.** Project skills go to both Claude Code (`.claude/skills`) and
  Codex (`.agents/skills`), because the product drives both. No project
  `.codex/` config: that folder is generated product state and gitignored.
- **Context7** was unavailable during setup (OAuth pending), so library facts
  came from official sources; it was authenticated and validated afterwards.

## Components

| Component | Action and reason | Scope | State and evidence |
|---|---|---|---|
| Superpowers 7.0.0 | installed (`claude plugin install superpowers@claude-plugins-official`) | user | loaded: its skills appear in the session |
| Ponytail 5.3.2 | installed; optional webhook/ledger options left unset (they send data out) | user | loaded |
| Caveman 4.0.1 | installed. The install noted that its pnpm-lockfile packages were not installed | user | loaded (skills and agents discovered) |
| Context7 | plugin `context7@claude-plugins-official` | user | authenticated via `/mcp` after a client restart. Live query on 2026-10-18: resolved Tauri to `/websites/tauri_app` and returned the `tauri dev` watch/`beforeDevCommand` docs |
| RTK 0.50.0 | `winget install rtk-ai.rtk` (upstream release is 1.0.1; winget lags). `rtk init --global` added the `rtk hook claude` PreToolUse hook, `~/.claude/RTK.md`, and `@RTK.md` in the user CLAUDE.md | user | hook rewrite tested (`git status` becomes `rtk git status`). Takes effect after restarting Claude Code. Codex hook not installed |
| Graphify 0.9.92 (uv 0.13.1) | `uv tool install graphifyy`, then `graphify install --platform claude` (adds a skill and a line in the user CLAUDE.md) | user + project | code-only graph built: 4184 nodes, 9645 edges, 173 communities; `graphify query` answered. `graphify-out/` gitignored; `.graphifyignore` versioned. No semantic/LLM extraction, no Git hook |
| Vercel web skills: `vercel-react-best-practices`, `vercel-composition-patterns`, `web-design-guidelines`, `writing-guidelines`, `vercel-react-view-transitions` | installed: the UI is React 19 | project | discovered by Claude Code. Codex discovery not checked |
| `deploy-to-vercel`, `vercel-optimize` | installed: `apps/docs` and `apps/relay` deploy to Vercel. Use only on request | project | discovered. Never run |
| `find-skills` | installed | project | discovered |
| `vercel-react-native-skills` | not applicable: there is no mobile app | n/a | |
| `vercel-cli-with-tokens` | not applicable: Vercel is reached through an OAuth connector, not tokens | n/a | |
| `next-dev-loop`, `next-bundle-optimizer` | not applicable: no Next.js | n/a | |
| `supabase-postgres-best-practices` | installed: `apps/social-api` uses PostgreSQL (Railway). Supabase is not adopted | project | discovered. No database check run |
| `differential-review` (Trail of Bits, CC-BY-SA-4.0) | installed for security-sensitive diffs | project | discovered. Not run on a real diff yet |
| Security review prompt | copied to `docs/prompts/security-review.md` (toolkit prompt 13, same revision) | project | used in the harness case below. No repository baseline review run yet |
| `gh-fix-ci` | blocked: no GitHub Actions, and `gh` is not authenticated. Resume when CI exists | n/a | |
| shadcn | not applicable: the project has its own token system (`tokens.css`) | n/a | |
| Sentry MCP | not applicable: no Sentry in the product | n/a | |
| agent-browser, Chrome DevTools MCP | reuse an equivalent: the Claude desktop browser pane covers `pnpm dev` (web mode). The Tauri webview is not reachable by either tool | n/a | |
| Memory | reuse: Claude native project memory plus versioned docs (`AGENTS.md`, `docs/*`). Obsidian not needed | client + project | |
| Anthropic Skills, skill-creator | reuse: already available through the claude.ai plugin | user | |
| aia-harness, other official plugins | not needed: the harness is covered by AGENTS.md and the docs | n/a | |
| Client hooks | only RTK's. No Git hooks, push blockers or branch rules | user | |
| ESLint 10 + typescript-eslint 8 + react-hooks 7 + toolkit `eslint-rules/` | adopted (root devDependencies, flat config, typed tier) | project | `pnpm lint` reports 0 errors and 51 warnings. `pnpm lint:rules` passes |
| clippy | measurement only (`pnpm lint:rust`) | project | 144 warnings, 0 errors |
| Dev loop | `.claude/launch.json`: `desktop` (`pnpm tauri dev`) and `desktop-web` (`pnpm dev`) | project | desktop window launched; UI hot-reload and Rust rebuild observed |

Skill sources and hashes are in `skills-lock.json`. The skills were copied with
`--copy`, because on Windows links would point at absolute paths. Upstream
`vercel-labs/agent-skills` declares no repository license. Some of its skills
declare MIT in their frontmatter; `deploy-to-vercel`, `vercel-optimize`,
`web-design-guidelines` and `writing-guidelines` declare none. Keep them inside
this private repository.

Global backups taken before changes are in
`~/.claude/backups/toolkit-setup-20261018/`. `rtk` also left `settings.json.bak`.

## Documentation created

`AGENTS.md` (short; maps to the existing docs), `CLAUDE.md` (`@AGENTS.md`),
`PRODUCT.md`, `docs/onboarding.md`, `docs/design.md`, `docs/quality.md`,
`docs/setup.md` and `docs/prompts/security-review.md`. The README links them.

## Changes to existing code

- `apps/desktop/src/surfaces/accounts/Accounts.tsx`: the click handler
  `useDifferentAccount` was renamed to `switchToDifferentAccount`. The `use`
  prefix made the Hooks linter treat a plain handler as a hook. Behavior is
  unchanged.
- `packages/editor/src/CodeEditor.tsx`: removed one stale
  `eslint-disable-next-line react-hooks/exhaustive-deps`. That effect's
  dependency list was already complete.
- `tools/rebuild.sh`: the repository root is now derived from the script's
  location instead of a path on the previous machine.
- `pnpm-workspace.yaml`: overrides `dompurify ^3.4.16`, `undici ^6.28.1` and
  `source-map-js ^1.2.2` fix all 23 audit advisories.

## Checks

| Item | Before (baseline) | After |
|---|---|---|
| `pnpm -r typecheck` | 7/7 pass | 7/7 pass |
| JS tests (relay, i18n, docs) | pass | pass |
| `cargo test` | 660 pass, 19 ignored | 660 pass, 19 ignored |
| `pnpm lint` | no linter | 0 errors, 51 warnings |
| `pnpm lint:typed` | n/a | 23 warnings; 9 s; about 1.35 GB peak memory |
| `pnpm lint:rust` | not configured | 144 warnings |
| `pnpm verify` | n/a | exit 0 |
| `pnpm build` (UI) | not run | pass. Vite warns about chunks over 500 kB |
| `pnpm audit` | 23 advisories (2 high, 15 moderate, 6 low) | no known vulnerabilities |
| `pnpm tauri dev` | not runnable (no Rust toolchain) | compiles in about 2 min cold and runs |

Monaco with the overridden dompurify 3.4.17 was exercised live in the dev
server: a markdown hover containing `<img onerror=…>` rendered bold text,
stripped the handler and executed nothing. Visual check of hovers in both themes
inside the desktop window was not done.

Pre-existing issue found: in web mode, `Friends.tsx` throws
`No browser fixture for command "social_report"` (unhandled promise). It does
not affect the desktop build.

## Memory and harness evidence

- Instructions: Claude loads `CLAUDE.md`, which imports `AGENTS.md`; Codex
  reads `AGENTS.md` natively. Native Claude memory lives in the user profile;
  durable project knowledge stays in versioned docs.
- Harness cases. Fixture `templates/harness-evaluation`; evaluator
  `scripts/validate-harness.mjs`, sha256 `ebc2cdd0…5b8c`. The untouched
  baseline failed the expected 2 of 7 checks.

| Case | Model / context | Result | Independent evidence |
|---|---|---|---|
| Boundary fix | Sonnet, fresh subagent | **pass** | evaluator 7/7 rerun by the coordinator; minimal diff; `local-notes.txt` untouched; evaluator unchanged |
| Stale handoff | Sonnet, fresh subagent | **pass** | false "7/7" claim rejected; fixed; HANDOFF corrected with real evidence; 7/7 rerun |
| Read-only security review | Sonnet, fresh subagent | **pass** | fixture hashes identical before and after; private report outside the fixture with a concrete cross-tenant actor/record example, severity, confidence and limits |
| Reapply setup | coordinator | **pass (partial)** | rerunning `rtk init`, `graphify install` and `skills add` left `settings.json` and the user CLAUDE.md byte-identical; one hook, one graphify section, 10 skills |
| Worker/resource conflict, partial blocker, stop/resume | — | **not run** | these need observable host interactions beyond this session |

Harness evaluation copies stayed in the session scratchpad, outside the repository.

## Review

One independent read-only review (Sonnet, fresh context) of the working tree,
excluding vendored skills. 11 findings, dispositioned:

- Fixed: personal path in `docs/onboarding.md` and HANDOFF §6 (now
  `git rev-parse --show-toplevel`); `engines.node` raised to `>=22.13` (ESLint
  10 requires it); dompurify override risk closed by the live Monaco check above.
- Rejected with evidence: `desktop-web` using bare `pnpm` (it launched in this
  session); "19 ignored" count (`cargo test` output reports 19, a grep of
  attributes over-counts); audit claim (`pnpm audit` re-run after install).
- Accepted, not changed (low): the typed config repeats the source globs;
  `vite.config.ts` and `.mjs` build scripts are outside lint scope; this file
  is a dated log by design.

## Pending work and next delivery

1. Done after setup: Claude Code restarted (RTK hook active, `rtk gain`
   records commands) and Context7 authenticated and queried.
2. Decide on CI: a GitHub Actions workflow running `pnpm verify` on Windows.
   It needs a push.
3. Burn down lint warnings rule by rule, then promote each rule to `error`.
   Split the largest files one at a time (docs/quality.md).
4. Add a browser fixture for `social_report`, or have `Friends.tsx` handle its
   rejection.
5. Run a repository-baseline security review with
   `docs/prompts/security-review.md`. Keep the report private.
6. Optional: `rtk init --global --codex`; `cargo install cargo-audit`.

**Repeat or update this setup:** rerun toolkit prompt 12 at a newer revision.
Update skills with `npx --yes skills@latest update --project --yes`, then
review the `skills-lock.json` diff. Regenerate the graph with
`graphify update .`. Updates are re-applied without duplicating anything
(verified above).
