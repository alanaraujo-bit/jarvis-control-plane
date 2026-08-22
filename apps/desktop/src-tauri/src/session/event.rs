//! Session event types.
//!
//! A session is one ordered, append-only log (§23). The terminal and the
//! conversation are two projections of the same log — not two sessions, and not
//! two independent stores that have to be kept in sync.
//!
//! Raw PTY bytes and structured provider events live in the *same* stream so
//! their relative order is preserved: "the agent said X, then this appeared on
//! the terminal, then the tool returned Y" is a fact about ordering, and it
//! would be lost if the two were stored separately.

use serde::{Deserialize, Serialize};

/// Discriminator for a log frame.
///
/// Encoded as a `u8` on disk. Values are explicit and **must never be reused
/// or renumbered**: existing logs on a user's machine are decoded with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EventKind {
    /// Bytes written by the process to the terminal. The terminal's truth.
    PtyOutput = 1,
    /// Bytes sent to the process. Kept so a replay shows what was typed.
    PtyInput = 2,

    /// A structured message (user / assistant / system), JSON payload.
    Message = 10,
    ToolCall = 11,
    ToolResult = 12,

    /// A token-usage or rate-limit sample, always carrying a confidence (§28).
    Usage = 20,
    /// A file the session created, modified or deleted.
    FileChange = 21,

    /// Session lifecycle: started, exited, state changed.
    Lifecycle = 30,
    /// An image or file attached to the session (§22).
    Attachment = 40,
    /// An approval request or its resolution (§35).
    Approval = 50,
}

impl EventKind {
    pub fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::PtyOutput,
            2 => Self::PtyInput,
            10 => Self::Message,
            11 => Self::ToolCall,
            12 => Self::ToolResult,
            20 => Self::Usage,
            21 => Self::FileChange,
            30 => Self::Lifecycle,
            40 => Self::Attachment,
            50 => Self::Approval,
            _ => return None,
        })
    }

    /// Whether this frame carries opaque bytes rather than UTF-8 JSON.
    ///
    /// PTY output is arbitrary binary (escape sequences, partial UTF-8
    /// sequences split across reads) and must never be lossily decoded.
    pub fn is_binary(self) -> bool {
        matches!(self, Self::PtyOutput | Self::PtyInput)
    }

    /// Whether Conversation View projects this frame (§24).
    pub fn is_structured(self) -> bool {
        !self.is_binary()
    }
}

/// One frame in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEvent {
    /// Monotonic position within the session, starting at 0.
    pub seq: u64,
    /// Milliseconds since the Unix epoch.
    pub ts_ms: i64,
    pub kind: EventKind,
    pub payload: Vec<u8>,
}

impl SessionEvent {
    pub fn new(seq: u64, ts_ms: i64, kind: EventKind, payload: Vec<u8>) -> Self {
        Self { seq, ts_ms, kind, payload }
    }

    /// Interpret the payload as UTF-8, for structured frames.
    pub fn payload_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.payload).ok()
    }
}

/// Confidence attached to every reported usage figure (§28).
///
/// An estimate must never be rendered as if the provider reported it, so the
/// level travels with the number rather than being inferred at the UI layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Confidence {
    /// Reported by the provider itself.
    Official,
    /// Measured by J.A.R.V.I.S. from something the provider emitted.
    Observed,
    /// Derived or extrapolated.
    Estimated,
    Unknown,
}

/// Lifecycle transitions, recorded as frames so session state is reconstructible
/// from the log alone rather than from a mutable row that loses its history.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Lifecycle {
    #[serde(rename_all = "camelCase")]
    Started { command: String, cwd: String },
    #[serde(rename_all = "camelCase")]
    StateChanged { state: SessionState },
    /// Terminal geometry at start and on every change.
    ///
    /// Replay needs this: output that used absolute cursor positioning renders
    /// as garbage if it is replayed into a terminal of a different size. Logged
    /// as a frame so the size is known at every point in the stream.
    #[serde(rename_all = "camelCase")]
    Resized { cols: u16, rows: u16 },
    #[serde(rename_all = "camelCase")]
    Exited { code: Option<i32> },
}

/// What an agent session is doing right now (§21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionState {
    Starting,
    /// The agent is actively producing output.
    Working,
    /// The agent is blocked on the human — a prompt or an approval.
    Waiting,
    /// Alive but quiet.
    Idle,
    Completed,
    Blocked,
    Failed,
}
