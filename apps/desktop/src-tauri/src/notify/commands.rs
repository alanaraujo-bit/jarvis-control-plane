//! What the surface can ask about notifications (§49).

use serde::Serialize;
use tauri::State;

use crate::AppState;

use super::store;

pub type Result<T> = std::result::Result<T, String>;

/// The whole notification centre in one answer.
///
/// One command rather than one per field, for the reason `settings` gives: the
/// panel wants all of it at once and a round trip per part would make it paint
/// in pieces.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Centre {
    pub notifications: Vec<store::Notification>,
    pub outstanding: u32,
    /// Whether notifications are on at all (§64), so the panel can say so
    /// rather than looking empty for a reason it cannot explain.
    pub enabled: bool,
}

/// How many rows the centre holds.
///
/// A person scanning "what did I miss" does not scroll past this, and the list
/// is a working surface rather than an archive — `activity` is the archive.
const CENTRE_LIMIT: u32 = 100;

/// How many rows survive a prune at startup.
pub const KEEP_ON_DISK: u32 = 400;

#[tauri::command]
pub fn notifications_centre(state: State<'_, AppState>) -> Result<Centre> {
    Ok(Centre {
        notifications: store::recent(&state.db, CENTRE_LIMIT).map_err(|e| e.to_string())?,
        outstanding: store::outstanding(&state.db).map_err(|e| e.to_string())?,
        enabled: state.attention.is_enabled(),
    })
}

/// Tell the core what the person is looking at.
///
/// Called by the surface on focus changes and whenever the session on screen
/// changes. This is the input to the rule in the module header, and it is the
/// one piece of the decision the core cannot work out for itself.
#[tauri::command]
pub fn notifications_attention(
    state: State<'_, AppState>,
    focused: bool,
    session_id: Option<String>,
) {
    state.attention.set_focused(focused);
    state.attention.set_visible_session(session_id);
}

#[tauri::command]
pub fn notifications_mark_seen(state: State<'_, AppState>, ids: Vec<i64>) -> Result<u32> {
    store::mark_seen(&state.db, &ids).map_err(|e| e.to_string())?;
    store::outstanding(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn notifications_mark_all_seen(state: State<'_, AppState>) -> Result<u32> {
    store::mark_all_seen(&state.db).map_err(|e| e.to_string())?;
    store::outstanding(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn notifications_mark_acted(state: State<'_, AppState>, id: i64) -> Result<u32> {
    store::mark_acted(&state.db, id).map_err(|e| e.to_string())?;
    store::outstanding(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn notifications_clear(state: State<'_, AppState>) -> Result<()> {
    store::clear(&state.db).map_err(|e| e.to_string())
}
