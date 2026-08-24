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
}

/// A message for the session's single log-writer thread.
enum LogCommand {
    Append { kind: EventKind, payload: Vec<u8> },
    Stop,
}

/// A running session.
pub struct LiveSession {
    pub id: String,
    pty: Arc<PtyHandle>,
    log_tx: Sender<LogCommand>,
    /// Whether a terminal view is currently rendering this session.
    view_attached: Arc<AtomicBool>,
    /// The UI sink. Replaced whenever a view attaches or detaches.
    sink: Arc<Mutex<Option<Channel<InvokeResponseBody>>>>,
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
        self.pty.write(data)?;
        Ok(())
    }

    /// Point this session's terminal stream at a notification watcher (§49).
    ///
    /// Separate from `start` because a watcher needs the database, the project
    /// and the provider, none of which the session manager knows or should.
    pub fn set_watch(&self, tx: Sender<crate::notify::watch::Beat>) {
        *self.watch.lock() = Some(tx);
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.pty.resize(cols, rows)?;
        let payload = serde_json::to_vec(&Lifecycle::Resized { cols, rows }).unwrap_or_default();
        let _ = self.log_tx.send(LogCommand::Append {
            kind: EventKind::Lifecycle,
            payload,
        });
        Ok(())
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
    pub fn attach(&self, channel: Channel<InvokeResponseBody>) {
        *self.sink.lock() = Some(channel);
        self.view_attached.store(true, Ordering::SeqCst);
    }

    /// Detach the view. The session keeps running and keeps logging — closing a
    /// tab must never stop an agent that is mid-task (§32).
    pub fn detach(&self) {
        *self.sink.lock() = None;
        self.view_attached.store(false, Ordering::SeqCst);
    }

    pub fn kill(&self) -> Result<()> {
        // Stop the follower first: it must not keep polling a transcript for a
        // session that no longer exists.
        self.stopped.store(true, Ordering::SeqCst);
        self.pty.kill()?;
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
        let mut log = SessionLog::open(&log_dir)?;

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
                        LogCommand::Stop => break,
                    }
                }
            })
            .expect("spawn session log writer");

        let (handle, pty_rx) = pty::spawn(options)?;
        let pty = Arc::new(handle);

        let session = Arc::new(LiveSession {
            id: id.clone(),
            pty: Arc::clone(&pty),
            log_tx: log_tx.clone(),
            view_attached: Arc::new(AtomicBool::new(false)),
            sink: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(SessionState::Starting)),
            stopped: Arc::new(AtomicBool::new(false)),
            watch: Arc::new(Mutex::new(None)),
        });

        spawn_pump(Arc::clone(&session), pty, pty_rx, log_tx);

        self.sessions.lock().insert(id, Arc::clone(&session));
        Ok(session)
    }
}

/// The pump: PTY output → log, → UI, and terminal-query handling.
fn spawn_pump(
    session: Arc<LiveSession>,
    pty: Arc<PtyHandle>,
    rx: Receiver<PtyEvent>,
    log_tx: Sender<LogCommand>,
) {
    let sink = Arc::clone(&session.sink);
    let view_attached = Arc::clone(&session.view_attached);
    let state = Arc::clone(&session.state);
    let watch = Arc::clone(&session.watch);
    let id = session.id.clone();

    std::thread::Builder::new()
        .name(format!("session-pump-{id}"))
        .spawn(move || {
            let mut pending: Vec<u8> = Vec::with_capacity(MAX_COALESCE);
            let mut last_flush = Instant::now();

            let flush = |pending: &mut Vec<u8>| {
                if pending.is_empty() {
                    return;
                }
                if let Some(channel) = sink.lock().as_ref() {
                    // Raw bytes, not JSON: the webview receives an ArrayBuffer,
                    // which xterm can consume directly. Decoding to a string
                    // here would corrupt UTF-8 split across read boundaries.
                    let _ = channel.send(InvokeResponseBody::Raw(std::mem::take(pending)));
                } else {
                    pending.clear();
                }
            };

            loop {
                let timeout = FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());
                match rx.recv_timeout(timeout) {
                    Ok(PtyEvent::Output(bytes)) => {
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
                            let _ = pty.write(DSR_REPLY);
                        }

                        {
                            let mut current = state.lock();
                            if *current == SessionState::Starting || *current == SessionState::Idle {
                                *current = SessionState::Working;
                            }
                        }

                        pending.extend_from_slice(&bytes);
                        if pending.len() >= MAX_COALESCE {
                            flush(&mut pending);
                            last_flush = Instant::now();
                        }
                    }
                    Ok(PtyEvent::Exited(code)) => {
                        flush(&mut pending);
                        let _ = log_tx.send(LogCommand::Append {
                            kind: EventKind::Lifecycle,
                            payload: serde_json::to_vec(&Lifecycle::Exited { code })
                                .unwrap_or_default(),
                        });
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
                        flush(&mut pending);
                        last_flush = Instant::now();
                        // Quiet for a while means the agent is waiting on us.
                        let mut current = state.lock();
                        if *current == SessionState::Working {
                            *current = SessionState::Idle;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        flush(&mut pending);
                        let _ = log_tx.send(LogCommand::Stop);
                        break;
                    }
                }
            }
            tracing::debug!(session = %id, "session pump finished");
        })
        .expect("spawn session pump");
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
