//! Recording what an agent CLI actually draws when it stops and asks.
//!
//! This is not part of the product. It is the harness that produced the
//! evidence `detect` is built on, kept in the tree so the evidence can be
//! reproduced when a provider changes its interface — the same reason the
//! Claude Code parser has a test that walks every transcript on this machine.
//!
//! Every test here is `#[ignore]`d: they spawn a real agent CLI, need it to be
//! installed and signed in, and most of them talk to the network. They write
//! their captures to `$JARVIS_CAPTURE_DIR`.
//!
//! ## Two things the first runs of this taught us
//!
//! **The harness has to stand in for a terminal.** The first run captured
//! exactly four bytes — `\x1b[6n`, ConPTY's cursor-position query — and nothing
//! else, because Claude Code will not draw a single character until something
//! answers it. Same trap D6 records for unattended sessions, met from the other
//! side.
//!
//! **The harness inherits the session it is run from.** The second run captured
//! a Claude Code that said "auto mode on" and never asked for anything: it had
//! picked up `CLAUDE_CODE_CHILD_SESSION` and the rest of the markers from the
//! agent session running this test. Those are stripped below, and the
//! permission mode is passed explicitly rather than left to whatever this
//! machine's settings happen to say.

#![cfg(test)]

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize};

/// ConPTY's startup handshake and the reply a terminal gives (D6).
const DSR_QUERY: &[u8] = b"\x1b[6n";
const DSR_REPLY: &[u8] = b"\x1b[1;1R";

/// Environment an agent CLI inherits from the agent session running this test.
const INHERITED_MARKERS: &[&str] = &[
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_ENABLE_TASKS",
    "CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING",
    "CLAUDE_AGENT_SDK_VERSION",
    "CLAUDE_EFFORT",
    "CLAUDE_PID",
    "CLAUDECODE",
    "AI_AGENT",
];

/// Run an agent CLI in a real PTY, type `script` into it, and return every byte
/// it drew.
fn record(
    program: &str,
    args: &[&str],
    cwd: &std::path::Path,
    env: &[(&str, &str)],
    script: &[(Duration, &str)],
    total: Duration,
) -> Vec<u8> {
    let pty = portable_pty::native_pty_system();
    let pair = pty
        .openpty(PtySize { rows: 30, cols: 120, pixel_width: 0, pixel_height: 0 })
        .expect("open pty");

    let mut cmd = CommandBuilder::new(program);
    for arg in args {
        cmd.arg(arg);
    }
    for marker in INHERITED_MARKERS {
        cmd.env_remove(marker);
    }
    for (key, value) in env {
        cmd.env(key, value);
    }
    cmd.cwd(cwd);
    let mut child = pair.slave.spawn_command(cmd).expect("spawn agent");
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().expect("reader");
    let writer = std::sync::Arc::new(parking_lot::Mutex::new(
        pair.master.take_writer().expect("writer"),
    ));

    let collected = std::sync::Arc::new(parking_lot::Mutex::new(Vec::<u8>::new()));
    let sink = std::sync::Arc::clone(&collected);
    let answerer = std::sync::Arc::clone(&writer);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let chunk = &buf[..n];
            if chunk.windows(DSR_QUERY.len()).any(|w| w == DSR_QUERY) {
                let mut w = answerer.lock();
                let _ = w.write_all(DSR_REPLY);
                let _ = w.flush();
            }
            sink.lock().extend_from_slice(chunk);
        }
    });

    let started = Instant::now();
    for (after, keys) in script {
        while started.elapsed() < *after {
            std::thread::sleep(Duration::from_millis(50));
        }
        // One write per fragment, and the submit key on its own write: a single
        // combined write loses characters going into these TUIs.
        let mut w = writer.lock();
        let _ = w.write_all(keys.as_bytes());
        let _ = w.flush();
    }
    while started.elapsed() < total {
        std::thread::sleep(Duration::from_millis(100));
    }

    let _ = child.kill();
    let out = collected.lock().clone();
    out
}

fn dump(name: &str, bytes: &[u8]) {
    let dir = std::env::var("JARVIS_CAPTURE_DIR").unwrap_or_else(|_| ".".into());
    let path = std::path::Path::new(&dir).join(name);
    std::fs::write(&path, bytes).expect("write capture");
    eprintln!("captured {} bytes -> {}", bytes.len(), path.display());
}

/// Ask Claude Code for something, in manual mode, and record the question.
fn ask_claude(request: &str, name: &str) {
    let dir = tempfile::tempdir().unwrap();
    let bytes = record(
        "claude",
        &["--permission-mode", "manual"],
        dir.path(),
        &[],
        &[
            (Duration::from_secs(8), request),
            (Duration::from_secs(10), "\r"),
        ],
        Duration::from_secs(70),
    );
    assert!(!bytes.is_empty(), "claude drew nothing at all");
    dump(name, &bytes);
}

/// The file-write question: `Do you want to create hello.txt?`
#[test]
#[ignore = "spawns a real, signed-in Claude Code and talks to the network"]
fn capture_claude_code_permission_prompts() {
    ask_claude(
        "create a file called hello.txt containing the single word hi",
        "claude-permission.bin",
    );
}

/// The shell-command question, which offers a third "don't ask again" option.
#[test]
#[ignore = "spawns a real, signed-in Claude Code and talks to the network"]
fn capture_claude_code_command_prompt() {
    ask_claude(
        "run the command: git --version . Do not explain, just run it.",
        "claude-command.bin",
    );
}

/// The folder-trust question, in a configuration directory Claude Code has
/// never seen. Needs no network and no model.
#[test]
#[ignore = "spawns a real Claude Code"]
fn capture_claude_code_trust_prompt() {
    let config = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let bytes = record(
        "claude",
        &[],
        dir.path(),
        &[("CLAUDE_CONFIG_DIR", &config.path().to_string_lossy())],
        &[],
        Duration::from_secs(15),
    );
    assert!(!bytes.is_empty());
    dump("claude-trust.bin", &bytes);
}

/// Codex, asked to run a command it must ask about.
#[test]
#[ignore = "spawns a real, signed-in Codex and talks to the network"]
fn capture_codex_permission_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = record(
        "codex",
        &[],
        dir.path(),
        &[],
        &[
            (
                Duration::from_secs(10),
                "create a file called hello.txt containing the single word hi",
            ),
            (Duration::from_secs(12), "\r"),
        ],
        Duration::from_secs(75),
    );
    assert!(!bytes.is_empty(), "codex drew nothing at all");
    dump("codex-permission.bin", &bytes);
}
