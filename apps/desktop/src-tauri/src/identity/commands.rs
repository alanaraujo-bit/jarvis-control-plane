//! Tauri commands for Identity (M20).
//!
//! Every one of them answers with the whole report rather than an
//! acknowledgement, for the reason `notebook::commands` gives: a surface that
//! patches its own copy after each call is a second model of the same data, and
//! it drifts.

use tauri::State;

use super::{prefs, IdentityReport, Result, SignInOutcome, SignUpOutcome};
use crate::AppState;

/// Push the notification switches an account just brought with it into the live
/// `Attention`.
///
/// `prefs::apply_to_machine` writes rows, and rows are read at startup. The
/// raising path runs in background threads that never look at the database
/// (`settings_set_notification` says so where it does the same thing), so
/// without this, signing in with notifications turned off would go on
/// interrupting somebody until the next restart.
fn sync_live(state: &AppState) {
    let enabled = crate::settings::get_or(&state.db, crate::notify::ENABLED_KEY, true);
    state.attention.set_enabled(enabled);
}

#[tauri::command]
pub fn identity_report(state: State<'_, AppState>) -> Result<IdentityReport> {
    super::report(&state.db)
}

#[tauri::command]
pub async fn identity_google_sign_in(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<super::cloud::GoogleSignIn> {
    let outcome = super::cloud::sign_in(app, std::sync::Arc::clone(&state.db)).await?;
    sync_live(&state);
    Ok(outcome)
}

/// Both of these are `async` where they used to be plain functions, and that is
/// load-bearing rather than stylistic: a synchronous Tauri command runs on the
/// main thread, so a network round trip inside one freezes the window for as
/// long as the server takes to answer. The work happens on a blocking thread
/// and the command awaits it — the same shape `identity_google_sign_in` has
/// used since M20.
#[tauri::command]
pub async fn identity_sign_up(
    state: State<'_, AppState>,
    display_name: String,
    email: String,
    password: String,
) -> Result<SignUpOutcome> {
    let outcome =
        super::cloud::sign_up(std::sync::Arc::clone(&state.db), display_name, email, password)
            .await?;
    if matches!(outcome, SignUpOutcome::Ok { .. }) {
        sync_live(&state);
    }
    Ok(outcome)
}

#[tauri::command]
pub async fn identity_sign_in(
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> Result<SignInOutcome> {
    let outcome =
        super::cloud::sign_in_password(std::sync::Arc::clone(&state.db), email, password).await?;
    if matches!(outcome, SignInOutcome::Ok { .. }) {
        sync_live(&state);
    }
    Ok(outcome)
}

#[tauri::command]
pub fn identity_sign_out(state: State<'_, AppState>) -> Result<IdentityReport> {
    super::cloud::sign_out(&state.db);
    super::sign_out(&state.db)
}

/// "Continue without an account."
///
/// It marks the welcome screen as asked and does nothing else. Nothing in the
/// product behaves differently for somebody who chose this — that is the point
/// of it being a real choice rather than a delay before the same wall.
#[tauri::command]
pub fn identity_skip(state: State<'_, AppState>) -> Result<IdentityReport> {
    super::mark_prompted(&state.db)?;
    super::report(&state.db)
}

/// Store a preference against whoever is signed in, if anybody.
///
/// The value crosses as JSON rather than as text, so what lands in the row is
/// encoded exactly the way `settings::set` would have encoded it — a mirror
/// that re-encodes is a mirror that eventually disagrees.
#[tauri::command]
pub fn identity_remember(
    state: State<'_, AppState>,
    key: String,
    value: serde_json::Value,
) -> Result<()> {
    let text = serde_json::to_string(&value).map_err(|e| e.to_string())?;
    prefs::remember(&state.db, &key, &text)?;
    super::cloud::push_preference(&state.db, &key, &value);
    Ok(())
}

#[tauri::command]
pub fn identity_update_profile(
    state: State<'_, AppState>,
    display_name: String,
    email: String,
) -> Result<IdentityReport> {
    let account = super::current(&state.db).ok_or_else(|| "identity.notSignedIn".to_string())?;
    super::update_profile(&state.db, &account.id, &display_name, &email)?;
    super::report(&state.db)
}

#[tauri::command]
pub fn identity_change_password(
    state: State<'_, AppState>,
    current_password: String,
    next_password: String,
) -> Result<()> {
    let account = super::current(&state.db).ok_or_else(|| "identity.notSignedIn".to_string())?;
    super::change_password(&state.db, &account.id, &current_password, &next_password)
}

/// Delete the signed-in account, proving the password first.
///
/// The password is asked for because this is irreversible and because a
/// destructive control that only needs one click is a control somebody
/// eventually hits by accident. An account with no local password (B7) has
/// nothing to prove, so the surface asks for the confirmation it *can* get
/// instead.
#[tauri::command]
pub fn identity_delete(state: State<'_, AppState>, password: String) -> Result<IdentityReport> {
    let account = super::current(&state.db).ok_or_else(|| "identity.notSignedIn".to_string())?;
    if account.has_password {
        // Reusing `change_password`'s own proof would mean writing a new hash
        // on the way to deleting the row, so this asks the question directly.
        match super::sign_in(&state.db, &account.email, &password)? {
            SignInOutcome::Ok { .. } => {}
            _ => return Err("identity.wrongPassword".into()),
        }
    }
    super::delete_account(&state.db, &account.id)
}
