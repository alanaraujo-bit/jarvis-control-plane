# M26 — Seamless account rotation

**State: core built and verified against the real Claude Code CLI; UI built;
app QA, Codex measurement and final docs pending.** (2026-10-18)

Design: [superpowers/specs/2026-10-18-account-rotation-design.md](superpowers/specs/2026-10-18-account-rotation-design.md).
Plan: [superpowers/plans/2026-10-18-account-rotation.md](superpowers/plans/2026-10-18-account-rotation.md).
Execution ledger (git-ignored): `.superpowers/sdd/2026-10-18-account-rotation/progress.md`.

## What it does

- A live session moves to another linked account **in place**: same J.A.R.V.I.S.
  session id, log, terminal and tailer. The provider process is replaced
  (`LiveSession::replace_process_with`). The transcript `<id>.jsonl` (plus the
  `<id>/` subagents dir) is copied into the destination account's `projects/`
  between the old exit and the new start. The new CLI runs `claude --resume <id>`
  with no `--fork-session`, so it appends to the same file. The tailer is
  retargeted at the same offset (`TailControl`).
- An engine owned by the core (`accounts/rotation/engine.rs`) handles timing and
  decisions:
  - It ticks every 15 s and probes live quota every 60 s for accounts with live
    sessions, 300 s for the rest.
  - It decides once per (account, window, reset).
  - **Near limit:** it moves at the end of the turn (TurnEnded + Idle).
    **Exhausted:** it moves at once and types the continue instruction.
  - **Ask mode:** it emits `Ask` and holds the autopilot while the question is open.
  - **All accounts out:** it emits `AllExhausted`. If accepted, it waits until
    the next reset + 60 s, then resumes on the account that reset, or on the
    best-ranked one.
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
  - untrusted for the folder;
  - not onboarded;
  - over its headroom limit (`limiar − 10`; under the exhausted trigger, 90%).
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
    switched, resumed and failed.
  - `SessionQuota` (a chip in the session tab strip plus a popover) shows the
    windows, resets and ETA, the session's tokens and duration, which accounts
    the session ran on, and a manual switch.
- Policy setting: `accounts.rotation`
  `{mode: off|ask|auto, trigger: nearLimit{percent}|exhausted}`. On first read
  it is migrated from the legacy `accounts.autoSwitch`.

## Verified

- `cargo test --lib`: 695 passed, 21 ignored. `pnpm typecheck` passes.
  `pnpm lint` shows 0 errors and 51 warnings, which is the baseline.
  The i18n tests pass.
- `real_claude_relay_continues_conversation` (`#[ignore]`, scratch repo) PASS.
  An interactive session on account A was told OSPREY-42. The relay moved it
  to B in place, and B answered OSPREY-42. Command:
  `JARVIS_TEST_CLAUDE_A=<dir> JARVIS_TEST_CLAUDE_B=<dir> cargo test --lib real_claude_relay -- --ignored --nocapture`.
- `real_isolation_ignores_ambient_overrides` PASS.
- Found by running for real:
  - Claude Code's trust dialog focuses "No, exit". Untrusted destinations were
    already excluded.
  - A directory that never finished first run parks on its onboarding screens,
    so it is now excluded via `hasCompletedOnboarding`. That flag is read and
    never written.
  - ConPTY yields EOF only when the pseudoconsole closes, so the pump holds the
    PTY weakly.

## Pending (in order)

1. **App QA** (`pnpm tauri dev`, using a QA identifier or a copy of the DB,
   never Alan's `jarvis.db`). Check these screens:
   - Settings → Account rotation;
   - the Accounts notices;
   - the SessionQuota chip and popover on a Claude session in a scratch repo;
   - a manual switch from the popover;
   - the RotationPrompt cards.

   Check them in light and dark themes, in pt-BR and en, and sample pixels for
   colour. Screenshots go in `.tmp/m26-qa/`. To trigger cards without
   exhausting quota, temporarily set Ask + NearLimit 50 on an account above 50%.
2. **Codex:** `CODEX_RESUME_VERIFIED = false` (`rotation/relay.rs`). Codex
   relays are refused with `accounts.codexResumeUnverified`.
   - The Codex account in J.A.R.V.I.S. is signed out: the machine Codex keeps
     its login in the Windows keyring, so there was nothing to copy.
   - Ask Alan to sign in to Codex in the app. Then measure whether
     `codex resume <id>` continues a rollout copied between two `CODEX_HOME`s.
   - If it does, flip the const and add `real_codex_resume_across_homes`.
     Otherwise keep it false and say so here.
3. **Final review:** one fresh reviewer on the whole range `1957826..HEAD`
   (superpowers:requesting-code-review). Run the Review Focus items in the plan.
   Fix Critical and Important findings with TDD.
4. **`pnpm verify`** green.
5. **Docs:**
   - correct M13 §4.7 ("--resume does not cross accounts" is wrong when the
     file is copied);
   - M24: credential cloning is closed;
   - HANDOFF current state and traps;
   - DECISIONS D51 (relay in place via `--resume` + transcript copy; no cloned
     credentials; core-owned engine);
   - BLOCKERS B6 status.
6. Push only with Alan's OK. Commits are local on `master`.

## Traps

- Never `claude auth logout` a copied login: that revokes the machine's token.
- Never write `hasTrustDialogAccepted` or `hasCompletedOnboarding`.
- Bash heredocs in this environment can corrupt `\r` / backslashes inside Python
  patch scripts. Write patch scripts with the Write tool, or use Edit.
- `relay_with` waits a 5 s refusal window. If the destination process exits in
  that window, the conversation is copied back and resumed on the source.
- Side effect already done: the real test completed CLI first run and trusted
  two temp scratch folders in Alan's J.A.R.V.I.S. account dir `01a14f3f…`
  (1a@outlook), through the CLI's own screens.
