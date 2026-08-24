//! The one place the rest of the application says "an agent stopped" (§49).
//!
//! ## Why this is a global, when almost nothing else here is
//!
//! A notification is raised from places that have nothing else in common: the
//! transcript tailer, the terminal watcher, the guardrail's decision log, a
//! mission changing status, an unattended run giving up. Half of them are
//! background threads several calls below any command handler, and none of
//! them is *about* notifications.
//!
//! Threading a database handle, an `Attention` and a channel to the webview
//! through all of that would put this feature's plumbing into the signature of
//! every function it passes through — including `mission::store`, which would
//! then need a webview handle to record that a mission completed. That is the
//! shape a cross-cutting concern has when it is threaded rather than installed,
//! and it is why `tracing` is a global too.
//!
//! ## What keeps it honest
//!
//! * The **decision** is not here. `store::raise` takes its inputs explicitly
//!   and is tested directly; this only supplies them.
//! * Not installed is a **no-op**, not a panic. Tests, and the guardrail hook
//!   running as its own process, never install one and must not have to care.
//! * It is installed **once**, in `setup`, and never replaced. There is no
//!   API here for swapping it at runtime, so there is no window in which two
//!   halves of the application disagree about where notifications go.

use std::sync::{Arc, OnceLock};

use crate::db::Database;
use crate::session::event::Confidence;

use super::{store, watch::Announce, Attention, Raise, Reason};

struct Bus {
    db: Arc<Database>,
    attention: Arc<Attention>,
    announce: Announce,
}

static BUS: OnceLock<Bus> = OnceLock::new();

/// Install the sink. Called once, from `setup`.
pub fn install(db: Arc<Database>, attention: Arc<Attention>, announce: Announce) {
    let already = BUS.set(Bus { db, attention, announce }).is_err();
    if already {
        tracing::warn!("the notification bus was already installed; ignoring");
    }
}

/// Raise a notification, if there is anywhere to raise it to.
///
/// Returns whether one was actually raised, which the caller is free to
/// ignore — and every caller does. It is there for tests.
pub fn raise(reason: Reason, confidence: Confidence, raised: Raise) -> bool {
    let Some(bus) = BUS.get() else {
        return false;
    };
    match store::raise(&bus.db, &bus.attention, reason, confidence, raised) {
        Some(notification) => {
            (bus.announce)(notification);
            true
        }
        None => false,
    }
}

/// Record that an autopilot has taken, or given up, the seat (§32, §49).
///
/// Here rather than only in the command that starts a run, because a run also
/// ends **on its own** — completed, out of turns, not converging — on a thread
/// that has no `AppState` to reach for. Without this the flag would survive the
/// run that set it, and the person who then takes the session over by hand
/// would get no notification when their own turns finished.
pub fn set_driven(session_id: &str, driven: bool) {
    if let Some(bus) = BUS.get() {
        bus.attention.set_driven(session_id, driven);
    }
}

/// Whether a sink has been installed.
pub fn is_installed() -> bool {
    BUS.get().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The property everything else depends on: a module that raises a
    /// notification must work in a test, where nothing is listening.
    #[test]
    fn raising_with_no_sink_installed_is_a_no_op() {
        // Deliberately not installing one. `install` is a `OnceLock`, so a test
        // that did would leak into every other test in this binary.
        if is_installed() {
            return;
        }
        assert!(!raise(Reason::TurnEnded, Confidence::Official, Raise::default()));
    }
}
