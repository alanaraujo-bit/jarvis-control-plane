//! Claude Code's stream-json protocol (M29).
//!
//! Every shape here was measured against Claude Code 2.1.302 on 2026-10-18 in
//! a scratch repository (`docs/M29-CHAT.md` §2):
//!
//! * one process for the whole conversation — a `{"type":"user"}` line is a
//!   turn, and a `{"type":"result"}` line ends it;
//! * `--permission-prompt-tool stdio` routes every permission request and every
//!   `AskUserQuestion` to us as a `can_use_tool` control request. Without it
//!   each one is denied on the spot and the agent says it needs approval;
//! * `set_model` and `apply_flag_settings {effortLevel}` change the next turn,
//!   and `get_settings` states what is actually applied;
//! * the transcript is written as in an interactive session, so the
//!   conversation itself reaches the log through `session::transcript`.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Value};

use super::model::{
    asked, first_text, interrupted, resolved, settings_logged, ChatAction, ChatAnswer, ChatEvent,
    ChatModel, ChatOption, ChatQuestion, ChatRequest, Driver, Effect, RequestKind,
};
use crate::session::event::SessionState;

/// The command line of a chat session. No `--model`, `--effort` or
/// `--permission-mode`: a chat starts where the person's own CLI already is.
pub fn launch_args(
    session_id: &str,
    resume: Option<&str>,
    guardrail_settings: Option<&Path>,
    brief: Option<&Path>,
) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--permission-prompt-tool",
        "stdio",
        "--session-id",
    ]
    .iter()
    .map(|arg| arg.to_string())
    .collect();
    args.push(session_id.to_string());
    // Continuing (D41): `--fork-session` keeps the new transcript named for our
    // id, exactly as for a terminal.
    if let Some(previous) = resume {
        args.extend(["--resume".to_string(), previous.to_string(), "--fork-session".to_string()]);
    }
    if let Some(path) = guardrail_settings {
        args.extend(["--settings".to_string(), path.to_string_lossy().to_string()]);
    }
    if let Some(path) = brief {
        args.extend(["--append-system-prompt-file".to_string(), path.to_string_lossy().to_string()]);
    }
    args
}

/// A request waiting on the person, with what answering it needs.
struct Pending {
    kind: RequestKind,
    input: Value,
    suggestions: Vec<Value>,
}

pub struct ClaudeDriver {
    ready: bool,
    models: Vec<ChatModel>,
    /// `(resolvedModel, value)`, so `get_settings`' resolved id can be named.
    aliases: Vec<(String, String)>,
    model: Option<String>,
    effort: Option<String>,
    /// Messages sent before the CLI answered `initialize`.
    queued: Vec<String>,
    /// Turns sent and not yet ended. Claude queues a message sent mid-turn, and
    /// each one ends with its own `result`.
    turns: u32,
    draft: Option<String>,
    pending: HashMap<String, Pending>,
    next: u64,
}

impl Default for ClaudeDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeDriver {
    pub fn new() -> Self {
        Self {
            ready: false,
            models: Vec::new(),
            aliases: Vec::new(),
            model: None,
            effort: None,
            queued: Vec::new(),
            turns: 0,
            draft: None,
            pending: HashMap::new(),
            next: 0,
        }
    }

    fn control(&mut self, prefix: &str, request: Value) -> Effect {
        self.next += 1;
        let id = format!("{prefix}-{}", self.next);
        Effect::Write(json!({ "type": "control_request", "request_id": id, "request": request }).to_string())
    }

    fn read_settings(&mut self) -> Effect {
        self.control("settings", json!({ "subtype": "get_settings" }))
    }

    fn user_line(text: &str) -> Effect {
        Effect::Write(json!({ "type": "user", "message": { "role": "user", "content": text } }).to_string())
    }

    fn reply(id: &str, response: Value) -> Effect {
        Effect::Write(
            json!({
                "type": "control_response",
                "response": { "subtype": "success", "request_id": id, "response": response }
            })
            .to_string(),
        )
    }

    fn idle_state(&self) -> SessionState {
        if self.pending.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Waiting
        }
    }

    fn on_response(&mut self, response: &Value) -> Vec<Effect> {
        let id = response.get("request_id").and_then(Value::as_str).unwrap_or_default();
        let body = response.get("response").cloned().unwrap_or(Value::Null);
        if id == "init" {
            self.read_models(&body);
            return vec![self.read_settings()];
        }
        if id.starts_with("settings-") {
            let applied = body.get("applied");
            let text = |key: &str| applied.and_then(|a| a.get(key)).and_then(Value::as_str).map(str::to_string);
            self.model = text("model").map(|resolved| self.name_of(&resolved));
            self.effort = text("effort");
            if !self.ready {
                self.ready = true;
                let mut effects = vec![Effect::Emit(ChatEvent::Ready {
                    models: self.models.clone(),
                    model: self.model.clone(),
                    effort: self.effort.clone(),
                })];
                for text in std::mem::take(&mut self.queued) {
                    effects.push(Self::user_line(&text));
                }
                return effects;
            }
            return vec![
                Effect::Emit(ChatEvent::Settings { model: self.model.clone(), effort: self.effort.clone() }),
                settings_logged(self.model.clone(), self.effort.clone()),
            ];
        }
        // A change was accepted or refused; either way the truth is what the
        // CLI says is applied now (§28), never what was asked for.
        if id.starts_with("model-") || id.starts_with("effort-") {
            return vec![self.read_settings()];
        }
        if id.starts_with("interrupt-") && self.turns > 0 {
            // The interrupted turn still ends with its own `result`; what is
            // still queued will run and end with theirs.
            let queued = body.get("still_queued").and_then(Value::as_array).map_or(0, Vec::len);
            self.turns = 1 + queued as u32;
        }
        Vec::new()
    }

    fn read_models(&mut self, body: &Value) {
        let Some(list) = body.get("models").and_then(Value::as_array) else {
            return;
        };
        let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        for entry in list {
            let id = text(entry, "value");
            let resolved = text(entry, "resolvedModel");
            if id.is_empty() {
                continue;
            }
            self.aliases.push((resolved.clone(), id.clone()));
            // "default" is a pure alias of another entry; listing both would
            // offer the person the same model twice under two names.
            let duplicate = id == "default"
                && list.iter().any(|other| text(other, "value") != "default" && text(other, "resolvedModel") == resolved);
            if duplicate {
                continue;
            }
            self.models.push(ChatModel {
                name: entry.get("displayName").and_then(Value::as_str).unwrap_or(&id).to_string(),
                description: text(entry, "description"),
                efforts: entry
                    .get("supportedEffortLevels")
                    .and_then(Value::as_array)
                    .map(|levels| levels.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default(),
                default_effort: None,
                id,
            });
        }
    }

    /// The listed model a resolved id belongs to, preferring a real name over
    /// the `default` alias; the raw id when nothing lists it.
    fn name_of(&self, resolved: &str) -> String {
        self.aliases
            .iter()
            .filter(|(r, _)| r == resolved)
            .map(|(_, value)| value)
            .find(|value| self.models.iter().any(|m| &m.id == *value))
            .cloned()
            .unwrap_or_else(|| resolved.to_string())
    }

    fn on_tool_request(&mut self, id: &str, request: &Value) -> Vec<Effect> {
        let tool = request.get("tool_name").and_then(Value::as_str).unwrap_or_default().to_string();
        let input = request.get("input").cloned().unwrap_or(Value::Null);
        let suggestions: Vec<Value> = request
            .get("permission_suggestions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let (kind, action, detail, questions) = if tool == "AskUserQuestion" {
            (RequestKind::Question, ChatAction::Question, String::new(), read_questions(&input))
        } else {
            let (action, keys): (ChatAction, &[&str]) = match tool.as_str() {
                "Bash" | "PowerShell" => (ChatAction::Command, &["command"]),
                "Write" => (ChatAction::Create, &["file_path"]),
                "Edit" | "MultiEdit" | "NotebookEdit" => (ChatAction::Edit, &["file_path", "notebook_path"]),
                "WebFetch" => (ChatAction::Web, &["url"]),
                "WebSearch" => (ChatAction::Web, &["query"]),
                _ => (ChatAction::Other, &["description"]),
            };
            (RequestKind::Permission, action, first_text(&input, keys), Vec::new())
        };

        let chat_request = ChatRequest {
            id: id.to_string(),
            kind,
            tool,
            action,
            detail,
            can_always: !suggestions.is_empty(),
            questions,
        };
        self.pending.insert(id.to_string(), Pending { kind, input, suggestions });
        vec![
            asked(&chat_request),
            Effect::Emit(ChatEvent::Request { request: chat_request }),
            Effect::State(SessionState::Waiting),
        ]
    }

    fn on_stream_event(&mut self, event: &Value) -> Vec<Effect> {
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let is_text = event.get("content_block").and_then(|b| b.get("type")).and_then(Value::as_str) == Some("text");
                if is_text {
                    self.draft = Some(String::new());
                }
                Vec::new()
            }
            Some("content_block_delta") => {
                let delta = event.get("delta");
                if delta.and_then(|d| d.get("type")).and_then(Value::as_str) != Some("text_delta") {
                    return Vec::new();
                }
                let piece = delta.and_then(|d| d.get("text")).and_then(Value::as_str).unwrap_or_default();
                let draft = self.draft.get_or_insert_with(String::new);
                draft.push_str(piece);
                vec![Effect::Emit(ChatEvent::Draft { text: draft.clone() })]
            }
            Some("content_block_stop") | Some("message_stop") => self.close_draft(),
            _ => Vec::new(),
        }
    }

    fn close_draft(&mut self) -> Vec<Effect> {
        match self.draft.take() {
            Some(_) => vec![Effect::Emit(ChatEvent::DraftDone)],
            None => Vec::new(),
        }
    }
}

fn read_questions(input: &Value) -> Vec<ChatQuestion> {
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    input
        .get("questions")
        .and_then(Value::as_array)
        .map(|questions| {
            questions
                .iter()
                .map(|q| ChatQuestion {
                    // Claude takes the answer back keyed by the question text.
                    id: text(q, "question"),
                    question: text(q, "question"),
                    header: text(q, "header"),
                    options: q
                        .get("options")
                        .and_then(Value::as_array)
                        .map(|options| {
                            options
                                .iter()
                                .map(|o| ChatOption { label: text(o, "label"), description: text(o, "description") })
                                .collect()
                        })
                        .unwrap_or_default(),
                    multi_select: q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Driver for ClaudeDriver {
    fn start(&mut self) -> Vec<Effect> {
        vec![Effect::Write(
            json!({ "type": "control_request", "request_id": "init", "request": { "subtype": "initialize" } }).to_string(),
        )]
    }

    fn on_line(&mut self, line: &str) -> Vec<Effect> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        match value.get("type").and_then(Value::as_str) {
            Some("control_response") => value.get("response").map(|r| self.on_response(r)).unwrap_or_default(),
            Some("control_request") => {
                let id = value.get("request_id").and_then(Value::as_str).unwrap_or_default().to_string();
                let request = value.get("request").cloned().unwrap_or(Value::Null);
                if request.get("subtype").and_then(Value::as_str) == Some("can_use_tool") {
                    return self.on_tool_request(&id, &request);
                }
                // Anything else would leave the CLI waiting forever on an
                // answer nobody is going to give.
                vec![Effect::Write(
                    json!({
                        "type": "control_response",
                        "response": { "subtype": "error", "request_id": id, "error": "unsupported by J.A.R.V.I.S." }
                    })
                    .to_string(),
                )]
            }
            // The CLI withdrew a request (an interrupt, a turn that moved on).
            Some("control_cancel_request") => {
                let id = value.get("request_id").and_then(Value::as_str).unwrap_or_default();
                if self.pending.remove(id).is_none() {
                    return Vec::new();
                }
                vec![
                    resolved(id, "cancelled"),
                    Effect::Emit(ChatEvent::Resolved { id: id.to_string() }),
                    Effect::State(if self.turns > 0 { SessionState::Working } else { self.idle_state() }),
                ]
            }
            Some("stream_event") => {
                // A subagent's text is its own thread, not the reply.
                if value.get("parent_tool_use_id").is_some_and(|p| !p.is_null()) {
                    return Vec::new();
                }
                value.get("event").map(|e| self.on_stream_event(e)).unwrap_or_default()
            }
            Some("result") => {
                let mut effects = self.close_draft();
                // A message sent mid-turn is usually folded into that turn —
                // measured on 2.1.302: two messages, one `result` — so the count
                // of what is still to run comes from the CLI, not from ours.
                self.turns = match value.get("queued_turn_count").and_then(Value::as_u64) {
                    Some(queued) => queued as u32,
                    None => self.turns.saturating_sub(1),
                };
                if self.turns == 0 {
                    effects.push(Effect::Emit(ChatEvent::Busy { busy: false }));
                    effects.push(Effect::State(self.idle_state()));
                }
                effects
            }
            _ => Vec::new(),
        }
    }

    fn send(&mut self, text: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.turns == 0 {
            effects.push(Effect::Emit(ChatEvent::Busy { busy: true }));
            effects.push(Effect::State(SessionState::Working));
        }
        self.turns += 1;
        if self.ready {
            effects.push(Self::user_line(text));
        } else {
            self.queued.push(text.to_string());
        }
        effects
    }

    fn answer(&mut self, id: &str, answer: ChatAnswer) -> Result<Vec<Effect>, String> {
        let pending = self.pending.remove(id).ok_or_else(|| "chat.requestGone".to_string())?;
        let (response, decision) = match (pending.kind, answer) {
            (_, ChatAnswer::Deny) => (json!({ "behavior": "deny", "message": "The person declined this request." }), "deny"),
            (RequestKind::Permission, ChatAnswer::Allow { always }) => {
                let mut response = json!({ "behavior": "allow", "updatedInput": pending.input });
                if always && !pending.suggestions.is_empty() {
                    // Scoped to this conversation, always. Claude suggests
                    // `localSettings` for a command, which would write a rule
                    // into the person's repository (D23).
                    let scoped: Vec<Value> = pending
                        .suggestions
                        .into_iter()
                        .map(|mut s| {
                            s["destination"] = json!("session");
                            s
                        })
                        .collect();
                    response["updatedPermissions"] = Value::Array(scoped);
                }
                (response, if always { "allowAlways" } else { "allow" })
            }
            (RequestKind::Question, ChatAnswer::Answers { answers }) => {
                let mut input = pending.input;
                let joined: serde_json::Map<String, Value> =
                    answers.into_iter().map(|(question, chosen)| (question, json!(chosen.join(", ")))).collect();
                input["answers"] = Value::Object(joined);
                (json!({ "behavior": "allow", "updatedInput": input }), "answered")
            }
            (kind, _) => {
                // Put it back: the request is still there to be answered right.
                let restored = Pending { kind, input: pending.input, suggestions: pending.suggestions };
                self.pending.insert(id.to_string(), restored);
                return Err("chat.wrongAnswer".to_string());
            }
        };
        let state = if self.pending.is_empty() { SessionState::Working } else { SessionState::Waiting };
        Ok(vec![
            Self::reply(id, response),
            resolved(id, decision),
            Effect::Emit(ChatEvent::Resolved { id: id.to_string() }),
            Effect::State(state),
        ])
    }

    fn set_model(&mut self, model: &str) -> Vec<Effect> {
        vec![self.control("model", json!({ "subtype": "set_model", "model": model }))]
    }

    fn set_effort(&mut self, effort: &str) -> Vec<Effect> {
        vec![self.control("effort", json!({ "subtype": "apply_flag_settings", "settings": { "effortLevel": effort } }))]
    }

    fn interrupt(&mut self) -> Vec<Effect> {
        if self.turns == 0 {
            return Vec::new();
        }
        vec![self.control("interrupt", json!({ "subtype": "interrupt" })), interrupted()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::event::EventKind;

    /// Trimmed from the real `initialize` answer of 2.1.302 on this machine.
    const INIT: &str = r#"{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{"commands":[],"models":[{"value":"default","resolvedModel":"claude-opus-5-5","displayName":"Default (recommended)","description":"Opus 5.5 · Best for everyday, complex tasks","supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"opus","resolvedModel":"claude-opus-5-5","displayName":"Opus 5.5","description":"For complex work and everyday tasks","supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"sonnet","resolvedModel":"claude-sonnet-5-5","displayName":"Sonnet 5.5","description":"Most efficient for simpler tasks","supportedEffortLevels":["low","medium","high","xhigh","max"]},{"value":"claude-haiku-4-5-20251001","resolvedModel":"claude-haiku-4-5-20251001","displayName":"Haiku 4.5","description":"Fastest for quick answers"}],"account":{"subscriptionType":"Claude Max"}}}}"#;
    const WRITE_REQUEST: &str = r#"{"type":"control_request","request_id":"1a27aad6","request":{"subtype":"can_use_tool","tool_name":"Write","display_name":"Write","input":{"file_path":"C:\\scratch\\chat-scratch\\hello.txt","content":"hi"},"description":"hello.txt","permission_suggestions":[{"type":"setMode","mode":"acceptEdits","destination":"session"}],"tool_use_id":"toolu_013A"}}"#;
    const BASH_REQUEST: &str = r#"{"type":"control_request","request_id":"ccc97ffa","request":{"subtype":"can_use_tool","tool_name":"Bash","display_name":"Bash","input":{"command":"node -e \"setTimeout(()=>{},60000)\"","timeout":90000,"description":"Run a 60-second node timer"},"description":"Run a 60-second node timer","permission_suggestions":[{"type":"addRules","rules":[{"toolName":"Bash","ruleContent":"node -e \"setTimeout(()=>{},60000)\""}],"behavior":"allow","destination":"localSettings"}],"decision_reason":"This command requires approval","tool_use_id":"toolu_01GP"}}"#;
    const QUESTION_REQUEST: &str = r#"{"type":"control_request","request_id":"70f3c343","request":{"subtype":"can_use_tool","tool_name":"AskUserQuestion","display_name":"AskUserQuestion","input":{"questions":[{"question":"Do you prefer red or blue?","header":"Color","options":[{"label":"Red","description":"Choose red"},{"label":"Blue","description":"Choose blue"}],"multiSelect":false}]},"tool_use_id":"toolu_019k","requires_user_interaction":true}}"#;
    const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"Blue.","session_id":"0f9f1a06","queued_turn_count":0}"#;
    const RESULT_MORE_QUEUED: &str = r#"{"type":"result","subtype":"success","is_error":false,"result":"one","session_id":"0f9f1a06","queued_turn_count":1}"#;

    fn settings(id: &str, model: &str, effort: &str) -> String {
        format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{id}","response":{{"applied":{{"model":"{model}","effort":"{effort}"}}}}}}}}"#
        )
    }

    fn writes(effects: &[Effect]) -> Vec<Value> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Write(line) => serde_json::from_str(line).ok(),
                _ => None,
            })
            .collect()
    }

    fn emitted(effects: &[Effect]) -> Vec<ChatEvent> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Emit(event) => Some(event.clone()),
                _ => None,
            })
            .collect()
    }

    /// A driver past `initialize` and `get_settings`, as the real CLI answers.
    fn ready() -> ClaudeDriver {
        let mut driver = ClaudeDriver::new();
        driver.start();
        let asked = writes(&driver.on_line(INIT));
        let id = asked[0]["request_id"].as_str().unwrap().to_string();
        driver.on_line(&settings(&id, "claude-opus-5-5", "xhigh"));
        driver
    }

    #[test]
    fn the_command_line_routes_permissions_to_us_and_sets_nothing_of_its_own() {
        let args = launch_args("sid", None, None, None);
        assert!(args.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]));
        assert!(args.windows(2).any(|w| w == ["--session-id", "sid"]));
        assert!(args.iter().any(|a| a == "--include-partial-messages"));
        for absent in ["--model", "--effort", "--permission-mode", "--dangerously-skip-permissions"] {
            assert!(!args.iter().any(|a| a == absent), "{absent} must not be set: {args:?}");
        }
        let resumed = launch_args("sid", Some("old"), None, None);
        assert!(resumed.windows(2).any(|w| w == ["--resume", "old"]));
        assert!(resumed.iter().any(|a| a == "--fork-session"));
    }

    #[test]
    fn it_is_ready_once_the_cli_says_what_is_applied() {
        let mut driver = ClaudeDriver::new();
        let start = writes(&driver.start());
        assert_eq!(start[0]["request"]["subtype"], "initialize");

        let after_init = driver.on_line(INIT);
        let asked = writes(&after_init);
        assert_eq!(asked[0]["request"]["subtype"], "get_settings");

        let id = asked[0]["request_id"].as_str().unwrap();
        match emitted(&driver.on_line(&settings(id, "claude-opus-5-5", "xhigh"))).as_slice() {
            [ChatEvent::Ready { models, model, effort }] => {
                assert_eq!(model.as_deref(), Some("opus"), "named after the listed model, not the alias");
                assert_eq!(effort.as_deref(), Some("xhigh"));
                assert!(!models.iter().any(|m| m.id == "default"), "the alias is not listed twice");
                assert_eq!(models[0].name, "Opus 5.5");
                assert_eq!(models[0].efforts, ["low", "medium", "high", "xhigh", "max"]);
                assert!(models.iter().any(|m| m.id == "claude-haiku-4-5-20251001" && m.efforts.is_empty()));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn a_message_sent_before_ready_waits_for_it() {
        let mut driver = ClaudeDriver::new();
        driver.start();
        let early = driver.send("hello");
        assert!(writes(&early).is_empty(), "nothing is written before initialize answers");
        assert!(emitted(&early).contains(&ChatEvent::Busy { busy: true }));

        let asked = writes(&driver.on_line(INIT));
        let id = asked[0]["request_id"].as_str().unwrap().to_string();
        let flushed = writes(&driver.on_line(&settings(&id, "claude-opus-5-5", "xhigh")));
        assert_eq!(flushed[0]["type"], "user");
        assert_eq!(flushed[0]["message"]["content"], "hello");
    }

    #[test]
    fn two_messages_keep_it_busy_until_both_turns_end() {
        let mut driver = ready();
        let first = driver.send("one");
        assert!(emitted(&first).contains(&ChatEvent::Busy { busy: true }));
        assert_eq!(writes(&first)[0]["message"]["content"], "one");
        let second = driver.send("two");
        assert!(!emitted(&second).contains(&ChatEvent::Busy { busy: true }), "already busy");
        assert_eq!(writes(&second)[0]["message"]["content"], "two");

        assert!(!emitted(&driver.on_line(RESULT_MORE_QUEUED)).contains(&ChatEvent::Busy { busy: false }));
        let last = driver.on_line(RESULT);
        assert!(emitted(&last).contains(&ChatEvent::Busy { busy: false }));
        assert!(last.contains(&Effect::State(SessionState::Idle)));
    }

    /// Measured on 2.1.302: a message sent during a turn joined that turn, and
    /// one `result` ended both. Counting our own sends would have left the
    /// chat "working" forever.
    #[test]
    fn a_message_folded_into_the_running_turn_does_not_keep_it_busy() {
        let mut driver = ready();
        driver.send("create the file");
        driver.send("then say done");
        let end = driver.on_line(RESULT);
        assert!(emitted(&end).contains(&ChatEvent::Busy { busy: false }));
    }

    #[test]
    fn text_streams_as_a_growing_draft() {
        let mut driver = ready();
        driver.on_line(r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}},"parent_tool_use_id":null}"#);
        driver.on_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Bl"}},"parent_tool_use_id":null}"#);
        let grown = driver.on_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ue."}},"parent_tool_use_id":null}"#);
        assert_eq!(emitted(&grown), [ChatEvent::Draft { text: "Blue.".into() }]);
        let done = driver.on_line(r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0},"parent_tool_use_id":null}"#);
        assert_eq!(emitted(&done), [ChatEvent::DraftDone]);

        let sub = driver.on_line(r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"inner"}},"parent_tool_use_id":"toolu_x"}"#);
        assert!(emitted(&sub).is_empty(), "a subagent's text is not the reply");
    }

    #[test]
    fn a_write_becomes_a_permission_card_and_is_logged() {
        let mut driver = ready();
        let effects = driver.on_line(WRITE_REQUEST);
        match emitted(&effects).as_slice() {
            [ChatEvent::Request { request }] => {
                assert_eq!(request.kind, RequestKind::Permission);
                assert_eq!(request.action, ChatAction::Create);
                assert_eq!(request.detail, "C:\\scratch\\chat-scratch\\hello.txt");
                assert!(request.can_always);
            }
            other => panic!("expected one request, got {other:?}"),
        }
        assert!(effects.contains(&Effect::State(SessionState::Waiting)));
        assert!(effects.iter().any(|e| matches!(e, Effect::Log(EventKind::Approval, payload)
            if String::from_utf8_lossy(payload).contains("\"event\":\"asked\""))));
    }

    #[test]
    fn allowing_hands_the_input_back_unchanged() {
        let mut driver = ready();
        driver.on_line(WRITE_REQUEST);
        let effects = driver.answer("1a27aad6", ChatAnswer::Allow { always: false }).unwrap();
        let reply = &writes(&effects)[0];
        assert_eq!(reply["type"], "control_response");
        assert_eq!(reply["response"]["request_id"], "1a27aad6");
        assert_eq!(reply["response"]["response"]["behavior"], "allow");
        assert_eq!(reply["response"]["response"]["updatedInput"]["content"], "hi");
        assert!(reply["response"]["response"].get("updatedPermissions").is_none());
        assert!(emitted(&effects).contains(&ChatEvent::Resolved { id: "1a27aad6".into() }));
    }

    /// The suggestion for a command comes back as `localSettings`, which would
    /// write a rule into the person's repository. It never leaves as that.
    #[test]
    fn always_allowing_is_scoped_to_this_conversation() {
        let mut driver = ready();
        driver.on_line(BASH_REQUEST);
        let effects = driver.answer("ccc97ffa", ChatAnswer::Allow { always: true }).unwrap();
        let permissions = &writes(&effects)[0]["response"]["response"]["updatedPermissions"];
        let list = permissions.as_array().expect("the suggestions are handed back");
        assert!(!list.is_empty());
        assert!(list.iter().all(|p| p["destination"] == "session"), "{permissions}");
        assert_eq!(list[0]["rules"][0]["toolName"], "Bash");
    }

    #[test]
    fn denying_tells_the_agent_plainly() {
        let mut driver = ready();
        driver.on_line(BASH_REQUEST);
        let effects = driver.answer("ccc97ffa", ChatAnswer::Deny).unwrap();
        assert_eq!(writes(&effects)[0]["response"]["response"]["behavior"], "deny");
    }

    #[test]
    fn a_question_is_answered_with_the_chosen_labels() {
        let mut driver = ready();
        let effects = driver.on_line(QUESTION_REQUEST);
        match emitted(&effects).as_slice() {
            [ChatEvent::Request { request }] => {
                assert_eq!(request.kind, RequestKind::Question);
                assert_eq!(request.questions[0].question, "Do you prefer red or blue?");
                assert_eq!(request.questions[0].options[1].label, "Blue");
                assert!(!request.questions[0].multi_select);
            }
            other => panic!("expected a question, got {other:?}"),
        }
        let answers = [("Do you prefer red or blue?".to_string(), vec!["Blue".to_string()])].into_iter().collect();
        let reply = &writes(&driver.answer("70f3c343", ChatAnswer::Answers { answers }).unwrap())[0];
        let input = &reply["response"]["response"]["updatedInput"];
        assert_eq!(input["answers"]["Do you prefer red or blue?"], "Blue");
        assert_eq!(input["questions"][0]["header"], "Color", "the original input travels back with the answers");
    }

    #[test]
    fn an_answer_to_a_request_that_is_gone_is_refused() {
        let mut driver = ready();
        assert_eq!(driver.answer("nope", ChatAnswer::Deny), Err("chat.requestGone".to_string()));
    }

    #[test]
    fn a_withdrawn_request_takes_its_card_away() {
        let mut driver = ready();
        driver.on_line(BASH_REQUEST);
        let effects = driver.on_line(r#"{"type":"control_cancel_request","request_id":"ccc97ffa"}"#);
        assert!(emitted(&effects).contains(&ChatEvent::Resolved { id: "ccc97ffa".into() }));
        assert_eq!(driver.answer("ccc97ffa", ChatAnswer::Deny), Err("chat.requestGone".to_string()));
    }

    #[test]
    fn a_model_change_is_shown_as_the_cli_confirms_it() {
        let mut driver = ready();
        let asked = writes(&driver.set_model("sonnet"));
        assert_eq!(asked[0]["request"], json!({ "subtype": "set_model", "model": "sonnet" }));
        let accepted = format!(
            r#"{{"type":"control_response","response":{{"subtype":"success","request_id":"{}","response":{{}}}}}}"#,
            asked[0]["request_id"].as_str().unwrap()
        );
        let reread = writes(&driver.on_line(&accepted));
        assert_eq!(reread[0]["request"]["subtype"], "get_settings");
        let effects = driver.on_line(&settings(reread[0]["request_id"].as_str().unwrap(), "claude-sonnet-5-5", "medium"));
        assert_eq!(
            emitted(&effects),
            [ChatEvent::Settings { model: Some("sonnet".into()), effort: Some("medium".into()) }]
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::Log(EventKind::Lifecycle, payload)
            if String::from_utf8_lossy(payload).contains("chatSettings"))));
    }

    #[test]
    fn an_effort_change_goes_through_the_flag_settings() {
        let mut driver = ready();
        let asked = writes(&driver.set_effort("medium"));
        assert_eq!(asked[0]["request"]["subtype"], "apply_flag_settings");
        assert_eq!(asked[0]["request"]["settings"]["effortLevel"], "medium");
    }

    #[test]
    fn stopping_interrupts_only_a_running_turn() {
        let mut driver = ready();
        assert!(driver.interrupt().is_empty(), "nothing to stop");
        driver.send("long job");
        let effects = driver.interrupt();
        assert_eq!(writes(&effects)[0]["request"]["subtype"], "interrupt");
        assert!(effects.iter().any(|e| matches!(e, Effect::Log(EventKind::Lifecycle, _))));
    }

    #[test]
    fn a_request_the_driver_does_not_know_is_refused_at_once() {
        let mut driver = ready();
        let effects = driver.on_line(r#"{"type":"control_request","request_id":"h1","request":{"subtype":"hook_callback","callback_id":"x"}}"#);
        let reply = &writes(&effects)[0];
        assert_eq!(reply["response"]["subtype"], "error");
        assert_eq!(reply["response"]["request_id"], "h1");
    }
}
