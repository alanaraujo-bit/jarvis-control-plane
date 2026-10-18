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
    /// Closing the job terminates the whole tree. Taken on `kill`: the root of
    /// an npm-installed CLI is `cmd.exe`, and killing only it left `node` and
    /// the CLI running with the output pipe open (found in M29 review).
    job: Mutex<Option<ProcessJob>>,
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
        drop(self.job.lock().take());
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

/// Start `options.program`, resolved from PATH, with piped stdio. `cols` and
/// `rows` mean nothing here.
///
/// The agent-session markers a parent agent would leak are removed first (D7):
/// a nested Claude Code that sees them turns transcript saving off, and the
/// transcript is where a chat's conversation is recorded.
pub fn spawn(options: &PtyOptions) -> Result<(ChatHandle, ChildStdout), String> {
    let resolved = crate::envscan::which(&options.program).ok_or_else(|| "chat.toolMissing".to_string())?;
    spawn_resolved(&resolved, options)
}

/// The program is handed to `Command` as resolved — a `.cmd` shim included —
/// rather than wrapped in `cmd /c`: the standard library escapes a batch
/// file's arguments itself, where `cmd /c` strips the outer quotes as soon as
/// more than two appear and broke on a profile path with a space (M29 review).
fn spawn_resolved(resolved: &str, options: &PtyOptions) -> Result<(ChatHandle, ChildStdout), String> {
    let mut command = std::process::Command::new(resolved);
    command.args(&options.args);
    command.current_dir(&options.cwd);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashes up for a chat.
        command.creation_flags(0x0800_0000);
    }
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
            job: Mutex::new(Some(job)),
        },
        stdout,
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::Read;
    use std::time::{Duration, Instant};

    fn options(args: Vec<String>) -> PtyOptions {
        PtyOptions {
            program: String::new(),
            args,
            cwd: std::env::temp_dir(),
            cols: 0,
            rows: 0,
            env: vec![],
            env_remove: vec![],
        }
    }

    /// An npm-style `.cmd` shim in a folder with a space, given two quoted
    /// paths: `cmd /c` mangled exactly this.
    #[test]
    fn a_batch_shim_under_a_path_with_spaces_gets_its_arguments_intact() {
        let dir = tempfile::Builder::new().prefix("chat shim ").tempdir().unwrap();
        let shim = dir.path().join("echo args.cmd");
        std::fs::write(&shim, "@echo %*\r\n").unwrap();
        let settings = r"C:\Users\Jane Doe\settings.json";
        let brief = r"C:\Users\Jane Doe\brief.md";
        let (_handle, mut stdout) = spawn_resolved(
            &shim.to_string_lossy(),
            &options(vec!["--settings".into(), settings.into(), "--brief".into(), brief.into()]),
        )
        .unwrap();
        let mut out = String::new();
        stdout.read_to_string(&mut out).unwrap();
        assert!(out.contains(settings) && out.contains(brief), "got: {out}");
    }

    /// Closing a chat must take the whole tree with it, or the reader never
    /// sees the end of the stream and the CLI lives on.
    #[test]
    fn killing_a_shim_ends_the_whole_tree() {
        if crate::envscan::which("node").is_none() {
            eprintln!("node is not installed; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let shim = dir.path().join("long.cmd");
        std::fs::write(&shim, "@node -e \"setInterval(()=>console.log('tick'),200)\"\r\n").unwrap();
        let (handle, mut stdout) = spawn_resolved(&shim.to_string_lossy(), &options(vec![])).unwrap();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut sink = Vec::new();
            let _ = stdout.read_to_end(&mut sink);
            let _ = done_tx.send(());
        });
        std::thread::sleep(Duration::from_millis(800));
        let killed_at = Instant::now();
        handle.kill();
        assert!(
            done_rx.recv_timeout(Duration::from_secs(5)).is_ok(),
            "the output pipe was still open {:?} after kill",
            killed_at.elapsed()
        );
    }
}
