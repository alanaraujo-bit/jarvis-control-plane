//! The process behind a chat session (M29): a provider CLI speaking its
//! structured protocol on piped stdio, one JSON object per line.
//!
//! It is contained exactly like a terminal's child. Both CLIs start helpers
//! (`node`, shells, MCP servers), so the child goes into a job object and the
//! whole tree dies with the session — or with the app.

use std::io::Write;
use std::process::{Child, ChildStdin, ChildStdout, Stdio};

use parking_lot::Mutex;

use crate::pty::job::ProcessJob;
use crate::pty::PtyOptions;

pub struct ChatHandle {
    stdin: Mutex<ChildStdin>,
    child: Mutex<Child>,
    /// Held for its Drop: closing the job terminates the whole tree.
    _job: ProcessJob,
}

impl ChatHandle {
    /// Send one protocol message. Each is a single line, so a message can never
    /// be read half-written.
    pub fn send_line(&self, line: &str) -> std::io::Result<()> {
        let mut stdin = self.stdin.lock();
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()
    }

    pub fn kill(&self) {
        let _ = self.child.lock().kill();
    }

    pub fn try_wait(&self) -> Option<i32> {
        self.child
            .lock()
            .try_wait()
            .ok()
            .flatten()
            .map(|status| status.code().unwrap_or(1))
    }
}

/// Start `options.program` with piped stdio. `cols`/`rows` mean nothing here.
///
/// Through `envscan::tool_command`, so the npm `.cmd` shim Codex installs is
/// run the way that took real debugging to get right, with no console window.
/// The agent-session markers a parent agent would leak are removed first (D7):
/// a nested Claude Code that sees them turns transcript saving off, and the
/// transcript is where a chat's conversation is recorded.
pub fn spawn(options: &PtyOptions) -> Result<(ChatHandle, ChildStdout), String> {
    let args: Vec<&str> = options.args.iter().map(String::as_str).collect();
    let mut command = crate::envscan::tool_command(&options.program, &args)
        .ok_or_else(|| "chat.toolMissing".to_string())?;
    command.current_dir(&options.cwd);
    for key in crate::pty::scrubbed_env_keys() {
        command.env_remove(key);
    }
    for key in &options.env_remove {
        command.env_remove(key);
    }
    for (key, value) in &options.env {
        command.env(key, value);
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    // Both CLIs log diagnostics there (Codex's tracing, Claude's warnings);
    // nothing on it is part of the protocol, and an unread pipe would fill up
    // and stall the child.
    command.stderr(Stdio::null());

    let job = ProcessJob::new().map_err(|_| "chat.spawnFailed".to_string())?;
    let mut child = command.spawn().map_err(|_| "chat.spawnFailed".to_string())?;
    if let Err(error) = job.assign(child.id()) {
        // Non-fatal, as for a terminal: the chat works, tree cleanup is weaker.
        tracing::warn!(%error, "could not assign the chat child to a job object");
    }
    let stdin = child.stdin.take().ok_or_else(|| "chat.spawnFailed".to_string())?;
    let stdout = child.stdout.take().ok_or_else(|| "chat.spawnFailed".to_string())?;
    Ok((
        ChatHandle {
            stdin: Mutex::new(stdin),
            child: Mutex::new(child),
            _job: job,
        },
        stdout,
    ))
}
