//! What an account carries from one machine to the next (M20 §5).
//!
//! ## Mirror, not scope
//!
//! The obvious design is to scope `settings` by account — add a column, and let
//! every reader see only the signed-in person's rows. It was rejected, and the
//! reason is worth keeping written down: `settings`'s contract is "unset has
//! one spelling: no row", and `mission::store`, `onboarding` and
//! `settings::get`/`set` all read it **unscoped**. A scope column changes what
//! every one of those sees, silently, including on a machine where nobody is
//! signed in.
//!
//! So `settings` stays machine-scoped and keeps working exactly as it does
//! today, `identity_settings` holds the person's copy, and this module mirrors
//! between them at the two moments that matter: signing in applies the
//! account's values, and changing one while signed in writes to both.
//!
//! ## Which preference is whose
//!
//! Decided per key rather than by a rule, because there is no rule. Type size,
//! language, theme, how many turns an unattended run may take, whether an agent
//! finishing is worth a notification — those are about a *person*, and they
//! should follow one to a second machine.
//!
//! What is deliberately **not** here: `onboarding.seen`, the whisper model on
//! disk, the environment scan, guardrail policy per project, the relay pairing.
//! Every one of those is a fact about this machine or this folder, and carrying
//! it to another one would be wrong rather than merely unhelpful — an account
//! that "remembered" onboarding was done would hide the welcome screen on a
//! machine that has never run the scan.
//!
//! ## Signing out does not put anything back
//!
//! Deliberate, and stated in M20 §5. Signing out is not a reason for the
//! interface to change appearance while somebody is looking at it. The
//! account's values stay in `identity_settings` for the next sign-in.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::Result;
use crate::db::Database;

/// Theme preference. Lives only here — the webview owns the live value in
/// `localStorage`, because the theme has to be applied before the first paint
/// and a database round trip would show the wrong one first.
pub const THEME_KEY: &str = "appearance.theme";

/// Interface language, for the same reason.
pub const LOCALE_KEY: &str = "appearance.locale";

/// The preferences the **core** owns, which therefore have a machine row to
/// mirror to and from.
///
/// A closed list, deliberately. `identity_remember` is reachable from the
/// webview, and an open one would let any bug in it write arbitrary rows into
/// the settings table — the same reasoning `settings_set_preference` gives for
/// its own closed match.
pub const MACHINE_CARRIED: &[&str] = &[
    crate::settings::TERMINAL_FONT_SIZE_KEY,
    crate::settings::TERMINAL_SCROLLBACK_KEY,
    crate::autopilot::plan::TURN_BUDGET_KEY,
    crate::notify::ENABLED_KEY,
    crate::notify::SYSTEM_KEY,
    crate::notify::SOUND_KEY,
    // The performance HUD. `usePreferences` already called `remember` for it;
    // the call returned `identity.notCarried` into a silent `.catch`, so the
    // switch looked like it followed the person and never did. An allowlist is
    // only closed when both halves are closed at the same time.
    crate::settings::PERFORMANCE_HUD_ENABLED_KEY,
];

/// Every key an account may hold, machine-mirrored or not.
pub fn is_carried(key: &str) -> bool {
    key == THEME_KEY || key == LOCALE_KEY || MACHINE_CARRIED.contains(&key)
}

/// The part of an account's preferences only the webview can apply.
///
/// `None` means this account has never expressed a preference, which is not the
/// same as preferring the default — the surface leaves what is on screen alone
/// rather than resetting it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Carried {
    pub theme: Option<String>,
    pub locale: Option<String>,
}

/// Read one stored value, as the raw JSON text it was written as.
///
/// Raw on purpose: the machine table stores JSON too, so a mirror in either
/// direction is a string copy and cannot change a value by re-encoding it.
pub fn raw(db: &Database, account_id: &str, key: &str) -> Option<String> {
    db.with(|conn| {
        conn.query_row(
            "SELECT value FROM identity_settings WHERE account_id = ?1 AND key = ?2",
            params![account_id, key],
            |row| row.get(0),
        )
        .optional()
    })
    .ok()
    .flatten()
}

/// Store one value against an account.
pub fn put_raw(db: &Database, account_id: &str, key: &str, value: &str) -> Result<()> {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO identity_settings (account_id, key, value) VALUES (?1, ?2, ?3)
             ON CONFLICT (account_id, key) DO UPDATE SET value = excluded.value",
            params![account_id, key, value],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// All carried values, decoded back to JSON for the sync API.
pub fn all(db: &Database, account_id: &str) -> HashMap<String, serde_json::Value> {
    let mut values = HashMap::new();
    for key in std::iter::once(THEME_KEY)
        .chain(std::iter::once(LOCALE_KEY))
        .chain(MACHINE_CARRIED.iter().copied())
    {
        if let Some(raw) = raw(db, account_id, key) {
            if let Ok(value) = serde_json::from_str(&raw) {
                values.insert(key.to_string(), value);
            }
        }
    }
    values
}

fn machine_raw(db: &Database, key: &str) -> Option<String> {
    db.with(|conn| {
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
    })
    .ok()
    .flatten()
}

fn put_machine_raw(db: &Database, key: &str, value: &str) -> Result<()> {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Give a brand-new account whatever this machine is already set to.
///
/// Only keys that actually have a row: an account that inherits "nothing
/// chosen" has chosen nothing, which is the honest reading and the one that
/// keeps `settings`'s own "unset is no row" rule true one table over.
pub fn adopt_machine_settings(db: &Database, account_id: &str) -> Result<()> {
    for key in MACHINE_CARRIED {
        if let Some(value) = machine_raw(db, key) {
            put_raw(db, account_id, key, &value)?;
        }
    }
    Ok(())
}

/// Apply an account's stored values to this machine, on sign-in.
///
/// A key the account has never set is left alone rather than reset to the
/// default. "I have no preference" is not "I prefer the default", and wiping a
/// machine's setting because somebody signed in would be a surprise in the one
/// direction nobody can undo by signing out.
pub fn apply_to_machine(db: &Database, account_id: &str) -> Result<()> {
    for key in MACHINE_CARRIED {
        if let Some(value) = raw(db, account_id, key) {
            put_machine_raw(db, key, &value)?;
        }
    }
    Ok(())
}

/// The webview's half of the same answer.
pub fn carried(db: &Database, account_id: &str) -> Result<Carried> {
    // Stored as JSON, like everything else in both tables, so a plain string
    // arrives quoted and has to be decoded rather than handed over as `"dark"`
    // with the quotes still on it.
    let decode = |key: &str| -> Option<String> {
        raw(db, account_id, key).and_then(|text| serde_json::from_str::<String>(&text).ok())
    };
    Ok(Carried {
        theme: decode(THEME_KEY),
        locale: decode(LOCALE_KEY),
    })
}

/// Remember one preference against whoever is signed in.
///
/// A no-op when nobody is — which is the whole reason this is one function
/// rather than a check at every call site. The surface calls it whenever a
/// carried preference changes and never has to ask first.
pub fn remember(db: &Database, key: &str, value_json: &str) -> Result<()> {
    if !is_carried(key) {
        return Err("identity.notCarried".into());
    }
    let Some(account) = super::current(db) else {
        return Ok(());
    };
    put_raw(db, &account.id, key, value_json)
}
