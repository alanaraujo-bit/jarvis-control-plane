//! Tauri commands for the Notebook (M19).
//!
//! Every mutation returns the **whole** report rather than an acknowledgement.
//! That is the shape `brain` already uses, and the reason is the same: a
//! surface that patches its own copy after each call is a second model of the
//! same data, and it drifts. One round trip, one truth.

use tauri::State;

use super::{NotebookReport, Result};
use crate::AppState;

/// Offer the library to the cloud after a change.
///
/// A no-op when nobody is signed in, and coalesced when they are — see
/// `identity::cloud::push_notebook`. Every mutation calls it and none of them
/// has to ask whether it should, which is the same bargain
/// `prefs::remember` already makes one module over.
fn carry(state: &AppState) {
    crate::identity::cloud::push_notebook(&state.db);
}

#[tauri::command]
pub fn notebook_report(state: State<'_, AppState>) -> Result<NotebookReport> {
    super::report(&state.db)
}

#[tauri::command]
pub fn notebook_create(state: State<'_, AppState>, name: String) -> Result<NotebookReport> {
    super::create_notebook(&state.db, &name)?;
    carry(&state);
    super::report(&state.db)
}

#[tauri::command]
pub fn notebook_rename(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<NotebookReport> {
    super::rename_notebook(&state.db, &id, &name)?;
    carry(&state);
    super::report(&state.db)
}

#[tauri::command]
pub fn notebook_delete(state: State<'_, AppState>, id: String) -> Result<NotebookReport> {
    super::delete_notebook(&state.db, &id)?;
    carry(&state);
    super::report(&state.db)
}

/// Create an empty note and say which one it is, so the surface can put a
/// cursor in it. The id is returned *alongside* the report rather than being
/// dug back out of it — "the newest row" is a guess, and two notes created in
/// the same millisecond would make it the wrong one.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    pub id: String,
    pub report: NotebookReport,
}

#[tauri::command]
pub fn notebook_note_create(
    state: State<'_, AppState>,
    notebook_id: Option<String>,
) -> Result<Created> {
    let id = super::create_note(&state.db, notebook_id.as_deref())?;
    carry(&state);
    Ok(Created {
        id,
        report: super::report(&state.db)?,
    })
}

#[tauri::command]
pub fn notebook_note_update(
    state: State<'_, AppState>,
    id: String,
    title: String,
    body: String,
) -> Result<NotebookReport> {
    super::update_note(&state.db, &id, &title, &body)?;
    carry(&state);
    super::report(&state.db)
}

#[tauri::command]
pub fn notebook_note_pin(
    state: State<'_, AppState>,
    id: String,
    pinned: bool,
) -> Result<NotebookReport> {
    super::set_note_pinned(&state.db, &id, pinned)?;
    carry(&state);
    super::report(&state.db)
}

#[tauri::command]
pub fn notebook_note_move(
    state: State<'_, AppState>,
    id: String,
    notebook_id: Option<String>,
) -> Result<NotebookReport> {
    super::move_note(&state.db, &id, notebook_id.as_deref())?;
    carry(&state);
    super::report(&state.db)
}

#[tauri::command]
pub fn notebook_note_duplicate(state: State<'_, AppState>, id: String) -> Result<Created> {
    let id = super::duplicate_note(&state.db, &id)?;
    carry(&state);
    Ok(Created {
        id,
        report: super::report(&state.db)?,
    })
}

#[tauri::command]
pub fn notebook_note_delete(state: State<'_, AppState>, id: String) -> Result<NotebookReport> {
    super::delete_note(&state.db, &id)?;
    carry(&state);
    super::report(&state.db)
}
