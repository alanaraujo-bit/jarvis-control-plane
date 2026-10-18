//! The drivers against the real CLIs (M29). Ignored: they spend tokens on the
//! signed-in account and depend on two external programs. Run them when either
//! provider ships a version:
//!
//! `cargo test --lib chat::real_tests -- --ignored --nocapture --test-threads=1`
//!
//! Each runs in a fresh temporary repository on its **long** path: Claude Code
//! refuses to write under an 8.3 `ALANAR~1` segment, and `%TEMP%` is one here.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::model::{ChatAnswer, ChatEvent, Driver, Effect, RequestKind};
use super::process::ChatHandle;
use crate::pty::PtyOptions;

struct Harness {
    driver: Box<dyn Driver>,
    handle: ChatHandle,
    lines: Receiver<String>,
    written: Vec<Value>,
    events: Vec<ChatEvent>,
}

impl Harness {
    fn start(program: &str, args: Vec<String>, cwd: &Path, driver: Box<dyn Driver>) -> Self {
        let options = PtyOptions {
            program: program.into(),
            args,
            cwd: cwd.to_path_buf(),
            cols: 0,
            rows: 0,
            env: vec![],
            env_remove: vec![],
        };
        let (handle, stdout) = super::process::spawn(&options).expect("the CLI is installed");
        let (tx, lines) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut harness = Self { driver, handle, lines, written: Vec::new(), events: Vec::new() };
        let effects = harness.driver.start();
        harness.apply(effects);
        harness
    }

    fn apply(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::Write(line) => {
                    self.written.push(serde_json::from_str(&line).unwrap());
                    self.handle.send_line(&line).unwrap();
                }
                Effect::Emit(event) => self.events.push(event),
                _ => {}
            }
        }
    }

    /// Pump lines until `done` holds, answering every request with `answer`.
    fn until(&mut self, timeout: Duration, answer: &dyn Fn(RequestKind) -> ChatAnswer, done: &dyn Fn(&[ChatEvent]) -> bool) {
        let deadline = Instant::now() + timeout;
        while !done(&self.events) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "timed out; events so far: {:#?}", self.events);
            let Ok(line) = self.lines.recv_timeout(left) else { continue };
            let effects = self.driver.on_line(&line);
            let requests: Vec<_> = effects
                .iter()
                .filter_map(|e| match e {
                    Effect::Emit(ChatEvent::Request { request }) => Some((request.id.clone(), request.kind)),
                    _ => None,
                })
                .collect();
            self.apply(effects);
            for (id, kind) in requests {
                let effects = self.driver.answer(&id, answer(kind)).unwrap();
                self.apply(effects);
            }
        }
    }

    fn mark(&mut self) -> usize {
        self.events.clear();
        self.written.len()
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.handle.kill();
    }
}

/// A scratch repository on its long path.
fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let long = std::fs::canonicalize(dir.path()).unwrap();
    let text = long.to_string_lossy().to_string();
    let path = PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text));
    std::process::Command::new("git").arg("init").arg("-q").current_dir(&path).status().unwrap();
    std::fs::write(path.join("README.md"), "# scratch\n").unwrap();
    (dir, path)
}

fn allow_all(kind: RequestKind) -> ChatAnswer {
    match kind {
        RequestKind::Permission => ChatAnswer::Allow { always: false },
        RequestKind::Question => ChatAnswer::Answers { answers: Default::default() },
    }
}

fn idle(events: &[ChatEvent]) -> bool {
    events.contains(&ChatEvent::Busy { busy: false })
}

#[test]
#[ignore = "runs the real claude CLI and spends tokens"]
fn real_claude_chat_turn() {
    let (_dir, path) = scratch();
    let user_settings = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap()
        .join(".claude")
        .join("settings.json");
    let before = std::fs::read(&user_settings).ok();

    let id = uuid::Uuid::new_v4().to_string();
    let args = super::claude::launch_args(&id, None, None, None);
    let mut chat = Harness::start("claude", args, &path, Box::new(super::claude::ClaudeDriver::new()));
    chat.until(Duration::from_secs(60), &allow_all, &|e| e.iter().any(|e| matches!(e, ChatEvent::Ready { .. })));

    // Two messages back to back: Claude queues the second, and busy must hold
    // until both turns have ended.
    let effects = chat.driver.send("Create a file named hello.txt containing exactly: hi");
    chat.apply(effects);
    let effects = chat.driver.send("Then reply with the single word: done");
    chat.apply(effects);
    chat.until(Duration::from_secs(240), &allow_all, &idle);
    assert_eq!(std::fs::read_to_string(path.join("hello.txt")).unwrap().trim(), "hi");
    assert!(chat.events.iter().any(|e| matches!(e, ChatEvent::Request { .. })), "the write was asked for");
    assert_eq!(
        chat.events.iter().filter(|e| **e == ChatEvent::Busy { busy: false }).count(),
        1,
        "busy ends once, after both turns"
    );

    chat.mark();
    let effects = chat.driver.set_model("haiku");
    chat.apply(effects);
    chat.until(Duration::from_secs(30), &allow_all, &|e| e.iter().any(|e| matches!(e, ChatEvent::Settings { .. })));
    assert!(chat.events.contains(&ChatEvent::Settings { model: Some("haiku".into()), effort: chat_effort(&chat.events) }));
    assert_eq!(std::fs::read(&user_settings).ok(), before, "a chat's model switch never touches the person's settings");

    // The transcript the tailer follows is written as for a terminal (D3).
    assert!(
        crate::providers::claude::find_transcript(&id).is_some(),
        "the conversation is on disk where the tailer looks"
    );
}

fn chat_effort(events: &[ChatEvent]) -> Option<String> {
    events.iter().find_map(|e| match e {
        ChatEvent::Settings { effort, .. } => effort.clone(),
        _ => None,
    })
}

#[test]
#[ignore = "runs the real codex CLI and spends tokens"]
fn real_codex_chat_turn() {
    let (_dir, path) = scratch();
    let cwd = path.to_string_lossy().to_string();
    let mut chat = Harness::start("codex", vec!["app-server".into()], &path, Box::new(super::codex::CodexDriver::new(cwd)));
    chat.until(Duration::from_secs(60), &allow_all, &|e| e.iter().any(|e| matches!(e, ChatEvent::Ready { .. })));
    let (models, current) = chat
        .events
        .iter()
        .find_map(|e| match e {
            ChatEvent::Ready { models, model, .. } => Some((models.clone(), model.clone())),
            _ => None,
        })
        .unwrap();

    chat.mark();
    let effects = chat
        .driver
        .send("Run the shell command node -e \"setTimeout(()=>{},15000)\", then create a file named hello2.txt containing exactly: hi");
    chat.apply(effects);
    // `send` says busy at once; `turn/started` says it again once the turn has
    // a name, which is when a second message can steer it.
    chat.until(Duration::from_secs(60), &allow_all, &|e| {
        e.iter().filter(|e| **e == ChatEvent::Busy { busy: true }).count() >= 2
    });
    let from = chat.written.len();
    let effects = chat.driver.send("When you are done, end your reply with the word: steered");
    chat.apply(effects);
    assert_eq!(chat.written[from]["method"], "turn/steer", "a message mid-turn goes into that turn");
    chat.until(Duration::from_secs(300), &allow_all, &idle);
    assert_eq!(std::fs::read_to_string(path.join("hello2.txt")).unwrap().trim(), "hi");

    let other = models.iter().find(|m| Some(&m.id) != current.as_ref()).expect("a second model").id.clone();
    let effects = chat.driver.set_model(&other);
    chat.apply(effects);
    let from = chat.mark();
    let effects = chat.driver.send("Answer with one number: 2+2");
    chat.apply(effects);
    chat.until(Duration::from_secs(120), &allow_all, &idle);
    assert_eq!(chat.written[from]["method"], "turn/start");
    assert_eq!(chat.written[from]["params"]["model"], other.as_str(), "the switch rides the next turn");
}
