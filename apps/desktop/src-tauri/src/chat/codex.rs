//! Codex's app-server protocol (M29): JSON-RPC 2.0, one object per line.
//!
//! Measured against Codex 0.163.0 on 2026-10-18 in a scratch repository
//! (`docs/M29-CHAT.md` §2): `initialize` → `initialized` → `model/list` →
//! `thread/start {cwd}`, then a `turn/start` per message, with `model` and
//! `effort` that hold "for this turn and subsequent turns". Approvals arrive as
//! server requests and carry the decisions the server will accept. The rollout
//! is written where `session::transcript` already finds it.

use std::collections::HashMap;

use serde_json::{json, Value};

use super::model::{
    asked, interrupted, resolved, settings_logged, ChatAction, ChatAnswer, ChatEvent, ChatModel,
    ChatOption, ChatQuestion, ChatRequest, Driver, Effect, RequestKind,
};
use crate::session::event::SessionState;

/// What one of our requests was for, so its response can be read.
enum Call {
    Initialize,
    ModelList,
    ThreadStart,
    TurnStart,
    /// The text, so a steer that lost the race with the turn ending can still
    /// be delivered as a turn of its own.
    Steer(String),
    Other,
}

/// A server request waiting on the person.
struct Pending {
    rpc_id: Value,
    kind: Approval,
    /// The decisions this request says it accepts. Empty means it did not say.
    available: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Approval {
    Command,
    FileChange,
    Question,
}

pub struct CodexDriver {
    cwd: String,
    next: u64,
    calls: HashMap<u64, Call>,
    ready: bool,
    thread_id: Option<String>,
    /// The turn running now, once `turn/started` named it.
    turn_id: Option<String>,
    /// A `turn/start` is out and its `turn/started` has not arrived.
    starting: bool,
    models: Vec<ChatModel>,
    model: Option<String>,
    effort: Option<String>,
    /// What the thread runs with, so a turn carries only a real change.
    sent_model: Option<String>,
    sent_effort: Option<String>,
    queued: Vec<String>,
    draft: Option<(String, String)>,
    pending: HashMap<String, Pending>,
    /// The paths a pending file change touches, from its `item/started`.
    file_changes: HashMap<String, String>,
}

impl CodexDriver {
    pub fn new(cwd: String) -> Self {
        Self {
            cwd,
            next: 0,
            calls: HashMap::new(),
            ready: false,
            thread_id: None,
            turn_id: None,
            starting: false,
            models: Vec::new(),
            model: None,
            effort: None,
            sent_model: None,
            sent_effort: None,
            queued: Vec::new(),
            draft: None,
            pending: HashMap::new(),
            file_changes: HashMap::new(),
        }
    }

    fn call(&mut self, call: Call, method: &str, params: Value) -> Effect {
        self.next += 1;
        self.calls.insert(self.next, call);
        Effect::Write(json!({ "jsonrpc": "2.0", "id": self.next, "method": method, "params": params }).to_string())
    }

    fn input(text: &str) -> Value {
        json!([{ "type": "text", "text": text, "text_elements": [] }])
    }

    /// Deliver a message: into the running turn, or as a new one.
    fn deliver(&mut self, text: &str) -> Vec<Effect> {
        let thread = self.thread_id.clone().unwrap_or_default();
        if let Some(turn) = self.turn_id.clone() {
            let params = json!({ "threadId": thread, "expectedTurnId": turn, "input": Self::input(text) });
            return vec![self.call(Call::Steer(text.to_string()), "turn/steer", params)];
        }
        if self.starting {
            // `turn/started` will name the turn; the message goes into it then.
            self.queued.push(text.to_string());
            return Vec::new();
        }
        let mut params = json!({ "threadId": thread, "input": Self::input(text) });
        if self.model.is_some() && self.model != self.sent_model {
            params["model"] = json!(self.model);
            self.sent_model = self.model.clone();
        }
        if self.effort.is_some() && self.effort != self.sent_effort {
            params["effort"] = json!(self.effort);
            self.sent_effort = self.effort.clone();
        }
        self.starting = true;
        vec![self.call(Call::TurnStart, "turn/start", params)]
    }

    fn flush(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        for text in std::mem::take(&mut self.queued) {
            effects.extend(self.deliver(&text));
        }
        effects
    }

    fn idle_state(&self) -> SessionState {
        if self.pending.is_empty() {
            SessionState::Idle
        } else {
            SessionState::Waiting
        }
    }

    fn on_response(&mut self, id: u64, value: &Value) -> Vec<Effect> {
        let Some(call) = self.calls.remove(&id) else {
            return Vec::new();
        };
        let result = value.get("result");
        let failed = value.get("error").is_some();
        match call {
            Call::Initialize => {
                let notice = Effect::Write(json!({ "jsonrpc": "2.0", "method": "initialized", "params": null }).to_string());
                let list = self.call(Call::ModelList, "model/list", json!({}));
                vec![notice, list]
            }
            Call::ModelList => {
                self.models = result.map(read_models).unwrap_or_default();
                let cwd = self.cwd.clone();
                vec![self.call(Call::ThreadStart, "thread/start", json!({ "cwd": cwd }))]
            }
            Call::ThreadStart => {
                let text = |key: &str| result.and_then(|r| r.get(key)).and_then(Value::as_str).map(str::to_string);
                self.thread_id = result.and_then(|r| r.pointer("/thread/id")).and_then(Value::as_str).map(str::to_string);
                self.model = text("model");
                self.effort = text("reasoningEffort");
                self.sent_model = self.model.clone();
                self.sent_effort = self.effort.clone();
                self.ready = true;
                let mut effects = vec![Effect::Emit(ChatEvent::Ready {
                    models: self.models.clone(),
                    model: self.model.clone(),
                    effort: self.effort.clone(),
                })];
                effects.extend(self.flush());
                effects
            }
            Call::TurnStart if failed => {
                self.starting = false;
                vec![Effect::Emit(ChatEvent::Busy { busy: false }), Effect::State(self.idle_state())]
            }
            Call::Steer(text) if failed => {
                // The turn ended between our look and the server's: a new one.
                self.turn_id = None;
                self.deliver(&text)
            }
            _ => Vec::new(),
        }
    }

    fn on_server_request(&mut self, rpc_id: &Value, method: &str, params: &Value) -> Vec<Effect> {
        let key = match rpc_id {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
        let available: Vec<String> = params
            .get("availableDecisions")
            .and_then(Value::as_array)
            .map(|list| list.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();
        let offers = |decision: &str| available.is_empty() || available.iter().any(|d| d == decision);

        let (kind, request) = match method {
            "item/commandExecution/requestApproval" => {
                // `commandActions` holds what the agent asked to run; `command`
                // is that wrapped in the shell invocation.
                let detail = params
                    .get("commandActions")
                    .and_then(Value::as_array)
                    .and_then(|actions| actions.last())
                    .and_then(|a| a.get("command"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| text("command"));
                (
                    Approval::Command,
                    ChatRequest {
                        id: key.clone(),
                        kind: RequestKind::Permission,
                        tool: "Shell".into(),
                        action: ChatAction::Command,
                        detail,
                        can_always: offers("acceptForSession"),
                        questions: Vec::new(),
                    },
                )
            }
            "item/fileChange/requestApproval" => {
                let item = text("itemId");
                let detail = self
                    .file_changes
                    .get(&item)
                    .cloned()
                    .filter(|paths| !paths.is_empty())
                    .unwrap_or_else(|| text("reason"));
                (
                    Approval::FileChange,
                    ChatRequest {
                        id: key.clone(),
                        kind: RequestKind::Permission,
                        tool: "Edit".into(),
                        action: ChatAction::Edit,
                        detail,
                        can_always: offers("acceptForSession"),
                        questions: Vec::new(),
                    },
                )
            }
            "item/tool/requestUserInput" => (
                Approval::Question,
                ChatRequest {
                    id: key.clone(),
                    kind: RequestKind::Question,
                    tool: "request_user_input".into(),
                    action: ChatAction::Question,
                    detail: String::new(),
                    can_always: false,
                    questions: read_questions(params),
                },
            ),
            _ => {
                // An answer nobody will give would hang the turn forever.
                return vec![Effect::Write(
                    json!({
                        "jsonrpc": "2.0",
                        "id": rpc_id,
                        "error": { "code": -32601, "message": "unsupported by J.A.R.V.I.S." }
                    })
                    .to_string(),
                )];
            }
        };
        self.pending.insert(key, Pending { rpc_id: rpc_id.clone(), kind, available });
        vec![
            asked(&request),
            Effect::Emit(ChatEvent::Request { request }),
            Effect::State(SessionState::Waiting),
        ]
    }

    fn on_notification(&mut self, method: &str, params: &Value) -> Vec<Effect> {
        match method {
            "turn/started" => {
                self.starting = false;
                self.turn_id = params.pointer("/turn/id").and_then(Value::as_str).map(str::to_string);
                let mut effects = vec![Effect::Emit(ChatEvent::Busy { busy: true }), Effect::State(SessionState::Working)];
                effects.extend(self.flush());
                effects
            }
            "turn/completed" => {
                self.turn_id = None;
                self.starting = false;
                let mut effects = self.close_draft();
                effects.push(Effect::Emit(ChatEvent::Busy { busy: false }));
                effects.push(Effect::State(self.idle_state()));
                // A message that arrived as the turn ended starts the next.
                effects.extend(self.flush());
                effects
            }
            "item/agentMessage/delta" => {
                let item = params.get("itemId").and_then(Value::as_str).unwrap_or_default().to_string();
                let piece = params.get("delta").and_then(Value::as_str).unwrap_or_default();
                let mut effects = Vec::new();
                if self.draft.as_ref().is_some_and(|(id, _)| *id != item) {
                    effects.extend(self.close_draft());
                }
                let draft = self.draft.get_or_insert_with(|| (item, String::new()));
                draft.1.push_str(piece);
                effects.push(Effect::Emit(ChatEvent::Draft { text: draft.1.clone() }));
                effects
            }
            "item/started" => {
                let item = params.get("item");
                if item.and_then(|i| i.get("type")).and_then(Value::as_str) == Some("fileChange") {
                    let id = item.and_then(|i| i.get("id")).and_then(Value::as_str).unwrap_or_default();
                    let paths: Vec<&str> = item
                        .and_then(|i| i.get("changes"))
                        .and_then(Value::as_array)
                        .map(|changes| changes.iter().filter_map(|c| c.get("path").and_then(Value::as_str)).collect())
                        .unwrap_or_default();
                    self.file_changes.insert(id.to_string(), paths.join("\n"));
                }
                Vec::new()
            }
            "item/completed" => {
                let item = params.get("item");
                if item.and_then(|i| i.get("type")).and_then(Value::as_str) != Some("agentMessage") {
                    return Vec::new();
                }
                let mut effects = self.close_draft();
                // In Default mode Codex asks inside the message instead of
                // through `request_user_input` (measured on 0.163.0).
                let options: Vec<String> = item
                    .and_then(|i| i.get("questions"))
                    .and_then(Value::as_array)
                    .and_then(|questions| questions.first())
                    .and_then(|q| q.get("options"))
                    .and_then(Value::as_array)
                    .map(|options| options.iter().filter_map(Value::as_str).map(str::to_string).collect())
                    .unwrap_or_default();
                if !options.is_empty() {
                    effects.push(Effect::Emit(ChatEvent::QuickReplies { options }));
                }
                effects
            }
            "serverRequest/resolved" => {
                let key = match params.get("requestId") {
                    Some(Value::String(text)) => text.clone(),
                    Some(other) => other.to_string(),
                    None => return Vec::new(),
                };
                if self.pending.remove(&key).is_none() {
                    return Vec::new();
                }
                vec![Effect::Emit(ChatEvent::Resolved { id: key }), Effect::State(SessionState::Working)]
            }
            "model/rerouted" => {
                // The provider moved the turn to another model; say so (§28).
                self.model = params.get("toModel").and_then(Value::as_str).map(str::to_string);
                self.sent_model = self.model.clone();
                vec![
                    Effect::Emit(ChatEvent::Settings { model: self.model.clone(), effort: self.effort.clone() }),
                    settings_logged(self.model.clone(), self.effort.clone()),
                ]
            }
            _ => Vec::new(),
        }
    }

    fn close_draft(&mut self) -> Vec<Effect> {
        match self.draft.take() {
            Some(_) => vec![Effect::Emit(ChatEvent::DraftDone)],
            None => Vec::new(),
        }
    }

    fn settings_changed(&self) -> Vec<Effect> {
        vec![
            Effect::Emit(ChatEvent::Settings { model: self.model.clone(), effort: self.effort.clone() }),
            settings_logged(self.model.clone(), self.effort.clone()),
        ]
    }
}

fn read_models(result: &Value) -> Vec<ChatModel> {
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    result
        .get("data")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|m| !m.get("hidden").and_then(Value::as_bool).unwrap_or(false))
                .map(|m| ChatModel {
                    id: text(m, "model"),
                    name: m
                        .get("displayName")
                        .and_then(Value::as_str)
                        .filter(|name| !name.is_empty())
                        .map(str::to_string)
                        .unwrap_or_else(|| text(m, "model")),
                    description: text(m, "description"),
                    efforts: m
                        .get("supportedReasoningEfforts")
                        .and_then(Value::as_array)
                        .map(|efforts| {
                            efforts
                                .iter()
                                .filter_map(|e| e.get("reasoningEffort").and_then(Value::as_str))
                                .map(str::to_string)
                                .collect()
                        })
                        .unwrap_or_default(),
                    default_effort: m.get("defaultReasoningEffort").and_then(Value::as_str).map(str::to_string),
                })
                .filter(|m| !m.id.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn read_questions(params: &Value) -> Vec<ChatQuestion> {
    let text = |value: &Value, key: &str| value.get(key).and_then(Value::as_str).unwrap_or_default().to_string();
    params
        .get("questions")
        .and_then(Value::as_array)
        .map(|questions| {
            questions
                .iter()
                .map(|q| ChatQuestion {
                    id: text(q, "id"),
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
                    multi_select: false,
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Driver for CodexDriver {
    fn start(&mut self) -> Vec<Effect> {
        let params = json!({
            "clientInfo": { "name": "jarvis", "title": "J.A.R.V.I.S.", "version": env!("CARGO_PKG_VERSION") }
        });
        vec![self.call(Call::Initialize, "initialize", params)]
    }

    fn on_line(&mut self, line: &str) -> Vec<Effect> {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            return Vec::new();
        };
        let method = value.get("method").and_then(Value::as_str);
        match (value.get("id"), method) {
            (Some(id), Some(method)) => {
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                self.on_server_request(id, method, &params)
            }
            (Some(id), None) => id.as_u64().map(|id| self.on_response(id, &value)).unwrap_or_default(),
            (None, Some(method)) => {
                let params = value.get("params").cloned().unwrap_or(Value::Null);
                self.on_notification(method, &params)
            }
            (None, None) => Vec::new(),
        }
    }

    fn send(&mut self, text: &str) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.turn_id.is_none() && !self.starting {
            effects.push(Effect::Emit(ChatEvent::Busy { busy: true }));
            effects.push(Effect::State(SessionState::Working));
        }
        if self.ready {
            effects.extend(self.deliver(text));
        } else {
            self.queued.push(text.to_string());
        }
        effects
    }

    fn answer(&mut self, id: &str, answer: ChatAnswer) -> Result<Vec<Effect>, String> {
        let pending = self.pending.remove(id).ok_or_else(|| "chat.requestGone".to_string())?;
        let offers = |decision: &str| pending.available.is_empty() || pending.available.iter().any(|d| d == decision);
        let (result, decision) = match (pending.kind, &answer) {
            (Approval::Question, ChatAnswer::Answers { answers }) => {
                let mapped: serde_json::Map<String, Value> = answers
                    .iter()
                    .map(|(question, chosen)| (question.clone(), json!({ "answers": chosen })))
                    .collect();
                (json!({ "answers": mapped }), "answered")
            }
            (Approval::Question, ChatAnswer::Deny) => (json!({ "answers": {} }), "deny"),
            (Approval::Command | Approval::FileChange, ChatAnswer::Allow { always: true }) if offers("acceptForSession") => {
                (json!({ "decision": "acceptForSession" }), "allowAlways")
            }
            (Approval::Command | Approval::FileChange, ChatAnswer::Allow { .. }) => (json!({ "decision": "accept" }), "allow"),
            // `decline` lets the turn go on without it; a request that does not
            // offer it only takes `cancel`, which ends the turn.
            (Approval::Command | Approval::FileChange, ChatAnswer::Deny) => {
                (json!({ "decision": if offers("decline") { "decline" } else { "cancel" } }), "deny")
            }
            _ => {
                self.pending.insert(id.to_string(), pending);
                return Err("chat.wrongAnswer".to_string());
            }
        };
        let reply = Effect::Write(json!({ "jsonrpc": "2.0", "id": pending.rpc_id, "result": result }).to_string());
        let state = if self.pending.is_empty() { SessionState::Working } else { SessionState::Waiting };
        Ok(vec![reply, resolved(id, decision), Effect::Emit(ChatEvent::Resolved { id: id.to_string() }), Effect::State(state)])
    }

    fn set_model(&mut self, model: &str) -> Vec<Effect> {
        self.model = Some(model.to_string());
        // A model that does not take the current effort runs at its own default.
        if let Some(chosen) = self.models.iter().find(|m| m.id == model) {
            let fits = self.effort.as_ref().is_some_and(|e| chosen.efforts.contains(e));
            if !fits {
                self.effort = chosen.default_effort.clone();
            }
        }
        self.settings_changed()
    }

    fn set_effort(&mut self, effort: &str) -> Vec<Effect> {
        self.effort = Some(effort.to_string());
        self.settings_changed()
    }

    fn interrupt(&mut self) -> Vec<Effect> {
        let (Some(thread), Some(turn)) = (self.thread_id.clone(), self.turn_id.clone()) else {
            return Vec::new();
        };
        vec![self.call(Call::Other, "turn/interrupt", json!({ "threadId": thread, "turnId": turn })), interrupted()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::event::EventKind;

    /// Trimmed from what 0.163.0 answered on this machine.
    const MODELS: &str = r#"{"id":2,"result":{"data":[{"model":"gpt-6.1-sol","displayName":"gpt-6.1-sol","description":"Frontier model","isDefault":true,"hidden":false,"defaultReasoningEffort":"medium","supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Fast"},{"reasoningEffort":"medium","description":"Balanced"},{"reasoningEffort":"high","description":"Deep"},{"reasoningEffort":"ultra","description":"Deepest"}]},{"model":"gpt-6-luna","displayName":"gpt-6-luna","description":"Small model","isDefault":false,"hidden":false,"defaultReasoningEffort":"low","supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Fast"},{"reasoningEffort":"medium","description":"Balanced"}]},{"model":"internal","displayName":"internal","hidden":true,"supportedReasoningEfforts":[]}],"nextCursor":null}}"#;
    const THREAD: &str = r#"{"id":3,"result":{"thread":{"id":"01a150de-c336","path":"C:\\Users\\a\\.codex\\sessions\\2026\\10\\18\\rollout-x-01a150de-c336.jsonl"},"model":"gpt-6.1-sol","reasoningEffort":"ultra","cwd":"C:\\scratch"}}"#;
    const TURN_STARTED: &str = r#"{"method":"turn/started","params":{"threadId":"01a150de-c336","turn":{"id":"turn-1","status":"inProgress"}}}"#;
    const TURN_COMPLETED: &str = r#"{"method":"turn/completed","params":{"threadId":"01a150de-c336","turn":{"id":"turn-1","status":"completed"}}}"#;
    const APPROVAL: &str = r#"{"method":"item/commandExecution/requestApproval","id":0,"params":{"kind":"command","threadId":"01a150de-c336","turnId":"turn-1","itemId":"exec-5bfb","command":"\"C:\\\\WINDOWS\\\\System32\\\\WindowsPowerShell\\\\v1.0\\\\powershell.exe\" -Command \"Set-Content -LiteralPath 'hello2.txt' -Value 'hi' -NoNewline\"","commandActions":[{"type":"unknown","command":"Set-Content -LiteralPath 'hello2.txt' -Value 'hi' -NoNewline"}],"availableDecisions":["accept",{"acceptWithExecpolicyAmendment":{"execpolicy_amendment":["Set-Content"]}},"cancel"]}}"#;

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

    fn ready() -> CodexDriver {
        let mut driver = CodexDriver::new("C:\\scratch".into());
        driver.start();
        driver.on_line(r#"{"id":1,"result":{"codexHome":"C:\\Users\\a\\.codex"}}"#);
        driver.on_line(MODELS);
        driver.on_line(THREAD);
        driver
    }

    #[test]
    fn the_handshake_ends_on_a_thread_in_the_projects_folder() {
        let mut driver = CodexDriver::new("C:\\scratch".into());
        assert_eq!(writes(&driver.start())[0]["method"], "initialize");
        let after_init = writes(&driver.on_line(r#"{"id":1,"result":{"codexHome":"C:\\Users\\a\\.codex"}}"#));
        assert_eq!(after_init[0]["method"], "initialized");
        assert_eq!(after_init[1]["method"], "model/list");
        let after_models = writes(&driver.on_line(MODELS));
        assert_eq!(after_models[0]["method"], "thread/start");
        assert_eq!(after_models[0]["params"], json!({ "cwd": "C:\\scratch" }), "the person's own approval policy and sandbox apply");

        match emitted(&driver.on_line(THREAD)).as_slice() {
            [ChatEvent::Ready { models, model, effort }] => {
                assert_eq!(model.as_deref(), Some("gpt-6.1-sol"));
                assert_eq!(effort.as_deref(), Some("ultra"));
                assert_eq!(models.len(), 2, "hidden models are not offered");
                assert_eq!(models[0].efforts, ["low", "medium", "high", "ultra"]);
                assert_eq!(models[1].default_effort.as_deref(), Some("low"));
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn a_message_starts_a_turn_carrying_only_a_changed_model() {
        let mut driver = ready();
        let first = writes(&driver.send("hi"));
        assert_eq!(first[0]["method"], "turn/start");
        assert_eq!(first[0]["params"]["input"][0]["text"], "hi");
        assert!(first[0]["params"].get("model").is_none(), "nothing changed, nothing sent");

        driver.on_line(TURN_STARTED);
        driver.on_line(TURN_COMPLETED);
        driver.set_model("gpt-6-luna");
        let second = writes(&driver.send("again"));
        assert_eq!(second[0]["params"]["model"], "gpt-6-luna");
        assert_eq!(second[0]["params"]["effort"], "low", "ultra is not offered by luna, so its default runs");
    }

    #[test]
    fn a_model_change_is_shown_at_once_and_logged() {
        let mut driver = ready();
        let effects = driver.set_model("gpt-6-luna");
        assert_eq!(
            emitted(&effects),
            [ChatEvent::Settings { model: Some("gpt-6-luna".into()), effort: Some("low".into()) }]
        );
        assert!(effects.iter().any(|e| matches!(e, Effect::Log(EventKind::Lifecycle, _))));
    }

    #[test]
    fn a_message_during_a_turn_steers_it() {
        let mut driver = ready();
        driver.send("first");
        driver.on_line(TURN_STARTED);
        let steer = writes(&driver.send("and also this"));
        assert_eq!(steer[0]["method"], "turn/steer");
        assert_eq!(steer[0]["params"]["expectedTurnId"], "turn-1");
        assert_eq!(steer[0]["params"]["input"][0]["text"], "and also this");
    }

    #[test]
    fn a_message_before_the_turn_is_named_waits_for_it() {
        let mut driver = ready();
        driver.send("first");
        assert!(writes(&driver.send("second")).is_empty(), "held until turn/started names the turn");
        let steer = writes(&driver.on_line(TURN_STARTED));
        assert_eq!(steer[0]["method"], "turn/steer");
    }

    #[test]
    fn a_steer_that_lost_the_race_becomes_its_own_turn() {
        let mut driver = ready();
        driver.send("first");
        driver.on_line(TURN_STARTED);
        let steer = writes(&driver.send("late"));
        let id = steer[0]["id"].as_u64().unwrap();
        let retried = writes(&driver.on_line(&format!(r#"{{"id":{id},"error":{{"code":-32600,"message":"no active turn"}}}}"#)));
        assert_eq!(retried[0]["method"], "turn/start");
        assert_eq!(retried[0]["params"]["input"][0]["text"], "late");
    }

    #[test]
    fn busy_follows_the_turn() {
        let mut driver = ready();
        assert!(emitted(&driver.send("go")).contains(&ChatEvent::Busy { busy: true }));
        let done = driver.on_line(TURN_COMPLETED);
        assert!(emitted(&done).contains(&ChatEvent::Busy { busy: false }));
        assert!(done.contains(&Effect::State(SessionState::Idle)));
    }

    #[test]
    fn text_streams_by_item() {
        let mut driver = ready();
        driver.on_line(r#"{"method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"I"}}"#);
        let grown = driver.on_line(r#"{"method":"item/agentMessage/delta","params":{"itemId":"m1","delta":"’ll create"}}"#);
        assert_eq!(emitted(&grown), [ChatEvent::Draft { text: "I’ll create".into() }]);
        let done = driver.on_line(r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"m1","text":"I’ll create"}}}"#);
        assert_eq!(emitted(&done), [ChatEvent::DraftDone]);
    }

    /// Captured on 0.163.0: in Default mode the question is a message.
    #[test]
    fn a_question_inside_a_message_offers_quick_replies() {
        let mut driver = ready();
        let effects = driver.on_line(r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"call_b497","text":"Do you prefer red or blue?\n- Red\n- Blue","phase":"final_answer","delivery":"async","questions":[{"title":"Do you prefer red or blue?","options":["Red","Blue"]}]}}}"#);
        assert!(emitted(&effects).contains(&ChatEvent::QuickReplies { options: vec!["Red".into(), "Blue".into()] }));
    }

    #[test]
    fn a_command_approval_names_what_will_run_and_offers_only_what_the_server_accepts() {
        let mut driver = ready();
        let effects = driver.on_line(APPROVAL);
        match emitted(&effects).as_slice() {
            [ChatEvent::Request { request }] => {
                assert_eq!(request.id, "0");
                assert_eq!(request.action, ChatAction::Command);
                assert_eq!(request.detail, "Set-Content -LiteralPath 'hello2.txt' -Value 'hi' -NoNewline");
                assert!(!request.can_always, "this request does not offer acceptForSession");
            }
            other => panic!("expected one request, got {other:?}"),
        }
        let allow = writes(&driver.answer("0", ChatAnswer::Allow { always: false }).unwrap());
        assert_eq!(allow[0], json!({ "jsonrpc": "2.0", "id": 0, "result": { "decision": "accept" } }));
    }

    #[test]
    fn denying_uses_decline_only_where_it_is_offered() {
        let mut driver = ready();
        driver.on_line(APPROVAL);
        let deny = writes(&driver.answer("0", ChatAnswer::Deny).unwrap());
        assert_eq!(deny[0]["result"]["decision"], "cancel");

        driver.on_line(r#"{"method":"item/fileChange/requestApproval","id":7,"params":{"itemId":"fc1","threadId":"t","turnId":"turn-1","startedAtMs":0,"reason":"write outside the workspace"}}"#);
        let deny = writes(&driver.answer("7", ChatAnswer::Deny).unwrap());
        assert_eq!(deny[0]["result"]["decision"], "decline");
    }

    #[test]
    fn a_file_change_names_its_paths_and_can_be_allowed_for_the_session() {
        let mut driver = ready();
        driver.on_line(r#"{"method":"item/started","params":{"item":{"type":"fileChange","id":"fc1","changes":[{"path":"src/app.ts","kind":"update","diff":""}]}}}"#);
        let effects = driver.on_line(r#"{"method":"item/fileChange/requestApproval","id":5,"params":{"itemId":"fc1","threadId":"t","turnId":"turn-1","startedAtMs":0}}"#);
        match emitted(&effects).as_slice() {
            [ChatEvent::Request { request }] => {
                assert_eq!(request.detail, "src/app.ts");
                assert!(request.can_always);
            }
            other => panic!("expected one request, got {other:?}"),
        }
        let always = writes(&driver.answer("5", ChatAnswer::Allow { always: true }).unwrap());
        assert_eq!(always[0]["result"]["decision"], "acceptForSession");
    }

    #[test]
    fn a_structured_question_is_answered_by_its_ids() {
        let mut driver = ready();
        driver.on_line(r#"{"method":"item/tool/requestUserInput","id":9,"params":{"isBlocking":true,"itemId":"i","threadId":"t","turnId":"turn-1","questions":[{"id":"color","header":"Color","question":"Red or blue?","options":[{"label":"Red","description":""},{"label":"Blue","description":""}]}]}}"#);
        let answers = [("color".to_string(), vec!["Blue".to_string()])].into_iter().collect();
        assert_eq!(
            driver.answer("9", ChatAnswer::Allow { always: true }),
            Err("chat.wrongAnswer".to_string()),
            "a question is answered, not allowed"
        );
        let reply = writes(&driver.answer("9", ChatAnswer::Answers { answers }).unwrap());
        assert_eq!(reply[0]["result"], json!({ "answers": { "color": { "answers": ["Blue"] } } }));
    }

    #[test]
    fn the_server_resolving_a_request_takes_its_card_away() {
        let mut driver = ready();
        driver.on_line(APPROVAL);
        let effects = driver.on_line(r#"{"method":"serverRequest/resolved","params":{"threadId":"t","requestId":0}}"#);
        assert!(emitted(&effects).contains(&ChatEvent::Resolved { id: "0".into() }));
        assert_eq!(driver.answer("0", ChatAnswer::Deny), Err("chat.requestGone".to_string()));
    }

    #[test]
    fn stopping_interrupts_the_running_turn() {
        let mut driver = ready();
        assert!(driver.interrupt().is_empty(), "nothing is running");
        driver.send("long");
        driver.on_line(TURN_STARTED);
        let stop = writes(&driver.interrupt());
        assert_eq!(stop[0]["method"], "turn/interrupt");
        assert_eq!(stop[0]["params"], json!({ "threadId": "01a150de-c336", "turnId": "turn-1" }));
    }

    #[test]
    fn a_server_request_the_driver_does_not_know_is_refused_at_once() {
        let mut driver = ready();
        let reply = writes(&driver.on_line(r#"{"method":"mcpServer/elicitation/request","id":11,"params":{}}"#));
        assert_eq!(reply[0]["id"], 11);
        assert_eq!(reply[0]["error"]["code"], -32601);
    }

    #[test]
    fn a_rerouted_turn_shows_the_model_that_ran_it() {
        let mut driver = ready();
        let effects = driver.on_line(r#"{"method":"model/rerouted","params":{"threadId":"t","turnId":"turn-1","fromModel":"gpt-6.1-sol","toModel":"gpt-6-luna","reason":"capacity"}}"#);
        assert!(emitted(&effects).contains(&ChatEvent::Settings { model: Some("gpt-6-luna".into()), effort: Some("ultra".into()) }));
    }
}
