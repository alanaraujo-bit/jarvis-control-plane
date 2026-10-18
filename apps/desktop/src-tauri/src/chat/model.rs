//! What a chat's drivers produce and accept (M29), the same for both
//! providers, so the runtime and the screen never see a provider's wire shape.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::event::{EventKind, Lifecycle, SessionState};

/// A model the provider offers, as the provider described it (§26: the list
/// comes from the CLI, never from this build).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatModel {
    /// What the provider takes back to switch to it.
    pub id: String,
    pub name: String,
    pub description: String,
    /// The efforts this model accepts, in the provider's own spelling.
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatQuestion {
    /// What the answer is keyed by when it goes back to the provider.
    pub id: String,
    pub question: String,
    pub header: String,
    pub options: Vec<ChatOption>,
    pub multi_select: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestKind {
    Permission,
    Question,
}

/// What the agent wants to do, in the words a card is titled with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatAction {
    Command,
    Edit,
    Create,
    Web,
    Question,
    Other,
}

/// The agent is waiting on the person: a permission or a question.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRequest {
    pub id: String,
    pub kind: RequestKind,
    /// The provider's own tool name, for the rare card that has nothing better.
    pub tool: String,
    pub action: ChatAction,
    /// The command, the file, the address — read out of the request, never the
    /// raw JSON.
    pub detail: String,
    /// Whether "always, in this conversation" can be offered.
    pub can_always: bool,
    pub questions: Vec<ChatQuestion>,
}

/// What the screen hears while it is attached.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatEvent {
    Ready {
        models: Vec<ChatModel>,
        model: Option<String>,
        effort: Option<String>,
    },
    Settings {
        model: Option<String>,
        effort: Option<String>,
    },
    Busy {
        busy: bool,
    },
    /// The text being written right now, whole so far.
    Draft {
        text: String,
    },
    DraftDone,
    Request {
        request: ChatRequest,
    },
    Resolved {
        id: String,
    },
    QuickReplies {
        options: Vec<String>,
    },
    Exited {
        code: Option<i32>,
    },
}

/// The person's answer to a request.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ChatAnswer {
    Allow { always: bool },
    Deny,
    /// Answers keyed by `ChatQuestion::id`.
    Answers { answers: BTreeMap<String, Vec<String>> },
}

/// What a driver asks the runtime to do. Drivers are pure so the protocol can
/// be tested line by line against what the real CLIs sent.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    Emit(ChatEvent),
    /// One protocol line for the child's stdin.
    Write(String),
    Log(EventKind, Vec<u8>),
    State(SessionState),
}

pub trait Driver: Send {
    fn start(&mut self) -> Vec<Effect>;
    fn on_line(&mut self, line: &str) -> Vec<Effect>;
    fn send(&mut self, text: &str) -> Vec<Effect>;
    fn answer(&mut self, id: &str, answer: ChatAnswer) -> Result<Vec<Effect>, String>;
    fn set_model(&mut self, model: &str) -> Vec<Effect>;
    fn set_effort(&mut self, effort: &str) -> Vec<Effect>;
    fn interrupt(&mut self) -> Vec<Effect>;
}

/// The log frame for a request being asked (§23, `Approval` kind 50).
pub fn asked(request: &ChatRequest) -> Effect {
    let payload = serde_json::json!({
        "source": "chat",
        "event": "asked",
        "id": request.id,
        "tool": request.tool,
        "action": request.action,
        "detail": request.detail,
    });
    Effect::Log(EventKind::Approval, payload.to_string().into_bytes())
}

/// The log frame for a request being answered. `decision` is `allow`,
/// `allowAlways`, `deny`, `answered` or `cancelled`.
pub fn resolved(id: &str, decision: &str) -> Effect {
    let payload = serde_json::json!({ "source": "chat", "event": "resolved", "id": id, "decision": decision });
    Effect::Log(EventKind::Approval, payload.to_string().into_bytes())
}

pub fn settings_logged(model: Option<String>, effort: Option<String>) -> Effect {
    let frame = Lifecycle::ChatSettings { model, effort };
    Effect::Log(EventKind::Lifecycle, serde_json::to_vec(&frame).unwrap_or_default())
}

pub fn interrupted() -> Effect {
    Effect::Log(EventKind::Lifecycle, serde_json::to_vec(&Lifecycle::Interrupted).unwrap_or_default())
}

/// The detail of a request, from the input field that names it — whole. Line
/// breaks are kept and nothing is cut: a command padded past any limit would
/// otherwise hide its tail from the person approving it (M29 review). The card
/// scrolls.
pub fn first_text(input: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .or_else(|| input.as_object().and_then(|map| map.values().find_map(Value::as_str)))
        .map(str::to_string)
        .unwrap_or_default()
}

/// The decision a request was settled with when nobody answered it: the CLI
/// withdrew it, or its turn ended around it.
pub const CANCELLED: &str = "cancelled";
