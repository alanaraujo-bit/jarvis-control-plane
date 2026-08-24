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

#[tauri::command]
pub fn accounts_refresh(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<AccountsReport> {
    let ids: Vec<String> = super::list(&state.db)?
        .into_iter()
        .map(|account| account.id)
        .collect();
    for id in ids {
        let _ = super::refresh_identity(&state.db, &id);
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
