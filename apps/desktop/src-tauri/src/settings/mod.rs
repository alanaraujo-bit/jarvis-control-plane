//! Application preferences (§64).
//!
//! One typed way in and out of the `settings` table.
//!
//! ## Why this module exists at all
//!
//! Before it, every area that needed a preference wrote its own SQL against
//! `settings`: `mission::store` for global autonomy, `onboarding` for whether
//! the welcome screen has been seen. Two call sites is not a problem; the
//! problem is the shape, because §64 adds more. Each one re-decides how
//! "unset" is spelled, whether clearing means `DELETE` or an empty string, and
//! what happens when the stored text is not what the reader expects — and they
//! drift, quietly, because nothing forces them to agree.
//!
//! `get`/`set`/`clear` here are the whole interface. Existing call sites are
//! deliberately left alone: they work, they are tested, and rewriting them to
//! prove a point would be churn (§9 says audit for inconsistency, not that
//! every duplicate must be hunted down today).
//!
//! ## Two rules that keep this honest
//!
//! **"Unset" has one spelling: no row.** Clearing deletes rather than storing
//! an empty string or a sentinel, so `Option<T>` from a read means exactly
//! what it says and nothing has to know which flavour of empty it is looking
//! at. This is the same choice `set_global_autonomy` already made.
//!
//! **A malformed value reads as unset, never as an error.** A preference that
//! cannot be parsed is a preference nobody chose — the right response is the
//! default, not a failed surface. A settings screen that refuses to render
//! because one row is corrupt is worse than one that quietly falls back, and
//! the alternative would let a bad write anywhere brick the whole screen.

use rusqlite::OptionalExtension;
use serde::{de::DeserializeOwned, Serialize};

use crate::db::Database;

pub type Result<T> = std::result::Result<T, String>;

/// Read a preference, or `None` when nothing has been chosen.
///
/// Deserialisation failure is `None` too — see the note above.
pub fn get<T: DeserializeOwned>(db: &Database, key: &str) -> Option<T> {
    let raw: Option<String> = db
        .with(|conn| {
            conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0))
                .optional()
        })
        .ok()
        .flatten();
    raw.and_then(|text| serde_json::from_str(&text).ok())
}

/// Read a preference, falling back to a default.
pub fn get_or<T: DeserializeOwned>(db: &Database, key: &str, fallback: T) -> T {
    get(db, key).unwrap_or(fallback)
}

/// Store a preference.
pub fn set<T: Serialize>(db: &Database, key: &str, value: &T) -> Result<()> {
    let text = serde_json::to_string(value).map_err(|e| e.to_string())?;
    db.with(|conn| {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            rusqlite::params![key, text],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Forget a preference, so the default applies again.
pub fn clear(db: &Database, key: &str) -> Result<()> {
    db.with(|conn| {
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Every preference §64 exposes, in one answer.
///
/// One command rather than one per preference: a settings screen wants all of
/// it at once, and a round trip per control would make the screen paint in
/// pieces. Each field is what the product will actually use — the stored value
/// where there is one, the default where there is not — so the surface never
/// has to know which is which.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub terminal_font_size: u32,
    pub terminal_scrollback: u32,
    pub autopilot_turn_budget: u32,
    /// Whether an agent stopping is worth interrupting the person for (§49).
    pub notifications_enabled: bool,
    /// Whether that interruption also reaches the desktop, so it can be seen
    /// while the application is behind something else.
    pub notifications_system: bool,
    pub notifications_sound: bool,
    /// Compact live statistics over the focused session.
    pub performance_hud_enabled: bool,
}

/// Whether J.A.R.V.I.S. starts with the machine, and how (§93).
///
/// Deliberately **not** part of `Preferences`, and the reason is where the
/// answer lives. Every other preference is a row in this database, so the
/// database is the truth. This one is a registry entry Windows owns: the person
/// can turn it off in Task Manager's Startup tab, and any tool that manages
/// startup items can remove it. A copy of the answer stored here would go stale
/// the moment they did, and a switch showing "on" for something Windows has
/// already disabled is worse than no switch.
///
/// So `starts_with_system` is read from the operating system on every call, and
/// only `start_minimized` — which is ours, and which Windows knows nothing
/// about — is stored.
#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchPreferences {
    /// Read from the OS, never from this database.
    pub starts_with_system: bool,
    /// Whether that automatic start opens the window or leaves it minimised.
    pub start_minimized: bool,
    /// False where this build cannot register a startup item at all, so the
    /// surface can be absent rather than offer a switch that does nothing.
    pub supported: bool,
}

/// Open minimised when started automatically.
pub const START_MINIMIZED_KEY: &str = "startup.minimized";

/// The argument an automatic start is registered with (§93).
///
/// A launched-by-Windows start and a start by hand have to be told apart: the
/// person double-clicking the icon wants the window, and the same binary
/// waking up at login usually does not want to be thrown in front of whatever
/// they were doing. Windows gives no flag for "you started me", so the startup
/// entry carries one of ours.
pub const AUTOSTART_ARG: &str = "--autostart";

/// Whether this process was started by the operating system at login.
pub fn started_by_system() -> bool {
    std::env::args().any(|arg| arg == AUTOSTART_ARG)
}

/// Terminal type size, in CSS pixels.
pub const TERMINAL_FONT_SIZE_KEY: &str = "terminal.fontSize";
pub const DEFAULT_TERMINAL_FONT_SIZE: u32 = 13;
pub const MIN_TERMINAL_FONT_SIZE: u32 = 10;
pub const MAX_TERMINAL_FONT_SIZE: u32 = 20;

/// How many lines of terminal history are kept per session.
///
/// Bounded because this is memory, per terminal, and a split can hold four:
/// the ceiling is the point at which four panes of history stop being free.
pub const TERMINAL_SCROLLBACK_KEY: &str = "terminal.scrollback";
pub const DEFAULT_TERMINAL_SCROLLBACK: u32 = 20_000;
pub const MIN_TERMINAL_SCROLLBACK: u32 = 1_000;
pub const MAX_TERMINAL_SCROLLBACK: u32 = 100_000;

pub const PERFORMANCE_HUD_ENABLED_KEY: &str = "performance.hudEnabled";

#[tauri::command]
pub fn settings_preferences(state: tauri::State<'_, crate::AppState>) -> Preferences {
    let db = &state.db;
    Preferences {
        terminal_font_size: get_or(db, TERMINAL_FONT_SIZE_KEY, DEFAULT_TERMINAL_FONT_SIZE)
            .clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE),
        terminal_scrollback: get_or(db, TERMINAL_SCROLLBACK_KEY, DEFAULT_TERMINAL_SCROLLBACK)
            .clamp(MIN_TERMINAL_SCROLLBACK, MAX_TERMINAL_SCROLLBACK),
        autopilot_turn_budget: crate::autopilot::plan::turn_budget(db),
        notifications_enabled: get_or(db, crate::notify::ENABLED_KEY, true),
        notifications_system: get_or(db, crate::notify::SYSTEM_KEY, true),
        notifications_sound: get_or(db, crate::notify::SOUND_KEY, true),
        // Opt-in: this panel deliberately sits over working content.
        performance_hud_enabled: get_or(db, PERFORMANCE_HUD_ENABLED_KEY, false),
    }
}

/// Read whether J.A.R.V.I.S. starts with the machine (§93).
#[tauri::command]
pub fn settings_launch(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
) -> LaunchPreferences {
    use tauri_plugin_autostart::ManagerExt;
    // `is_enabled` failing is not the same as "off": the registry could not be
    // read. Reported as off, because that is what the machine will do — but it
    // is a read of the OS either way, never of a value we cached.
    let starts_with_system = app.autolaunch().is_enabled().unwrap_or(false);
    LaunchPreferences {
        starts_with_system,
        start_minimized: get_or(&state.db, START_MINIMIZED_KEY, true),
        supported: cfg!(any(windows, target_os = "macos", target_os = "linux")),
    }
}

/// Turn the startup entry on or off, or change how it opens.
///
/// Both arguments are optional so one switch can move without restating the
/// other, and the whole set comes back so the surface renders what is now true
/// rather than what it asked for.
#[tauri::command]
pub fn settings_set_launch(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::AppState>,
    starts_with_system: Option<bool>,
    start_minimized: Option<bool>,
) -> Result<LaunchPreferences> {
    use tauri_plugin_autostart::ManagerExt;

    if let Some(enabled) = starts_with_system {
        let manager = app.autolaunch();
        let outcome = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        // Surfaced rather than swallowed: this writes outside the app's own
        // data, and a locked-down machine can refuse. A switch that silently
        // slid back would leave the person retrying something that cannot work.
        outcome.map_err(|error| format!("startup.registerFailed: {error}"))?;
    }
    if let Some(minimized) = start_minimized {
        set(&state.db, START_MINIMIZED_KEY, &minimized)?;
    }
    Ok(settings_launch(app, state))
}

/// Show or hide the live performance HUD.
#[tauri::command]
pub fn settings_set_performance_hud(
    state: tauri::State<'_, crate::AppState>,
    value: bool,
) -> Result<Preferences> {
    set(&state.db, PERFORMANCE_HUD_ENABLED_KEY, &value)?;
    Ok(settings_preferences(state))
}

/// Turn one of the notification switches on or off (§49, §64).
///
/// Separate from `settings_set_preference` because these are booleans and that
/// one validates against numeric bounds. Folding a `bool` into a function whose
/// whole contract is "reject anything outside a range" would mean inventing a
/// range for a value that has none.
///
/// `enabled` is applied to the live `Attention` as well as stored, because it
/// is read on the raising path in background threads that never look at the
/// database — turning notifications off has to stop the next one, not the one
/// after a restart.
#[tauri::command]
pub fn settings_set_notification(
    state: tauri::State<'_, crate::AppState>,
    key: String,
    value: bool,
) -> Result<Preferences> {
    match key.as_str() {
        crate::notify::ENABLED_KEY => {
            set(&state.db, &key, &value)?;
            state.attention.set_enabled(value);
        }
        crate::notify::SYSTEM_KEY | crate::notify::SOUND_KEY => set(&state.db, &key, &value)?,
        // A closed list, for the reason `settings_set_preference` gives.
        _ => return Err("settings.unknownKey".into()),
    }
    Ok(settings_preferences(state))
}

/// Change one preference, and answer with the whole set.
///
/// **Validated here, in the core.** The surface renders a bounded control, so
/// an out-of-range value should be impossible — which is exactly why it must
/// not be trusted: a command reachable from the webview is reachable from any
/// bug in it. Refusing rather than clamping on the way *in* means a stored
/// value is always one somebody could have chosen.
///
/// Returning the full set means the surface never has to guess what the core
/// did with what it sent.
#[tauri::command]
pub fn settings_set_preference(
    state: tauri::State<'_, crate::AppState>,
    key: String,
    value: Option<u32>,
) -> Result<Preferences> {
    use crate::autopilot::plan::{MAX_TURN_BUDGET, MIN_TURN_BUDGET, TURN_BUDGET_KEY};

    let bounds = match key.as_str() {
        TERMINAL_FONT_SIZE_KEY => (MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE),
        TERMINAL_SCROLLBACK_KEY => (MIN_TERMINAL_SCROLLBACK, MAX_TERMINAL_SCROLLBACK),
        TURN_BUDGET_KEY => (MIN_TURN_BUDGET, MAX_TURN_BUDGET),
        // A closed list, so a typo in the webview cannot write an arbitrary
        // row into the settings table.
        _ => return Err("settings.unknownKey".into()),
    };

    match value {
        Some(v) if v < bounds.0 || v > bounds.1 => return Err("settings.outOfRange".into()),
        // `None` restores the default, which is a real choice and not the same
        // as picking whatever the default happens to be today.
        Some(v) => set(&state.db, &key, &v)?,
        None => clear(&state.db, &key)?,
    }

    Ok(settings_preferences(state))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preference_survives_a_round_trip() {
        let db = Database::open_in_memory().unwrap();
        set(&db, "terminal.fontSize", &15u32).unwrap();
        assert_eq!(get::<u32>(&db, "terminal.fontSize"), Some(15));
    }

    #[test]
    fn nothing_chosen_reads_as_none_and_takes_the_default() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(get::<u32>(&db, "never.set"), None);
        assert_eq!(get_or(&db, "never.set", 13u32), 13);
    }

    /// Clearing removes the row rather than storing a sentinel, so "unset" has
    /// exactly one representation in the database.
    #[test]
    fn clearing_leaves_no_row_behind() {
        let db = Database::open_in_memory().unwrap();
        set(&db, "terminal.fontSize", &15u32).unwrap();
        clear(&db, "terminal.fontSize").unwrap();

        assert_eq!(get::<u32>(&db, "terminal.fontSize"), None);
        let rows: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM settings WHERE key = 'terminal.fontSize'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn setting_the_same_key_twice_replaces_it() {
        let db = Database::open_in_memory().unwrap();
        set(&db, "k", &1u32).unwrap();
        set(&db, "k", &2u32).unwrap();
        assert_eq!(get::<u32>(&db, "k"), Some(2));
    }

    /// The rule that keeps a bad row from taking down a whole screen: a value
    /// that will not parse is a preference nobody chose, so the default
    /// applies. Reachable from an older build, a hand-edited database, or a
    /// type that changed shape between versions.
    #[test]
    fn a_value_of_the_wrong_shape_falls_back_instead_of_failing() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES ('terminal.fontSize', 'not a number')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        assert_eq!(get::<u32>(&db, "terminal.fontSize"), None);
        assert_eq!(get_or(&db, "terminal.fontSize", 13u32), 13);
    }

    /// A stored value outside the bounds is clamped on the way *out*, so a
    /// tightened bound or an older build's row cannot produce an unusable
    /// terminal — or, for the budget, a run that can never finish.
    #[test]
    fn a_stored_value_outside_the_bounds_is_clamped_when_read() {
        let db = Database::open_in_memory().unwrap();
        set(&db, TERMINAL_FONT_SIZE_KEY, &400u32).unwrap();
        set(&db, crate::autopilot::plan::TURN_BUDGET_KEY, &0u32).unwrap();

        assert_eq!(
            get_or(&db, TERMINAL_FONT_SIZE_KEY, DEFAULT_TERMINAL_FONT_SIZE)
                .clamp(MIN_TERMINAL_FONT_SIZE, MAX_TERMINAL_FONT_SIZE),
            MAX_TERMINAL_FONT_SIZE
        );
        assert_eq!(
            crate::autopilot::plan::turn_budget(&db),
            crate::autopilot::plan::MIN_TURN_BUDGET,
            "a zero budget would fail every run before it started"
        );
    }
}
