//! Tauri boundary for provider accounts (§66).

use serde::Serialize;
use tauri::State;

use crate::AppState;

use super::quota::AccountQuota;
use super::switch::AutoSwitchPolicy;
use super::{Account, Result};

/// One account card. Trust is scoped to the project currently being viewed;
/// `None` means either not Claude Code, no project was supplied, or the
/// provider's configuration could not be understood.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCard {
    pub account: Account,
    pub quota: AccountQuota,
    pub folder_trusted: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsReport {
    pub accounts: Vec<AccountCard>,
    pub auto_switch: AutoSwitchPolicy,
    pub threshold_percent: f64,
}

fn project_path(state: &AppState, project_id: Option<&str>) -> Option<String> {
    let id = project_id?;
    state
        .db
        .with(|conn| {
            conn.query_row("SELECT path FROM projects WHERE id = ?1", [id], |row| {
                row.get(0)
            })
        })
        .ok()
}

fn report(state: &AppState, project_id: Option<&str>) -> Result<AccountsReport> {
    let project = project_path(state, project_id);
    let accounts = super::list(&state.db)?
        .into_iter()
        .map(|account| {
            let folder_trusted = project.as_deref().and_then(|cwd| {
                (account.provider == "claude-code")
                    .then(|| {
                        crate::providers::claude::folder_is_trusted_in(
                            std::path::Path::new(&account.config_dir),
                            account.adopted,
                            std::path::Path::new(cwd),
                        )
                    })
                    .flatten()
            });
            let quota = super::quota::for_account(&state.db, &account);
            AccountCard {
                account,
                quota,
                folder_trusted,
            }
        })
        .collect();

    Ok(AccountsReport {
        accounts,
        auto_switch: super::switch::policy(&state.db),
        threshold_percent: super::quota::NEARING_PERCENT,
    })
}

#[tauri::command]
pub fn accounts_report(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<AccountsReport> {
    report(&state, project_id.as_deref())
}

/// Re-check identity **and** ask both providers for live quota.
///
/// Deliberately slow and deliberately explicit. A probe is a CLI startup per
/// account — a second or two each, run in parallel — which is fine behind a
/// spinner the person asked for and far too slow to sit in front of a panel
/// opening. `accounts_report` returns the stored reading instantly; this is
/// what replaces it.
///
/// `async` so Tauri runs it off the main thread: the same work on the command
/// thread would freeze the window for as long as the slowest CLI takes to
/// start, which is exactly the kind of thing that only shows up when you run
/// the real app and look at it.
/// Whether a refresh should also re-interrogate who each directory is.
///
/// It doubles the process spawns — `claude auth status` per account on top of
/// the probe — for a fact that changes when somebody signs in or out and at no
/// other time. Worth paying when a person presses "Check now" or a login just
/// finished; pure waste on the five-minute background tick behind the status
/// bar, which is the one that runs all day.
#[tauri::command]
pub async fn accounts_refresh(
    state: State<'_, AppState>,
    project_id: Option<String>,
    identity: Option<bool>,
) -> Result<AccountsReport> {
    let accounts = super::list(&state.db)?;
    if identity.unwrap_or(true) {
        for account in &accounts {
            let _ = super::refresh_identity(&state.db, &account.id);
        }
    }
    // Identity may have changed above — probe against the rows as they are now,
    // so an account that just finished signing in is asked rather than skipped.
    let accounts = super::list(&state.db)?;
    super::live::refresh_all(&state.db, &accounts);

    // A fresh reading is the moment the automatic policy can finally act on
    // something other than a refusal. Before live quota existed the only
    // trigger was a turn being refused mid-session, which meant "switch before
    // it runs out" could not fire until it had already run out.
    //
    // Only accounts that are active right now are considered, and only when
    // the person turned the policy on. A switch changes which account the
    // *next* session starts on and nothing else; it is one click to undo, and
    // it is written to the activity log by `set_active`'s callers.
    if super::switch::policy(&state.db) != super::switch::AutoSwitchPolicy::Off {
        for account in accounts.iter().filter(|a| a.active) {
            let _ = super::switch::maybe_rotate(&state.db, &account.id);
        }
    }

    report(&state, project_id.as_deref())
}

/// Ask one account's provider for its quota, now.
///
/// Exists so a single card can be retried without paying for every other
/// account's CLI startup — the case where one directory is mid-login and the
/// rest are fine.
#[tauri::command]
pub async fn account_refresh_live(
    state: State<'_, AppState>,
    account_id: String,
    project_id: Option<String>,
) -> Result<AccountsReport> {
    let _ = super::refresh_identity(&state.db, &account_id);
    if let Some(account) = super::get(&state.db, &account_id)? {
        super::live::refresh_all(&state.db, std::slice::from_ref(&account));
    }
    report(&state, project_id.as_deref())
}

#[tauri::command]
pub fn account_create(
    state: State<'_, AppState>,
    provider: String,
    label: String,
) -> Result<Account> {
    super::create(&state.db, &provider, &state.data_dir, &label)
}

#[tauri::command]
pub fn account_rename(state: State<'_, AppState>, account_id: String, label: String) -> Result<()> {
    super::rename(&state.db, &account_id, &label)
}

#[tauri::command]
pub fn account_set_paused(
    state: State<'_, AppState>,
    account_id: String,
    paused: bool,
) -> Result<()> {
    super::set_paused(&state.db, &account_id, paused)
}

#[tauri::command]
pub fn account_remove(state: State<'_, AppState>, account_id: String) -> Result<()> {
    super::remove(&state.db, &account_id)
}

#[tauri::command]
pub fn account_set_active(state: State<'_, AppState>, account_id: String) -> Result<Account> {
    super::switch::set_active(&state.db, &account_id)
}

#[tauri::command]
pub fn account_set_auto_switch(state: State<'_, AppState>, policy: AutoSwitchPolicy) -> Result<()> {
    super::switch::set_policy(&state.db, policy)
}

/// Start the provider-owned OAuth flow and return immediately. The spawned CLI
/// owns browser login; J.A.R.V.I.S. never receives a password or token.
#[tauri::command]
pub fn account_begin_sign_in(
    state: State<'_, AppState>,
    account_id: String,
    email: Option<String>,
) -> Result<()> {
    let account =
        super::get(&state.db, &account_id)?.ok_or_else(|| "accounts.notFound".to_string())?;
    let mut command = super::switch::sign_in_command(&account, email.as_deref())?;
    let db = std::sync::Arc::clone(&state.db);
    std::thread::Builder::new()
        .name(format!("account-login-{account_id}"))
        .spawn(move || match command.spawn() {
            Ok(mut child) => {
                let _ = child.wait();
                let _ = super::refresh_identity(&db, &account_id);
            }
            Err(error) => tracing::warn!(%error, %account_id, "could not start provider login"),
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}
