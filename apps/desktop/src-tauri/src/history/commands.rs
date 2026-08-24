//! The IPC surface for Session History (§88).
//!
//! Thin on purpose: every one of these is a call into `history` with the live
//! session ids attached. Liveness is the one fact the database cannot answer —
//! a row outlives its process after a crash — so it comes from the manager
//! here, once, rather than being guessed from the stored state.

use tauri::State;

use super::{Deleted, Entry, Page, Query, Storage};
use crate::AppState;

pub type Result<T> = std::result::Result<T, String>;

/// One page of history, browsing or searching.
#[tauri::command]
pub fn history_page(state: State<'_, AppState>, query: Query) -> Result<Page> {
    super::page(&state.db, &state.sessions.ids(), &query)
}

/// Rename a session. The name a person gives outranks every other source.
#[tauri::command]
pub fn history_rename(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> Result<String> {
    super::rename(&state.db, &session_id, &title)
}

/// Delete a session, its search index entries and its log directory (D39).
#[tauri::command]
pub fn history_delete(state: State<'_, AppState>, session_id: String) -> Result<Deleted> {
    super::delete(&state.db, &state.sessions.ids(), &session_id)
}

/// How much disk the session logs on this machine occupy.
#[tauri::command]
pub fn history_storage(state: State<'_, AppState>) -> Result<Storage> {
    super::storage(&state.db)
}

/// Providers that have actually run here, for the filter row.
#[tauri::command]
pub fn history_providers(state: State<'_, AppState>) -> Result<Vec<String>> {
    super::providers_seen(&state.db)
}

/// One session, by id — for a row the surface needs to redraw on its own after
/// a rename, without re-fetching the page it sits in.
#[tauri::command]
pub fn history_entry(state: State<'_, AppState>, session_id: String) -> Result<Option<Entry>> {
    super::entry(&state.db, &state.sessions.ids(), &session_id)
}
