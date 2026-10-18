//! What the person chose: whether rotation acts on its own, asks first, or
//! stays out of the way, and what makes it act.

use serde::{Deserialize, Serialize};

use crate::db::Database;

pub const POLICY_KEY: &str = "accounts.rotation";
/// The M13 setting (`off | onExhaustion | onThreshold`), read once to migrate.
pub const LEGACY_KEY: &str = "accounts.autoSwitch";

/// Who decides when a trigger fires.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Off,
    /// A small modal asks before moving.
    Ask,
    /// Moves, then says so.
    Auto,
}

/// What makes rotation act.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Trigger {
    /// Any window of the session's account at or above `percent` used; the move
    /// waits for the end of the agent's turn.
    NearLimit { percent: u8 },
    /// The provider refused a turn; the move is immediate.
    Exhausted,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Policy {
    pub mode: Mode,
    pub trigger: Trigger,
}

impl Default for Policy {
    fn default() -> Self {
        Policy { mode: Mode::Off, trigger: Trigger::NearLimit { percent: 90 } }
    }
}

fn clamped(mut value: Policy) -> Policy {
    if let Trigger::NearLimit { percent } = value.trigger {
        value.trigger = Trigger::NearLimit { percent: percent.clamp(50, 98) };
    }
    value
}

pub fn policy(db: &Database) -> Policy {
    if let Some(value) = crate::settings::get::<Policy>(db, POLICY_KEY) {
        return clamped(value);
    }
    // First read on an installation that chose under the old setting: carry the
    // choice over rather than silently switching rotation off.
    match crate::settings::get::<String>(db, LEGACY_KEY).as_deref() {
        Some("onExhaustion") => Policy { mode: Mode::Auto, trigger: Trigger::Exhausted },
        Some("onThreshold") => {
            Policy { mode: Mode::Auto, trigger: Trigger::NearLimit { percent: 85 } }
        }
        _ => Policy::default(),
    }
}

pub fn set_policy(db: &Database, value: Policy) -> Result<(), String> {
    crate::settings::set(db, POLICY_KEY, &clamped(value)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;

    #[test]
    fn default_is_off_near_90() {
        let db = Database::open_in_memory().unwrap();
        assert_eq!(
            policy(&db),
            Policy { mode: Mode::Off, trigger: Trigger::NearLimit { percent: 90 } }
        );
    }

    #[test]
    fn legacy_on_threshold_migrates_to_auto_near_85() {
        let db = Database::open_in_memory().unwrap();
        crate::settings::set(&db, LEGACY_KEY, &"onThreshold").unwrap();
        assert_eq!(
            policy(&db),
            Policy { mode: Mode::Auto, trigger: Trigger::NearLimit { percent: 85 } }
        );
    }

    #[test]
    fn legacy_on_exhaustion_migrates() {
        let db = Database::open_in_memory().unwrap();
        crate::settings::set(&db, LEGACY_KEY, &"onExhaustion").unwrap();
        assert_eq!(policy(&db), Policy { mode: Mode::Auto, trigger: Trigger::Exhausted });
    }

    #[test]
    fn percent_is_clamped() {
        let db = Database::open_in_memory().unwrap();
        set_policy(&db, Policy { mode: Mode::Ask, trigger: Trigger::NearLimit { percent: 30 } })
            .unwrap();
        assert_eq!(policy(&db).trigger, Trigger::NearLimit { percent: 50 });
        set_policy(&db, Policy { mode: Mode::Ask, trigger: Trigger::NearLimit { percent: 99 } })
            .unwrap();
        assert_eq!(policy(&db).trigger, Trigger::NearLimit { percent: 98 });
    }
}
