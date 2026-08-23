//! Agents driving missions under Unattended (§32).
//!
//! ## What this is
//!
//! Guided and Autonomous both assume a person is at the keyboard: the agent
//! finishes a turn and someone reads it and types the next thing. Unattended is
//! the mode where nobody does — the mission is meant to be left alone until it
//! is done, genuinely blocked, or failed.
//!
//! So something has to take the human's seat. That is this module: it watches a
//! session's own event log, and when the provider says the agent has finished a
//! turn, it decides whether the mission is finished and, if not, sends the next
//! instruction.
//!
//! ## Why it reads the log rather than the terminal
//!
//! The log is the session (§23, D2). Reading terminal bytes to guess whether an
//! agent is done would mean scraping ANSI — the thing D3 exists to avoid — and
//! it would be wrong in the expensive direction: a long compile and a finished
//! turn look identical from the outside.
//!
//! Both providers state it outright. Claude Code reports
//! `stop_reason: "end_turn"` where `"tool_use"` means it is still working;
//! Codex emits `event_msg/task_complete`. Verified across every transcript on
//! this machine — 26,928 assistant messages carrying a stop reason — before any
//! of this was designed. The parsers turn both into `ConversationItem::TurnEnded`,
//! and this module reacts to that and nothing else.
//!
//! ## The three ways a run stops
//!
//! §34 is the whole design constraint: a run that cannot proceed must say so
//! rather than consume resources indefinitely. There are exactly three endings
//! and none of them is "keep going and hope":
//!
//! * **Completed** — every required criterion verified, by evidence (§30).
//! * **Blocked / Waiting** — something a person has to resolve. A guardrail
//!   refusal with nobody to ask lands here (§35).
//! * **Failed** — the turn budget ran out, or verification kept failing.
//!
//! The turn budget is not a safety net bolted on; it is the honest admission
//! that an agent which has taken twenty turns without verifying anything is not
//! about to succeed on the twenty-first.

pub mod commands;
pub mod driver;
pub mod plan;

pub use driver::{start, stop, Autopilot, RunState};
pub use plan::{next_instruction, Step};
