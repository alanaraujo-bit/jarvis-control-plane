# Seamless Account Rotation (M26) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A J.A.R.V.I.S. session moves to another linked provider account, in place, and the agent continues the same conversation. Rotation picks the account whose quota expires soonest. Accounts never share credentials with the machine. Live quota, ETA, tokens and duration are visible in the session.

**Architecture:** The core gains `accounts::rotation`, which holds pure ranking/trigger logic, a background engine thread and the in-place relay. A `LiveSession` can replace its PTY child without ending the session log. The transcript tailer can be retargeted to the destination account's copy of the transcript at the same byte offset. The UI subscribes to one `accounts:rotation` event for the toast, the ask modal and the all-exhausted balloon.

**Tech Stack:** Rust (Tauri 2, rusqlite, portable-pty), React 19 + TypeScript, `@jarvis/i18n` (en + pt-BR).

**Spec:** `docs/superpowers/specs/2026-10-18-account-rotation-design.md`

## Global Constraints

- Every number carries Official/Observed/Estimated/Unknown; ETA is always `Estimated`.
- i18n: every new string in `packages/i18n` en and pt-BR (pt-BR typed against en).
- No registered account ⇒ provider processes start exactly as before: no env removal, no rotation.
- Never read `.credentials.json` / `auth.json` contents; copy or exclude them as opaque files only.
- Never write `hasTrustDialogAccepted`; never consume a Codex reset credit.
- Schema changes go in a new migration (`SCHEMA_VERSION` 20 → 21); shipped migrations are frozen.
- Defaults: near-limit threshold 90 (allowed 50–98); destination headroom 10 points under the threshold (near-limit) or < 90% on every window (exhausted); reset-tie window 15 min; ETA slope window 30 min; probe cadence 60 s for accounts with live sessions, 300 s otherwise; wake from the all-exhausted wait at reset + 60 s.
- Continue instruction (exact text): `Continue exactly where you left off; the previous turn was interrupted by a usage limit.`
- Real-CLI tests: `#[ignore]`, run in a scratch git repo under the temp dir, never in a user project.

## Review Focus

1. A rotation fires mid-tool-call under the near-limit trigger → it must wait for `TurnEnded` + `Idle`; pinned in Task 8 (`near_limit_waits_for_turn_end`).
2. Two live sessions on account A trip together → one decision, both move to the same destination, one prompt; pinned in Task 8 (`one_decision_per_account_window`).
3. Destination launch fails (not trusted / signed out) → try the next candidate, and fall back to the original account with `--resume`; nothing lost. Pinned in Task 7 (`relay_falls_back_when_destination_refuses`).
4. Replayed or copied transcript lines are counted twice after a retarget → the offset carries over and usage is not double counted. Pinned in Task 6 (`retarget_keeps_offset_and_new_account`).
5. A twin subscription or a copied-credential account is chosen as a destination → excluded. Pinned in Task 4 (`twins_and_copied_credentials_never_rank`).

---

### Task 1: Provider environment hygiene

**Files:**
- Modify: `apps/desktop/src-tauri/src/accounts/mod.rs` (add `provider_env`; `session_env` becomes a thin caller)
- Modify: `apps/desktop/src-tauri/src/pty/mod.rs` (`PtyOptions.env_remove`)
- Modify: `apps/desktop/src-tauri/src/accounts/live.rs` (`account_command`), `accounts/switch.rs` (sign-in/out commands), `accounts/mod.rs` (`read_identity`)
- Modify: `apps/desktop/src-tauri/src/session/commands.rs` (launch applies `env_remove`)
- Test: `apps/desktop/src-tauri/src/accounts/tests.rs`, `pty/mod.rs` tests

**Interfaces:**
- Produces: `pub struct ProviderEnv { pub remove: Vec<String>, pub set: Vec<(String, String)> }`; `pub fn provider_env(account: &Account) -> ProviderEnv`; `pub fn apply_env(cmd: &mut std::process::Command, env: &ProviderEnv)`; `PtyOptions { …, env_remove: Vec<String> }` (removed after the inherited env and before `env`).
- `remove` = `ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN, ANTHROPIC_BASE_URL, CLAUDE_CODE_OAUTH_TOKEN, CLAUDE_CONFIG_DIR, OPENAI_API_KEY, OPENAI_BASE_URL, CODEX_HOME` (both providers get the full list).

- [ ] **Step 1: Failing tests** — `provider_env_strips_ambient_overrides` (asserts `remove` contains every name above and `set == [("CLAUDE_CONFIG_DIR", dir)]` for claude, `[("CODEX_HOME", dir)]` for codex); `pty_env_remove_beats_inherited` (sets `ANTHROPIC_API_KEY` in the test process, spawns `cmd /c set` / `env` with `env_remove`, asserts the variable is absent).
- [ ] **Step 2:** `cargo test provider_env pty_env_remove` → FAIL (undefined).
- [ ] **Step 3:** Implement. Every `tool_command` use that runs on an account (`account_command`, `read_identity`, `sign_in_command`, `sign_out_command`) calls `apply_env`. Launch fills `env_remove` only when `account.is_some()`.
- [ ] **Step 4:** `cargo test accounts pty` → PASS.
- [ ] **Step 5:** Commit `fix(accounts): strip ambient provider overrides from account processes`.

### Task 2: Credentials are never cloned (migration 21)

**Files:**
- Modify: `apps/desktop/src-tauri/src/db/migrations.rs` (migration 21, `SCHEMA_VERSION = 21`, fingerprint in `SHIPPED`)
- Modify: `apps/desktop/src-tauri/src/accounts/mod.rs` (`Account.credential_origin`, `SELECT`, `row_to_account`, `copy_provider_state`, `isolate_adopted_accounts`)
- Modify: `apps/desktop/src-tauri/src/accounts/commands.rs` (successful sign-in sets `own`)
- Test: `apps/desktop/src-tauri/src/accounts/tests.rs`

**Interfaces:**
- Produces: column `provider_accounts.credential_origin TEXT NOT NULL DEFAULT 'own'`; `Account.credential_origin: String` (`"own" | "copied"`); `pub fn set_credential_origin(db: &Database, id: &str, origin: &str) -> Result<()>`.
- Migration 21 sets `'copied'` for every `adopted = 1` row whose `config_dir` contains `.jarvis-isolated`.

- [ ] **Step 1: Failing tests:**
  - `migration_21_marks_isolated_adopted_rows_copied`;
  - `fresh_import_excludes_credential_files`: the source holds `.credentials.json` and `auth.json`, and neither exists in the target after `isolate_adopted_accounts`;
  - `sign_in_success_marks_own`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement:
  - `copy_tree` top-level exclusion adds `.credentials.json` and `auth.json`;
  - a row imported fresh is `signed_in = 0`;
  - the sign-in completion path calls `set_credential_origin(.., "own")` after `refresh_identity` reports signed in.
- [ ] **Step 4:** `cargo test accounts db::` → PASS (migration fingerprint test included).
- [ ] **Step 5:** Commit `fix(accounts): stop sharing an OAuth lineage with the machine`.

### Task 3: Rotation policy setting

**Files:**
- Create: `apps/desktop/src-tauri/src/accounts/rotation/mod.rs` (module root), `accounts/rotation/policy.rs`
- Modify: `accounts/mod.rs` (`pub mod rotation;`), `accounts/commands.rs` (`account_rotation_policy`, `account_set_rotation_policy`; delete `account_set_auto_switch`), `lib.rs` (register commands)
- Test: `accounts/rotation/policy.rs` `#[cfg(test)]`

**Interfaces:**
- Produces: `#[serde(rename_all="camelCase")] pub enum Mode { Off, Ask, Auto }`; `#[serde(tag="kind", rename_all="camelCase")] pub enum Trigger { NearLimit { percent: u8 }, Exhausted }`; `pub struct Policy { pub mode: Mode, pub trigger: Trigger }` (default `Off` + `NearLimit{90}`); `pub const POLICY_KEY: &str = "accounts.rotation"`; `pub fn policy(db) -> Policy` (migrates the legacy `accounts.autoSwitch` on first read: `onExhaustion → Auto+Exhausted`, `onThreshold → Auto+NearLimit{85}`, `off → Off`); `pub fn set_policy(db, Policy) -> Result<()>` (clamps percent to 50..=98).

- [ ] **Step 1: Failing tests:**
  - `legacy_on_threshold_migrates_to_auto_near_85`;
  - `legacy_on_exhaustion_migrates`;
  - `percent_is_clamped` (set 30 → 50, set 99 → 98);
  - `default_is_off_near_90`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement on `crate::settings::{get_or,set}`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(accounts): rotation mode and trigger as one policy`.

### Task 4: Ranking ("use it or lose it")

**Files:**
- Create: `apps/desktop/src-tauri/src/accounts/rotation/rank.rs`
- Test: same file `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub struct WindowState { pub percent_used: Option<f64>, pub resets_at_ms: Option<i64>, pub exhausted: bool, pub confidence: Confidence }`;
  - `pub struct Candidate { pub account: Account, pub session: Option<WindowState>, pub weekly: Option<WindowState>, pub folder_trusted: Option<bool> }`;
  - `pub enum Why { ExpiresSoonest { resets_at_ms: i64 }, Reserve, WeeklyUrgency, Unknown }`;
  - `pub struct Ranked { pub account: Account, pub why: Why }`;
  - `pub fn eligible(c: &Candidate, current: &Account, trigger: &Trigger) -> bool`;
  - `pub fn rank(now_ms: i64, current: &Account, trigger: &Trigger, candidates: Vec<Candidate>) -> Vec<Ranked>`;
  - `pub fn candidate_for(db: &Database, account: &Account, project_root: Option<&Path>) -> Candidate`. It reads the live reading first, then falls back to `quota::for_account`.
- Eligibility comes from spec §6.2: same provider, not current, `!same_subscription`, `signed_in`, `!paused`, `credential_origin == "own"`, `folder_trusted != Some(false)`, plus the headroom rule from Global Constraints.
- Order comes from spec §6.3:
  1. Accounts with a running session window, sorted by `resets_at_ms` ascending.
  2. Reserves (no session reset).
  3. Unknown accounts (no numbers).
  - Within 15 min of each other, the higher weekly urgency `(100 − weekly%) / hours_to_weekly_reset` wins.
  - The final tie-break is `position`.

- [ ] **Step 1: Failing tests:**
  - `soonest_reset_wins` — A resets in 40 min at 30%, B in 4 h at 10%, so the order is A, B;
  - `running_window_beats_reserve`;
  - `unknown_ranks_last`;
  - `weekly_urgency_breaks_reset_ties`;
  - `twins_and_copied_credentials_never_rank`;
  - `headroom_excludes_near_full_destination` — near-limit 90, candidate at 81%, so it is excluded;
  - `exhausted_trigger_uses_ninety_ceiling`;
  - `untrusted_folder_excluded`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement `rank` and `candidate_for`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(accounts): rank destinations by quota that expires first`.

### Task 5: In-place process replacement

**Files:**
- Modify: `apps/desktop/src-tauri/src/session/manager.rs` (`LiveSession.pty: Arc<Mutex<Arc<PtyHandle>>>`, `replacing: Arc<AtomicBool>`, `replace_process`)
- Modify: `apps/desktop/src-tauri/src/session/event.rs` (`Lifecycle::AccountSwitch { from: String, to: String, reason: String }`, `Lifecycle::Relaunched { command: String }`)
- Test: `session/manager.rs` tests (real PTY, `cmd /c` / `sh -c` child)

**Interfaces:**
- Produces: `impl LiveSession { pub fn replace_process(&self, options: PtyOptions) -> Result<()> }`. It sets `replacing`, kills the child and waits for the old pump's `Exited`. On that path the pump writes `Lifecycle::Exited` but does NOT send `LogCommand::Stop`, does NOT forward `Beat::Exited` to the watcher, and does NOT set Completed/Failed. Then it spawns the new PTY with the same `cols`/`rows` and starts a new pump on the same `log_tx`/`sink`/`watch`. State becomes `Starting`, and `Lifecycle::Relaunched` is logged. `write`, `resize` and `kill` go through the current handle.

- [ ] **Step 1: Failing test** `replace_process_keeps_id_log_and_attachment`. It starts a child that prints `one`, then replaces it with one that prints `two`, and asserts:
  - the same `id`;
  - the log contains `one`, `Relaunched` and `two`, in order;
  - the state is not `Completed` after the swap;
  - an attached sink received `two`;
  - the watcher got no `Exited`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test session::manager` → PASS.
- [ ] **Step 5:** Commit `feat(session): replace the provider process without ending the session`.

### Task 6: Retargetable transcript tailer

**Files:**
- Modify: `apps/desktop/src-tauri/src/session/transcript.rs` (`spawn` returns `TailControl`; remove the `maybe_rotate_recorded` call)
- Modify: `apps/desktop/src-tauri/src/providers/tail.rs` (`JsonlTailer::retarget(path)` keeping offset)
- Modify: `session/manager.rs` (`LiveSession.tail: Mutex<Option<TailControl>>`, setter/getter), `session/commands.rs` (store it)
- Test: `session/transcript.rs` tests

**Interfaces:**
- Produces: `pub struct TailControl(std::sync::mpsc::Sender<Retarget>)`; `pub struct Retarget { pub path: PathBuf, pub account_id: Option<String> }`; `impl TailControl { pub fn retarget(&self, r: Retarget) }`; `JsonlTailer::retarget(&mut self, path: PathBuf)` — new path, same byte offset and partial-line buffer. The tailer loop drains the channel before each poll and swaps `account_id`.

- [ ] **Step 1: Failing test** `retarget_keeps_offset_and_new_account`:
  - set-up: file A has 3 lines; copy it to B and append 2 lines to B; retarget to B with account `b`;
  - assertion: exactly 2 new items are mirrored, and their `usage_samples.account_id = 'b'`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test session::transcript providers::tail` → PASS.
- [ ] **Step 5:** Commit `feat(session): a transcript tailer that can follow a moved conversation`.

### Task 7: The in-place relay

**Files:**
- Create: `apps/desktop/src-tauri/src/accounts/rotation/relay.rs`
- Modify: `apps/desktop/src-tauri/src/session/commands.rs`:
  - `command_for` takes `ResumeMode`;
  - extract `fn pty_options_for(state, session_id, account, resume: ResumeMode) -> Result<PtyOptions>` from `launch`, reusing the guardrail settings and the brief already written in the session dir.
- Modify: `apps/desktop/src-tauri/src/autopilot/driver.rs` (delete the `relay_needed`/`relay_autopilot` block); `accounts/switch.rs` (delete `maybe_rotate*`, `relay_needed`, `relay_autopilot`, `next_available`; keep sign-in helpers, `set_active`, `record_rotation`)
- Test: `accounts/rotation/relay.rs` tests (fake provider binary = a script on `PATH` in a temp dir), `accounts/tests.rs` (drop obsolete tests)

**Interfaces:**
- Consumes:
  - `LiveSession::replace_process` (Task 5);
  - `TailControl::retarget` (Task 6);
  - `rank` and `candidate_for` (Task 4);
  - `provider_env` (Task 1).
- Produces:
  - `pub enum ResumeMode { None, Fork(String), Continue(String) }`. Claude `Continue(id)` gives `--resume <id>` with no `--fork-session` and no `--session-id`. Codex `Continue(id)` gives `codex resume <id>`.
  - `pub fn carry_conversation(provider: &str, from: &Path, to: &Path, cwd: &str, provider_session_id: &str) -> Result<PathBuf>`. Claude copies `projects/<encoded cwd>/<id>.jsonl` plus the `<id>/` dir if present. Codex copies `sessions/**/rollout-*-<id>.jsonl` to the same relative path. Both return the destination transcript path.
  - `pub enum Reason { NearLimit, Exhausted, Manual, ResumeAfterWait }`.
  - `pub fn relay_session(state: &AppState, session_id: &str, to: &Account, reason: Reason) -> Result<()>`. Steps:
    1. Log `AccountSwitch`.
    2. Call `replace_process`. The kill comes before the copy because the file must be closed.
    3. Carry the conversation.
    4. Run `UPDATE sessions SET account_id`, then `stamp_used`.
    5. Retarget the tailer.
    6. If `reason ∈ {Exhausted, ResumeAfterWait}`, write the continue instruction through `autopilot::driver::send` once the session leaves `Starting`.
    7. Record activity `account.relayed` with `{from,to,reason,sessionId}`.
  - On a launch or copy failure, relaunch on the original account with `Continue(id)` and return `Err(code)`.
  - `pub const CODEX_RESUME_VERIFIED: bool = false;` (flipped in Task 12 only if the measurement passes). When false, `relay_session` refuses Codex with `accounts.codexResumeUnverified`.

- [ ] **Step 1: Failing tests:**
  - `claude_continue_args_have_resume_without_fork`;
  - `carry_conversation_copies_claude_transcript_and_subagents`;
  - `carry_conversation_finds_codex_rollout_by_id`;
  - `relay_switches_account_row_and_logs` (fake `claude` script);
  - `relay_falls_back_when_destination_refuses` (fake script exits 1 under destination dir → original account relaunched, row unchanged);
  - `codex_relay_refused_until_verified`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test accounts autopilot session` → PASS.
- [ ] **Step 5:** Commit `feat(accounts): move a live session to another account and keep the conversation`.

### Task 8: Rotation engine (triggers, safe point, ask, wait)

**Files:**
- Create: `apps/desktop/src-tauri/src/accounts/rotation/engine.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs` (start the engine in setup; register commands), `accounts/commands.rs` (stop UI-driven `refresh_all` from rotating; the engine owns probing cadence), `autopilot/driver.rs` (hold `Step::Say` while `engine::holds(session_id)`; freeze `turns`/`stalled_rounds` while held), `session/transcript.rs` (on `TurnEnded` call `engine::turn_ended(session_id)`)
- Test: `engine.rs` tests driving `Engine::tick(now)` with an injected `Probe` fn and in-memory db

**Interfaces:**
- Produces:
  - `pub struct Engine`, `pub fn start(app: tauri::AppHandle) -> Arc<Engine>` — a thread that ticks every 15 s;
  - `pub fn tick(&self, now_ms: i64)` (testable without a thread);
  - `pub fn turn_ended(&self, session_id: &str)`;
  - `pub fn holds(&self, session_id: &str) -> bool`;
  - `pub fn answer(&self, decision_id: &str, accept: bool) -> Result<()>`;
  - `pub fn accept_wait(&self, decision_id: &str, accept: bool) -> Result<()>`;
  - `pub fn switch_now(&self, session_id: &str, to: &str) -> Result<()>` (manual);
  - `pub fn queue(&self, session_id: &str) -> Vec<Ranked>`.
- Emits `accounts:rotation` with `#[serde(tag="kind", rename_all="camelCase")] enum RotationEvent`:
  - `Switched { decisionId, fromAccountId, toAccountId, sessionIds, reason, confidence, resetsAtMs }`;
  - `Ask { decisionId, fromAccountId, toAccountId, sessionIds, percent, confidence, resetsAtMs }`;
  - `AllExhausted { decisionId, sessionIds, nextAccountId, nextResetMs }`;
  - `Waiting { decisionId, untilMs }`;
  - `Resumed { decisionId, accountId }`;
  - `Failed { decisionId, code }`.
- Behaviour:
  - **Probe cadence:** 60 s / 300 s, using `live::probe` + `store` + `record_reading`.
  - **Trigger:** evaluated per account with live sessions.
  - **Near-limit decision:** waits until each session's next `turn_ended` + `Idle`.
  - **Exhausted:** immediate.
  - **Decisions are keyed** `(account_id, window, resets_at_ms)`. "Agora não" stores the key, so the same window never asks again.
  - **Moving `active`:** if the trigger account is `active`, call `set_active(destination)`.
  - **No eligible candidate:** emit `AllExhausted`. On accept, sleep until the earliest reset + 60 s, re-probe, then relay with `ResumeAfterWait`.
  - **Hysteresis:** a session relayed within this window never re-triggers until its new account itself trips.

- [ ] **Step 1: Failing tests:**
  - `near_limit_waits_for_turn_end`;
  - `exhausted_relays_immediately`;
  - `one_decision_per_account_window`;
  - `ask_mode_emits_ask_and_holds_autopilot`;
  - `declined_window_is_not_asked_again`;
  - `all_exhausted_then_resume_at_reset`;
  - `active_moves_only_when_active_account_trips`;
  - `off_mode_does_nothing`;
  - `no_accounts_means_no_engine_work`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test accounts::rotation autopilot` → PASS.
- [ ] **Step 5:** Commit `feat(accounts): a core-owned rotation engine with ask, auto and wait`.

### Task 9: Commands, ETA and session usage summary

**Files:**
- Create: `apps/desktop/src-tauri/src/accounts/rotation/commands.rs`
- Modify: `accounts/quota.rs` (`eta`), `lib.rs`
- Test: `accounts/quota.rs` tests, `rotation/commands.rs` tests

**Interfaces:**
- Produces commands:
  - `rotation_answer(decision_id: String, accept: bool)`;
  - `rotation_accept_wait(decision_id: String, accept: bool)`;
  - `session_switch_account(session_id: String, account_id: String)`;
  - `session_account_queue(session_id: String) -> Vec<RankedDto>`;
  - `session_quota_summary(session_id: String) -> SessionQuotaSummary`.
- `pub struct Eta { pub at_ms: i64, pub confidence: Confidence /* always Estimated */ }`.
- `pub fn eta(db: &Database, account_id: &str, window: &str, now_ms: i64) -> Option<Eta>`. It takes the least-squares slope of `percent` over `account_limit_events` in the last 30 min of the current window. It returns `None` for fewer than 2 readings or a slope ≤ 0. It returns `None` if the ETA lands after `resets_at_ms`.
- `SessionQuotaSummary { account: Account, windows: Vec<LiveWindow>, eta: Vec<(String /*window*/, Eta)>, tokens_in: i64, tokens_out: i64, tokens_cache: i64, started_at_ms: i64, segments: Vec<{account_id, from_ms, to_ms}>, queue_head: Option<RankedDto>, confidence per figure }`. Segments come from `Lifecycle::AccountSwitch` entries / `account.relayed` activity.

- [ ] **Step 1: Failing tests:**
  - `eta_from_rising_readings` — 40% → 60% over 20 min, so 100% is at +40 min;
  - `eta_none_when_flat_or_single_reading`;
  - `eta_none_past_reset`;
  - `summary_splits_segments_by_switch`.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(accounts): ETA and per-session quota summary`.

### Task 10: Settings room and Accounts screen changes

**Files:**
- Create: `apps/desktop/src/surfaces/settings/RotationPanel.tsx` (+ `.css`)
- Modify: `surfaces/settings/categories.ts` (`"rotation"`, icon `Gauge`, after `"account"`), `Settings.tsx` (route), `surfaces/accounts/Accounts.tsx` (replace the auto-switch radiogroup with the current policy summary + "Configurar" link; credential-origin notice "Login compartilhado com a máquina — entre por aqui para usar no rodízio" with the existing sign-in action; queue line), `surfaces/accounts/useAccounts.ts` (types), `packages/i18n` en + pt-BR
- Test: `pnpm typecheck`, i18n tests

**Interfaces:**
- Consumes: `account_rotation_policy`, `account_set_rotation_policy` (Task 3); `Account.credentialOrigin` (Task 2).
- The panel has three parts:
  - a mode segmented control: Desligado / Perguntar antes / Automático;
  - a trigger radio: Perto do limite (with a `NumberSetting` 50–98) / Quando acabar;
  - one sentence explaining the rotation order.
- Reuse the existing `NumberSetting` and settings-section styles.

- [ ] **Step 1:** Add i18n keys under `accounts.rotation.*` (en + pt-BR); `pnpm --filter @jarvis/i18n test` → PASS.
- [ ] **Step 2:** Implement the panel and the Accounts changes.
- [ ] **Step 3:** `pnpm typecheck` → 0 errors.
- [ ] **Step 4:** Commit `feat(ui): rotation settings and shared-login notice`.

### Task 11: Rotation prompts and session quota strip

**Files:**
- Create: `apps/desktop/src/shell/RotationPrompt.tsx` (+ `.css`) — listens to `accounts:rotation`:
  - **Ask:** a small modal with Trocar / Agora não.
  - **AllExhausted:** a balloon with Retomar automaticamente / Não.
  - **Waiting:** a slim pill with a countdown and a Cancelar action.
  - **Switched / Resumed / Failed:** go to the existing in-app `Toasts` as announcements.
- Create: `apps/desktop/src/surfaces/project/SessionQuota.tsx` (+ `.css`):
  - compact strip in the session header: account label · `5h 62% · 15:40` · `sem 41%` · `acaba ~14:55`;
  - click opens a detailed popover: both windows with bars, used/free, countdown, ETA, session tokens in/out/cache, duration, segments per account, next in queue, and "Trocar conta" listing `session_account_queue`.
- Modify: `App.tsx` (mount `RotationPrompt`), `surfaces/project/ProjectWorkspace.tsx` (mount `SessionQuota` for agent sessions), `packages/i18n`.
- Reuse: `surfaces/accounts/format.ts` (`countdown`, `nextTickDelay`, `remaining`, `severityBand`), `Gauge.tsx`, design tokens; confidence badges as on Accounts cards.

- [ ] **Step 1:** i18n keys `rotation.prompt.*`, `session.quota.*` (en + pt-BR); i18n tests PASS.
- [ ] **Step 2:** Implement both components; refresh `session_quota_summary` on `accounts:rotation` events and every 60 s while mounted.
- [ ] **Step 3:** `pnpm typecheck && pnpm lint` → 0 errors, no new warnings over the 51 baseline.
- [ ] **Step 4:** Commit `feat(ui): rotation prompts and live session quota`.

### Task 12: Real-CLI verification, app QA, docs

**Files:**
- Modify: `apps/desktop/src-tauri/src/accounts/tests.rs` (ignored real-CLI tests), `accounts/rotation/relay.rs` (`CODEX_RESUME_VERIFIED` per measurement)
- Create: `docs/M26-ACCOUNT-ROTATION.md`
- Modify: `docs/M13-ACCOUNTS.md` §4.7 (correct the `--resume` claim and link M26), `docs/M24-ACCOUNT-RESILIENCE.md` (credential cloning closed), `docs/HANDOFF.md` (current state + traps), `docs/DECISIONS.md` (D51: rotation relays in place via `--resume` + transcript copy; no cloned credentials), `docs/BLOCKERS.md` B6 (status)

- [ ] **Step 1:** `#[ignore] real_claude_relay_continues_conversation`:
  - setup: scratch repo and the two registered J.A.R.V.I.S. Claude account dirs, read from the real DB copy;
  - action: `relay_session` with `Manual`;
  - assertion: the agent repeats a word stated before the switch.
  - Run: `cargo test real_claude_relay -- --ignored --nocapture` → PASS.
- [ ] **Step 2:** `#[ignore] real_isolation_ignores_ambient_overrides` — with the ambient env set, `auth status` for the account still matches its own email. Run → PASS.
- [ ] **Step 3:** Codex measurement: ask Alan to sign in to Codex in J.A.R.V.I.S. Then:
  - Run `#[ignore] real_codex_resume_across_homes` between that home and a second signed-in home.
  - If it passes, set `CODEX_RESUME_VERIFIED = true` and re-run the suite.
  - If it fails or no second home exists, leave it false and record the result in M26.
- [ ] **Step 4:** App QA:
  - Run `pnpm tauri dev` with a QA identifier on a copy of `jarvis.db`.
  - Exercise the Settings room, ask modal, toast, all-exhausted balloon (forced via the engine's test hook), strip and popover, and the Accounts notice.
  - Check pt-BR and en, light and dark.
  - Save screenshots in `.tmp/m26-qa/`.
- [ ] **Step 5:** `pnpm verify` → green.
- [ ] **Step 6:** Write the docs listed above.
- [ ] **Step 7:** Commit `docs(m26): seamless rotation, verified against the real CLIs`.
