//! Sessions — the core abstraction (§23).
//!
//! Terminal and Conversation are two projections of one append-only log, not
//! two separate sessions. `event` defines the frames, `log` stores them and
//! `manager` runs the live ones.

pub mod commands;
pub mod event;
pub mod log;
pub mod manager;

pub use event::{EventKind, Lifecycle, SessionState};
pub use log::{SessionLog, SessionLogReader};
pub use manager::{SessionInfo, SessionManager};
