//! Account rotation policy (§66).
//!
//! Switching changes only which configuration directory the *next* session
//! receives. A process that is already running keeps the credentials it
//! loaded at startup; touching it would break the continuity this feature is
//! meant to preserve.

use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::db::Database;

use super::{get, list, Account, Result};

pub const POLICY_KEY: &str = "accounts.autoSwitch";

/// Build the provider's own persistent sign-in flow for one configuration
/// directory. The caller spawns it off the Tauri command thread.
pub fn sign_in_command(account: &Account, email: Option<&str>) -> Result<std::process::Command> {
    let (bin, mut args): (&str, Vec<&str>) = match account.provider.as_str() {
        "claude-code" => ("claude", vec!["auth", "login", "--claudeai"]),
        "codex" => ("codex", vec!["login"]),
        _ => return Err("accounts.unknownProvider".into()),
    };
    if account.provider == "claude-code" {
        if let Some(email) = email.filter(|value| !value.trim().is_empty()) {
            args.extend(["--email", email]);
        }
    }
    let mut command = crate::envscan::tool_command(bin, &args)
        .ok_or_else(|| "accounts.providerMissing".to_string())?;
    if !account.adopted {
        if let Some(key) = super::config_env_key(&account.provider) {
            command.env(key, &account.config_dir);
        }
    }
    Ok(command)
}

/// When J.A.R.V.I.S. may move new work away from an account.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AutoSwitchPolicy {
    #[default]
    Off,
    /// Move only after the provider has explicitly refused a turn.
    OnExhaustion,
    /// Also move at the estimated nearing threshold.
    OnThreshold,
}

pub fn policy(db: &Database) -> AutoSwitchPolicy {
    crate::settings::get_or(db, POLICY_KEY, AutoSwitchPolicy::Off)
}

pub fn set_policy(db: &Database, value: AutoSwitchPolicy) -> Result<()> {
    crate::settings::set(db, POLICY_KEY, &value)
}

/// Make one account the destination for future sessions.
pub fn set_active(db: &Database, account_id: &str) -> Result<Account> {
    let account = get(db, account_id)?.ok_or_else(|| "accounts.notFound".to_string())?;
    if !account.signed_in {
        return Err("accounts.signedOutCannotActivate".into());
    }
    if account.paused {
        return Err("accounts.pausedCannotActivate".into());
    }

    db.with(|conn| {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE provider_accounts SET active = 0 WHERE provider = ?1",
            [&account.provider],
        )?;
        tx.execute(
            "UPDATE provider_accounts SET active = 1 WHERE id = ?1",
            [account_id],
        )?;
        tx.commit()?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    get(db, account_id)?.ok_or_else(|| "accounts.notFound".to_string())
}

/// Next signed-in, unpaused and non-exhausted account in rotation order.
pub fn next_available(db: &Database, current: &Account) -> Option<Account> {
    let mut accounts: Vec<_> = list(db)
        .ok()?
        .into_iter()
        .filter(|candidate| {
            candidate.provider == current.provider
                && candidate.id != current.id
                && candidate.signed_in
                && !candidate.paused
                && !super::quota::for_account(db, candidate)
                    .windows
                    .iter()
                    .any(|window| window.exhausted)
        })
        .collect();
    accounts.sort_by_key(|candidate| candidate.position);
    accounts
        .iter()
        .find(|candidate| candidate.position > current.position)
        .cloned()
        .or_else(|| accounts.into_iter().next())
}

/// Apply automatic rotation after quota state changes.
///
/// Returns the newly active account when a switch happened. An Official
/// rejection is the trigger for both enabled policies; only `OnThreshold`
/// may act on the Estimated percentage learned from prior refusals.
pub fn maybe_rotate(db: &Database, current_id: &str) -> Option<Account> {
    let current = get(db, current_id).ok().flatten()?;
    if !current.active {
        return None;
    }
    let policy = policy(db);
    if policy == AutoSwitchPolicy::Off {
        return None;
    }

    let quota = super::quota::for_account(db, &current);
    let should_switch = quota.windows.iter().any(|window| window.exhausted)
        || (policy == AutoSwitchPolicy::OnThreshold
            && quota.windows.iter().any(|window| {
                matches!(
                    window.confidence,
                    crate::session::event::Confidence::Official
                        | crate::session::event::Confidence::Estimated
                ) && window
                    .percent
                    .map(|percent| percent >= super::quota::NEARING_PERCENT)
                    .unwrap_or(false)
            }));
    if !should_switch {
        return None;
    }

    let next = next_available(db, &current)?;
    set_active(db, &next.id).ok()
}

/// Why a driven session should move to the now-active account.
#[derive(Debug, Clone)]
pub struct RelayNeed {
    pub from: Account,
    pub to: Account,
    pub estimated: bool,
}

/// A manual switch never moves a running session. A relay is required only
/// when automatic policy is enabled *and* the account that owns this session
/// reached the policy's quota trigger.
pub fn relay_needed(db: &Database, session_id: &str) -> Option<RelayNeed> {
    let account_id: Option<String> = db
        .with(|conn| {
            conn.query_row(
                "SELECT account_id FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )
        })
        .ok()?;
    let from = get(db, account_id.as_deref()?).ok().flatten()?;
    let to = super::active(db, &from.provider)?;
    if from.id == to.id {
        return None;
    }

    let policy = policy(db);
    if policy == AutoSwitchPolicy::Off {
        return None;
    }
    let quota = super::quota::for_account(db, &from);
    let exhausted = quota.windows.iter().any(|window| window.exhausted);
    let estimated = policy == AutoSwitchPolicy::OnThreshold
        && quota.windows.iter().any(|window| {
            window.confidence == crate::session::event::Confidence::Estimated
                && window
                    .percent
                    .map(|percent| percent >= super::quota::NEARING_PERCENT)
                    .unwrap_or(false)
        });
    let official_threshold = policy == AutoSwitchPolicy::OnThreshold
        && quota.windows.iter().any(|window| {
            window.confidence == crate::session::event::Confidence::Official
                && window
                    .percent
                    .map(|percent| percent >= super::quota::NEARING_PERCENT)
                    .unwrap_or(false)
        });
    (exhausted || estimated || official_threshold).then_some(RelayNeed {
        from,
        to,
        estimated,
    })
}

/// Replace the driver, not the provider process.
///
/// The old CLI remains alive for a person to inspect or take over. The new
/// session receives the destination account's config directory, a fresh Brain
/// brief and `plan::opening_instruction` from the fully persisted mission.
/// No provider transcript is copied and `--resume` is never used.
#[allow(clippy::too_many_arguments)]
pub fn relay_autopilot(
    app: &tauri::AppHandle,
    old_run: &std::sync::Arc<crate::autopilot::driver::Autopilot>,
    project_id: &str,
    turns: u32,
    budget: u32,
    stalled_rounds: u32,
    last_failing: Vec<String>,
) -> Result<Option<std::sync::Arc<crate::autopilot::driver::Autopilot>>> {
    let state = app.state::<crate::AppState>();
    let Some(need) = relay_needed(&state.db, &old_run.session_id) else {
        return Ok(None);
    };

    let root = crate::files::project_root(&state, project_id).map_err(|error| error.to_string())?;
    if need.to.provider == "claude-code"
        && crate::providers::claude::folder_is_trusted_in(
            std::path::Path::new(&need.to.config_dir),
            need.to.adopted,
            &root,
        ) == Some(false)
    {
        return Err("autopilot.folderNotTrusted".into());
    }

    let kind = match need.to.provider.as_str() {
        "claude-code" => crate::session::commands::SessionKind::ClaudeCode,
        "codex" => crate::session::commands::SessionKind::Codex,
        _ => return Err("accounts.unknownProvider".into()),
    };
    let started = crate::session::commands::start_agent_session(
        &state,
        project_id,
        kind,
        Some(old_run.mission_id.clone()),
        true,
    )
    .map_err(|error| error.to_string())?;

    let new_run = crate::autopilot::driver::start_relayed(
        std::sync::Arc::clone(&started.session),
        std::sync::Arc::clone(&state.db),
        state.session_dir(&started.id),
        old_run.mission_id.clone(),
        project_id.to_string(),
        app.clone(),
        turns,
        budget,
        stalled_rounds,
        last_failing,
    );

    // Start the replacement before freeing the old seat. There is never a
    // moment where the mission has no driver if launch succeeded.
    old_run.stop();
    crate::guardrail::sessions::set_driven(&state.session_dir(&old_run.session_id), false);
    // The run moved to a new session; the old one is nobody's autopilot now
    // and the new one is (§49).
    state.attention.set_driven(&old_run.session_id, false);
    state.attention.set_driven(&started.id, true);
    state.autopilots.remove(&old_run.session_id);
    state.autopilots.insert(std::sync::Arc::clone(&new_run));

    let detail = serde_json::json!({
        "fromAccountId": need.from.id,
        "toAccountId": need.to.id,
        "fromSessionId": old_run.session_id,
        "toSessionId": new_run.session_id,
        "estimated": need.estimated,
    })
    .to_string();
    crate::activity::record(
        &state.db,
        "account.relayStarted",
        crate::activity::Severity::Info,
        &need.to.provider,
        Some(detail),
        Some(project_id),
        Some(&new_run.session_id),
        Some(&new_run.mission_id),
    );

    Ok(Some(new_run))
}

/// Stable audit detail for an automatic switch. Prose is localised by the UI.
pub fn record_rotation(db: &Database, from: &Account, to: &Account, estimated: bool) {
    let detail = serde_json::json!({
        "fromAccountId": from.id,
        "toAccountId": to.id,
        "estimated": estimated,
    })
    .to_string();
    crate::activity::record(
        db,
        "account.autoSwitched",
        crate::activity::Severity::Info,
        &to.provider,
        Some(detail),
        None,
        None,
        None,
    );
}
