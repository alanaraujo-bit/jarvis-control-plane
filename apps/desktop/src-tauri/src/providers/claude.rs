//! Claude Code adapter.
//!
//! Verified against Claude Code 2.1.240. Claude Code writes a JSONL transcript
//! while it runs, so Conversation View is built on structured data the provider
//! itself produced — not on scraped terminal output. That is the difference
//! between a faithful view and a fragile one, and it is also where the official
//! token counts come from (§28).
//!
//! ## Locating the transcript
//!
//! `~/.claude/projects/<encoded-cwd>/<session-id>.jsonl`
//!
//! The directory encoding is **not** reconstructed from the working directory.
//! Observed encodings on one machine disagree about case and about how dots are
//! handled, so a derived path would be wrong in ways that are invisible until
//! it silently finds nothing. The session id is unique, so the file is found by
//! searching for it — and the first line's `cwd` confirms the match.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::conversation::{parse_timestamp, truncate, ConversationItem, Role, TokenUsage};
use super::{
    BriefingSupport, ConversationSource, Correlation, GuardrailSupport, Provider,
    ProviderCapabilities, TitleSupport, UsageReporting,
};
use crate::session::event::Confidence;

pub struct ClaudeCode;

impl Provider for ClaudeCode {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            executable: "claude".into(),
            correlation: Correlation::Deterministic,
            conversation: ConversationSource::Transcript,
            usage: UsageReporting::Official,
            images: true,
            resume: true,
            approvals: true,
            // Verified against 2.1.240: a PreToolUse hook is consulted before a
            // tool runs and a refusal genuinely stops it (§35).
            guardrails: GuardrailSupport::PreExecution,
            worktrees: true,
            // Verified through a real PTY, not from --help.
            briefing: BriefingSupport::SystemPrompt,
            account_switching: true,
            // It writes its own `ai-title` line. Verified against 124 real
            // transcripts on this machine, 89 of which carry one.
            titles: TitleSupport::Provider,
        }
    }

    fn launch_args(&self, session_id: &str) -> Vec<String> {
        vec!["--session-id".into(), session_id.into()]
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Whether Claude Code has been told this folder is trusted.
///
/// ## Why this exists
///
/// Found by starting a real agent in a brand-new temporary directory: Claude
/// Code opens with **"Is this a project you created or one you trust?"** and
/// waits. Nothing else happens until somebody answers, and everything typed
/// before then goes into that dialog rather than to the agent.
///
/// Under Guided or Autonomous a person is watching and simply answers it. Under
/// Unattended (§32) nobody is, and the run would sit there until its budget ran
/// out — the indefinite consumption §34 exists to forbid, wearing a different
/// hat to D12's permission prompt.
///
/// It is not a hypothetical: a **worktree is a brand-new folder** (§45), so
/// "start an agent in a fresh worktree, unattended" is precisely the case that
/// hangs, and it is a thing this product now invites people to do.
///
/// `None` means the answer is genuinely **unknown** — the config is missing or
/// will not parse — and unknown is deliberately not reported as untrusted:
/// guessing that way blocks a run that would have worked fine (§28).
///
/// A folder that simply has **no entry** is a different thing and is reported
/// as `Some(false)`. Claude Code has never opened it, so it will certainly ask.
/// That distinction is the whole feature: a fresh worktree has no entry, and
/// folding it in with "unknown" would let exactly the case this exists to catch
/// walk straight past.
///
/// **Nothing here ever writes trust.** Marking a folder trusted is a security
/// decision that belongs to the person, in the other product's own interface.
pub fn folder_is_trusted(cwd: &Path) -> Option<bool> {
    let root = home()?.join(".claude");
    folder_is_trusted_in(&root, true, cwd)
}

/// Account-scoped trust lookup. The machine account keeps `.claude.json`
/// beside its default `.claude/` directory; an explicit CLAUDE_CONFIG_DIR
/// carries the file inside that directory (verified against 2.1.241).
pub fn folder_is_trusted_in(config_dir: &Path, adopted: bool, cwd: &Path) -> Option<bool> {
    let config = if adopted {
        config_dir.parent()?.join(".claude.json")
    } else {
        config_dir.join(".claude.json")
    };
    trusted_in(&std::fs::read_to_string(config).ok()?, cwd)
}

/// The lookup itself, separated so it can be tested against captured config.
///
/// Claude Code keys `projects` by the path it was launched in, spelled the way
/// the platform spells it. Comparison folds separators and case for the same
/// reason `files::contains` does: on Windows these are one folder, and a
/// mismatch here silently reports every folder as untrusted.
pub fn trusted_in(config_json: &str, cwd: &Path) -> Option<bool> {
    let value: Value = serde_json::from_str(config_json).ok()?;
    let projects = value.get("projects")?.as_object()?;

    let normalise = |text: &str| {
        let flattened = text.replace('\\', "/");
        let trimmed = flattened.trim_end_matches('/').to_string();
        if cfg!(windows) {
            trimmed.to_lowercase()
        } else {
            trimmed
        }
    };
    let wanted = normalise(&cwd.to_string_lossy());

    let entry = projects
        .iter()
        .find(|(path, _)| normalise(path) == wanted)
        .map(|(_, entry)| entry);

    // No entry means Claude Code has never opened this folder, so it will ask.
    // Answered, not unknown — see the note above.
    Some(
        entry
            .and_then(|e| e.get("hasTrustDialogAccepted"))
            .and_then(Value::as_bool)
            .unwrap_or(false),
    )
}

/// Find the transcript for a session id, if it has been written yet.
///
/// Returns `None` rather than an error while the file does not exist: Claude
/// Code only creates it once the session has something to record, so absence is
/// the normal state for the first seconds of a session.
pub fn find_transcript(session_id: &str) -> Option<PathBuf> {
    let root = home()?.join(".claude").join("projects");
    find_transcript_in(&root, session_id)
}

/// Find a transcript below one account's already-resolved projects root.
pub fn find_transcript_in(root: &Path, session_id: &str) -> Option<PathBuf> {
    let wanted = format!("{session_id}.jsonl");

    for entry in std::fs::read_dir(root).ok()? {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let candidate = entry.path().join(&wanted);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Read the `cwd` a transcript reports, for verifying a match.
pub fn transcript_cwd(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    for line in text.lines().take(40) {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
            return Some(cwd.to_string());
        }
    }
    None
}

/// Entries that exist for the agent's benefit, not the reader's.
///
/// A transcript is full of machinery — hook stdout, skill and tool listings,
/// context reminders, queue bookkeeping. Rendering it verbatim would bury the
/// actual work. Conversation View shows development, so this is dropped (§24).
fn is_internal_noise(value: &Value) -> bool {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match kind {
        // Bookkeeping with no reader-facing meaning.
        "queue-operation" | "last-prompt" | "ai-title" | "atis-latch" | "frame-link" => true,

        // Attachments are mostly injected context; none of it was said by anyone.
        "attachment" => true,

        // System entries are hook summaries and similar.
        "system" => true,

        // Messages the harness injected into the user turn, not typed by a human.
        "user" => value
            .get("isMeta")
            .and_then(Value::as_bool)
            .unwrap_or(false),

        _ => false,
    }
}

/// Extract plain text from a message `content` field.
///
/// The field is either a bare string or an array of typed blocks, so both are
/// handled rather than assuming the shape that happens to be in front of us.
fn content_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_usage(message: &Value) -> Option<TokenUsage> {
    let usage = message.get("usage")?;
    let get = |key: &str| usage.get(key).and_then(Value::as_u64);
    Some(TokenUsage {
        input: get("input_tokens"),
        output: get("output_tokens"),
        cache_read: get("cache_read_input_tokens"),
        cache_write: get("cache_creation_input_tokens"),
        cost_usd: None, // reported per-session, not per-turn
        model: message
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_string),
        // Claude Code states these numbers itself.
        confidence: Confidence::Official,
        limit_percent: None,
        limit_resets_at: None,
    })
}

/// Render a tool's input as a short, meaningful summary.
///
/// The point is that a reader can tell what the agent did at a glance, so the
/// most identifying argument is chosen per tool rather than dumping JSON.
fn tool_summary(name: &str, input: &Value) -> String {
    let field = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or_default();

    let raw = match name {
        "Bash" | "PowerShell" => field("command"),
        "Read" | "Edit" | "Write" | "NotebookEdit" => field("file_path"),
        "Glob" | "Grep" => {
            let pattern = field("pattern");
            if pattern.is_empty() {
                field("query")
            } else {
                pattern
            }
        }
        "WebFetch" => field("url"),
        "WebSearch" => field("query"),
        "Task" | "Agent" => field("description"),
        _ => "",
    };

    if !raw.is_empty() {
        return truncate(raw, 120);
    }
    // Unknown tool: fall back to the first string argument rather than nothing.
    input
        .as_object()
        .and_then(|map| {
            map.values()
                .find_map(Value::as_str)
                .map(|text| truncate(text, 120))
        })
        .unwrap_or_default()
}

/// The longest a session title may be.
///
/// Claude Code's own titles run to a few words; this is a bound on a value
/// read off disk, not a style choice. A row that wraps to three lines is not a
/// list any more.
pub const MAX_TITLE_CHARS: usize = 72;

/// Convert one transcript line into conversation items.
///
/// A single line can yield several items — an assistant turn commonly contains
/// thinking, text and one or more tool calls.
pub fn parse_line(line: &str) -> Vec<ConversationItem> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };

    // The title is lifted **before** the noise filter, not by loosening it.
    //
    // `is_internal_noise` drops `ai-title` on purpose and is right to: §24 says
    // Conversation View shows development, and a title is not something anybody
    // said. It is, however, exactly what Session History needs to label a row
    // with (§88, D36) — so the one reader that wants it takes it here, and the
    // filter goes on meaning what it says for every other reader.
    //
    // The line carries no `timestamp` of its own, unlike every other entry in
    // the transcript, so this item is stamped when it is read.
    if value.get("type").and_then(Value::as_str) == Some("ai-title") {
        let text = value
            .get("aiTitle")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if text.is_empty() {
            return Vec::new();
        }
        return vec![ConversationItem::Title {
            text: truncate(text, MAX_TITLE_CHARS),
            ts_ms: crate::session::log::now_ms(),
        }];
    }

    if is_internal_noise(&value) {
        return Vec::new();
    }

    // Sub-agent traffic belongs to its own thread, not the main conversation.
    if value
        .get("isSidechain")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }

    let ts_ms = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)
        .unwrap_or(0);

    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    // File history: the provider is telling us which files it touched. Keeping
    // it is what lets a session answer "what did this actually change?" (§39).
    if kind == "file-history-delta" {
        return value
            .get("trackingPath")
            .and_then(Value::as_str)
            .map(|path| {
                vec![ConversationItem::FileChange {
                    path: path.to_string(),
                    ts_ms,
                }]
            })
            .unwrap_or_default();
    }
    if kind == "file-history-snapshot" {
        // A snapshot is a checkpoint; only its tracked files carry information.
        //
        // Its timestamp lives *inside* `snapshot`, not at the top level like
        // every other entry — so the outer lookup above finds nothing and the
        // change would be stamped 0, putting it at the epoch in the timeline
        // (§39). Found by parsing every transcript on this machine, not by
        // reading the format description.
        let snapshot = value.get("snapshot");
        let ts_ms = snapshot
            .and_then(|s| s.get("timestamp"))
            .and_then(Value::as_str)
            .and_then(parse_timestamp)
            .unwrap_or(ts_ms);

        return snapshot
            .and_then(|s| s.get("trackedFileBackups"))
            .and_then(Value::as_object)
            .map(|files| {
                files
                    .keys()
                    .map(|path| ConversationItem::FileChange {
                        path: path.clone(),
                        ts_ms,
                    })
                    .collect()
            })
            .unwrap_or_default();
    }

    let Some(message) = value.get("message") else {
        return Vec::new();
    };

    let mut items = Vec::new();

    match kind {
        "user" => {
            // A user entry carrying tool_result blocks is the harness returning
            // results, not a person speaking.
            if let Some(blocks) = message.get("content").and_then(Value::as_array) {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let id = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let ok = !block
                        .get("is_error")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    let summary = truncate(
                        &content_text(block.get("content").unwrap_or(&Value::Null)),
                        200,
                    );
                    items.push(ConversationItem::ToolResult {
                        id,
                        ok,
                        summary,
                        ts_ms,
                    });
                }
            }

            let text = content_text(message.get("content").unwrap_or(&Value::Null));
            if !text.trim().is_empty() {
                items.push(ConversationItem::Message {
                    role: Role::User,
                    text,
                    ts_ms,
                    usage: None,
                });
            }
        }

        "assistant" => {
            let usage = parse_usage(message);

            // Whether the agent is done or still mid-tool-loop. `tool_use`
            // means more is coming; `end_turn` and `stop_sequence` mean it has
            // handed control back. Emitted after the message below so the
            // conversation reads in the order it happened.
            let stop_reason = message
                .get("stop_reason")
                .and_then(Value::as_str)
                .map(str::to_string);

            if let Some(blocks) = message.get("content").and_then(Value::as_array) {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("thinking") => {
                            let text = block
                                .get("thinking")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            if !text.trim().is_empty() {
                                items.push(ConversationItem::Thinking { text, ts_ms });
                            }
                        }
                        Some("tool_use") => {
                            let name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string();
                            let empty = Value::Null;
                            let summary = tool_summary(&name, block.get("input").unwrap_or(&empty));
                            items.push(ConversationItem::ToolCall {
                                id: block
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .to_string(),
                                name,
                                summary,
                                ts_ms,
                            });
                        }
                        _ => {}
                    }
                }
            }

            let text = content_text(message.get("content").unwrap_or(&Value::Null));
            if !text.trim().is_empty() {
                items.push(ConversationItem::Message {
                    role: Role::Assistant,
                    text,
                    ts_ms,
                    // Attach usage to the spoken turn, so the UI has one place
                    // to show what it cost.
                    usage,
                });
            } else if let Some(usage) = usage {
                // A tool-only turn still consumed tokens; keep them by hanging
                // them on the last tool call rather than losing them.
                if let Some(ConversationItem::ToolCall { ts_ms, .. }) = items.last() {
                    let ts = *ts_ms;
                    items.push(ConversationItem::Message {
                        role: Role::Assistant,
                        text: String::new(),
                        ts_ms: ts,
                        usage: Some(usage),
                    });
                }
            }

            // Last, so the conversation reads in the order it happened: the
            // agent speaks, then hands control back.
            //
            // `tool_use` is deliberately excluded — it means the agent is still
            // working and will be back. Treating it as the end of a turn would
            // make an autopilot interrupt an agent mid-tool-loop.
            if let Some(reason) = stop_reason {
                if reason != "tool_use" {
                    items.push(ConversationItem::TurnEnded { reason, ts_ms });
                }
            }
        }

        _ => {}
    }

    items
}

/// Parse a whole transcript.
pub fn parse_transcript(text: &str) -> Vec<ConversationItem> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .flat_map(parse_line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines copied verbatim from a transcript this machine produced, so the
    /// parser is tested against the provider's real output rather than against
    /// a shape invented to match the parser (§80).
    const REAL_USER: &str = r#"{"parentUuid":"da383d5e","isSidechain":false,"promptId":"ac3e4142","type":"user","message":{"role":"user","content":"Reply with exactly: CORRELATION_OK"},"uuid":"2c568e62","timestamp":"2026-08-22T21:57:01.375Z","userType":"external","cwd":"C:\\tmp","sessionId":"a9383893","version":"2.1.240"}"#;

    const REAL_ASSISTANT: &str = r#"{"parentUuid":"7978f792","isSidechain":false,"message":{"model":"claude-opus-5","id":"msg_011","type":"message","role":"assistant","content":[{"type":"text","text":"CORRELATION_OK"}],"stop_reason":"end_turn","usage":{"input_tokens":2,"cache_creation_input_tokens":13653,"cache_read_input_tokens":14134,"output_tokens":11}},"type":"assistant","uuid":"d3ce8f9a","timestamp":"2026-08-22T21:57:03.100Z"}"#;

    const REAL_ATTACHMENT: &str = r#"{"parentUuid":null,"isSidechain":false,"attachment":{"type":"hook_success","hookName":"SessionStart:startup","content":"\u001b[?9001h noise"},"type":"attachment","uuid":"e4b4b2e3","timestamp":"2026-08-22T21:57:01.375Z"}"#;

    const REAL_QUEUE_OP: &str = r#"{"type":"queue-operation","operation":"enqueue","timestamp":"2026-08-22T21:57:01.355Z","sessionId":"a9383893","content":"Reply with exactly: CORRELATION_OK"}"#;

    #[test]
    fn parses_a_real_user_turn() {
        let items = parse_line(REAL_USER);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ConversationItem::Message {
                role, text, ts_ms, ..
            } => {
                assert_eq!(*role, Role::User);
                assert_eq!(text, "Reply with exactly: CORRELATION_OK");
                assert!(*ts_ms > 0);
            }
            other => panic!("expected a user message, got {other:?}"),
        }
    }

    /// A snapshot keeps its timestamp inside `snapshot`, unlike every other
    /// entry, so reading only the top level stamps the change at the epoch and
    /// drops it at the start of the timeline (§39). Found by parsing every
    /// transcript on this machine.
    #[test]
    fn a_file_history_snapshot_takes_its_timestamp_from_inside_itself() {
        let line = r#"{"type":"file-history-snapshot","messageId":"m1","snapshot":{"messageId":"m1","timestamp":"2026-08-22T21:57:03.100Z","trackedFileBackups":{"src/logic.ts":{}}},"isSnapshotUpdate":false}"#;
        let items = parse_line(line);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ConversationItem::FileChange { path, ts_ms } => {
                assert_eq!(path, "src/logic.ts");
                assert!(
                    *ts_ms > 1_500_000_000_000,
                    "a file change stamped 0 lands at the epoch in the timeline"
                );
            }
            other => panic!("expected a file change, got {other:?}"),
        }
    }

    #[test]
    fn a_finished_turn_is_reported_and_a_tool_loop_is_not() {
        // The autopilot's entire stopping condition (§32). `tool_use` means the
        // agent is still working; reporting it as finished would have an
        // autopilot interrupt an agent mid-tool-loop.
        let ended = parse_line(REAL_ASSISTANT);
        assert!(
            ended.iter().any(
                |i| matches!(i, ConversationItem::TurnEnded { reason, .. } if reason == "end_turn")
            ),
            "a turn that ended must say so"
        );

        let mid_loop =
            REAL_ASSISTANT.replace(r#""stop_reason":"end_turn""#, r#""stop_reason":"tool_use""#);
        assert!(
            !parse_line(&mid_loop)
                .iter()
                .any(|i| matches!(i, ConversationItem::TurnEnded { .. })),
            "an agent still running tools has not finished its turn"
        );
    }

    #[test]
    fn parses_a_real_assistant_turn_with_official_usage() {
        let items = parse_line(REAL_ASSISTANT);
        // The spoken turn, then the turn boundary the provider reported.
        assert_eq!(items.len(), 2);
        match &items[0] {
            ConversationItem::Message {
                role, text, usage, ..
            } => {
                assert_eq!(*role, Role::Assistant);
                assert_eq!(text, "CORRELATION_OK");
                let usage = usage.as_ref().expect("assistant turns report usage");
                assert_eq!(usage.input, Some(2));
                assert_eq!(usage.output, Some(11));
                assert_eq!(usage.cache_write, Some(13653));
                assert_eq!(usage.cache_read, Some(14134));
                assert_eq!(usage.model.as_deref(), Some("claude-opus-5"));
            }
            other => panic!("expected an assistant message, got {other:?}"),
        }
    }

    /// The signal-to-noise ratio is the whole point of Conversation View.
    #[test]
    fn drops_transcript_machinery() {
        assert!(
            parse_line(REAL_ATTACHMENT).is_empty(),
            "hook output is not conversation"
        );
        assert!(
            parse_line(REAL_QUEUE_OP).is_empty(),
            "queue bookkeeping is not conversation"
        );
    }

    #[test]
    fn drops_harness_injected_user_turns() {
        let meta = r#"{"type":"user","isMeta":true,"message":{"role":"user","content":"<local-command-caveat>…"},"timestamp":"2026-08-22T21:57:01.375Z"}"#;
        assert!(
            parse_line(meta).is_empty(),
            "isMeta turns were not typed by a person"
        );
    }

    #[test]
    fn keeps_subagent_traffic_out_of_the_main_thread() {
        let side = r#"{"type":"assistant","isSidechain":true,"message":{"role":"assistant","content":[{"type":"text","text":"inner"}]},"timestamp":"2026-08-22T21:57:01.375Z"}"#;
        assert!(parse_line(side).is_empty());
    }

    #[test]
    fn parses_tool_calls_into_readable_summaries() {
        let line = r#"{"type":"assistant","isSidechain":false,"message":{"role":"assistant","model":"claude-opus-5","content":[{"type":"tool_use","id":"tu_1","name":"Bash","input":{"command":"git status --porcelain","description":"check"}}]},"timestamp":"2026-08-22T21:57:01.375Z"}"#;
        let items = parse_line(line);
        let call = items
            .iter()
            .find_map(|i| match i {
                ConversationItem::ToolCall { name, summary, .. } => Some((name, summary)),
                _ => None,
            })
            .expect("a tool call");
        assert_eq!(call.0, "Bash");
        assert_eq!(call.1, "git status --porcelain");
    }

    #[test]
    fn tool_results_report_success_and_failure() {
        let ok = r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_1","content":"done"}]},"timestamp":"2026-08-22T21:57:01.375Z"}"#;
        let failed = r#"{"type":"user","isSidechain":false,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tu_2","is_error":true,"content":"boom"}]},"timestamp":"2026-08-22T21:57:01.375Z"}"#;

        match &parse_line(ok)[0] {
            ConversationItem::ToolResult {
                id, ok, summary, ..
            } => {
                assert_eq!(id, "tu_1");
                assert!(*ok);
                assert_eq!(summary, "done");
            }
            other => panic!("expected a tool result, got {other:?}"),
        }
        match &parse_line(failed)[0] {
            ConversationItem::ToolResult { ok, .. } => assert!(!*ok),
            other => panic!("expected a tool result, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_line_is_skipped_rather_than_fatal() {
        // A transcript is read while it is being written, so a partially
        // flushed final line is normal and must not break the view.
        assert!(parse_line("{\"type\":\"assist").is_empty());
        assert!(parse_line("").is_empty());
    }

    /// Parse every real transcript on this machine.
    ///
    /// Ignored by default because it depends on local state, but run
    /// explicitly it is the check that matters: fixtures prove the parser
    /// handles what I *think* the format is, this proves it handles what the
    /// provider actually writes, across many sessions and versions (§80).
    ///
    ///   cargo test --lib -- --ignored --nocapture
    #[test]
    #[ignore = "reads the local Claude Code transcript store"]
    fn parses_every_local_transcript_without_panicking() {
        let Some(root) = home().map(|h| h.join(".claude").join("projects")) else {
            return;
        };
        let Ok(dirs) = std::fs::read_dir(&root) else {
            return;
        };

        let mut files = 0usize;
        let mut items = 0usize;
        let mut with_usage = 0usize;

        for dir in dirs.flatten() {
            let Ok(entries) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                files += 1;
                let parsed = parse_transcript(&text);
                items += parsed.len();
                with_usage += parsed
                    .iter()
                    .filter(|i| matches!(i, ConversationItem::Message { usage: Some(_), .. }))
                    .count();

                // Timestamps must be real, or the timeline (§39) is meaningless.
                for item in &parsed {
                    assert!(
                        item.ts_ms() > 1_500_000_000_000,
                        "implausible timestamp in {}: {item:?}",
                        path.display()
                    );
                }
            }
        }

        println!("parsed {files} transcripts -> {items} items, {with_usage} carrying usage");
        assert!(files > 0, "expected at least one local transcript");
        assert!(items > 0, "parsed nothing at all from real transcripts");
        assert!(
            with_usage > 0,
            "no official usage recovered from real transcripts"
        );
    }

    /// The autopilot's stopping condition, checked against reality (§32).
    ///
    /// The whole loop turns on `TurnEnded` being recovered from what the
    /// provider actually writes. A fixture proves the parser handles the shape I
    /// believe in; this proves it handles the shape on disk — and that
    /// `tool_use` is never mistaken for a finished turn, which would have an
    /// autopilot interrupt an agent mid-tool-loop.
    ///
    ///   cargo test --lib -- --ignored --nocapture
    #[test]
    #[ignore = "reads the local Claude Code transcript store"]
    fn recovers_turn_boundaries_from_every_local_transcript() {
        let Some(root) = home().map(|h| h.join(".claude").join("projects")) else {
            return;
        };
        let Ok(dirs) = std::fs::read_dir(&root) else {
            return;
        };

        let mut transcripts = 0usize;
        let mut with_a_boundary = 0usize;
        let mut boundaries = 0usize;
        let mut reasons: std::collections::BTreeMap<String, usize> = Default::default();

        for dir in dirs.flatten() {
            let Ok(entries) = std::fs::read_dir(dir.path()) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(&path) else {
                    continue;
                };
                transcripts += 1;

                let found: Vec<String> = parse_transcript(&text)
                    .into_iter()
                    .filter_map(|item| match item {
                        ConversationItem::TurnEnded { reason, .. } => Some(reason),
                        _ => None,
                    })
                    .collect();

                if !found.is_empty() {
                    with_a_boundary += 1;
                }
                boundaries += found.len();
                for reason in found {
                    assert_ne!(
                        reason, "tool_use",
                        "an agent still running tools must never be reported as finished"
                    );
                    *reasons.entry(reason).or_default() += 1;
                }
            }
        }

        println!(
            "{transcripts} transcripts -> {boundaries} turn boundaries in {with_a_boundary}; {reasons:?}"
        );
        assert!(transcripts > 0, "expected at least one local transcript");
        assert!(
            boundaries > 0,
            "no turn boundaries recovered — the autopilot would never act"
        );
        // Most sessions end a turn at some point. A tiny number legitimately do
        // not (killed mid-tool-loop), so this is a majority check, not all.
        assert!(
            with_a_boundary * 2 > transcripts,
            "only {with_a_boundary} of {transcripts} transcripts had a turn boundary"
        );
    }

    #[test]
    fn parses_a_whole_transcript_in_order() {
        let text = format!("{REAL_QUEUE_OP}\n{REAL_ATTACHMENT}\n{REAL_USER}\n{REAL_ASSISTANT}\n");
        let items = parse_transcript(&text);
        // Two real turns, plus the end-of-turn marker that closes the second.
        let spoken: Vec<_> = items
            .iter()
            .filter(|i| !matches!(i, ConversationItem::TurnEnded { .. }))
            .collect();
        assert_eq!(spoken.len(), 2, "only the two real turns survive");
        assert!(items[0].ts_ms() <= items[1].ts_ms());
    }
}

/// Does a briefing passed at launch actually reach an **interactive** agent?
///
/// This test exists because the cheap version of it lies.
/// `--append-system-prompt-file` was first checked with `claude -p`, where it
/// plainly works — and every session J.A.R.V.I.S. starts is an interactive PTY,
/// not a print-mode run. That is the same shape as the Monaco option that
/// exists, type-checks and does nothing: verified in one context, assumed in
/// another.
///
/// So the capability is earned here, through **our own `pty::spawn`**. §26 says
/// capabilities are data; this is where this particular datum comes from.
///
/// Ignored by default: it starts a real agent and spends real tokens. Run with
/// `cargo test -- --ignored briefing`.
#[cfg(test)]
mod briefing_capability {
    use crate::pty::{spawn, PtyEvent, PtyHandle, PtyOptions};
    use std::sync::mpsc::Receiver;
    use std::time::{Duration, Instant};

    /// A word no model could produce by accident, so a pass cannot be a
    /// coincidence and a failure cannot be a paraphrase.
    const CODENAME: &str = "ZARAGOZA-7";

    /// Read until `needle` appears, answering ConPTY's startup cursor query.
    ///
    /// The reply is not optional: with no view attached nothing else answers it
    /// and the stream stalls forever (D6).
    fn drain_until(
        handle: &PtyHandle,
        rx: &Receiver<PtyEvent>,
        needle: &str,
        timeout: Duration,
    ) -> String {
        const DSR_QUERY: &[u8] = b"\x1b[6n";
        let deadline = Instant::now() + timeout;
        let mut out: Vec<u8> = Vec::new();
        let mut answered = false;

        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(PtyEvent::Output(bytes)) => {
                    if !answered && bytes.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY) {
                        let _ = handle.write(b"\x1b[1;1R");
                        answered = true;
                    }
                    out.extend_from_slice(&bytes);
                    if String::from_utf8_lossy(&out).contains(needle) {
                        break;
                    }
                }
                Ok(PtyEvent::Exited(_)) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        String::from_utf8_lossy(&out).to_string()
    }

    /// Type the way `autopilot::driver::send` types, and for the same reasons:
    /// one write loses characters, and a carriage return appended to the text
    /// is swallowed while the line editor is still catching up (D16).
    fn type_and_submit(handle: &PtyHandle, text: &str) {
        for chunk in text.as_bytes().chunks(12) {
            let _ = handle.write(chunk);
            std::thread::sleep(Duration::from_millis(40));
        }
        std::thread::sleep(Duration::from_millis(700));
        let _ = handle.write(b"\r");
    }

    /// Read until any of `needles` appears.
    fn drain_until_any(
        handle: &PtyHandle,
        rx: &Receiver<PtyEvent>,
        needles: &[&str],
        timeout: Duration,
    ) -> String {
        const DSR_QUERY: &[u8] = b"\x1b[6n";
        let deadline = Instant::now() + timeout;
        let mut out: Vec<u8> = Vec::new();
        let mut answered = false;

        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(PtyEvent::Output(bytes)) => {
                    if !answered && bytes.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY) {
                        let _ = handle.write(b"\x1b[1;1R");
                        answered = true;
                    }
                    out.extend_from_slice(&bytes);
                    let seen = String::from_utf8_lossy(&out);
                    if needles.iter().any(|n| seen.contains(n)) {
                        break;
                    }
                }
                Ok(PtyEvent::Exited(_)) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        String::from_utf8_lossy(&out).to_string()
    }

    /// The last of a capture, so a failure message can be read.
    fn tail(text: &str) -> String {
        let cut = text.len().saturating_sub(1500);
        text[cut..].to_string()
    }

    fn ask(mut args: Vec<String>, cwd: &std::path::Path) -> String {
        // Our own id, so the transcript can be found without guessing — the
        // deterministic correlation §26 credits this provider with.
        let session_id = uuid::Uuid::now_v7().to_string();
        args.push("--session-id".into());
        args.push(session_id.clone());

        let (handle, rx) = spawn(PtyOptions {
            program: "claude".into(),
            args,
            cwd: cwd.to_path_buf(),
            cols: 100,
            rows: 30,
            env: vec![],
        })
        .expect("claude must be on PATH for this test");

        // A folder Claude Code has never seen opens with a trust prompt before
        // anything else, and everything typed before it is answered goes into
        // *that* dialog rather than to the agent. Found by running this test in
        // a fresh temporary directory and reading the bytes it captured.
        //
        // Both openings are waited for together, so a session that does not ask
        // does not burn the whole trust-prompt timeout before anything is typed.
        let opening = drain_until_any(
            &handle,
            &rx,
            &["trust this folder", "Welcome back", "Try \""],
            Duration::from_secs(45),
        );
        if opening.contains("trust this folder") {
            std::thread::sleep(Duration::from_millis(500));
            let _ = handle.write(b"\r");
            drain_until_any(
                &handle,
                &rx,
                &["Welcome back", "Try \""],
                Duration::from_secs(30),
            );
        }

        // Let the input line settle before typing into it (D16).
        std::thread::sleep(Duration::from_secs(5));
        type_and_submit(
            &handle,
            "What is the project codename? Reply with the one word only.",
        );

        // Wait for the answer, then stop.
        //
        // **The verdict is not read from these bytes.** An earlier version of
        // this test searched the PTY stream for the codename and reported that
        // the question "never reached the agent" — while the agent was in fact
        // answering. Claude Code's TUI redraws its input line character by
        // character with cursor moves interleaved, so a word the user typed
        // very often never appears as a contiguous run of bytes anywhere in the
        // stream. Scraping it is exactly what D3 rejected for the product, and
        // it is no more sound in a test.
        //
        // So this drain only gives the agent time to reply; `transcript_says`
        // reads the structured JSONL Claude Code writes and that is the answer.
        drain_until(&handle, &rx, CODENAME, Duration::from_secs(180));
        let _ = handle.kill();
        transcript_text(&session_id)
    }

    /// Everything the agent said, read from its own transcript (D3).
    ///
    /// Returns an empty string when the transcript never appeared, which is a
    /// real outcome and is reported as such rather than as a missing briefing.
    fn transcript_text(session_id: &str) -> String {
        // The file is written as the session goes; give it a moment to land.
        for _ in 0..20 {
            if let Some(path) = super::find_transcript(session_id) {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if !text.trim().is_empty() {
                        return text;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        String::new()
    }

    #[test]
    #[ignore = "starts a real Claude Code session and spends tokens"]
    fn a_briefing_file_reaches_an_interactive_session() {
        let dir = tempfile::tempdir().unwrap();
        // The brief lives in **our** directory, never in the user's project —
        // that is the whole point of this delivery route. The working directory
        // is deliberately empty so a pass cannot come from the agent simply
        // reading the file off the disk in front of it.
        let brief_dir = tempfile::tempdir().unwrap();
        let brief = brief_dir.path().join("brief.md");
        std::fs::write(
            &brief,
            format!(
                "The user's project has a codename that only you know: {CODENAME}.\n\
                 If asked what the project codename is, answer with exactly that word.\n"
            ),
        )
        .unwrap();

        let briefed = ask(
            vec![
                "--append-system-prompt-file".into(),
                brief.to_string_lossy().to_string(),
            ],
            dir.path(),
        );

        assert!(
            briefed.contains(CODENAME),
            "a brief passed at launch did not reach an interactive session. It \
             works under `claude -p`; if it does not work here then \
             BriefingSupport::SystemPrompt is not real and the capability model \
             must say so. Got:\n{briefed}"
        );
    }

    /// The control. Without the flag the agent cannot know the word, so a pass
    /// above is the flag working rather than the word arriving another way.
    #[test]
    #[ignore = "starts a real Claude Code session and spends tokens"]
    fn without_the_briefing_the_agent_does_not_know() {
        let dir = tempfile::tempdir().unwrap();
        let plain = ask(vec![], dir.path());
        assert!(
            !plain.contains(CODENAME),
            "the codename reached the agent without any briefing, so the other \
             test proves nothing. Got:\n{plain}"
        );
    }
}

#[cfg(test)]
mod folder_trust {
    use super::*;

    /// Captured from the real `~/.claude.json` on this machine — the shape is
    /// the provider's, not ours, so it is pinned against what it actually
    /// writes rather than what would be convenient.
    const CONFIG: &str = r#"{
      "projects": {
        "C:/Users/Alan Araujo/Projetos/app": { "hasTrustDialogAccepted": true },
        "C:/Users/Alan Araujo": { "hasTrustDialogAccepted": false },
        "C:/Users/Alan Araujo/Projetos/seen": { "projectOnboardingSeenCount": 2 }
      }
    }"#;

    #[test]
    fn a_trusted_folder_is_recognised() {
        assert_eq!(
            trusted_in(CONFIG, Path::new("C:/Users/Alan Araujo/Projetos/app")),
            Some(true)
        );
    }

    #[test]
    fn the_spelling_of_a_path_does_not_change_the_answer() {
        // Windows hands us backslashes; Claude Code stores forward slashes.
        // Comparing the raw strings reports every folder as untrusted, which
        // would block every unattended run on this platform.
        assert_eq!(
            trusted_in(CONFIG, Path::new(r"C:\Users\Alan Araujo\Projetos\app")),
            Some(true)
        );
        assert_eq!(
            trusted_in(CONFIG, Path::new("C:/Users/Alan Araujo/Projetos/app/")),
            Some(true)
        );
        #[cfg(windows)]
        assert_eq!(
            trusted_in(CONFIG, Path::new(r"c:\users\alan araujo\projetos\app")),
            Some(true)
        );
    }

    #[test]
    fn a_folder_that_was_declined_is_not_trusted() {
        assert_eq!(
            trusted_in(CONFIG, Path::new("C:/Users/Alan Araujo")),
            Some(false)
        );
    }

    /// The case that hangs: a folder Claude Code has never opened. A fresh
    /// worktree (§45) is exactly this, and it is the reason the function exists.
    ///
    /// It must answer `Some(false)`, not `None`. Reporting it as unknown was
    /// the first version of this, and it would have let the one case this is
    /// meant to catch walk straight past — the feature would have been an
    /// elaborate no-op on precisely the folders that matter.
    #[test]
    fn a_folder_it_has_never_seen_is_not_trusted() {
        assert_eq!(
            trusted_in(CONFIG, Path::new("C:/Users/Alan Araujo/Projetos/brand-new")),
            Some(false),
            "no entry means Claude Code has never opened it, so it will ask"
        );
    }

    #[test]
    fn an_entry_without_the_flag_has_not_accepted_it() {
        assert_eq!(
            trusted_in(CONFIG, Path::new("C:/Users/Alan Araujo/Projetos/seen")),
            Some(false),
            "having been seen is not the same as having been trusted"
        );
    }

    /// Unknown must never masquerade as untrusted: guessing that way blocks a
    /// run that would have worked (§28).
    ///
    /// Note the difference from the test above, which is the whole point of
    /// this pair: a config we cannot read tells us nothing, while a config we
    /// can read that has no entry for the folder tells us plenty.
    #[test]
    fn an_unreadable_config_is_unknown_rather_than_untrusted() {
        assert_eq!(trusted_in("not json at all", Path::new("C:/x")), None);
        // No `projects` object at all: this is not a config we understand.
        assert_eq!(trusted_in("{}", Path::new("C:/x")), None);
    }
}
