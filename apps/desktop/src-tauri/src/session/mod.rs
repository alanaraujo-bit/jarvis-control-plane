//! Sessions — the core abstraction (§23).
//!
//! Terminal and Conversation are two projections of one append-only log, not
//! two separate sessions. `event` defines the frames, `log` stores them.

pub mod event;
pub mod log;

pub use event::{Confidence, EventKind, Lifecycle, SessionEvent, SessionState};
pub use log::{SessionLog, SessionLogReader};
