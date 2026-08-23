//! Managing `whisper-server.exe` as a warm, long-lived process (§54 streaming).
//!
//! Live captions need to repeat inference every second or two while a person
//! speaks, and `whisper-cli.exe` pays the full model-load cost — confirmed
//! several seconds against `ggml-small.bin` — on every single invocation.
//! `whisper-server.exe` loads the model once and answers over HTTP for as
//! long as it stays up, which is what makes polling affordable. It ships in
//! the same `b4938` release as `whisper-cli.exe`, but as a *different build*
//! of the underlying libraries — its `whisper.dll`/`ggml*.dll` are not
//! interchangeable with the ones next to `whisper-cli.exe`, so it lives in
//! its own `resources/whisper/server/` subfolder rather than sharing one.
//! See D30 in docs/DECISIONS.md.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

use crate::pty::job::ProcessJob;

use super::{Result, VoiceError};

const BINARY_NAME: &str = "whisper-server.exe";

/// How long to wait for the model to finish loading and the HTTP listener to
/// come up before giving up. Measured against `ggml-small.bin` on this
/// machine: a few seconds. Generous headroom for a slower disk or CPU.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
const READY_POLL_INTERVAL: Duration = Duration::from_millis(150);

/// A running `whisper-server.exe`, bound to a loopback port chosen at
/// startup rather than a fixed one — this product already has a
/// port-collision story (see the dev server note in DECISIONS.md) and a
/// second J.A.R.V.I.S. instance, or anything else on the machine, must never
/// fight this over 8080.
pub struct ServerHandle {
    child: Child,
    port: u16,
    /// Same containment `pty::spawn` uses for agent CLIs (see `pty::job`):
    /// closing this handle kills the process even if J.A.R.V.I.S. itself is
    /// killed outright rather than closed normally, so `whisper-server.exe`
    /// — kept alive for the whole app session, not scoped to one PTY the way
    /// an agent's children are — never survives its parent. Confirmed live:
    /// without this, `taskkill`ing the app left `whisper-server.exe` running
    /// on its own. See D30.
    _job: ProcessJob,
}

impl ServerHandle {
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Spawn the server against `model_path` and block until it answers on
    /// its port, or `READY_TIMEOUT` elapses.
    ///
    /// The port is chosen by binding a listener to `:0`, reading back the
    /// port the OS assigned, then dropping the listener before the child
    /// binds it — a small window where something else could grab the same
    /// port exists, same as any "ask the OS for a free port" scheme, and is
    /// treated as an ordinary startup failure (readiness never arrives, this
    /// returns an error) rather than specially detected.
    pub fn start(resource_dir: &Path, model_path: &Path) -> Result<Self> {
        let binary = resource_dir
            .join("whisper")
            .join("server")
            .join(BINARY_NAME);
        if !binary.is_file() {
            return Err(VoiceError::Server(format!(
                "whisper-server.exe not found at {}",
                binary.display()
            )));
        }

        let port = free_loopback_port()?;

        let mut cmd = Command::new(&binary);
        cmd.arg("-m")
            .arg(model_path)
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let child = cmd
            .spawn()
            .map_err(|e| VoiceError::Server(format!("could not start whisper-server: {e}")))?;

        let job = ProcessJob::new()
            .map_err(|e| VoiceError::Server(format!("could not contain whisper-server: {e}")))?;
        job.assign(child.id())
            .map_err(|e| VoiceError::Server(format!("could not contain whisper-server: {e}")))?;

        let handle = Self { child, port, _job: job };
        handle.wait_until_ready()?;
        Ok(handle)
    }

    fn wait_until_ready(&self) -> Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        let url = format!("{}/", self.base_url());
        while Instant::now() < deadline {
            match ureq::get(&url).timeout(Duration::from_millis(500)).call() {
                // Any HTTP response — including a 404 for a path the server
                // does not route — proves the listener is up and the model
                // finished loading (it does not start listening until it
                // has). A refused connection is the only "not ready yet"
                // signal worth retrying.
                Ok(_) | Err(ureq::Error::Status(_, _)) => return Ok(()),
                Err(ureq::Error::Transport(_)) => std::thread::sleep(READY_POLL_INTERVAL),
            }
        }
        Err(VoiceError::Server(
            "whisper-server did not become ready in time".into(),
        ))
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_loopback_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| VoiceError::Server(format!("could not reserve a port: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| VoiceError::Server(format!("could not read the reserved port: {e}")))?
        .port();
    drop(listener);
    Ok(port)
}
