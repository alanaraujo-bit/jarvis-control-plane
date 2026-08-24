//! Notifications (§49) — telling a person their agent has stopped.
//!
//! ## The one question this feature answers
//!
//! > Something I started is no longer moving. Do I need to do anything?
//!
//! Everything here follows from that. An agent that stops because it finished,
//! and an agent that stops because it is waiting for permission, are the same
//! event to the person who walked away from the screen — and neither of them
//! reaches that person today.
//!
//! ## The rule that decides whether this is useful or unbearable
//!
//! > **Notify only about what the person is not already looking at.**
//!
//! A finished turn is worth telling somebody about *only* if they are not
//! watching that session. Sitting in front of a terminal, an agent finishes a
//! turn every minute or two; a product that toasts each one is unusable inside
//! ten minutes, and worse, it trains the person to dismiss the toast that
//! actually mattered.
//!
//! So a suppressed notification is **dropped entirely** — not stored and marked
//! read. That makes the notification centre a list of things you *missed*
//! rather than a transcript of things you watched happen, which is what makes
//! it worth opening. What happened is what `activity` is for.
//!
//! The decision lives here, in the core, rather than in the surface that draws
//! the toast. The surface knows what is on screen, so it keeps `Attention`
//! updated; but "is this worth raising" is then answered in one place, at the
//! moment of raising, and can be tested.
//!
//! ## Where the facts come from
//!
//! | Source | Confidence |
//! |---|---|
//! | `ConversationItem::TurnEnded` from the provider's own transcript | Official |
//! | A guardrail decision (§35) | Official |
//! | An autopilot run that stopped for a §34 reason | Official |
//! | A question the provider drew on its own terminal (`detect`) | **Observed** |
//!
//! The last is the one this milestone had to build, and it is the one that
//! covers the ordinary case — see `detect`'s own header for why nothing that
//! already existed could see it. It is labelled Observed for the same reason a
//! usage figure is (§28): it was read off a screen, not stated by a provider,
//! and a surface must be able to tell those apart.

pub mod bus;
#[cfg(test)]
mod capture;
pub mod commands;
pub mod detect;
pub mod render;
pub mod store;
pub mod watch;

#[cfg(test)]
mod tests;

use std::sync::atomic::{AtomicBool, Ordering};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

pub use store::Notification;

/// What a person is looking at right now.
///
/// Kept by the surface, read by the core. Both fields are deliberately about
/// *attention*, not about window state: a focused window showing Mission
/// Control is not looking at an agent, and neither is a focused window showing
/// a different agent's terminal.
#[derive(Debug)]
pub struct Attention {
    /// Whether the application window has keyboard focus.
    focused: AtomicBool,
    /// Every session on screen right now.
    ///
    /// A list, not one id: split panes put up to four terminals side by side
    /// (§20), and treating only the focused one as watched would notify about
    /// an agent the person can see finishing.
    visible_sessions: Mutex<Vec<String>>,
    /// Whether the person wants to be told at all (§64).
    enabled: AtomicBool,
}

impl Default for Attention {
    fn default() -> Self {
        Self {
            // Focused until the surface says otherwise. The alternative —
            // assuming nobody is there until told — would fire a toast for the
            // first turn of the first session after every launch, before the
            // webview has had a chance to report anything.
            focused: AtomicBool::new(true),
            visible_sessions: Mutex::new(Vec::new()),
            enabled: AtomicBool::new(true),
        }
    }
}

impl Attention {
    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::SeqCst);
    }

    pub fn set_visible_sessions(&self, session_ids: Vec<String>) {
        *self.visible_sessions.lock() = session_ids;
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::SeqCst);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Whether the person is already watching this exact session.
    ///
    /// Both halves are required. A visible terminal in an unfocused window is
    /// behind whatever they are actually working in, and a focused window on
    /// another surface is not showing this session at all.
    pub fn is_watching(&self, session_id: Option<&str>) -> bool {
        if !self.focused.load(Ordering::SeqCst) {
            return false;
        }
        let Some(asked) = session_id else {
            // Something with no session behind it — a mission, a held
            // verification — is never "already on screen". Nothing can be
            // watching it, so it is always worth raising.
            return false;
        };
        self.visible_sessions.lock().iter().any(|id| id == asked)
    }
}

/// What kind of stop this is.
///
/// Three, because three is what a person actually distinguishes when they come
/// back to the screen: something wants me, something is done, something broke.
/// The precise cause travels in `reason`, so a new cause never needs a new
/// colour, a new icon, or a new branch in the surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// An agent is waiting for a person to authorise something.
    NeedsApproval,
    /// An agent finished what it was doing.
    Finished,
    /// An agent stopped and cannot continue on its own.
    Stopped,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NeedsApproval => "needsApproval",
            Self::Finished => "finished",
            Self::Stopped => "stopped",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "needsApproval" => Self::NeedsApproval,
            "stopped" => Self::Stopped,
            _ => Self::Finished,
        }
    }
}

/// Why the agent stopped. A stable identifier the surface localises (§65).
///
/// Spelled out as a type rather than passed as a string so that adding a cause
/// forces every place that renders one to be revisited — including the
/// translation catalogues, which are typed against the English one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// The provider drew a question on its own terminal and is waiting.
    ProviderPrompt,
    /// A guardrail held an operation for a decision (§35).
    GuardrailPending,
    /// A guardrail handed the decision to the provider's own prompt (§35).
    GuardrailAsked,
    /// A guardrail refused an operation and the agent could not go on (§35).
    GuardrailBlocked,
    /// The provider reported a finished turn.
    TurnEnded,
    /// A mission reached completed, with its criteria verified (§30).
    MissionCompleted,
    /// An unattended run finished the mission on its own (§32).
    RunCompleted,
    /// The agent's process ended.
    SessionEnded,
    /// The agent's process ended badly.
    SessionFailed,
    /// The mission is blocked and needs a person.
    MissionBlocked,
    /// A run stopped: out of turns, not converging, or needing a human check.
    RunStopped,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderPrompt => "providerPrompt",
            Self::GuardrailPending => "guardrailPending",
            Self::GuardrailAsked => "guardrailAsked",
            Self::GuardrailBlocked => "guardrailBlocked",
            Self::TurnEnded => "turnEnded",
            Self::MissionCompleted => "missionCompleted",
            Self::RunCompleted => "runCompleted",
            Self::SessionEnded => "sessionEnded",
            Self::SessionFailed => "sessionFailed",
            Self::MissionBlocked => "missionBlocked",
            Self::RunStopped => "runStopped",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "providerPrompt" => Self::ProviderPrompt,
            "guardrailPending" => Self::GuardrailPending,
            "guardrailAsked" => Self::GuardrailAsked,
            "guardrailBlocked" => Self::GuardrailBlocked,
            "missionCompleted" => Self::MissionCompleted,
            "runCompleted" => Self::RunCompleted,
            "sessionEnded" => Self::SessionEnded,
            "sessionFailed" => Self::SessionFailed,
            "missionBlocked" => Self::MissionBlocked,
            "runStopped" => Self::RunStopped,
            _ => Self::TurnEnded,
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Self::ProviderPrompt | Self::GuardrailPending | Self::GuardrailAsked => {
                Kind::NeedsApproval
            }
            Self::TurnEnded | Self::MissionCompleted | Self::RunCompleted | Self::SessionEnded => {
                Kind::Finished
            }
            Self::SessionFailed
            | Self::MissionBlocked
            | Self::RunStopped
            | Self::GuardrailBlocked => Kind::Stopped,
        }
    }
}

/// Everything needed to raise one notification.
#[derive(Debug, Clone, Default)]
pub struct Raise {
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub mission_id: Option<String>,
    pub provider: Option<String>,
    /// The agent's own words. Never ours, never translated — see the migration.
    pub preview: Option<String>,
    /// A stable identifier the surface localises, when the reason alone is not
    /// specific enough. Carries e.g. the autopilot's own stop reason.
    pub detail_code: Option<String>,
}

/// Whether the person wants to be told at all (§64).
pub const ENABLED_KEY: &str = "notifications.enabled";

/// Whether a Windows toast should be raised as well as the in-app one (§64).
///
/// Separate from `ENABLED_KEY` because they are different questions. Somebody
/// who wants the badge and the centre but no desktop toast has asked for
/// something coherent, and a single on/off switch cannot express it.
pub const SYSTEM_KEY: &str = "notifications.system";

/// Whether a sound plays with a notification (§64).
pub const SOUND_KEY: &str = "notifications.sound";

pub fn enabled(db: &crate::db::Database) -> bool {
    crate::settings::get_or(db, ENABLED_KEY, true)
}

/// The longest preview stored, in characters.
///
/// A toast shows two lines and a list row shows one. Anything past this is
/// never read and only makes the row harder to scan.
pub const PREVIEW_CHARS: usize = 140;

#[cfg(test)]
mod model_tests {
    use super::*;

    #[test]
    fn every_reason_belongs_to_exactly_one_kind() {
        // Not a tautology: `kind()` is a match, and a new variant added without
        // an arm would not compile — this proves the mapping is also sensible.
        assert_eq!(Reason::ProviderPrompt.kind(), Kind::NeedsApproval);
        assert_eq!(Reason::TurnEnded.kind(), Kind::Finished);
        assert_eq!(Reason::SessionFailed.kind(), Kind::Stopped);
    }

    #[test]
    fn reasons_and_kinds_survive_a_round_trip_through_the_database() {
        for reason in [
            Reason::ProviderPrompt,
            Reason::GuardrailPending,
            Reason::GuardrailAsked,
            Reason::GuardrailBlocked,
            Reason::TurnEnded,
            Reason::MissionCompleted,
            Reason::RunCompleted,
            Reason::SessionEnded,
            Reason::SessionFailed,
            Reason::MissionBlocked,
            Reason::RunStopped,
        ] {
            assert_eq!(Reason::parse(reason.as_str()), reason);
        }
        for kind in [Kind::NeedsApproval, Kind::Finished, Kind::Stopped] {
            assert_eq!(Kind::parse(kind.as_str()), kind);
        }
    }

    #[test]
    fn watching_needs_both_focus_and_the_right_session_on_screen() {
        let attention = Attention::default();
        attention.set_focused(true);
        attention.set_visible_sessions(vec!["a".into()]);

        assert!(attention.is_watching(Some("a")));
        assert!(!attention.is_watching(Some("b")));
        // Nothing on screen belongs to a session with no id.
        assert!(!attention.is_watching(None));

        // A visible terminal in a window behind something else is not being
        // watched, however visible it is within its own application.
        attention.set_focused(false);
        assert!(!attention.is_watching(Some("a")));
    }

    #[test]
    fn nothing_is_being_watched_before_the_surface_has_said_anything() {
        let attention = Attention::default();
        assert!(!attention.is_watching(Some("a")));
        assert!(attention.is_enabled());
    }
}
