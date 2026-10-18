//! Live session management.
//!
//! Ties three things together for each running session:
//!
//! * a PTY (the process),
//! * the append-only log (the record),
//! * a channel to the UI (the view).
//!
//! ## One writer, many producers
//!
//! `SessionLog::append` needs `&mut self`, and a session has two producers: the
//! PTY reader and (from M3) the provider transcript tailer. Sharing the log
//! behind a mutex would give contention and, worse, no ordering guarantee.
//! Instead a single writer thread owns the log and both producers send into one
//! channel. The §23 ordering guarantee is then structural: whatever reaches the
//! channel first is what happened first.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::event::{EventKind, Lifecycle, SessionState};
use super::log::{now_ms, SessionLog};
use crate::chat::process::ChatHandle;
use crate::pty::{self, PtyEvent, PtyHandle, PtyOptions};

/// How often coalesced terminal output is pushed to the UI.
///
/// One frame at 60Hz. Emitting per PTY read would flood the webview event loop
/// during a burst of build output; batching keeps the UI responsive without
/// perceptible lag.
const FLUSH_INTERVAL: Duration = Duration::from_millis(16);

/// Flush early once this much output has accumulated, so a large burst is not
/// held back waiting for the tick.
const MAX_COALESCE: usize = 64 * 1024;

/// ConPTY's startup handshake: "report the cursor position".
const DSR_QUERY: &[u8] = b"\x1b[6n";

/// The reply a terminal gives. See docs/DECISIONS.md D6 for why the core sends
/// this itself when no view is attached.
const DSR_REPLY: &[u8] = b"\x1b[1;1R";

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error(transparent)]
    Pty(#[from] pty::PtyError),
    #[error(transparent)]
    Log(#[from] super::log::LogError),
    #[error("no such session: {0}")]
    Unknown(String),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error(transparent)]
    DbOpen(#[from] crate::db::DbError),
    /// A refusal the surface localises (§65). The payload is a stable code,
    /// never prose — `resume.unsupported` and friends.
    #[error("{0}")]
    Refused(String),
}

impl serde::Serialize for SessionError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SessionError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub project_id: String,
    pub provider: String,
    pub title: Option<String>,
    pub cwd: String,
    pub state: SessionState,
    pub created_at: i64,
    /// True while the process is alive.
    pub live: bool,
    /// Terminal or chat (M29).
    pub mode: super::event::SessionMode,
}

/// A message for the session's single log-writer thread.
enum LogCommand {
    Append { kind: EventKind, payload: Vec<u8> },
    /// Everything sent before this command is durable when the reply arrives.
    /// Used to make terminal replay and live attachment one gap-free boundary.
    Barrier(Sender<()>),
    Stop,
}

struct AttachedView {
    id: String,
    channel: Channel<InvokeResponseBody>,
}

/// What a session's process is: a terminal, or a protocol pipe (M29).
enum Process {
    Terminal(Arc<PtyHandle>),
    Chat(Arc<ChatHandle>),
}

/// A running session.
pub struct LiveSession {
    pub id: String,
    /// The current provider process. Behind a lock because an account switch
    /// replaces it while the session goes on (M26).
    process: Mutex<Process>,
    /// The current pump's retirement flag. `replace_process` sets it on the
    /// pump it is retiring, so that pump records the exit without ending the
    /// session; each pump gets its own, so the new process is never mistaken
    /// for the old one.
    retiring: Mutex<Arc<AtomicBool>>,
    /// Signalled by a pump when it finishes; `replace_process` waits on it.
    pump_done: Mutex<Option<Receiver<()>>>,
    /// The terminal geometry, carried onto a replacement process.
    size: Mutex<(u16, u16)>,
    /// Steers the transcript tailer when the conversation moves (M26).
    tail: Mutex<Option<crate::session::transcript::TailControl>>,
    log_tx: Sender<LogCommand>,
    /// Whether a terminal view is currently rendering this session.
    view_attached: Arc<AtomicBool>,
    /// The UI sink and the identity of the mount that owns it. A cleanup from
    /// an older React mount must never detach a newer one.
    sink: Arc<Mutex<Option<AttachedView>>>,
    /// Serialises the pump's output boundary with replay + attachment.
    view_gate: Arc<Mutex<()>>,
    state: Arc<Mutex<SessionState>>,
    /// Set when the session ends, so background followers wind down.
    stopped: Arc<AtomicBool>,
    /// The notification watcher, when this session has one (§49).
    ///
    /// An unbounded `Sender`, so this can never block the pump. The pump is
    /// what keeps the terminal and the log alive, and a watcher that fell
    /// behind must cost nothing more than its own lateness.
    watch: Arc<Mutex<Option<Sender<crate::notify::watch::Beat>>>>,
}

impl LiveSession {
    pub fn write(&self, data: &[u8]) -> Result<()> {
        // A chat's stdin is a protocol. Raw bytes from dictation, the Notebook,
        // the autopilot or the rotation relay would corrupt it, so they are
        // refused here, once, for every caller (M29).
        let pty = match &*self.process.lock() {
            Process::Terminal(pty) => Arc::clone(pty),
            Process::Chat(_) => return Err(SessionError::Refused("session.chatMode".into())),
        };
        // Record what was typed before sending it, so the log reflects intent
        // even if the process dies on receiving it.
        let _ = self.log_tx.send(LogCommand::Append {
            kind: EventKind::PtyInput,
            payload: data.to_vec(),
        });
        // Something was answered. The watcher needs to know before it decides a
        // question is still outstanding (§49).
        if let Some(watch) = self.watch.lock().as_ref() {
            let _ = watch.send(crate::notify::watch::Beat::Input);
        }
        pty.write(data)?;
        Ok(())
    }

    /// Whether this session is a chat (M29).
    pub fn is_chat(&self) -> bool {
        matches!(&*self.process.lock(), Process::Chat(_))
    }

    /// The protocol pipe of a chat session.
    pub fn chat(&self) -> Option<Arc<ChatHandle>> {
        match &*self.process.lock() {
            Process::Chat(handle) => Some(Arc::clone(handle)),
            Process::Terminal(_) => None,
        }
    }

    /// Set what the session is doing. A chat knows exactly, from its protocol,
    /// where a terminal can only infer it from output going quiet.
    pub fn set_state(&self, state: SessionState) {
        *self.state.lock() = state;
    }

    /// The end a terminal's pump records when its process exits: the exit
    /// frame, the final state, the watcher told, the writer stopped. A chat's
    /// reader calls it when the protocol stream closes.
    pub fn finish(&self, code: Option<i32>) {
        let _ = self.log_tx.send(LogCommand::Append {
            kind: EventKind::Lifecycle,
            payload: serde_json::to_vec(&Lifecycle::Exited { code }).unwrap_or_default(),
        });
        *self.state.lock() = match code {
            Some(0) | None => SessionState::Completed,
            Some(_) => SessionState::Failed,
        };
        if let Some(watch) = self.watch.lock().as_ref() {
            let _ = watch.send(crate::notify::watch::Beat::Exited(code));
        }
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.log_tx.send(LogCommand::Stop);
    }

    /// Point this session's terminal stream at a notification watcher (§49).
    ///
    /// Separate from `start` because a watcher needs the database, the project
    /// and the provider, none of which the session manager knows or should.
    pub fn set_watch(&self, tx: Sender<crate::notify::watch::Beat>) {
        *self.watch.lock() = Some(tx);
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        // A chat has no geometry; there is nothing to lay out or record.
        let pty = match &*self.process.lock() {
            Process::Terminal(pty) => Arc::clone(pty),
            Process::Chat(_) => return Ok(()),
        };
        pty.resize(cols, rows)?;
        *self.size.lock() = (cols, rows);
        let payload = serde_json::to_vec(&Lifecycle::Resized { cols, rows }).unwrap_or_default();
        let _ = self.log_tx.send(LogCommand::Append {
            kind: EventKind::Lifecycle,
            payload,
        });
        Ok(())
    }

    pub fn set_tail(&self, control: crate::session::transcript::TailControl) {
        *self.tail.lock() = Some(control);
    }

    pub fn tail(&self) -> Option<crate::session::transcript::TailControl> {
        self.tail.lock().clone()
    }

    pub fn state(&self) -> SessionState {
        *self.state.lock()
    }

    /// Append a structured frame to this session's log.
    ///
    /// Used by the provider transcript tailer, which is the second producer
    /// feeding the single writer. Going through the same channel as the PTY
    /// reader is what preserves ordering between what the terminal showed and
    /// what the provider reported (§23).
    pub fn log(&self, kind: EventKind, payload: Vec<u8>) {
        let _ = self.log_tx.send(LogCommand::Append { kind, payload });
    }

    /// Signals the transcript tailer to stop when the session ends.
    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stopped)
    }

    /// Point the session's output at a terminal view.
    ///
    /// From here on the view answers terminal queries, so the core stops
    /// standing in for it.
    pub fn attach_with_replay<F>(
        &self,
        attachment_id: String,
        ui_channel: Channel<InvokeResponseBody>,
        replay: F,
    ) -> Result<Vec<u8>>
    where
        F: FnOnce() -> Result<Vec<u8>>,
    {
        let _boundary = self.view_gate.lock();
        let (done_tx, done_rx) = channel();
        self.log_tx
            .send(LogCommand::Barrier(done_tx))
            .map_err(|_| SessionError::Refused("session.logClosed".into()))?;
        done_rx
            .recv()
            .map_err(|_| SessionError::Refused("session.logClosed".into()))?;
        let history = replay()?;
        *self.sink.lock() = Some(AttachedView {
            id: attachment_id,
            channel: ui_channel,
        });
        self.view_attached.store(true, Ordering::SeqCst);
        Ok(history)
    }

    /// Detach the view. The session keeps running and keeps logging — closing a
    /// tab must never stop an agent that is mid-task (§32).
    pub fn detach(&self, attachment_id: &str) -> bool {
        let _boundary = self.view_gate.lock();
        let mut sink = self.sink.lock();
        if !owns_attachment(sink.as_ref().map(|view| view.id.as_str()), attachment_id) {
            return false;
        }
        *sink = None;
        self.view_attached.store(false, Ordering::SeqCst);
        true
    }

    /// Replace the provider process, keeping this session — id, log, attached
    /// view, watcher — exactly as it is (M26).
    ///
    /// The old child is killed and its pump allowed to finish: it records the
    /// exit, but does not stop the log writer, tell the notification watcher
    /// the session ended, or mark it finished. Then the new process starts at
    /// the same terminal size on a new pump feeding the same log.
    /// End the session when a replacement left it with no process (M26): the
    /// end its pump would have recorded, so nothing reads it as still running.
    pub fn end_without_process(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        *self.state.lock() = SessionState::Failed;
        if let Some(watch) = self.watch.lock().as_ref() {
            let _ = watch.send(crate::notify::watch::Beat::Exited(Some(1)));
        }
        let _ = self.log_tx.send(LogCommand::Stop);
    }

    pub fn replace_process(&self, options: PtyOptions) -> Result<()> {
        self.replace_process_with(options, || Ok(())).map_err(SessionError::Refused)
    }

    /// The exit code of the current process, if it has ended.
    pub fn exited(&self) -> Option<i32> {
        match &*self.process.lock() {
            Process::Terminal(pty) => pty.try_wait(),
            Process::Chat(chat) => chat.try_wait(),
        }
    }

    /// `replace_process`, with `prepare` run after the old process has exited
    /// and before the new one starts — the window in which a transcript can be
    /// copied without the old CLI still writing to it. If `prepare` fails,
    /// nothing is started and the error is returned.
    pub fn replace_process_with(
        &self,
        options: PtyOptions,
        prepare: impl FnOnce() -> std::result::Result<(), String>,
    ) -> std::result::Result<(), String> {
        // Only a terminal is relaunched in place; a chat would come back as a
        // TUI nobody can see (M29).
        let old = match &*self.process.lock() {
            Process::Terminal(pty) => Arc::clone(pty),
            Process::Chat(_) => return Err("session.chatMode".into()),
        };
        // Retire the current pump before its process dies.
        self.retiring.lock().store(true, Ordering::SeqCst);
        let old_done = self.pump_done.lock().take();
        let _ = old.kill();
        drop(old);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.exited().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        prepare()?;
        // Closed while the old process was going down: start nothing for it.
        if self.stopped.load(Ordering::SeqCst) {
            return Err("session.stopped".into());
        }

        let (cols, rows) = *self.size.lock();
        let relaunched = Lifecycle::Relaunched {
            command: format!("{} {}", options.program, options.args.join(" ")),
        };
        let (handle, rx) =
            pty::spawn(PtyOptions { cols, rows, ..options }).map_err(|e| e.to_string())?;
        let handle = Arc::new(handle);
        // Swapping drops the last strong reference to the old pseudoconsole.
        // On Windows that is what ends the old reader: ConPTY yields EOF only
        // once it is closed, not when the child exits.
        let old = std::mem::replace(&mut *self.process.lock(), Process::Terminal(Arc::clone(&handle)));
        drop(old);
        if let Some(done) = old_done {
            let _ = done.recv_timeout(Duration::from_secs(15));
        }

        self.log(EventKind::Lifecycle, serde_json::to_vec(&relaunched).unwrap_or_default());
        *self.state.lock() = SessionState::Starting;
        let retiring = Arc::new(AtomicBool::new(false));
        *self.retiring.lock() = Arc::clone(&retiring);
        let (done_tx, done_rx) = channel();
        *self.pump_done.lock() = Some(done_rx);
        spawn_pump(self, &handle, rx, self.log_tx.clone(), done_tx, retiring);
        Ok(())
    }

    pub fn kill(&self) -> Result<()> {
        // Stop the follower first: it must not keep polling a transcript for a
        // session that no longer exists.
        self.stopped.store(true, Ordering::SeqCst);
        match &*self.process.lock() {
            Process::Terminal(pty) => pty.kill()?,
            Process::Chat(chat) => chat.kill(),
        }
        let _ = self.log_tx.send(LogCommand::Stop);
        Ok(())
    }
}

/// All live sessions in the application.
#[derive(Default)]
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Arc<LiveSession>>>,
}

impl SessionManager {
    pub fn get(&self, id: &str) -> Result<Arc<LiveSession>> {
        self.sessions
            .lock()
            .get(id)
            .cloned()
            .ok_or_else(|| SessionError::Unknown(id.to_string()))
    }

    pub fn remove(&self, id: &str) {
        self.sessions.lock().remove(id);
    }

    pub fn ids(&self) -> Vec<String> {
        self.sessions.lock().keys().cloned().collect()
    }

    /// Start a session: open the log, spawn the process, wire the pump.
    pub fn start(
        &self,
        id: String,
        log_dir: std::path::PathBuf,
        options: PtyOptions,
    ) -> Result<Arc<LiveSession>> {
        let log_tx = open_writer(&id, &log_dir, &options)?;

        let (cols, rows) = (options.cols, options.rows);
        let (handle, pty_rx) = pty::spawn(options)?;
        let pty = Arc::new(handle);

        let (done_tx, done_rx) = channel();
        let retiring = Arc::new(AtomicBool::new(false));
        let session = Arc::new(LiveSession::new(
            id.clone(),
            Process::Terminal(Arc::clone(&pty)),
            Arc::clone(&retiring),
            Some(done_rx),
            (cols, rows),
            log_tx.clone(),
        ));

        spawn_pump(&session, &pty, pty_rx, log_tx, done_tx, retiring);

        self.sessions.lock().insert(id, Arc::clone(&session));
        Ok(session)
    }

    /// Start a chat session over a process already spawned (M29): the same log
    /// and single writer as a terminal, and no pump — the chat runtime reads
    /// the protocol and calls `LiveSession::finish` when it closes.
    pub fn start_chat(
        &self,
        id: String,
        log_dir: std::path::PathBuf,
        options: &PtyOptions,
        handle: ChatHandle,
    ) -> Result<Arc<LiveSession>> {
        let log_tx = open_writer(&id, &log_dir, options)?;
        let session = Arc::new(LiveSession::new(
            id.clone(),
            Process::Chat(Arc::new(handle)),
            Arc::new(AtomicBool::new(false)),
            None,
            (options.cols, options.rows),
            log_tx,
        ));
        self.sessions.lock().insert(id, Arc::clone(&session));
        Ok(session)
    }
}

impl LiveSession {
    fn new(
        id: String,
        process: Process,
        retiring: Arc<AtomicBool>,
        pump_done: Option<Receiver<()>>,
        size: (u16, u16),
        log_tx: Sender<LogCommand>,
    ) -> Self {
        Self {
            id,
            process: Mutex::new(process),
            retiring: Mutex::new(retiring),
            pump_done: Mutex::new(pump_done),
            size: Mutex::new(size),
            tail: Mutex::new(None),
            log_tx,
            view_attached: Arc::new(AtomicBool::new(false)),
            sink: Arc::new(Mutex::new(None)),
            view_gate: Arc::new(Mutex::new(())),
            state: Arc::new(Mutex::new(SessionState::Starting)),
            stopped: Arc::new(AtomicBool::new(false)),
            watch: Arc::new(Mutex::new(None)),
        }
    }
}

/// Open a session's log, record how it began, and start its single writer.
fn open_writer(id: &str, log_dir: &std::path::Path, options: &PtyOptions) -> Result<Sender<LogCommand>> {
    let mut log = SessionLog::open(log_dir)?;

    // Record how the session began, so the log alone explains it later.
    let started = Lifecycle::Started {
        command: format!("{} {}", options.program, options.args.join(" ")),
        cwd: options.cwd.to_string_lossy().to_string(),
    };
    log.append(
        EventKind::Lifecycle,
        &serde_json::to_vec(&started).unwrap_or_default(),
    )?;
    log.append(
        EventKind::Lifecycle,
        &serde_json::to_vec(&Lifecycle::Resized {
            cols: options.cols,
            rows: options.rows,
        })
        .unwrap_or_default(),
    )?;

    // ---- The single writer thread ------------------------------------
    let (log_tx, log_rx) = channel::<LogCommand>();
    std::thread::Builder::new()
        .name(format!("session-log-{id}"))
        .spawn(move || {
            while let Ok(command) = log_rx.recv() {
                match command {
                    LogCommand::Append { kind, payload } => {
                        if let Err(e) = log.append(kind, &payload) {
                            tracing::error!(error = %e, "failed to append to the session log");
                        }
                    }
                    LogCommand::Barrier(done) => {
                        let _ = done.send(());
                    }
                    LogCommand::Stop => break,
                }
            }
        })
        .expect("spawn session log writer");
    Ok(log_tx)
}

/// The pump: PTY output → log, → UI, and terminal-query handling.
fn spawn_pump(
    session: &LiveSession,
    pty: &Arc<PtyHandle>,
    rx: Receiver<PtyEvent>,
    log_tx: Sender<LogCommand>,
    done: Sender<()>,
    replacing: Arc<AtomicBool>,
) {
    // Weak: the pump must not keep a replaced pseudoconsole alive (see
    // `replace_process`).
    let pty = Arc::downgrade(pty);
    let sink = Arc::clone(&session.sink);
    let view_gate = Arc::clone(&session.view_gate);
    let view_attached = Arc::clone(&session.view_attached);
    let state = Arc::clone(&session.state);
    let watch = Arc::clone(&session.watch);
    let id = session.id.clone();

    std::thread::Builder::new()
        .name(format!("session-pump-{id}"))
        .spawn(move || {
            let mut pending: Vec<u8> = Vec::with_capacity(MAX_COALESCE);
            let mut pending_owner: Option<String> = None;
            let mut last_flush = Instant::now();

            let flush = |pending: &mut Vec<u8>, pending_owner: &mut Option<String>| {
                if pending.is_empty() {
                    *pending_owner = None;
                    return;
                }
                let current = sink.lock();
                if let Some(view) = current
                    .as_ref()
                    .filter(|view| pending_owner.as_deref() == Some(view.id.as_str()))
                {
                    // Raw bytes, not JSON: the webview receives an ArrayBuffer,
                    // which xterm can consume directly. Decoding to a string
                    // here would corrupt UTF-8 split across read boundaries.
                    let _ = view
                        .channel
                        .send(InvokeResponseBody::Raw(std::mem::take(pending)));
                } else {
                    pending.clear();
                }
                *pending_owner = None;
            };

            loop {
                let timeout = FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());
                match rx.recv_timeout(timeout) {
                    Ok(PtyEvent::Output(bytes)) => {
                        let _boundary = view_gate.lock();
                        // The log is authoritative and gets every byte, exactly
                        // as read, regardless of what the UI does with it.
                        let _ = log_tx.send(LogCommand::Append {
                            kind: EventKind::PtyOutput,
                            payload: bytes.clone(),
                        });

                        // The notification watcher sees the same bytes (§49).
                        // Sent, never inspected here: whether this output means
                        // anything is entirely the watcher's business, and the
                        // pump must stay a pump.
                        if let Some(watch) = watch.lock().as_ref() {
                            let _ = watch.send(crate::notify::watch::Beat::Output(bytes.clone()));
                        }

                        // Stand in for the terminal when nobody is watching.
                        // Without this the session stalls forever in unattended
                        // mode (docs/DECISIONS.md D6).
                        if !view_attached.load(Ordering::SeqCst)
                            && contains(&bytes, DSR_QUERY)
                        {
                            if let Some(pty) = pty.upgrade() {
                                let _ = pty.write(DSR_REPLY);
                            }
                        }

                        {
                            let mut current = state.lock();
                            if *current == SessionState::Starting || *current == SessionState::Idle {
                                *current = SessionState::Working;
                            }
                        }

                        let owner = sink.lock().as_ref().map(|view| view.id.clone());
                        if pending_owner != owner {
                            pending.clear();
                            pending_owner = owner;
                        }
                        if pending_owner.is_some() {
                            pending.extend_from_slice(&bytes);
                        }
                        if pending.len() >= MAX_COALESCE {
                            flush(&mut pending, &mut pending_owner);
                            last_flush = Instant::now();
                        }
                    }
                    Ok(PtyEvent::Exited(code)) => {
                        let _boundary = view_gate.lock();
                        flush(&mut pending, &mut pending_owner);
                        let _ = log_tx.send(LogCommand::Append {
                            kind: EventKind::Lifecycle,
                            payload: serde_json::to_vec(&Lifecycle::Exited { code })
                                .unwrap_or_default(),
                        });
                        // A planned replacement: the session goes on under the
                        // next process, so nothing downstream may hear it ended.
                        if replacing.load(Ordering::SeqCst) {
                            break;
                        }
                        *state.lock() = match code {
                            Some(0) | None => SessionState::Completed,
                            Some(_) => SessionState::Failed,
                        };
                        if let Some(watch) = watch.lock().as_ref() {
                            let _ = watch.send(crate::notify::watch::Beat::Exited(code));
                        }
                        let _ = log_tx.send(LogCommand::Stop);
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let _boundary = view_gate.lock();
                        flush(&mut pending, &mut pending_owner);
                        last_flush = Instant::now();
                        // Quiet for a while means the agent is waiting on us.
                        let mut current = state.lock();
                        if *current == SessionState::Working {
                            *current = SessionState::Idle;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        let _boundary = view_gate.lock();
                        flush(&mut pending, &mut pending_owner);
                        if !replacing.load(Ordering::SeqCst) {
                            let _ = log_tx.send(LogCommand::Stop);
                        }
                        break;
                    }
                }
            }
            let _ = done.send(());
            tracing::debug!(session = %id, "session pump finished");
        })
        .expect("spawn session pump");
}

fn owns_attachment(current: Option<&str>, requested: &str) -> bool {
    current == Some(requested)
}

/// Naive substring search over bytes. The haystack is a single PTY read and the
/// needle is four bytes, so anything cleverer would be noise.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

pub fn new_session_id() -> String {
    // v7 is time-ordered, so session directories sort chronologically on disk.
    uuid::Uuid::now_v7().to_string()
}

pub fn timestamp() -> i64 {
    now_ms()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_terminal_query_in_a_chunk() {
        assert!(contains(b"\x1b[6n", DSR_QUERY));
        assert!(contains(b"prefix\x1b[6nsuffix", DSR_QUERY));
        assert!(!contains(b"\x1b[6", DSR_QUERY));
        assert!(!contains(b"", DSR_QUERY));
    }

    #[test]
    fn a_stale_detach_cannot_own_a_newer_attachment() {
        assert!(owns_attachment(Some("new-view"), "new-view"));
        assert!(!owns_attachment(Some("new-view"), "old-view"));
        assert!(!owns_attachment(None, "old-view"));
    }

    /// A chat over a real child that echoes its stdin: the stand-in for a CLI
    /// speaking a line protocol (M29). `None` where `node` is not installed.
    fn echo_chat(dir: &std::path::Path) -> Option<(Arc<LiveSession>, std::process::ChildStdout)> {
        crate::envscan::tool_command("node", &["--version"])?;
        let options = PtyOptions {
            program: "node".into(),
            args: vec!["-e".into(), "process.stdin.pipe(process.stdout)".into()],
            cwd: std::env::temp_dir(),
            cols: 0,
            rows: 0,
            env: vec![],
            env_remove: Vec::new(),
        };
        let (handle, stdout) = crate::chat::process::spawn(&options).expect("spawn the echo child");
        let session = SessionManager::default()
            .start_chat(new_session_id(), dir.to_path_buf(), &options, handle)
            .expect("start the chat session");
        Some((session, stdout))
    }

    #[test]
    fn a_chat_session_refuses_raw_terminal_input() {
        let dir = tempfile::tempdir().unwrap();
        let Some((session, _stdout)) = echo_chat(dir.path()) else {
            eprintln!("node is not installed; skipping");
            return;
        };
        assert!(session.is_chat());
        match session.write(b"x") {
            Err(SessionError::Refused(code)) => assert_eq!(code, "session.chatMode"),
            other => panic!("raw bytes must never reach a protocol pipe: {other:?}"),
        }
        assert!(session.resize(80, 24).is_ok(), "a chat has no geometry to refuse");
        // The rotation relay's way in (M26): a chat would come back as a TUI.
        let relaunched = session.replace_process(PtyOptions {
            program: "node".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: vec![],
            env_remove: Vec::new(),
        });
        assert!(matches!(relaunched, Err(SessionError::Refused(code)) if code == "session.chatMode"));
        assert!(session.is_chat(), "the chat is untouched");
        let _ = session.kill();
    }

    #[test]
    fn a_chat_process_echoes_a_line() {
        use std::io::BufRead;
        let dir = tempfile::tempdir().unwrap();
        let Some((session, stdout)) = echo_chat(dir.path()) else {
            eprintln!("node is not installed; skipping");
            return;
        };
        session.chat().unwrap().send_line(r#"{"type":"hello"}"#).unwrap();
        let mut line = String::new();
        std::io::BufReader::new(stdout).read_line(&mut line).unwrap();
        assert_eq!(line.trim_end(), r#"{"type":"hello"}"#);
        let _ = session.kill();
    }

    #[test]
    fn finishing_a_chat_session_records_its_exit() {
        let dir = tempfile::tempdir().unwrap();
        let Some((session, _stdout)) = echo_chat(dir.path()) else {
            eprintln!("node is not installed; skipping");
            return;
        };
        // What the reader does when the protocol stream closes on its own.
        session.finish(Some(0));
        assert_eq!(session.state(), SessionState::Completed);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exited = false;
        while Instant::now() < deadline && !exited {
            let reader = super::super::log::SessionLogReader::open(dir.path()).unwrap();
            exited = reader.read_from(0).unwrap().iter().any(|event| {
                event.kind == EventKind::Lifecycle
                    && event.payload_str().is_some_and(|text| text.contains("\"exited\""))
            });
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = session.kill();
        assert!(exited, "the log must say the chat ended");
    }

    /// End-to-end proof that a session with **no view attached** still runs.
    ///
    /// This is the unattended case (§32) and the one that silently broke before
    /// the core learned to answer ConPTY's startup query.
    #[test]
    fn a_session_without_a_view_still_produces_output() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let id = new_session_id();

        let (program, args) = if cfg!(windows) {
            ("cmd".to_string(), vec!["/c".to_string(), "echo unattended-ok".to_string()])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), "echo unattended-ok".to_string()])
        };

        let session = manager
            .start(
                id.clone(),
                dir.path().to_path_buf(),
                PtyOptions {
                    program,
                    args,
                    cwd: std::env::temp_dir(),
                    cols: 80,
                    rows: 24,
                    env: vec![],
                    env_remove: Vec::new(),
                },
            )
            .expect("start session");

        // Wait for the log to contain the output — no UI, no channel, nobody
        // watching, exactly as an unattended agent would run.
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut found = false;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(150));
            let reader = super::super::log::SessionLogReader::open(dir.path()).unwrap();
            let replayed = reader.replay_pty(usize::MAX).unwrap();
            if String::from_utf8_lossy(&replayed).contains("unattended-ok") {
                found = true;
                break;
            }
        }

        let _ = session.kill();
        assert!(
            found,
            "an unattended session must still run and record its output"
        );
    }

    /// An account switch swaps the provider process under a session that keeps
    /// its id, its log and its attached view (M26).
    #[test]
    fn replace_process_keeps_id_log_and_attachment() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let id = new_session_id();
        let echo = |word: &str| {
            if cfg!(windows) {
                ("cmd".to_string(), vec!["/c".to_string(), format!("echo {word}& ping -n 30 127.0.0.1 >NUL")])
            } else {
                ("sh".to_string(), vec!["-c".to_string(), format!("echo {word}; sleep 30")])
            }
        };
        let options = |word: &str| {
            let (program, args) = echo(word);
            PtyOptions {
                program,
                args,
                cwd: std::env::temp_dir(),
                cols: 80,
                rows: 24,
                env: vec![],
                env_remove: Vec::new(),
            }
        };
        let session = manager.start(id.clone(), dir.path().to_path_buf(), options("first-proc")).unwrap();
        let (watch_tx, watch_rx) = channel();
        session.set_watch(watch_tx);

        let read_log = || {
            let reader = super::super::log::SessionLogReader::open(dir.path()).unwrap();
            String::from_utf8_lossy(&reader.replay_pty(usize::MAX).unwrap()).to_string()
        };
        let wait_for = |needle: &str| {
            let deadline = Instant::now() + Duration::from_secs(20);
            while Instant::now() < deadline {
                if read_log().contains(needle) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            false
        };
        assert!(wait_for("first-proc"));

        let started = Instant::now();
        session.replace_process(options("second-proc")).expect("replace");
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the old pump must finish promptly, not time out: {:?}",
            started.elapsed()
        );
        assert!(wait_for("second-proc"), "the new process must write into the same log");
        assert_eq!(session.id, id);
        assert!(manager.get(&id).is_ok());
        assert!(
            !matches!(session.state(), SessionState::Completed | SessionState::Failed),
            "a replaced process is not a finished session"
        );

        let reader = super::super::log::SessionLogReader::open(dir.path()).unwrap();
        let lifecycle: Vec<String> = reader
            .read_from(0)
            .unwrap()
            .iter()
            .filter(|e| e.kind == EventKind::Lifecycle)
            .map(|e| String::from_utf8_lossy(&e.payload).to_string())
            .collect();
        assert!(lifecycle.iter().any(|l| l.contains("relaunched")), "{lifecycle:?}");

        let beats: Vec<_> = watch_rx.try_iter().collect();
        assert!(
            !beats.iter().any(|b| matches!(b, crate::notify::watch::Beat::Exited(_))),
            "the notification watcher must not hear about a planned replacement"
        );
        let _ = session.kill();
    }

    /// Input written to a session is recorded before it is sent.
    #[test]
    fn typed_input_is_recorded_in_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let id = new_session_id();
        let program = if cfg!(windows) { "cmd" } else { "sh" };

        let session = manager
            .start(
                id,
                dir.path().to_path_buf(),
                PtyOptions {
                    program: program.into(),
                    args: vec![],
                    cwd: std::env::temp_dir(),
                    cols: 80,
                    rows: 24,
                    env: vec![],
                    env_remove: Vec::new(),
                },
            )
            .expect("start session");

        session.write(b"echo recorded-input\r\n").expect("write");
        std::thread::sleep(Duration::from_millis(600));

        let reader = super::super::log::SessionLogReader::open(dir.path()).unwrap();
        let events = reader.read_from(0).unwrap();
        let input: Vec<_> = events
            .iter()
            .filter(|e| e.kind == EventKind::PtyInput)
            .collect();

        assert!(!input.is_empty(), "keystrokes must be logged");
        assert_eq!(input[0].payload, b"echo recorded-input\r\n");

        // The session opens by recording how it started and how big it was.
        let lifecycle: Vec<_> = events
            .iter()
            .filter(|e| e.kind == EventKind::Lifecycle)
            .collect();
        assert!(lifecycle.len() >= 2, "start and size must be recorded");

        let _ = session.kill();
    }
}
