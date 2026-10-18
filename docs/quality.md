# Quality

## Commands

| Command | Proves | Notes |
|---|---|---|
| `pnpm typecheck` | every package compiles under strict TypeScript | `tsc --noEmit` per package |
| `pnpm lint` | ESLint fast tier: JS/TS recommended, typescript-eslint recommended, React Hooks (v7), 350-line budget | ~5 s; must report 0 errors |
| `pnpm lint:typed` | `no-floating-promises`, `no-misused-promises` with type information | ~9 s, ~1.4 GB peak; not in `verify` |
| `pnpm lint:rules` | self-check of the copied `eslint-rules/` (RuleTester) | run after editing `eslint-rules/` |
| `pnpm lint:rust` | `cargo clippy --all-targets` | ~50 s cold; measurement only |
| `pnpm test` | JS tests (relay, i18n, docs build/search/audit) + `cargo test` | real PTY, real Git repos |
| `pnpm verify` | typecheck + lint + lint:rules + test | the combined local gate |
| `pnpm build` | UI production build (`tsc` + `vite build`) | |
| `pnpm audit` / `pnpm audit --prod` | known advisories | sends the lockfile's package list to the registry |

None of these publish, deploy, migrate or touch a remote. There are no Git
hooks and no CI yet (see "Not yet" below).

## Baseline rules

- Strict TypeScript everywhere; pt-BR is typed against the en catalogue, so a
  missing translation is a compile error (§65).
- 350-line budget per source file (`quality/max-lines`, toolkit rule). The i18n
  catalogues are exempt: they are data.
- React Hooks rules. `rules-of-hooks` and `exhaustive-deps` keep their preset
  severity; React Compiler-era rules start at `warn` (counts below).
- `@typescript-eslint/no-unused-vars` is an error (`_` prefix to opt out).
- Never weaken a rule to make a check pass; never add a blanket ignore.

## Lint adoption — measured 2026-10-18

Promote a rule to `error` (delete its override in `eslint.config.mjs`) when its
count reaches zero. Re-measure with `pnpm lint -f json`.

| Rule | Severity | Count at adoption |
|---|---|---|
| `react-hooks/set-state-in-effect` | warn | 24 |
| `quality/max-lines` | warn | 15 files |
| `react-hooks/refs` | warn | 8 |
| `react-hooks/purity` | warn | 2 |
| `react-hooks/immutability` | warn | 1 |
| `react-hooks/exhaustive-deps` | warn (preset) | 1 |
| `react-hooks/rules-of-hooks` | error | 0 (one false positive fixed by renaming a handler that started with `use`) |
| `@typescript-eslint/no-misused-promises` (typed) | warn | 14 |
| `@typescript-eslint/no-floating-promises` (typed) | warn | 9 |
| clippy (all lints) | warning | 144 across 18 lints; top: `needless_question_mark` 39, `dead_code` 36, `unused_imports` 20, `needless_borrow` 16 |

Large files are a refactoring backlog (largest: `mission/store.rs` 1497 lines,
`accounts/live.rs` 1455, `Accounts.tsx` 1419). Split by responsibility, one file
per change, with tests before and after — not as a sweep.

## Testing strategy

Tests go with the code that needs them and prefer real infrastructure — a real
PTY, a real Git repository, real provider transcripts — over mocks (§80).
19 Rust tests are `#[ignore]`d on purpose (real CLIs, microphone, this machine's
history, capture harness); HANDOFF §4 says when to run them.

A green suite is not done: UI work is verified in the running app, both themes,
both languages (HANDOFF §3). `pnpm tauri dev` makes that loop live.

## Review after implementation or fixes

Pick review scope from the risk and the actual diff: behavior and regressions;
security and data for IPC, process spawning, guardrails, Git/filesystem
operations, relay and social-api; accessibility for UI. For nontrivial work use
an independent review (a fresh agent, read-only) over the exact revision range
plus connected callers and tests. Finish when findings are fixed or explicitly
dispositioned and affected checks pass; record residual risk. More rounds need
new evidence or an explicit request.

## Security review

Use [docs/prompts/security-review.md](prompts/security-review.md) — repository
baseline or precise diff, read-only, redacted report written **outside** the
repository (never commit findings). `differential-review` is installed for
security-sensitive diffs. Fixes are a separate, explicit task. A clean report
is not a security guarantee.

## Dependencies

`pnpm-workspace.yaml` carries narrow `overrides` for transitive advisories
(dompurify via monaco-editor, undici via @vercel/blob, source-map-js via vite),
each within its major version. Remove an override once the parent package ships
the fix. After the overrides: `pnpm audit` → no known vulnerabilities
(2026-10-18). Rust crates are not audited yet (`cargo audit` is not installed).

## Not yet

- **CI.** There is no `.github/workflows`. A workflow running `pnpm verify` on
  Windows is the natural next step; it needs a push to GitHub, so it is a
  separate decision.
- **Rust audit** (`cargo audit` / `cargo deny`).
