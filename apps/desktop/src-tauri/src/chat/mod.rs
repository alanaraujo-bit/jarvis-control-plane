//! Chat sessions (M29): the same agents as the terminal, worked with through
//! each provider's structured protocol instead of a screen.
//!
//! The provider's transcript is still the conversation's record, followed into
//! the session log by `session::transcript` exactly as for a terminal (D3). The
//! protocol's stdout is the live half: streaming text, permission requests and
//! questions, whether a turn is running, and which model and effort are in use.

pub mod claude;
pub mod codex;
pub mod model;
pub mod process;
