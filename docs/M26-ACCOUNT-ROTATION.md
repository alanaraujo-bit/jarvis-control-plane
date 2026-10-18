# M26 — Seamless account rotation

**State: complete (2026-10-18).** Core verified against the real Claude Code
and Codex CLIs, app QA done, final review findings fixed, `pnpm verify` green.
Commits are local on `master`; pushing waits for Alan's OK.

Design: [superpowers/specs/2026-10-18-account-rotation-design.md](superpowers/specs/2026-10-18-account-rotation-design.md).
Plan: [superpowers/plans/2026-10-18-account-rotation.md](superpowers/plans/2026-10-18-account-rotation.md).
Decision: [D51](DECISIONS.md). Execution ledger (git-ignored):
`.superpowers/sdd/2026-10-18-account-rotation/progress.md`.

## What it does

- A live session moves to another linked account **in place**: same J.A.R.V.I.S.
  session id, log, terminal and tailer. The provider process is replaced
  (`LiveSession::replace_process_with`). Between the old exit and the new start,
  the transcript is copied into the destination account's directory at the same
  relative path:
  - Claude: `<id>.jsonl` plus the `<id>/` subagents dir under `projects/`; the
    new CLI runs `claude --resume <id>` with no `--fork-session`.
  - Codex: `rollout-…-<id>.jsonl` under `sessions/`; the new CLI runs
    `codex resume <id>`.

  Either way the new process appends to the same file, and the tailer is
  retargeted at the same offset (`TailControl`).
- Nothing is killed before the transcript exists: a session that has not
  written its conversation yet is not moved (`accounts.transcriptMissing`).
- If the destination exits within 5 s, what it appended is added back to the
  original transcript (append, never a rewrite under the tailer), and the
  original account resumes. If even that fails, the session ends as failed.
- An engine owned by the core (`accounts/rotation/engine.rs`) handles timing and
  decisions:
  - It ticks every 15 s and probes live quota every 60 s for accounts with live
    sessions, 300 s for the rest. One pass runs at a time.
  - It decides once per (account, window, reset rounded to the minute, reason).
  - **Near limit:** it moves at the end of the turn (a `TurnEnded` that no later
    agent work has superseded, and Idle). With nowhere to go it does nothing yet
    and asks again once another account frees up.
  - **Exhausted:** it moves at once and types the continue instruction.
  - A refused destination hands over to the next ranked one. When every one
    refuses an exhausted session, it waits for a reset.
  - **Ask mode:** it emits `Ask` and holds the autopilot while the question is open.
    Open questions can be listed (`rotation_pending`) for a screen that loads late.
  - **All accounts out:** it emits `AllExhausted`. If accepted, or left
    unanswered past the reset, it waits until the next reset + 60 s, then
    resumes on the account that reset, or on the best-ranked one.
  - `active` moves only when the active account trips.
- Ranking (`rank.rs`, "use it or lose it"):
  - first, running 5 h windows, earliest reset first;
  - then reserves (no window open);
  - then accounts with unknown numbers;
  - ties within 15 min go to the higher weekly urgency.
- An account is excluded as a destination if it is any of:
  - a twin subscription;
  - a copied login;
  - signed out or paused;
  - untrusted for **any** of the moving sessions' folders (Claude
    `.claude.json`, Codex `config.toml`, keyed by the Git root);
  - not onboarded (Claude `hasCompletedOnboarding`);
  - over its headroom limit (`limiar − 10`; under the exhausted trigger, 90%).

  Trust and onboarding are read, never written.
- Isolation: provider processes on a registered account drop ambient
  `ANTHROPIC_*`/`OPENAI_*`/`CLAUDE_CODE_OAUTH_TOKEN`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`
  (`accounts::provider_env`, PTY `env_remove`, and the probes too).
  Credentials are never copied again. Migration 21 adds
  `provider_accounts.credential_origin` and marks adopted rows `copied`.
  `forget_copied_login` drops only the app-owned copy (no provider logout,
  which would revoke the machine's token). After that comes the in-app sign-in
  (`account_sign_in_own`).
- UI:
  - Settings → "Account rotation" (`RotationPanel`): the mode, the trigger and
    a 50–98% slider.
  - The Accounts screen shows a summary of the policy, a notice for copied
    logins, and a notice for accounts that are not onboarded.
  - `RotationPrompt` (top-centre cards) covers ask, allExhausted, waiting,
    switched, resumed and failed (stayed / ended / nothing to move yet).
  - `SessionQuota` (a chip in the session tab strip plus a popover) shows the
    windows (each coloured by its own band), resets and ETA, the session's
    tokens and duration, which accounts the session ran on, and a manual switch.
- Policy setting: `accounts.rotation`
  `{mode: off|ask|auto, trigger: nearLimit{percent}|exhausted}`. On first read
  it is migrated from the legacy `accounts.autoSwitch`.

## Verified

- `pnpm verify` (2026-10-18): typecheck green; lint 0 errors / 51 warnings
  (baseline); `cargo test --lib` 717 passed, 22 ignored; relay 22, i18n 9;
  docs no problems.
- Real CLIs, scratch repositories (`#[ignore]`):
  - `real_claude_relay_continues_conversation` PASS (Claude Code 2.1.302): an
    interactive session on account A heard OSPREY-42, moved to B in place, and
    B answered OSPREY-42.
  - `real_codex_resume_across_homes` PASS (Codex 0.163.0): before the copy B
    answers `no rollout found for thread id`; after it, B recalled the word A
    heard and appended to the copied rollout at the same relative path. Also
    reproduced by hand with `codex exec resume` (KESTREL-9).
  - `real_isolation_ignores_ambient_overrides` PASS.
  - Commands: `JARVIS_TEST_CLAUDE_A=<dir> JARVIS_TEST_CLAUDE_B=<dir> cargo test --lib real_claude_relay -- --ignored --nocapture`;
    `JARVIS_TEST_CODEX_A=<home> JARVIS_TEST_CODEX_B=<home> cargo test --lib real_codex_resume -- --ignored --nocapture`.
- App QA (`pnpm tauri dev` with identifier `dev.jarvis.desktop.m26qa` over a
  copy of Alan's DB, scratch repo; screenshots in `.tmp/m26-qa/`):
  - Settings → Account rotation, the Accounts notices (copied login), the
    SessionQuota chip and popover, and every RotationPrompt card, in light and
    dark, pt-BR and en, with pixels sampled.
  - **A real Ask, end to end:** Ask + Near limit 80% on `…2a@gmail` (weekly 97%).
    The card asked "Continue on Claude 2?". Accepting moved the session to
    Claude 2 after the turn, the agent there answered the word from before the
    move (PLOVER-31), Activity recorded `account.relayed` (nearLimit), `active`
    followed, and the popover listed both segments.
  - A Codex session starts in the app (it could not before; see below).
- Final review of `1957826..HEAD` by a fresh reviewer: 2 Critical and 9
  Important findings, all fixed with regression tests proven by mutation
  (commits `36605a0`, `910aeda`).

## Found by running it

- Claude Code's trust dialog focuses "No, exit". Untrusted destinations were
  already excluded.
- A directory that never finished first run parks on its onboarding screens,
  so it is excluded via `hasCompletedOnboarding`.
- ConPTY yields EOF only when the pseudoconsole closes, so the pump holds the
  PTY weakly.
- A near-limit trip with nowhere to go was settled for the whole window, so no
  question came when another account freed up, and the window running out was
  silenced too (`e5a44c6`).
- The chip coloured only the 5 h window, the popover showed stale tokens, and
  run-out estimates read "Resets in" (`82660e1`).
- Claude writes `ai-title` after the turn closes. Read as "a turn started", it
  erased the safe point and an approved switch never happened (`8b1b773`).
- No Codex session could start on this machine: the PTY handed npm's
  extensionless `codex` shell script to CreateProcessW (`os error 193`)
  (`c26fde8`).
- Codex sets up its Windows sandbox on the first run in each `CODEX_HOME` and
  drops keystrokes until "Sandbox ready" (HANDOFF item 72).
- Right at a reset, Claude's probe can answer `rate_limits_available: false`
  for one read, and the card says "This plan has no subscription quota" until
  the next probe. Pre-existing (M16), harmless, not fixed here.

## Not verified, and why

- **A successful manual switch from the popover.** The only other Claude
  account (`…2a@gmail`) is a copied login, so it is never a destination, and
  the queue correctly says no account can take the work. The command path is
  covered by engine and relay tests. Signing `…2a@gmail` in from the app
  ("Sign in here") would make it testable.
- **A provider refusing a turn in the installed app.** Observing the exhausted
  trigger live means spending an account to zero; the logic is covered by tests
  and the move by the real runs above.

## Left on the table (minor review findings)

- A destination that parks on a screen *without exiting* (login, onboarding)
  reads as a success; only an exit within 5 s is a refusal. Trust and
  onboarding are checked beforehand to keep this rare.
- The slider writes the policy on every step.
- "Sign in here" drops the copied login before the OAuth flow; a cancelled
  sign-in leaves the account signed out.
- The environment scrub removes `CODEX_HOME`, not every inherited `CODEX_*`.
- Official and Observed look the same on the chip (only Estimated is marked).

## Side effects on Alan's machine

- The real Claude test completed CLI first run and trusted two temp scratch
  folders in the J.A.R.V.I.S. account dir `01a14f3f…` (`…1a@outlook`), through
  the CLI's own screens.
- The real Codex test trusted six temp scratch folders in the J.A.R.V.I.S.
  Codex account (`01a1494c…`) through Codex's own screen, and Codex wrote its
  own `[windows] sandbox = "elevated"` there. A second home was signed in for
  the measurement in this session's scratchpad (temporary).
- QA ran on a copy of the database under `%APPDATA%\dev.jarvis.desktop.m26qa`;
  Alan's `jarvis.db` was not opened for writing.

## Traps

- Never `claude auth logout` a copied login: that revokes the machine's token.
- Never write `hasTrustDialogAccepted`, `hasCompletedOnboarding`, or Codex
  `trust_level`.
- Bash heredocs in this environment can corrupt `\r` / backslashes inside Python
  patch scripts. Write patch scripts with the Write tool, or use Edit.
- See HANDOFF items 68–72.
