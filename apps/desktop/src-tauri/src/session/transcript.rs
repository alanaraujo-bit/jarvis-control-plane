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
        ConversationItem::Message { text, usage, .. }
            if text.trim().is_empty() && usage.is_some() =>
        {
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
        // A title is a fact about the session, not something anybody said
        // (§88, D37). Filing it as a message would put a stray bubble in
        // Conversation View, which reads `Message` frames and would have no
        // idea this one is not part of the conversation.
        ConversationItem::Title { .. } => EventKind::Lifecycle,
    }
}

/// Whether a transcript line is a **replay** of a conversation this session is
/// continuing rather than something that just happened (§88, D41).
///
/// ## Why this cannot be done any other way
///
/// Resuming hands the provider a past conversation, and both providers then put
/// that conversation into the file we are tailing — by different routes, with
/// the same consequence:
///
/// * **Claude Code** (`--resume --fork-session`, 2.1.241) writes a *new*
///   transcript that opens with a **full copy** of the prior conversation. Every
///   copied line is rewritten with the new session id, so the id cannot tell a
///   copy from a new turn.
/// * **Codex** (`codex resume <id>`, 0.147.0) **appends** to the original
///   rollout, which already holds the whole prior conversation.
///
/// Either way a tailer reading from the top would mirror the old conversation a
/// second time: every token counted twice in Analytics (§52), every sentence
/// found twice in Global Search (§51), and — before this existed — a
/// notification raised for every finished turn the person already read, and a
/// quota event recorded for consumption that happened hours ago.
///
/// Offsets cannot be used for the boundary: the file is created by the provider
/// after we launch, so there is no "before" position to seek past, and a naive
/// seek-to-end races the first real turn.
///
/// **The timestamp is the boundary that actually works.** A replayed line keeps
/// its *original* time — verified on this machine: a fork's copied lines carry
/// 16:18–16:22 while the fork itself was written at 17:13. Nothing that happened
/// before this session launched can be something this session did.
///
/// A line with no timestamp at all is deliberately **not** treated as a replay.
/// Claude Code's `ai-title` is the only such line (§88), and taking it is right:
/// a continued session should carry the name of the conversation it continues
/// until the provider chooses a better one.
fn is_replayed_line(line: &str, boundary_ms: i64) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        // Unparseable is not evidence of anything. Let the parsers decide;
        // they already ignore what they cannot read.
        return false;
    };
    value
        .get("timestamp")
        .and_then(serde_json::Value::as_str)
        .and_then(crate::providers::conversation::parse_timestamp)
        .map(|ts| ts < boundary_ms)
        .unwrap_or(false)
}

/// Follow a provider transcript for the lifetime of a session.
///
/// `provider` is the id from the capability model. A provider without a
/// transcript simply never starts a tailer.
///
/// `replay_boundary_ms` is set only for a **resumed** session (§88, D41): every
/// line older than it is a replay of the conversation being continued and is
/// dropped whole, before anything reads it. See `is_replayed_line`.
#[allow(clippy::too_many_arguments)]
pub fn spawn(
    session: Arc<LiveSession>,
    db: Arc<Database>,
    project_id: String,
    provider: String,
    account_id: Option<String>,
    transcript_root: Option<std::path::PathBuf>,
    cwd: String,
    launched_at_ms: i64,
    stop: Arc<AtomicBool>,
    mission_id: Option<String>,
    replay_boundary_ms: Option<i64>,
) {
    if provider == "shell" {
        return;
    }

    let session_id = session.id.clone();
    std::thread::Builder::new()
        .name(format!("transcript-{session_id}"))
        .spawn(move || {
            let Some(path) = locate(
                &provider,
                transcript_root.as_deref(),
                &session_id,
                &cwd,
                launched_at_ms,
                &stop,
            ) else {
                tracing::warn!(
                    session = %session_id,
                    %provider,
                    "gave up locating the provider transcript; conversation view will be empty"
                );
                return;
            };
            tracing::info!(session = %session_id, path = ?path, "following provider transcript");

            // Record the id the provider itself uses for this session.
            //
            // Claude Code is handed ours at launch, so its row already carries
            // it. **Codex assigns its own**, and until now nothing ever wrote it
            // down — the column existed from migration 1 with a comment saying
            // it is there "so a session can be resumed", and for Codex it was
            // always NULL. The id is right here in the rollout's filename, and
            // this is the only moment we know it (§88, D41).
            if provider == "codex" {
                if let Some(rollout_id) = codex::session_id_from_path(&path) {
                    let recorded = db.with(|conn| {
                        conn.execute(
                            "UPDATE sessions SET provider_session_id = ?2 WHERE id = ?1",
                            rusqlite::params![&session_id, &rollout_id],
                        )?;
                        Ok(())
                    });
                    if let Err(e) = recorded {
                        tracing::warn!(error = %e, session = %session_id, "could not record the rollout id");
                    }
                }
            }

            let mut tailer = JsonlTailer::new(path);
            let mut last_said: Option<String> = None;
            while !stop.load(Ordering::Relaxed) {
                match tailer.poll() {
                    Ok(lines) => {
                        for line in lines {
                            // Dropped **whole**, before anything reads it.
                            //
                            // Gating only the conversation items would leave
                            // three other readers running on replayed history:
                            // `observe_line` below records quota consumption
                            // that happened hours ago, the rotation check under
                            // it can switch accounts because of it, and
                            // `announce_turn_ended` raises a notification for
                            // every turn of the old conversation — one per
                            // finished turn, for turns the person has already
                            // read. See D41.
                            if let Some(boundary) = replay_boundary_ms {
                                if is_replayed_line(&line, boundary) {
                                    continue;
                                }
                            }

                            if let Some(account_id) = account_id.as_deref() {
                                crate::accounts::quota::observe_line(
                                    &db,
                                    account_id,
                                    &session_id,
                                    &provider,
                                    &line,
                                );
                            }
                            let items = match provider.as_str() {
                                "claude-code" => claude::parse_line(&line),
                                "codex" => codex::parse_line(&line),
                                _ => Vec::new(),
                            };
                            for item in items {
                                // What the agent last said, kept so a finished
                                // turn can carry it as its preview. The reply
                                // arrives immediately before the `TurnEnded`
                                // that closes the turn, in the same batch.
                                if let ConversationItem::Message {
                                    role: crate::providers::conversation::Role::Assistant,
                                    text,
                                    ..
                                } = &item
                                {
                                    if !text.trim().is_empty() {
                                        last_said = Some(text.clone());
                                    }
                                }
                                if matches!(item, ConversationItem::TurnEnded { .. }) {
                                    announce_turn_ended(
                                        &session_id,
                                        &project_id,
                                        &provider,
                                        mission_id.as_deref(),
                                        last_said.take(),
                                    );
                                }
                                if let Ok(payload) = serde_json::to_vec(&item) {
                                    session.log(kind_for(&item), payload);
                                }
                                // The log is the record; these tables are the
                                // index. Analytics and the timeline need to
                                // aggregate across sessions without reading
                                // every log file (§52, §39).
                                mirror(
                                    &db,
                                    &session.id,
                                    &project_id,
                                    &provider,
                                    account_id.as_deref(),
                                    &item,
                                );
                            }

                            if let Some(account_id) = account_id.as_deref() {
                                let _ = crate::accounts::switch::maybe_rotate_recorded(
                                    &db,
                                    account_id,
                                );
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

/// Tell somebody the agent finished a turn (§49).
///
/// The preview is the agent's **last reply**, not a sentence of ours. Somebody
/// coming back to the screen wants to know what it said, and a translated
/// "the agent finished" tells them nothing they did not already know from the
/// fact that they were notified at all.
fn announce_turn_ended(
    session_id: &str,
    project_id: &str,
    provider: &str,
    mission_id: Option<&str>,
    last_said: Option<String>,
) {
    crate::notify::bus::raise(
        crate::notify::Reason::TurnEnded,
        // The provider said so; we did not infer it (§28).
        crate::session::event::Confidence::Official,
        crate::notify::Raise {
            session_id: Some(session_id.to_string()),
            project_id: Some(project_id.to_string()),
            mission_id: mission_id.map(str::to_string),
            provider: Some(provider.to_string()),
            preview: last_said,
            detail_code: None,
        },
    );
}

/// Wait for the provider to create its transcript, then return its path.
fn locate(
    provider: &str,
    transcript_root: Option<&std::path::Path>,
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
            "claude-code" => match transcript_root {
                Some(root) => claude::find_transcript_in(root, session_id),
                None => claude::find_transcript(session_id),
            },
            // Heuristic: match on working directory and start time, because
            // Codex assigns its own id (§26).
            "codex" => match transcript_root {
                Some(root) => codex::correlate_in(root, cwd, launched_at_ms),
                None => codex::correlate(cwd, launched_at_ms),
            },
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
mod replay_boundary {
    use super::*;

    /// The instant a resumed session launched. Everything older is a replay.
    const BOUNDARY: i64 = 1_787_435_821_375; // 2026-08-22T21:57:01.375Z

    /// Verbatim shapes from real transcripts on this machine.
    #[test]
    fn a_line_from_before_the_resume_is_a_replay() {
        // A copied line keeps its original time — this is the whole mechanism.
        let old = r#"{"type":"user","timestamp":"2026-08-22T16:18:16.034Z","message":{"role":"user","content":"hi"}}"#;
        assert!(is_replayed_line(old, BOUNDARY));
    }

    #[test]
    fn a_line_from_after_the_resume_is_this_sessions_own_work() {
        let new = r#"{"type":"assistant","timestamp":"2026-08-22T23:13:18.901Z","message":{"role":"assistant"}}"#;
        assert!(!is_replayed_line(new, BOUNDARY));
    }

    /// The boundary is the launch instant itself; a line stamped exactly then
    /// belongs to this session. Off by one here would drop a real first turn.
    #[test]
    fn the_boundary_instant_itself_is_not_a_replay() {
        let exact = r#"{"type":"assistant","timestamp":"2026-08-22T21:57:01.375Z"}"#;
        assert!(!is_replayed_line(exact, BOUNDARY));
    }

    /// Claude Code's `ai-title` carries no timestamp at all (§88). Taking it is
    /// deliberate: a continued session should carry the name of the
    /// conversation it continues until the provider picks a better one.
    #[test]
    fn a_line_with_no_timestamp_is_kept_rather_than_guessed_at() {
        let title = r#"{"type":"ai-title","aiTitle":"README.md review","sessionId":"x"}"#;
        assert!(!is_replayed_line(title, BOUNDARY));
    }

    #[test]
    fn nonsense_is_not_evidence_of_a_replay() {
        assert!(!is_replayed_line("", BOUNDARY));
        assert!(!is_replayed_line("{not json", BOUNDARY));
        assert!(!is_replayed_line(r#"{"timestamp":"not-a-time"}"#, BOUNDARY));
    }

    /// A Codex envelope has the same top-level `timestamp`, so one rule covers
    /// both providers — which matters because they replay for *different*
    /// reasons: Claude Code copies the conversation into a new file, Codex
    /// appends to the original one.
    #[test]
    fn the_same_rule_reads_a_codex_envelope() {
        let old = r#"{"timestamp":"2026-08-22T16:29:52.000Z","ordinal":0,"type":"event_msg","payload":{}}"#;
        let new = r#"{"timestamp":"2026-08-23T16:29:52.000Z","ordinal":9,"type":"event_msg","payload":{}}"#;
        assert!(is_replayed_line(old, BOUNDARY));
        assert!(!is_replayed_line(new, BOUNDARY));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::conversation::{Role, TokenUsage};

    /// A database with the one project and session `mirror` needs to satisfy
    /// the foreign keys on `session_events`.
    fn seeded_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'demo', 'C:/demo', 0, 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at)
                 VALUES ('s1', 'p1', 'claude-code', 'C:/demo', 'idle', 'C:/logs', 0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn event_row(db: &Database) -> (String, Option<String>, String) {
        db.with(|conn| {
            conn.query_row(
                "SELECT kind, label, text FROM session_events WHERE session_id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .unwrap()
    }

    fn event_count(db: &Database) -> i64 {
        db.with(|conn| conn.query_row("SELECT COUNT(*) FROM session_events", [], |r| r.get(0)))
            .unwrap()
    }

    /// The one thing this write path exists for: what an agent said has to
    /// actually be findable afterwards, through the same FTS5 index Global
    /// Search queries (§51).
    #[test]
    fn a_message_is_mirrored_and_findable_through_the_fts_index() {
        let db = seeded_db();
        let message = ConversationItem::Message {
            role: Role::Assistant,
            text: "the tree is clean".into(),
            ts_ms: 1000,
            usage: None,
        };

        mirror(&db, "s1", "p1", "claude-code", None, &message);

        let (kind, label, text) = event_row(&db);
        assert_eq!(kind, "message");
        assert_eq!(label.as_deref(), Some("assistant"));
        assert_eq!(text, "the tree is clean");

        let hits: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM session_events_fts WHERE session_events_fts MATCH 'clean'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(
            hits, 1,
            "the mirrored text must reach the search index, not just the row"
        );
    }

    /// A turn that carries only usage numbers is a usage sample (§28), not
    /// something for a person to search for — indexing an empty string would
    /// put a blank, unfindable row in the table for every such turn.
    #[test]
    fn a_usage_only_message_is_not_mirrored_as_searchable_text() {
        let db = seeded_db();
        let usage = TokenUsage {
            input: Some(120),
            ..TokenUsage::default()
        };
        let message = ConversationItem::Message {
            role: Role::Assistant,
            text: String::new(),
            ts_ms: 1,
            usage: Some(usage),
        };

        mirror(&db, "s1", "p1", "claude-code", None, &message);

        assert_eq!(event_count(&db), 0);
        let usage_rows: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM usage_samples WHERE session_id = 's1'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(usage_rows, 1);
    }

    #[test]
    fn usage_is_attributed_to_the_account_that_started_the_session() {
        let db = seeded_db();
        let message = ConversationItem::Message {
            role: Role::Assistant,
            text: String::new(),
            ts_ms: 1,
            usage: Some(TokenUsage {
                input: Some(120),
                ..TokenUsage::default()
            }),
        };

        mirror(&db, "s1", "p1", "claude-code", Some("account-a"), &message);

        let account_id: Option<String> = db
            .with(|conn| {
                conn.query_row(
                    "SELECT account_id FROM usage_samples WHERE session_id = 's1'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(account_id.as_deref(), Some("account-a"));
    }

    /// Found by running a real Claude Code turn and searching for its own
    /// reply: it was not there. A real assistant turn almost always carries
    /// **both** real text and usage — the two concerns are not exclusive, and
    /// routing on `match` picked one arm and silently dropped the other. Every
    /// substantive reply an agent ever gives was going missing from search.
    #[test]
    fn a_reply_with_both_text_and_usage_is_recorded_both_ways() {
        let db = seeded_db();
        let usage = TokenUsage {
            input: Some(120),
            output: Some(40),
            ..TokenUsage::default()
        };
        let message = ConversationItem::Message {
            role: Role::Assistant,
            text: "jarvis-search-probe-9d3f".into(),
            ts_ms: 1,
            usage: Some(usage),
        };

        mirror(&db, "s1", "p1", "claude-code", None, &message);

        assert_eq!(
            event_count(&db),
            1,
            "the reply's own text must reach the search index"
        );
        let (kind, label, text) = event_row(&db);
        assert_eq!(kind, "message");
        assert_eq!(label.as_deref(), Some("assistant"));
        assert_eq!(text, "jarvis-search-probe-9d3f");

        let usage_rows: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM usage_samples WHERE session_id = 's1'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(
            usage_rows, 1,
            "the usage figures still belong to Analytics too"
        );
    }

    /// Thinking is what a person might search for later ("why did it decide
    /// to..."); a turn boundary carries no text of its own and must not leave
    /// an empty row behind.
    #[test]
    fn thinking_is_indexed_and_a_turn_boundary_is_not() {
        let db = seeded_db();
        mirror(
            &db,
            "s1",
            "p1",
            "claude-code",
            None,
            &ConversationItem::Thinking {
                text: "weighing the tradeoffs".into(),
                ts_ms: 1,
            },
        );
        mirror(
            &db,
            "s1",
            "p1",
            "claude-code",
            None,
            &ConversationItem::TurnEnded {
                reason: "end_turn".into(),
                ts_ms: 2,
            },
        );

        assert_eq!(
            event_count(&db),
            1,
            "only the thinking item carries text worth finding"
        );
        let (kind, label, text) = event_row(&db);
        assert_eq!(kind, "thinking");
        assert!(label.is_none());
        assert_eq!(text, "weighing the tradeoffs");
    }

    /// A tool's own name has to be searchable alongside its summary, or
    /// looking for "Bash" would never find the calls that ran it.
    #[test]
    fn a_tool_call_is_indexed_under_its_own_name() {
        let db = seeded_db();
        mirror(
            &db,
            "s1",
            "p1",
            "claude-code",
            None,
            &ConversationItem::ToolCall {
                id: "t1".into(),
                name: "Bash".into(),
                summary: "git status".into(),
                ts_ms: 1,
            },
        );

        let (kind, label, text) = event_row(&db);
        assert_eq!(kind, "toolCall");
        assert_eq!(label.as_deref(), Some("Bash"));
        assert!(text.contains("Bash"));
        assert!(text.contains("git status"));
    }

    /// Captured verbatim (cwd genericised) from the real Claude Code turn that
    /// exposed the bug fixed above: a real reply's own `Message` line, straight
    /// from `claude::parse_line`, carrying both text and real usage numbers
    /// together — which is the ordinary shape, not an edge case. Run through
    /// the actual parser rather than a hand-built `ConversationItem`, so this
    /// pins the whole pipeline the app runs, not just `mirror`'s contract.
    #[test]
    fn a_real_claude_code_reply_survives_the_full_pipeline_into_search() {
        let line = r#"{"parentUuid":"90326252-19f4-45cd-9c72-dfabcb910997","isSidechain":false,"message":{"model":"claude-sonnet-5","id":"msg_011CeKrYjxgqzrAPfN2jSs6z","type":"message","role":"assistant","content":[{"type":"text","text":"jarvis-search-dbg-55e3"}],"stop_reason":"end_turn","stop_sequence":null,"stop_details":null,"usage":{"input_tokens":2,"cache_creation_input_tokens":17709,"cache_read_input_tokens":33226,"output_tokens":16,"output_tokens_details":{"thinking_tokens":0},"server_tool_use":{"web_search_requests":0,"web_fetch_requests":0},"service_tier":"standard"},"diagnostics":null},"requestId":"req_011CeKrYhe3Jo4cNf37UpPeq","type":"assistant","uuid":"143054d7-7d38-4637-92eb-a5d40035b662","timestamp":"2026-08-23T14:41:24.000Z","session_id":"01a02f11-903f-7731-ab39-41dac1350593","userType":"external","entrypoint":"cli","cwd":"/scratch/brain-demo","sessionId":"01a02f11-903f-7731-ab39-41dac1350593","version":"2.1.241","gitBranch":"main"}"#;

        let items = claude::parse_line(line);
        let message = items
            .iter()
            .find(|item| {
                matches!(
                    item,
                    ConversationItem::Message {
                        role: Role::Assistant,
                        ..
                    }
                )
            })
            .expect("a real assistant turn must parse into a Message item");

        let db = seeded_db();
        mirror(&db, "s1", "p1", "claude-code", None, message);

        let hits: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM session_events_fts WHERE session_events_fts MATCH 'dbg'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(
            hits, 1,
            "a real assistant reply, usage and all, must land in the search index"
        );

        let usage_rows: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM usage_samples WHERE session_id = 's1'",
                    [],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(usage_rows, 1, "and Analytics must still get its tokens");
    }

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
///
/// The two things below are independent, not alternatives picked by a
/// `match`: a real assistant turn almost always carries **both** its own text
/// and the usage the provider reported for producing it. An earlier version
/// of this function used one `match` to route between "record usage" and
/// "record searchable text", so a normal reply — text and usage together —
/// took the usage arm and its text was never indexed. Found by running a real
/// Claude Code turn and searching for its own reply: it was not there.
fn mirror(
    db: &Database,
    session_id: &str,
    project_id: &str,
    provider: &str,
    account_id: Option<&str>,
    item: &ConversationItem,
) {
    if let ConversationItem::Message {
        usage: Some(usage),
        ts_ms,
        ..
    } = item
    {
        if !usage.is_empty() {
            let outcome = db.with(|conn| {
                conn.execute(
                    "INSERT INTO usage_samples
                         (session_id, project_id, provider, model, ts_ms,
                          input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                          cost_usd, confidence, limit_percent, limit_resets_at, account_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
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
                        account_id,
                    ],
                )?;
                Ok(())
            });
            if let Err(e) = outcome {
                tracing::warn!(error = %e, session = session_id, "could not mirror usage");
            }
        }
    }

    // The provider named the session itself (§88, D36). `title::set` decides
    // whether it is allowed to land — a rename outranks it and is never
    // overwritten — so there is nothing to check here.
    if let ConversationItem::Title { text, .. } = item {
        if let Err(e) = crate::session::title::set(
            db,
            session_id,
            crate::session::title::Source::Provider,
            text,
        ) {
            tracing::warn!(error = %e, session = session_id, "could not record a provider title");
        }
        // Nothing further: a title is not conversation content and has no
        // business in the search index as a thing that was said.
        return;
    }

    if let ConversationItem::FileChange { path, ts_ms } = item {
        let outcome = db.with(|conn| {
            conn.execute(
                "INSERT INTO file_changes (session_id, project_id, path, ts_ms)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, project_id, path, ts_ms],
            )?;
            Ok(())
        });
        if let Err(e) = outcome {
            tracing::warn!(error = %e, session = session_id, "could not mirror a file change");
        }
        // Nothing further: `search_event` has no case for `FileChange` either.
        return;
    }

    // Everything Global Search (§51) can find later: what was said, thought,
    // run, and got back — regardless of whether the same item also carried
    // usage above. `TurnEnded` carries no text worth indexing.
    // The first thing a person types names the session until something better
    // arrives (§88, D36). Written here rather than by re-reading the table a
    // line later: `title::set` refuses a derived title over any existing one,
    // so every message after the first is a no-op rather than a rename, and
    // Codex — which never states a title of its own — gets one anyway.
    if let ConversationItem::Message {
        role: crate::providers::conversation::Role::User,
        text,
        ..
    } = item
    {
        if !text.trim().is_empty() {
            let _ = crate::session::title::set(
                db,
                session_id,
                crate::session::title::Source::Derived,
                text,
            );
        }
    }

    if let Some((kind, label, text)) = search_event(item) {
        if !text.trim().is_empty() {
            let outcome = insert_search_event(
                db,
                session_id,
                project_id,
                item.ts_ms(),
                kind,
                label,
                &text,
                item,
            );
            if let Err(e) = outcome {
                tracing::warn!(error = %e, session = session_id, "could not mirror a session event");
            }
        }
    }
}

/// The plain-text shape of an item worth finding later, if it has one.
///
/// Returns the `ConversationItem`'s own tag (never the coarser `EventKind` —
/// see the migration 9 comment), an optional label naming who or what, and the
/// text itself. A `usage`-only `Message` still reaches here with empty text
/// and is dropped by the empty check at the call site, same as an empty
/// `ToolResult` summary.
pub(crate) fn search_event(
    item: &ConversationItem,
) -> Option<(&'static str, Option<String>, String)> {
    match item {
        ConversationItem::Message { role, text, .. } => {
            let role = serde_json::to_string(role).unwrap_or_default();
            Some((
                "message",
                Some(role.trim_matches('"').to_string()),
                text.clone(),
            ))
        }
        ConversationItem::Thinking { text, .. } => Some(("thinking", None, text.clone())),
        // The tool's own name is folded into the indexed text, not left only
        // in `label`: a search for "Bash" has to find the calls that ran it,
        // and `label` is not a column search touches.
        ConversationItem::ToolCall { name, summary, .. } => {
            let text = if summary.trim().is_empty() {
                name.clone()
            } else {
                format!("{name}: {summary}")
            };
            Some(("toolCall", Some(name.clone()), text))
        }
        ConversationItem::ToolResult { ok, summary, .. } => Some((
            "toolResult",
            Some(if *ok { "ok" } else { "error" }.into()),
            summary.clone(),
        )),
        ConversationItem::Error { message, .. } => Some(("error", None, message.clone())),
        // A title is the *name* of a conversation, not a line in it. Indexing
        // it would make a session match a search for words nobody in it ever
        // said — and Session History searches titles separately anyway (§88).
        ConversationItem::FileChange { .. }
        | ConversationItem::TurnEnded { .. }
        | ConversationItem::Title { .. } => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_search_event(
    db: &Database,
    session_id: &str,
    project_id: &str,
    ts_ms: i64,
    kind: &str,
    label: Option<String>,
    text: &str,
    item: &ConversationItem,
) -> crate::db::Result<()> {
    let payload = serde_json::to_string(item).unwrap_or_default();
    db.with(|conn| {
        // `seq` keeps the composite primary key migration 1 declared; nothing
        // downstream reads it as anything but a per-session tiebreaker; there
        // is exactly one writer per session (this tailer thread), so a plain
        // count cannot race with itself.
        let seq: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_events WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO session_events
                 (session_id, seq, ts_ms, project_id, kind, label, text, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![session_id, seq, ts_ms, project_id, kind, label, text, payload],
        )?;
        conn.execute(
            "INSERT INTO session_events_fts (session_id, ts_ms, project_id, kind, label, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, ts_ms, project_id, kind, label, text],
        )?;
        Ok(())
    })
}
