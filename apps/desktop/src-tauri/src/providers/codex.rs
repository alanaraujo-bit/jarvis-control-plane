//! Codex adapter.
//!
//! Verified against Codex CLI 0.147.0, and re-verified on 0.149.0.
//!
//! Codex writes a "rollout" JSONL per session under
//! `~/.codex/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl`, with a
//! strictly ordered envelope: `{timestamp, ordinal, type, payload}`.
//!
//! ## Why correlation differs from Claude Code
//!
//! Codex 0.147.0 has no flag to accept an externally supplied session id, so
//! the transcript path cannot be known in advance. A session is instead
//! identified by finding the rollout whose `session_meta` reports the working
//! directory we launched in and a start time at or after the launch. That is a
//! weaker guarantee than Claude Code's, and the capability model says so rather
//! than hiding it (§26).

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::conversation::{parse_timestamp, truncate, ConversationItem, Role, TokenUsage};
use super::{
    BriefingSupport, ConversationSource, Correlation, GuardrailSupport, Provider,
    ProviderCapabilities, UsageReporting,
};
use crate::session::event::Confidence;

pub struct Codex;

impl Provider for Codex {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: "codex".into(),
            name: "Codex".into(),
            executable: "codex".into(),
            // The honest value. See the module docs.
            correlation: Correlation::FileWatch,
            conversation: ConversationSource::Transcript,
            // Codex reports rate limits directly in `token_count` events.
            usage: UsageReporting::Official,
            images: true,
            resume: true,
            approvals: true,
            // Verified against 0.149.0 by experiment: Codex has PreToolUse
            // hooks with the same wire shape as Claude Code, but will not run
            // one until the person has reviewed and trusted it in its own
            // interface. Written on session start so it is there to be
            // trusted; until it is, this session is observed, not guarded.
            guardrails: GuardrailSupport::PreExecutionWhenTrusted,
            worktrees: true,
            // No out-of-band flag exists on 0.147.0.
            briefing: BriefingSupport::OpeningMessage,
            account_switching: true,
        }
    }

    fn launch_args(&self, _session_id: &str) -> Vec<String> {
        // Deliberately empty: this provider cannot be told our session id, and
        // inventing a flag would break the launch outright.
        Vec::new()
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

pub fn sessions_root() -> Option<PathBuf> {
    Some(home()?.join(".codex").join("sessions"))
}

/// Collect rollout files modified at or after `since_ms`.
///
/// The tree is `sessions/YYYY/MM/DD`, so this walks three levels rather than
/// recursing arbitrarily deep.
pub fn recent_rollouts(since_ms: i64) -> Vec<PathBuf> {
    let Some(root) = sessions_root() else {
        return Vec::new();
    };
    recent_rollouts_in(&root, since_ms)
}

/// Collect rollout files from one account's resolved sessions root.
pub fn recent_rollouts_in(root: &Path, since_ms: i64) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk(root, 0, since_ms, &mut found);
    found
}

fn walk(dir: &Path, depth: usize, since_ms: i64, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };

        if kind.is_dir() {
            if depth < 3 {
                walk(&path, depth + 1, since_ms, out);
            }
            continue;
        }

        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let recent = entry
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64 >= since_ms)
            .unwrap_or(false);
        if recent {
            out.push(path);
        }
    }
}

/// The working directory a rollout reports, from its `session_meta` line.
pub fn rollout_cwd(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let first = text.lines().find(|l| !l.trim().is_empty())?;
    let value: Value = serde_json::from_str(first).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    value
        .get("payload")?
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Identify the rollout belonging to a session we launched.
///
/// Matching is on working directory plus start time. If several match, the
/// oldest at or after the launch wins, since that is the one we caused.
pub fn correlate(cwd: &str, launched_at_ms: i64) -> Option<PathBuf> {
    let root = sessions_root()?;
    correlate_in(&root, cwd, launched_at_ms)
}

/// Correlate only within the account that launched the process.
pub fn correlate_in(root: &Path, cwd: &str, launched_at_ms: i64) -> Option<PathBuf> {
    let mut candidates: Vec<_> = recent_rollouts_in(root, launched_at_ms - 2_000)
        .into_iter()
        .filter(|path| {
            rollout_cwd(path)
                .map(|found| paths_equal(&found, cwd))
                .unwrap_or(false)
        })
        .collect();

    candidates.sort();
    candidates.into_iter().next()
}

/// Compare paths the way Windows does: case-insensitively, ignoring separator
/// style and a trailing slash.
fn paths_equal(a: &str, b: &str) -> bool {
    let norm = |p: &str| p.replace('/', "\\").trim_end_matches('\\').to_lowercase();
    norm(a) == norm(b)
}

fn item_text(item: &Value) -> String {
    item.get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// Convert one rollout line into conversation items.
pub fn parse_line(line: &str) -> Vec<ConversationItem> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };

    let ts_ms = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)
        .unwrap_or(0);

    // Only `event_msg` carries reader-facing content. `response_item`,
    // `world_state` and `turn_context` are the model's own context plumbing —
    // including the full system prompt, which must never surface as a message.
    if value.get("type").and_then(Value::as_str) != Some("event_msg") {
        return Vec::new();
    }
    let Some(payload) = value.get("payload") else {
        return Vec::new();
    };

    match payload.get("type").and_then(Value::as_str) {
        Some("item_completed") => {
            let Some(item) = payload.get("item") else {
                return Vec::new();
            };
            match item.get("type").and_then(Value::as_str) {
                Some("UserMessage") => {
                    let text = item_text(item);
                    if text.trim().is_empty() {
                        return Vec::new();
                    }
                    vec![ConversationItem::Message {
                        role: Role::User,
                        // Codex prefixes queued input with a marker glyph.
                        text: text.trim_start_matches('❯').trim().to_string(),
                        ts_ms,
                        usage: None,
                    }]
                }
                Some("AgentMessage" | "AssistantMessage") => {
                    let text = item_text(item);
                    if text.trim().is_empty() {
                        return Vec::new();
                    }
                    vec![ConversationItem::Message {
                        role: Role::Assistant,
                        text,
                        ts_ms,
                        usage: None,
                    }]
                }
                Some("CommandExecution") => {
                    let command = item
                        .get("command")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    vec![ConversationItem::ToolCall {
                        id: item
                            .get("id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        name: "Shell".into(),
                        summary: truncate(command, 120),
                        ts_ms,
                    }]
                }
                _ => Vec::new(),
            }
        }

        Some("token_count") => {
            let info = payload.get("info");
            let get = |key: &str| info.and_then(|i| i.get(key)).and_then(Value::as_u64);

            // Codex reports quota consumption alongside token counts. Reading
            // only the tokens would discard the half of §28 that answers
            // "how close am I to the limit, and when does it reset?".
            let limits = payload.get("rate_limits");
            // `usage_samples` has one legacy summary slot. Keep the most
            // constraining window there; §66 records both windows separately
            // in `account_limit_events`, so the Accounts surface loses none.
            let window = limits.and_then(|limits| {
                ["primary", "secondary"]
                    .into_iter()
                    .filter_map(|key| limits.get(key).filter(|value| !value.is_null()))
                    .max_by(|a, b| {
                        a.get("used_percent")
                            .and_then(Value::as_f64)
                            .unwrap_or_default()
                            .total_cmp(
                                &b.get("used_percent")
                                    .and_then(Value::as_f64)
                                    .unwrap_or_default(),
                            )
                    })
            });

            let limit_percent = window
                .and_then(|w| w.get("used_percent"))
                .and_then(Value::as_f64);
            let limit_resets_at = window.and_then(|window| {
                window
                    .get("resets_at")
                    .and_then(Value::as_i64)
                    .map(|seconds| seconds * 1_000)
                    .or_else(|| {
                        window
                            .get("resets_in_seconds")
                            .and_then(Value::as_i64)
                            .map(|seconds| ts_ms + seconds * 1_000)
                    })
            });

            let usage = TokenUsage {
                input: get("input_tokens"),
                output: get("output_tokens"),
                cache_read: get("cached_input_tokens"),
                cache_write: None,
                cost_usd: None,
                model: None,
                // Codex states these itself, same as Claude Code.
                confidence: Confidence::Official,
                limit_percent,
                limit_resets_at,
            };
            if usage.is_empty() {
                return Vec::new();
            }
            vec![ConversationItem::Message {
                role: Role::Assistant,
                text: String::new(),
                ts_ms,
                usage: Some(usage),
            }]
        }

        // Codex's equivalent of Claude Code's `end_turn`: the turn is over and
        // it is waiting for input (§32). An error ends the turn too, so both
        // are reported — the error first, because it explains the ending.
        Some("task_complete") => {
            let mut items = Vec::new();
            if let Some(message) = payload
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
            {
                items.push(ConversationItem::Error {
                    message: message.to_string(),
                    ts_ms,
                });
            }
            items.push(ConversationItem::TurnEnded {
                reason: "taskComplete".into(),
                ts_ms,
            });
            items
        }

        _ => Vec::new(),
    }
}

pub fn parse_rollout(text: &str) -> Vec<ConversationItem> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .flat_map(parse_line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines shaped exactly like the rollouts on this machine.
    const REAL_USER: &str = r#"{"timestamp":"2026-08-19T02:06:16.019Z","ordinal":9,"type":"event_msg","payload":{"type":"item_completed","thread_id":"01a017c4","turn_id":"01a017c4","item":{"type":"UserMessage","id":"01a017c4","content":[{"type":"text","text":"❯ build me a thing"}]}}}"#;

    const REAL_META: &str = r#"{"timestamp":"2026-08-19T02:06:15.415Z","ordinal":0,"type":"session_meta","payload":{"session_id":"01a017c4","cwd":"C:\\Users\\Alan Araujo","originator":"codex-tui","cli_version":"0.147.0"}}"#;

    const REAL_ERROR: &str = r#"{"timestamp":"2026-08-19T02:06:16.945Z","ordinal":11,"type":"event_msg","payload":{"type":"task_complete","turn_id":"01a017c4","last_agent_message":null,"error":{"message":"You've hit your usage limit.","codex_error_info":"usage_limit_exceeded"}}}"#;

    const REAL_RESPONSE_ITEM: &str = r#"{"timestamp":"2026-08-19T02:06:15.944Z","ordinal":2,"type":"response_item","payload":{"type":"message","id":"msg_01","role":"developer","content":[{"type":"input_text","text":"<skills_instructions>secret system prompt</skills_instructions>"}]}}"#;

    const REAL_LIMITS: &str = r#"{"timestamp":"2026-08-19T02:06:16.019Z","type":"event_msg","payload":{"type":"token_count","info":{"input_tokens":120,"output_tokens":30,"cached_input_tokens":20},"rate_limits":{"limit_id":"codex","primary":{"used_percent":12.5,"window_minutes":10080,"resets_at":1788050255},"secondary":{"used_percent":81.0,"window_minutes":300,"resets_at":1787540000},"credits":{"has_credits":false,"unlimited":false,"balance":"0"}}}}"#;

    #[test]
    fn parses_a_real_user_turn_and_strips_the_prompt_glyph() {
        let items = parse_line(REAL_USER);
        assert_eq!(items.len(), 1);
        match &items[0] {
            ConversationItem::Message { role, text, .. } => {
                assert_eq!(*role, Role::User);
                assert_eq!(text, "build me a thing");
            }
            other => panic!("expected a user message, got {other:?}"),
        }
    }

    /// `response_item` carries the system prompt and developer instructions.
    /// Surfacing it would leak the provider's internals into the user's
    /// conversation, so it must be dropped.
    #[test]
    fn never_surfaces_model_context_plumbing() {
        assert!(parse_line(REAL_RESPONSE_ITEM).is_empty());
        assert!(parse_line(REAL_META).is_empty());
    }

    #[test]
    fn surfaces_provider_errors() {
        match &parse_line(REAL_ERROR)[0] {
            ConversationItem::Error { message, .. } => {
                assert!(message.contains("usage limit"));
            }
            other => panic!("expected an error item, got {other:?}"),
        }
    }

    #[test]
    fn reads_absolute_reset_time_from_the_binding_codex_window() {
        let items = parse_line(REAL_LIMITS);
        match &items[0] {
            ConversationItem::Message {
                usage: Some(usage), ..
            } => {
                assert_eq!(usage.limit_percent, Some(81.0));
                assert_eq!(usage.limit_resets_at, Some(1_787_540_000_000));
            }
            other => panic!("expected usage, got {other:?}"),
        }
    }

    #[test]
    fn a_malformed_line_is_skipped() {
        assert!(parse_line("{\"timestamp\":").is_empty());
        assert!(parse_line("").is_empty());
    }

    #[test]
    fn compares_windows_paths_forgivingly() {
        assert!(paths_equal(r"C:\Users\Alan", r"c:\users\alan"));
        assert!(paths_equal(r"C:\Users\Alan\", r"C:/Users/Alan"));
        assert!(!paths_equal(r"C:\Users\Alan", r"C:\Users\Bruno"));
    }

    #[test]
    fn reads_the_working_directory_from_a_rollout() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout-test.jsonl");
        std::fs::write(&path, format!("{REAL_META}\n{REAL_USER}\n")).unwrap();
        assert_eq!(rollout_cwd(&path).as_deref(), Some(r"C:\Users\Alan Araujo"));
    }

    #[test]
    fn ignores_a_file_that_does_not_start_with_session_meta() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not-a-rollout.jsonl");
        std::fs::write(&path, format!("{REAL_USER}\n")).unwrap();
        assert!(rollout_cwd(&path).is_none());
    }
}
