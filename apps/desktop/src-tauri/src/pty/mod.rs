//! PTY host.
//!
//! Spawns real pseudo-terminals (ConPTY on Windows) and streams their output.
//! This is the substrate the terminal is built on (§21) and the transport for
//! agent CLIs (§25) — the same mechanism either way, because an agent session
//! *is* a terminal session that happens to be running an agent.

pub(crate) mod job;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use parking_lot::Mutex;
use portable_pty::{Child, CommandBuilder, MasterPty, NativePtySystem, PtySize, PtySystem};

use job::ProcessJob;

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("could not open a pseudo-terminal: {0}")]
    Open(String),
    #[error("could not start {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: anyhow::Error,
    },
    #[error("terminal I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, PtyError>;

#[derive(Debug, Clone)]
pub struct PtyOptions {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub cols: u16,
    pub rows: u16,
    /// Extra environment for the child, on top of the inherited environment.
    pub env: Vec<(String, String)>,
}

/// Environment prefixes scrubbed from every session.
///
/// If J.A.R.V.I.S. is itself launched from inside an agent session, that
/// agent's session markers are in our environment and would be inherited by
/// every process we spawn. A nested Claude Code then sees
/// `CLAUDE_CODE_CHILD_SESSION`, decides it is a child of an existing session,
/// and **turns transcript saving off** — which silently removes the structured
/// stream that Conversation View (§24) and usage reporting (§28) are built on.
///
/// Observed in practice, not theorised: the first agent session started this
/// way printed "Transcript saving is off — inherited CLAUDE_CODE_CHILD_SESSION
/// marker".
///
/// A session launched by J.A.R.V.I.S. is a new top-level session, so these are
/// removed for every session kind — a plain shell should not believe it is
/// running inside an agent either.
const SCRUBBED_ENV_PREFIXES: &[&str] = &["CLAUDE_CODE_", "CLAUDECODE", "CLAUDE_AGENT_", "CODEX_"];

/// Exact variables to scrub that do not share a prefix with the above.
const SCRUBBED_ENV_KEYS: &[&str] = &["CLAUDE_PID", "CLAUDE_EFFORT"];

fn scrubbed_env_keys() -> Vec<String> {
    std::env::vars()
        .map(|(key, _)| key)
        .filter(|key| {
            SCRUBBED_ENV_PREFIXES
                .iter()
                .any(|prefix| key.starts_with(prefix))
                || SCRUBBED_ENV_KEYS.contains(&key.as_str())
        })
        .collect()
}

/// What the reader thread reports.
pub enum PtyEvent {
    /// Bytes read from the terminal. Never decoded here: a UTF-8 sequence can
    /// straddle a read boundary, so decoding must happen downstream where the
    /// stream can be reassembled.
    Output(Vec<u8>),
    /// The process ended.
    Exited(Option<i32>),
}

/// A live pseudo-terminal.
///
/// Dropping this kills the process tree, because `ProcessJob` is dropped with it.
pub struct PtyHandle {
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    pid: Option<u32>,
    /// Held purely for its Drop: closing the job terminates the whole tree.
    _job: ProcessJob,
}

impl PtyHandle {
    /// Send bytes to the process (keystrokes, pasted text, agent prompts).
    pub fn write(&self, data: &[u8]) -> Result<()> {
        let mut writer = self.writer.lock();
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Tell the terminal its new size.
    ///
    /// Full-screen programs redraw from this, so it must reach the pty before
    /// the UI repaints or the output will be laid out for the old geometry.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master
            .lock()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| PtyError::Open(e.to_string()))
    }

    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Terminate the process tree.
    pub fn kill(&self) -> Result<()> {
        // The job object is what actually guarantees descendants die; killing
        // the direct child alone would leave `node` and friends behind.
        let _ = self.child.lock().kill();
        Ok(())
    }

    pub fn try_wait(&self) -> Option<i32> {
        self.child
            .lock()
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.exit_code() as i32)
    }
}

/// Open a pseudo-terminal and start reading it.
///
/// Returns the handle and a receiver of output. The reader runs on its own
/// thread because the read is blocking and must never stall the UI.
pub fn spawn(options: PtyOptions) -> Result<(PtyHandle, Receiver<PtyEvent>)> {
    let system = NativePtySystem::default();
    let pair = system
        .openpty(PtySize {
            rows: options.rows.max(1),
            cols: options.cols.max(1),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| PtyError::Open(e.to_string()))?;

    let mut command = CommandBuilder::new(&options.program);
    command.args(&options.args);
    command.cwd(&options.cwd);

    // Drop any agent-session markers we inherited before applying our own.
    for key in scrubbed_env_keys() {
        command.env_remove(&key);
    }
    for (key, value) in &options.env {
        command.env(key, value);
    }
    // Agent CLIs and build tools emit colour when they believe a terminal is
    // attached; a ConPTY is one, so make that explicit for tools that check.
    command.env("TERM", "xterm-256color");

    let portable_pty::PtyPair { master, slave } = pair;

    let child = slave
        .spawn_command(command)
        .map_err(|source| PtyError::Spawn {
            program: options.program.clone(),
            source,
        })?;

    let pid = child.process_id();

    // Release the slave immediately, before anything reads the master.
    //
    // This is load-bearing on Windows, not tidiness. While this process still
    // holds a slave handle, ConPTY does not pump the session: the master yields
    // only the initial cursor-position query and then blocks indefinitely.
    // Verified empirically — holding the slave hangs the stream outright.
    drop(slave);

    // Contain the tree before it has a chance to spawn anything.
    let job = ProcessJob::new()?;
    if let Some(pid) = pid {
        if let Err(e) = job.assign(pid) {
            // Non-fatal: the terminal still works, but tree cleanup is weaker.
            tracing::warn!(pid, error = %e, "could not assign the pty child to a job object");
        }
    }

    let writer = master
        .take_writer()
        .map_err(|e| PtyError::Open(e.to_string()))?;
    let mut reader = master
        .try_clone_reader()
        .map_err(|e| PtyError::Open(e.to_string()))?;

    let (tx, rx): (Sender<PtyEvent>, Receiver<PtyEvent>) = channel();

    std::thread::Builder::new()
        .name(format!("pty-read-{}", pid.unwrap_or(0)))
        .spawn(move || {
            // 64 KiB matches the pty buffer size, so a burst of output is
            // usually one read rather than dozens.
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break, // pty closed
                    Ok(n) => {
                        if tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                            break; // consumer gone
                        }
                    }
                    Err(e) => {
                        // A closed pty surfaces as an error on Windows rather
                        // than a clean EOF, so this is the normal exit path.
                        tracing::debug!(error = %e, "pty reader finished");
                        break;
                    }
                }
            }
            let _ = tx.send(PtyEvent::Exited(None));
        })
        .map_err(PtyError::Io)?;

    Ok((
        PtyHandle {
            master: Mutex::new(master),
            writer: Mutex::new(writer),
            child: Mutex::new(child),
            pid,
            _job: job,
        },
        rx,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn shell_options(command: &str) -> PtyOptions {
        let (program, args) = if cfg!(windows) {
            ("cmd".to_string(), vec!["/c".into(), command.to_string()])
        } else {
            ("sh".to_string(), vec!["-c".into(), command.to_string()])
        };
        PtyOptions {
            program,
            args,
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: vec![],
        }
    }

    /// Collect output until `needle` appears, the process exits, or time runs out.
    ///
    /// Acts as a terminal would, which at this layer means two things:
    ///
    /// 1. A gap between chunks is normal — ConPTY emits its startup query
    ///    immediately, then nothing until the child has actually started — so a
    ///    receive timeout keeps waiting rather than being read as end-of-stream.
    /// 2. It answers the cursor-position query. `pty::spawn` deliberately does
    ///    not: this layer is a transport, and answering terminal queries belongs
    ///    to whoever is emulating the terminal. The session manager does it for
    ///    unattended sessions; here the test does it. Without a reply ConPTY
    ///    never pumps the session at all (docs/DECISIONS.md D6).
    fn drain_until(
        handle: &PtyHandle,
        rx: &Receiver<PtyEvent>,
        needle: &str,
        timeout: Duration,
    ) -> String {
        const DSR_QUERY: &[u8] = b"[6n";
        let deadline = Instant::now() + timeout;
        let mut out: Vec<u8> = Vec::new();
        let mut answered = false;

        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(PtyEvent::Output(bytes)) => {
                    if !answered && bytes.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY) {
                        let _ = handle.write(b"[1;1R");
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
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Drives a real pseudo-terminal rather than a mock: the point of this
    /// module is the integration itself, so faking it would prove nothing (§80).
    #[test]
    fn runs_a_real_command_and_streams_its_output() {
        let (handle, rx) = spawn(shell_options("echo jarvis-pty-works")).expect("spawn pty");
        let text = drain_until(&handle, &rx, "jarvis-pty-works", Duration::from_secs(20));

        assert!(
            text.contains("jarvis-pty-works"),
            "expected the command output in the pty stream, got: {text:?}"
        );
        let _ = handle.kill();
    }

    #[test]
    fn accepts_input_written_to_the_terminal() {
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        let (handle, rx) = spawn(PtyOptions {
            program: program.into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: vec![],
        })
        .expect("spawn shell");

        handle.write(b"echo typed-into-the-pty\r\n").expect("write");
        let text = drain_until(&handle, &rx, "typed-into-the-pty", Duration::from_secs(20));

        assert!(
            text.contains("typed-into-the-pty"),
            "expected echoed input in the stream, got: {text:?}"
        );
        let _ = handle.kill();
    }

    #[test]
    fn resize_is_accepted_by_a_live_terminal() {
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        let (handle, _rx) = spawn(PtyOptions {
            program: program.into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: vec![],
        })
        .expect("spawn shell");

        handle.resize(120, 40).expect("resize should succeed");
        let _ = handle.kill();
    }

    /// A spawned session must not inherit the parent agent's session markers.
    ///
    /// Asserted against a real environment variable set for this process, and
    /// checked by reading the child's own view of its environment.
    #[test]
    fn agent_session_markers_are_not_inherited() {
        // SAFETY: single-threaded setup at the top of this test, before any
        // child process reads the environment.
        unsafe {
            std::env::set_var("CLAUDE_CODE_CHILD_SESSION", "1");
            std::env::set_var("CLAUDE_PID", "424242");
        }
        assert!(
            scrubbed_env_keys().contains(&"CLAUDE_CODE_CHILD_SESSION".to_string()),
            "the prefix rule must catch CLAUDE_CODE_* markers"
        );
        assert!(
            scrubbed_env_keys().contains(&"CLAUDE_PID".to_string()),
            "exact-match keys must be caught too"
        );

        let command = if cfg!(windows) {
            "echo marker=[%CLAUDE_CODE_CHILD_SESSION%]"
        } else {
            "echo marker=[$CLAUDE_CODE_CHILD_SESSION]"
        };
        let (handle, rx) = spawn(shell_options(command)).expect("spawn pty");
        let text = drain_until(&handle, &rx, "marker=", Duration::from_secs(20));
        let _ = handle.kill();

        unsafe {
            std::env::remove_var("CLAUDE_CODE_CHILD_SESSION");
            std::env::remove_var("CLAUDE_PID");
        }

        assert!(
            !text.contains("marker=[1]"),
            "the child inherited the parent agent's session marker: {text:?}"
        );
    }

    /// Pins the account-isolation contract against the provider CLIs actually
    /// installed on this machine. No credential is copied or read: both tools
    /// are deliberately started with empty configuration roots. Even while
    /// signed out they initialise enough state to prove where configuration
    /// and transcripts live.
    ///
    /// `cargo test -- --ignored provider_config_roots_isolate_state_and_transcripts`
    #[test]
    #[ignore]
    fn provider_config_roots_isolate_state_and_transcripts() {
        fn contains_file(root: &std::path::Path, extension: &str) -> bool {
            let Ok(entries) = std::fs::read_dir(root) else {
                return false;
            };
            entries.flatten().any(|entry| {
                let path = entry.path();
                if path.is_dir() {
                    contains_file(&path, extension)
                } else {
                    path.extension().and_then(|value| value.to_str()) == Some(extension)
                }
            })
        }

        fn wait_for_path(path: &std::path::Path, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if path.exists() {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            false
        }

        fn wait_for_extension(root: &std::path::Path, extension: &str, timeout: Duration) -> bool {
            let deadline = Instant::now() + timeout;
            while Instant::now() < deadline {
                if contains_file(root, extension) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            false
        }

        let temp = tempfile::tempdir().expect("temporary probe root");
        let scratch = temp.path().join("repo");
        let claude_root = temp.path().join("claude");
        let codex_root = temp.path().join("codex");
        std::fs::create_dir_all(&scratch).expect("scratch repository directory");
        std::fs::create_dir_all(&claude_root).expect("Claude configuration directory");
        std::fs::create_dir_all(&codex_root).expect("Codex configuration directory");

        let git = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&scratch)
            .status()
            .expect("run real git");
        assert!(git.success(), "the scratch repository must be real");

        let claude_session = uuid::Uuid::new_v4().to_string();
        let (claude, claude_rx) = spawn(PtyOptions {
            program: "claude".into(),
            args: vec![
                "--session-id".into(),
                claude_session.clone(),
                "--print".into(),
                "--".into(),
                "Reply with probe-ok.".into(),
            ],
            cwd: scratch.clone(),
            cols: 120,
            rows: 30,
            env: vec![(
                "CLAUDE_CONFIG_DIR".into(),
                claude_root.to_string_lossy().into_owned(),
            )],
        })
        .expect("start the real Claude CLI");
        let _ = drain_until(
            &claude,
            &claude_rx,
            // A needle that cannot occur makes the helper wait for the real
            // process to exit. Claude flushes the transcript during shutdown;
            // killing as soon as "Not logged in" was painted races that flush.
            "jarvis-probe-never-appears",
            Duration::from_secs(20),
        );
        let _ = claude.kill();

        assert!(
            wait_for_path(&claude_root.join(".claude.json"), Duration::from_secs(5)),
            "CLAUDE_CONFIG_DIR must carry .claude.json with the account"
        );
        let claude_projects = claude_root.join("projects");
        assert!(
            wait_for_extension(&claude_projects, "jsonl", Duration::from_secs(5)),
            "Claude transcripts must live under CLAUDE_CONFIG_DIR/projects"
        );

        let (codex, codex_rx) = spawn(PtyOptions {
            program: "codex".into(),
            args: vec![
                "exec".into(),
                "--sandbox".into(),
                "read-only".into(),
                "--skip-git-repo-check".into(),
                "-C".into(),
                scratch.to_string_lossy().into_owned(),
                "Reply with probe-ok.".into(),
            ],
            cwd: scratch,
            cols: 120,
            rows: 30,
            // CODEX_* is scrubbed from inherited state. Supplying CODEX_HOME
            // here must win because account env is applied after that scrub.
            env: vec![(
                "CODEX_HOME".into(),
                codex_root.to_string_lossy().into_owned(),
            )],
        })
        .expect("start the real Codex CLI");
        let _ = drain_until(&codex, &codex_rx, "session id", Duration::from_secs(20));
        let codex_sessions = codex_root.join("sessions");
        assert!(
            wait_for_extension(&codex_sessions, "jsonl", Duration::from_secs(20)),
            "CODEX_HOME must survive the PTY scrub and own Codex rollouts"
        );
        let _ = codex.kill();
    }

    #[test]
    fn reports_a_clear_error_for_a_missing_program() {
        let result = spawn(PtyOptions {
            program: "jarvis-no-such-program-9f2a".into(),
            args: vec![],
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: vec![],
        });
        assert!(matches!(result, Err(PtyError::Spawn { .. })));
    }
}
