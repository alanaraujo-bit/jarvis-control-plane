//! First run (§13).
//!
//! One fact: has anyone ever gotten past the welcome screen on this machine.
//! `settings` already holds exactly this shape of app-level flag — the same
//! table `mission::store::global_autonomy` reads — so this is the second
//! value stored there rather than a new table for one row.
//!
//! There is deliberately no way to un-see onboarding from the UI. Seeing it
//! twice teaches a user to click past it without reading; the reset path is
//! `DELETE FROM settings WHERE key = 'onboarding.seen'`, for development.

use rusqlite::OptionalExtension;
use tauri::State;

use crate::db::Database;
use crate::AppState;

const SEEN_KEY: &str = "onboarding.seen";

pub fn has_seen(db: &Database) -> bool {
    db.with(|conn| {
        conn.query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [SEEN_KEY],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .ok()
    .flatten()
    .is_some()
}

pub fn mark_seen(db: &Database) -> crate::db::Result<()> {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, '1')
             ON CONFLICT(key) DO UPDATE SET value = '1'",
            [SEEN_KEY],
        )?;
        Ok(())
    })
}

/// Whether the welcome screen has already been shown on this machine.
///
/// Never fails: a database that cannot be read yields the same answer as one
/// with no row at all — "not seen" — rather than blocking the window from
/// ever appearing (see item 31 in HANDOFF for why a command that can fail
/// silently must never sit between launch and the window becoming visible).
#[tauri::command]
pub fn onboarding_status(state: State<'_, AppState>) -> bool {
    has_seen(&state.db)
}

#[tauri::command]
pub fn onboarding_mark_seen(state: State<'_, AppState>) -> Result<(), String> {
    mark_seen(&state.db).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_is_unseen_until_marked() {
        let db = Database::open_in_memory().unwrap();
        assert!(!has_seen(&db));
        mark_seen(&db).unwrap();
        assert!(has_seen(&db));
    }

    #[test]
    fn marking_seen_twice_does_not_error() {
        let db = Database::open_in_memory().unwrap();
        mark_seen(&db).unwrap();
        mark_seen(&db).unwrap();
        assert!(has_seen(&db));
    }
}
