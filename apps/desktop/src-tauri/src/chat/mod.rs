//! Chat sessions (M29): the same agents as the terminal, worked with through
//! each provider's structured protocol instead of a screen.
//!
//! The provider's transcript is still the conversation's record, followed into
//! the session log by `session::transcript` exactly as for a terminal (D3). The
//! protocol's stdout is the live half: streaming text, permission requests and
//! questions, whether a turn is running, and which model and effort are in use.

pub mod claude;
pub mod codex;
pub mod commands;
pub mod model;
pub mod process;
#[cfg(test)]
mod real_tests;

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::ChildStdout;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::Serialize;
use tauri::ipc::Channel;

use model::{ChatEvent, ChatModel, ChatRequest, Driver, Effect};
use crate::session::manager::LiveSession;

/// Everything the screen needs to draw a chat it has just attached to. Built
/// by folding the same events the screen hears live, so the two cannot drift.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub ready: bool,
    pub models: Vec<ChatModel>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub busy: bool,
    pub draft: Option<String>,
    pub pending: Vec<ChatRequest>,
    pub quick_replies: Vec<String>,
    pub ended: bool,
}

pub fn fold(snapshot: &mut Snapshot, event: &ChatEvent) {
    match event {
        ChatEvent::Ready { models, model, effort } => {
            snapshot.ready = true;
            snapshot.models = models.clone();
            snapshot.model = model.clone();
            snapshot.effort = effort.clone();
        }
        ChatEvent::Settings { model, effort } => {
            snapshot.model = model.clone();
            snapshot.effort = effort.clone();
        }
        ChatEvent::Busy { busy } => {
            snapshot.busy = *busy;
            // Replies offered for the last answer mean nothing once the agent
            // is working on the next one.
            if *busy {
                snapshot.quick_replies.clear();
            }
        }
        ChatEvent::Draft { text } => snapshot.draft = Some(text.clone()),
        ChatEvent::DraftDone => snapshot.draft = None,
        ChatEvent::Request { request } => snapshot.pending.push(request.clone()),
        ChatEvent::Resolved { id } => snapshot.pending.retain(|r| r.id != *id),
        ChatEvent::QuickReplies { options } => snapshot.quick_replies = options.clone(),
        ChatEvent::Exited { .. } => {
            snapshot.ended = true;
            snapshot.busy = false;
            snapshot.draft = None;
            snapshot.pending.clear();
            snapshot.quick_replies.clear();
        }
    }
}

/// A chat may only start where the provider has been told the folder is
/// trusted (M29 §6.4.1). `-p` skips the trust question, so starting anyway
/// would load an unknown repository's hooks and MCP servers without consent.
/// Unknown is not untrusted (§28): it does not block.
pub fn trust_gate(trusted: Option<bool>) -> Result<(), String> {
    match trusted {
        Some(false) => Err("chat.folderNotTrusted".into()),
        _ => Ok(()),
    }
}

/// The program and arguments for a chat of `provider`, or `None` when that
/// provider has no measured chat protocol.
pub fn command_for(
    provider: &str,
    session_id: &str,
    guardrail_settings: Option<&std::path::Path>,
    brief: Option<&std::path::Path>,
    resume: Option<&str>,
) -> Option<(String, Vec<String>)> {
    let support = crate::providers::by_id(provider)?.capabilities().chat;
    match support {
        crate::providers::ChatSupport::StreamJson => Some((
            "claude".into(),
            claude::launch_args(session_id, resume, guardrail_settings, brief),
        )),
        crate::providers::ChatSupport::AppServer => Some(("codex".into(), vec!["app-server".into()])),
        crate::providers::ChatSupport::None => None,
    }
}

/// The driver for `provider`'s chat protocol.
pub fn driver_for(provider: &str, cwd: &str) -> Option<Box<dyn Driver>> {
    match crate::providers::by_id(provider)?.capabilities().chat {
        crate::providers::ChatSupport::StreamJson => Some(Box::new(claude::ClaudeDriver::new())),
        crate::providers::ChatSupport::AppServer => Some(Box::new(codex::CodexDriver::new(cwd.to_string()))),
        crate::providers::ChatSupport::None => None,
    }
}

struct Inner {
    driver: Box<dyn Driver>,
    snapshot: Snapshot,
}

struct View {
    id: String,
    channel: Channel<ChatEvent>,
}

/// One chat session's live half.
pub struct ChatRuntime {
    session: Arc<LiveSession>,
    project_id: String,
    provider: String,
    /// Driver and snapshot behind one lock, so effects are applied in the
    /// order they were produced, whichever thread produced them.
    inner: Mutex<Inner>,
    view: Mutex<Option<View>>,
    /// Lines for the child's stdin, written by a thread of their own so a
    /// child that stops reading can never block the reader that drains it.
    writer: Sender<String>,
}

impl ChatRuntime {
    /// Ask the driver for something and apply what it says.
    pub fn run(&self, ask: impl FnOnce(&mut dyn Driver) -> Result<Vec<Effect>, String>) -> Result<(), String> {
        let mut inner = self.inner.lock();
        if inner.snapshot.ended {
            return Err("chat.ended".into());
        }
        let effects = ask(inner.driver.as_mut())?;
        self.apply(&mut inner, effects);
        Ok(())
    }

    fn apply(&self, inner: &mut Inner, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Emit(event) => {
                    fold(&mut inner.snapshot, &event);
                    if let ChatEvent::Request { request } = &event {
                        self.announce(request);
                    }
                    if let Some(view) = self.view.lock().as_ref() {
                        let _ = view.channel.send(event);
                    }
                }
                Effect::Write(line) => {
                    let _ = self.writer.send(line);
                }
                Effect::Log(kind, payload) => self.session.log(kind, payload),
                Effect::State(state) => self.session.set_state(state),
            }
        }
    }

    /// Tell somebody the agent is waiting on them (§49). Official: the provider
    /// stated it on its own protocol, where a terminal can only be read (D35).
    /// The bus drops it when this chat is on screen.
    fn announce(&self, request: &ChatRequest) {
        let preview = match request.questions.first() {
            Some(question) => question.question.clone(),
            None => request.detail.clone(),
        };
        crate::notify::bus::raise(
            crate::notify::Reason::ProviderPrompt,
            crate::session::event::Confidence::Official,
            crate::notify::Raise {
                session_id: Some(self.session.id.clone()),
                project_id: Some(self.project_id.clone()),
                mission_id: None,
                provider: Some(self.provider.clone()),
                preview: Some(preview).filter(|p| !p.is_empty()),
                detail_code: None,
            },
        );
    }

    /// Start streaming to a view. The snapshot is taken under the same lock
    /// every event passes through, so nothing falls between it and the stream.
    pub fn attach(&self, attachment_id: String, channel: Channel<ChatEvent>) -> Snapshot {
        let inner = self.inner.lock();
        *self.view.lock() = Some(View { id: attachment_id, channel });
        inner.snapshot.clone()
    }

    /// Stop streaming, unless a newer view has taken over since. Returns
    /// whether this attachment was the one detached.
    pub fn detach(&self, attachment_id: &str) -> bool {
        let mut view = self.view.lock();
        if view.as_ref().map(|v| v.id.as_str()) != Some(attachment_id) {
            return false;
        }
        *view = None;
        true
    }

    fn end(&self, code: Option<i32>) {
        let mut inner = self.inner.lock();
        if inner.snapshot.ended {
            return;
        }
        self.apply(&mut inner, vec![Effect::Emit(ChatEvent::Exited { code })]);
    }
}

/// Every live chat, by session id.
#[derive(Default)]
pub struct Chats(Mutex<HashMap<String, Arc<ChatRuntime>>>);

impl Chats {
    pub fn get(&self, session_id: &str) -> Result<Arc<ChatRuntime>, String> {
        self.0.lock().get(session_id).cloned().ok_or_else(|| "chat.unknown".to_string())
    }

    pub fn remove(&self, session_id: &str) {
        self.0.lock().remove(session_id);
    }
}

/// Bring a chat to life over a session already started with `start_chat`.
pub fn start(
    chats: &Chats,
    session: Arc<LiveSession>,
    stdout: ChildStdout,
    driver: Box<dyn Driver>,
    project_id: String,
    provider: String,
) {
    let (writer, lines) = channel::<String>();
    if let Some(pipe) = session.chat() {
        let id = session.id.clone();
        std::thread::Builder::new()
            .name(format!("chat-write-{id}"))
            .spawn(move || {
                while let Ok(line) = lines.recv() {
                    if pipe.send_line(&line).is_err() {
                        break;
                    }
                }
            })
            .expect("spawn chat writer");
    }

    let runtime = Arc::new(ChatRuntime {
        session: Arc::clone(&session),
        project_id,
        provider,
        inner: Mutex::new(Inner { driver, snapshot: Snapshot::default() }),
        view: Mutex::new(None),
        writer,
    });
    chats.0.lock().insert(session.id.clone(), Arc::clone(&runtime));
    let _ = runtime.run(|driver| Ok(driver.start()));

    std::thread::Builder::new()
        .name(format!("chat-read-{}", session.id))
        .spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut buffer = Vec::new();
            loop {
                buffer.clear();
                // Bytes, not `lines()`: one invalid UTF-8 byte must not end the
                // conversation.
                match reader.read_until(b'\n', &mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buffer);
                        let line = line.trim_end();
                        if !line.is_empty() {
                            let _ = runtime.run(|driver| Ok(driver.on_line(line)));
                        }
                    }
                }
            }
            // The stream closed; give the process a moment to report how.
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut code = runtime.session.exited();
            while code.is_none() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
                code = runtime.session.exited();
            }
            runtime.end(code);
            runtime.session.finish(code);
        })
        .expect("spawn chat reader");
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{ChatAction, RequestKind};

    fn request(id: &str) -> ChatRequest {
        ChatRequest {
            id: id.into(),
            kind: RequestKind::Permission,
            tool: "Bash".into(),
            action: ChatAction::Command,
            detail: "ls".into(),
            can_always: false,
            questions: Vec::new(),
        }
    }

    #[test]
    fn the_snapshot_is_what_the_events_say() {
        let mut snapshot = Snapshot::default();
        fold(&mut snapshot, &ChatEvent::Ready { models: Vec::new(), model: Some("opus".into()), effort: Some("high".into()) });
        assert!(snapshot.ready);
        fold(&mut snapshot, &ChatEvent::Busy { busy: true });
        fold(&mut snapshot, &ChatEvent::Draft { text: "Hel".into() });
        assert_eq!(snapshot.draft.as_deref(), Some("Hel"));
        fold(&mut snapshot, &ChatEvent::DraftDone);
        assert_eq!(snapshot.draft, None);
        fold(&mut snapshot, &ChatEvent::Request { request: request("r1") });
        fold(&mut snapshot, &ChatEvent::Request { request: request("r2") });
        fold(&mut snapshot, &ChatEvent::Resolved { id: "r1".into() });
        assert_eq!(snapshot.pending.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["r2"]);
        fold(&mut snapshot, &ChatEvent::Settings { model: Some("sonnet".into()), effort: None });
        assert_eq!(snapshot.model.as_deref(), Some("sonnet"));
        fold(&mut snapshot, &ChatEvent::QuickReplies { options: vec!["Red".into()] });
        fold(&mut snapshot, &ChatEvent::Busy { busy: true });
        assert!(snapshot.quick_replies.is_empty(), "replies to the last answer go once work resumes");
        fold(&mut snapshot, &ChatEvent::Exited { code: Some(0) });
        assert!(snapshot.ended && !snapshot.busy && snapshot.pending.is_empty());
    }

    #[test]
    fn a_chat_needs_a_folder_the_provider_trusts() {
        assert_eq!(trust_gate(Some(false)), Err("chat.folderNotTrusted".to_string()));
        assert_eq!(trust_gate(Some(true)), Ok(()));
        assert_eq!(trust_gate(None), Ok(()), "unknown is not untrusted");
    }

    #[test]
    fn only_providers_with_a_chat_protocol_get_a_command() {
        let (program, args) = command_for("claude-code", "sid", None, None, None).unwrap();
        assert_eq!(program, "claude");
        assert!(args.windows(2).any(|w| w == ["--permission-prompt-tool", "stdio"]));
        assert_eq!(command_for("codex", "sid", None, None, None).unwrap(), ("codex".to_string(), vec!["app-server".to_string()]));
        assert!(command_for("local", "sid", None, None, None).is_none());
        assert!(command_for("shell", "sid", None, None, None).is_none());
        assert!(driver_for("local", "C:/p").is_none());
    }
}
