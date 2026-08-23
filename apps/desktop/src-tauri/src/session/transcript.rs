//! Following a provider's transcript into the session log.
//!
//! This is what makes §23 real. The provider's structured events are appended
//! to the **same** append-only log as the terminal bytes, through the same
//! single writer. Terminal View and Conversation View are then two projections
//! of one stream, in one order — not two sources that have to agree.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rusqlite::params;

use crate::db::Database;
use crate::providers::conversation::ConversationItem;
use crate::providers::{claude, codex, tail::JsonlTailer};
use crate::session::event::EventKind;
use crate::session::manager::LiveSession;

/// How often the transcript is polled once found.
///
/// Fast enough that Conversation View feels live, slow enough that following a
/// session costs nothing measurable.
const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// How long to keep looking for the transcript before giving up.
///
/// A provider writes nothing until the session has something to record, and a
/// user may sit at a prompt for a while before typing, so this is generous.
const LOCATE_TIMEOUT: Duration = Duration::from_secs(600);

/// Which log frame carries an item, so projections can filter cheaply without
/// deserialising everything.
fn kind_for(item: &ConversationItem) -> EventKind {
    match item {
        // A turn that carries only usage is a usage sample, not a message.
        // Filing it as a message would bury the numbers Analytics reads and
        // would put an empty bubble in the conversation.
        ConversationItem::Message { text, usage, .. } if text.trim().is_empty() && usage.is_some() => {
            EventKind::Usage
        }
        ConversationItem::Message { .. } | ConversationItem::Thinking { .. } => EventKind::Message,
        // A finished turn is a message frame on purpose, not a lifecycle one.
        //
        // The autopilot reads it from the conversation projection (§32), and
        // Conversation View can render it as the turn boundary it is. Filing it
        // under lifecycle would put it in a stream nothing structured reads.
        ConversationItem::TurnEnded { .. } => EventKind::Message,
        ConversationItem::ToolCall { .. } => EventKind::ToolCall,
        ConversationItem::ToolResult { .. } => EventKind::ToolResult,
        ConversationItem::FileChange { .. } => EventKind::FileChange,
        ConversationItem::Error { .. } => EventKind::Message,
    }
}

/// Follow a provider transcript for the lifetime of a session.
///
/// `provider` is the id from the capability model. A provider without a
/// transcript simply never starts a tailer.
pub fn spawn(
    session: Arc<LiveSession>,
    db: Arc<Database>,
    project_id: String,
    provider: String,
    cwd: String,
    launched_at_ms: i64,
    stop: Arc<AtomicBool>,
) {
    if provider == "shell" {
        return;
    }

    let session_id = session.id.clone();
    std::thread::Builder::new()
        .name(format!("transcript-{session_id}"))
        .spawn(move || {
            let Some(path) = locate(&provider, &session_id, &cwd, launched_at_ms, &stop) else {
                tracing::warn!(
                    session = %session_id,
                    %provider,
                    "gave up locating the provider transcript; conversation view will be empty"
                );
                return;
            };
            tracing::info!(session = %session_id, path = ?path, "following provider transcript");

            let mut tailer = JsonlTailer::new(path);
            while !stop.load(Ordering::Relaxed) {
                match tailer.poll() {
                    Ok(lines) => {
                        for line in lines {
                            let items = match provider.as_str() {
                                "claude-code" => claude::parse_line(&line),
                                "codex" => codex::parse_line(&line),
                                _ => Vec::new(),
                            };
                            for item in items {
                                if let Ok(payload) = serde_json::to_vec(&item) {
                                    session.log(kind_for(&item), payload);
                                }
                                // The log is the record; these tables are the
                                // index. Analytics and the timeline need to
                                // aggregate across sessions without reading
                                // every log file (§52, §39).
                                mirror(&db, &session.id, &project_id, &provider, &item);
                            }
                        }
                    }
                    Err(e) => {
                        // A transient read error must not end the follow; the
                        // provider may be mid-write.
                        tracing::debug!(session = %session_id, error = %e, "transcript poll failed");
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        })
        .expect("spawn transcript tailer");
}

/// Wait for the provider to create its transcript, then return its path.
fn locate(
    provider: &str,
    session_id: &str,
    cwd: &str,
    launched_at_ms: i64,
    stop: &AtomicBool,
) -> Option<std::path::PathBuf> {
    let deadline = std::time::Instant::now() + LOCATE_TIMEOUT;

    while std::time::Instant::now() < deadline {
        if stop.load(Ordering::Relaxed) {
            return None;
        }

        let found = match provider {
            // Deterministic: we chose the id, so the file is named for it.
            "claude-code" => claude::find_transcript(session_id),
            // Heuristic: match on working directory and start time, because
            // Codex assigns its own id (§26).
            "codex" => codex::correlate(cwd, launched_at_ms),
            _ => None,
        };

        if let Some(path) = found {
            return Some(path);
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::conversation::Role;

    #[test]
    fn items_map_to_the_frame_kind_that_carries_them() {
        let message = ConversationItem::Message {
            role: Role::Assistant,
            text: "hi".into(),
            ts_ms: 1,
            usage: None,
        };
        let call = ConversationItem::ToolCall {
            id: "t".into(),
            name: "Bash".into(),
            summary: "ls".into(),
            ts_ms: 1,
        };
        let result = ConversationItem::ToolResult {
            id: "t".into(),
            ok: true,
            summary: "".into(),
            ts_ms: 1,
        };

        assert_eq!(kind_for(&message), EventKind::Message);
        assert_eq!(kind_for(&call), EventKind::ToolCall);
        assert_eq!(kind_for(&result), EventKind::ToolResult);
    }

    /// The frames written here are read back by the conversation projection, so
    /// the encoding has to survive a round trip exactly.
    #[test]
    fn conversation_items_round_trip_through_the_log_encoding() {
        let original = ConversationItem::ToolCall {
            id: "tu_1".into(),
            name: "Bash".into(),
            summary: "git status".into(),
            ts_ms: 1_787_435_821_375,
        };
        let bytes = serde_json::to_vec(&original).unwrap();
        let decoded: ConversationItem = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded, original);
    }

    /// An error is conversation, not machinery: it belongs in the stream the
    /// reader sees rather than only in a log line.
    #[test]
    fn provider_errors_are_carried_as_messages() {
        let error = ConversationItem::Error {
            message: "usage limit reached".into(),
            ts_ms: 1,
        };
        assert_eq!(kind_for(&error), EventKind::Message);
    }
}

/// Mirror the queryable parts of an item into SQLite.
///
/// Failures are logged and swallowed. This is a derived index; losing a row
/// costs a number in a chart, and must never disturb the session it describes.
fn mirror(
    db: &Database,
    session_id: &str,
    project_id: &str,
    provider: &str,
    item: &ConversationItem,
) {
    let outcome = match item {
        ConversationItem::Message { usage: Some(usage), ts_ms, .. } if !usage.is_empty() => {
            db.with(|conn| {
                conn.execute(
                    "INSERT INTO usage_samples
                         (session_id, project_id, provider, model, ts_ms,
                          input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                          cost_usd, confidence, limit_percent, limit_resets_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![
                        session_id,
                        project_id,
                        provider,
                        usage.model,
                        ts_ms,
                        usage.input.map(|v| v as i64),
                        usage.output.map(|v| v as i64),
                        usage.cache_read.map(|v| v as i64),
                        usage.cache_write.map(|v| v as i64),
                        usage.cost_usd,
                        // Carried through from the adapter, never inferred here.
                        serde_json::to_string(&usage.confidence)
                            .unwrap_or_default()
                            .trim_matches('"'),
                        usage.limit_percent,
                        usage.limit_resets_at,
                    ],
                )?;
                Ok(())
            })
        }

        ConversationItem::FileChange { path, ts_ms } => db.with(|conn| {
            conn.execute(
                "INSERT INTO file_changes (session_id, project_id, path, ts_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, project_id, path, ts_ms],
            )?;
            Ok(())
        }),

        _ => return,
    };

    if let Err(e) = outcome {
        tracing::warn!(error = %e, session = session_id, "could not mirror a session event");
    }
}
