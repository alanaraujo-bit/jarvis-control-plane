//! Verification (§30).
//!
//! This module is the difference between the two sentences §30 draws apart:
//!
//! > *the agent claims it is done*
//!
//! versus
//!
//! > *the result has evidence that it is done*
//!
//! Nothing here asks an agent whether the work is finished. It runs the check
//! the criterion describes and records what actually happened, including the
//! command's own output, so a human can disagree with the verdict.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use super::model::{Evidence, EvidenceKind, Verification};
use crate::session::log::now_ms;

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// How long a verification command may run before it is treated as failed.
///
/// A verification that hangs is not a pass. Without a bound, an Unattended
/// mission (§32) could sit on a wedged test runner forever, which is exactly
/// the "consuming resources indefinitely" §34 forbids.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(600);

/// Keep evidence readable. The full output of a test run can be megabytes;
/// what a human needs is the end of it, where failures are reported.
const MAX_DETAIL: usize = 8 * 1024;

/// The outcome of checking one criterion.
pub struct Outcome {
    pub ok: bool,
    pub summary: String,
    pub detail: Option<String>,
    pub kind: EvidenceKind,
}

/// Check a criterion and report what was observed.
///
/// `Manual` is never silently passed: it returns `ok: false` with an
/// explanation, because a criterion only a human can judge has not been judged
/// until a human does it.
pub fn check(verification: &Verification, project_dir: &Path) -> Outcome {
    match verification {
        Verification::Command {
            command,
            cwd,
            expect_exit,
        } => run_command(command, cwd.as_deref(), *expect_exit, project_dir),

        Verification::FileExists { path } => {
            let full = resolve(project_dir, path);
            let exists = full.exists();
            Outcome {
                ok: exists,
                summary: if exists {
                    format!("{path} exists")
                } else {
                    format!("{path} does not exist")
                },
                detail: Some(full.to_string_lossy().to_string()),
                kind: EvidenceKind::File,
            }
        }

        Verification::FileContains { path, text } => {
            let full = resolve(project_dir, path);
            match std::fs::read_to_string(&full) {
                Ok(content) => {
                    let found = content.contains(text);
                    Outcome {
                        ok: found,
                        summary: if found {
                            format!("{path} contains the expected text")
                        } else {
                            format!("{path} does not contain the expected text")
                        },
                        detail: Some(format!("looked for: {text}")),
                        kind: EvidenceKind::File,
                    }
                }
                Err(e) => Outcome {
                    ok: false,
                    summary: format!("{path} could not be read"),
                    detail: Some(e.to_string()),
                    kind: EvidenceKind::File,
                },
            }
        }

        Verification::Manual => Outcome {
            ok: false,
            summary: "Needs a person to confirm".into(),
            detail: Some(
                "This criterion cannot be checked automatically, so it stays unverified \
                 until someone confirms it."
                    .into(),
            ),
            kind: EvidenceKind::Manual,
        },
    }
}

fn resolve(project_dir: &Path, path: &str) -> PathBuf {
    let candidate = Path::new(path);
    if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        project_dir.join(candidate)
    }
}

fn run_command(
    command: &str,
    cwd: Option<&str>,
    expect_exit: i32,
    project_dir: &Path,
) -> Outcome {
    let working_dir = cwd.map(|c| resolve(project_dir, c)).unwrap_or_else(|| project_dir.to_path_buf());

    // Run through the shell so criteria can use ordinary command lines with
    // pipes and arguments, which is how people actually write a check.
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
        c.arg("/c").arg(command);
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(&working_dir);
    // A verification must never stop to ask a question; there is nobody
    // attached to answer it.
    cmd.env("GIT_TERMINAL_PROMPT", "0").env("CI", "1");
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let started = std::time::Instant::now();
    let output = match spawn_with_timeout(cmd, COMMAND_TIMEOUT) {
        Ok(Some(output)) => output,
        Ok(None) => {
            return Outcome {
                ok: false,
                summary: format!("`{command}` did not finish within {}s", COMMAND_TIMEOUT.as_secs()),
                detail: Some(
                    "A verification that never finishes is not a pass; it was stopped.".into(),
                ),
                kind: EvidenceKind::Command,
            }
        }
        Err(e) => {
            return Outcome {
                ok: false,
                summary: format!("`{command}` could not be run"),
                detail: Some(e.to_string()),
                kind: EvidenceKind::Command,
            }
        }
    };

    let code = output.status.code().unwrap_or(-1);
    let ok = code == expect_exit;
    let elapsed = started.elapsed();

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    Outcome {
        ok,
        summary: if ok {
            format!("`{command}` exited {code} in {:.1}s", elapsed.as_secs_f32())
        } else {
            format!("`{command}` exited {code}, expected {expect_exit}")
        },
        detail: Some(tail(&combined, MAX_DETAIL)),
        kind: EvidenceKind::Command,
    }
}

/// Run a command, giving up after `timeout`.
///
/// `std::process` has no timed wait, so the process is polled and killed if it
/// overruns. Output is drained on threads so a child that fills a pipe buffer
/// cannot deadlock us while we wait.
fn spawn_with_timeout(
    mut cmd: Command,
    timeout: Duration,
) -> std::io::Result<Option<std::process::Output>> {
    use std::io::Read;
    use std::process::Stdio;

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stdout.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(s) = stderr.as_mut() {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };

    Ok(Some(std::process::Output {
        status,
        stdout: out_handle.join().unwrap_or_default(),
        stderr: err_handle.join().unwrap_or_default(),
    }))
}

/// Keep the last `max` bytes, on a character boundary.
fn tail(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut start = text.len() - max;
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    format!("…\n{}", &text[start..])
}

/// Build an evidence record from an outcome.
pub fn evidence_from(
    mission_id: &str,
    criterion_id: Option<&str>,
    session_id: Option<&str>,
    outcome: &Outcome,
) -> Evidence {
    Evidence {
        id: uuid::Uuid::now_v7().to_string(),
        mission_id: mission_id.to_string(),
        criterion_id: criterion_id.map(str::to_string),
        session_id: session_id.map(str::to_string),
        kind: outcome.kind,
        ok: outcome.ok,
        summary: outcome.summary.clone(),
        // A command's own output is not ours to translate — it is the tool
        // speaking, not J.A.R.V.I.S. Codes are for sentences we author.
        code: None,
        code_args: None,
        detail: outcome.detail.clone(),
        ts_ms: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    /// Runs a real command in a real directory. Mocking this would defeat the
    /// point of the module (§80).
    #[test]
    fn a_passing_command_produces_positive_evidence() {
        let dir = project();
        let outcome = check(
            &Verification::Command {
                command: "echo verification-ran".into(),
                cwd: None,
                expect_exit: 0,
            },
            dir.path(),
        );
        assert!(outcome.ok, "summary: {}", outcome.summary);
        assert!(outcome.detail.unwrap().contains("verification-ran"));
    }

    #[test]
    fn a_failing_command_is_recorded_as_failing_with_its_output() {
        let dir = project();
        let command = if cfg!(windows) {
            "echo something-broke && exit 1"
        } else {
            "echo something-broke; exit 1"
        };
        let outcome = check(
            &Verification::Command {
                command: command.into(),
                cwd: None,
                expect_exit: 0,
            },
            dir.path(),
        );
        assert!(!outcome.ok);
        assert!(outcome.summary.contains("exited 1"));
        // The output matters: a human has to be able to see *why* it failed.
        assert!(outcome.detail.unwrap().contains("something-broke"));
    }

    #[test]
    fn a_command_that_hangs_is_not_a_pass() {
        let dir = project();
        // A short timeout stands in for the real ten-minute bound.
        let mut cmd = if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/c").arg("ping -n 30 127.0.0.1 > nul");
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg("sleep 30");
            c
        };
        cmd.current_dir(dir.path());
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let result = spawn_with_timeout(cmd, Duration::from_millis(700)).unwrap();
        assert!(result.is_none(), "an overrunning command must be stopped");
    }

    #[test]
    fn file_existence_is_checked_against_the_project_directory() {
        let dir = project();
        std::fs::write(dir.path().join("built.txt"), "ok").unwrap();

        let present = check(
            &Verification::FileExists { path: "built.txt".into() },
            dir.path(),
        );
        assert!(present.ok);

        let absent = check(
            &Verification::FileExists { path: "missing.txt".into() },
            dir.path(),
        );
        assert!(!absent.ok);
    }

    #[test]
    fn file_contents_are_checked() {
        let dir = project();
        std::fs::write(dir.path().join("out.log"), "all tests passed\n").unwrap();

        let hit = check(
            &Verification::FileContains {
                path: "out.log".into(),
                text: "tests passed".into(),
            },
            dir.path(),
        );
        assert!(hit.ok);

        let miss = check(
            &Verification::FileContains {
                path: "out.log".into(),
                text: "coverage".into(),
            },
            dir.path(),
        );
        assert!(!miss.ok);
    }

    /// The single most important assertion in this module.
    #[test]
    fn manual_criteria_are_never_auto_passed() {
        let dir = project();
        let outcome = check(&Verification::Manual, dir.path());
        assert!(
            !outcome.ok,
            "a criterion only a human can judge must not pass itself"
        );
        assert_eq!(outcome.kind, EvidenceKind::Manual);
    }

    #[test]
    fn evidence_carries_the_outcome_and_a_timestamp() {
        let dir = project();
        let outcome = check(
            &Verification::Command {
                command: "echo hi".into(),
                cwd: None,
                expect_exit: 0,
            },
            dir.path(),
        );
        let evidence = evidence_from("m1", Some("c1"), Some("s1"), &outcome);

        assert_eq!(evidence.mission_id, "m1");
        assert_eq!(evidence.criterion_id.as_deref(), Some("c1"));
        assert_eq!(evidence.session_id.as_deref(), Some("s1"));
        assert!(evidence.ok);
        assert!(evidence.ts_ms > 1_500_000_000_000);
    }

    #[test]
    fn long_output_is_trimmed_from_the_front_on_a_character_boundary() {
        let text = "ação ".repeat(4000);
        let trimmed = tail(&text, 100);
        assert!(trimmed.len() <= 110);
        // Must still be valid text, not a split multi-byte character.
        assert!(trimmed.contains("ação"));
    }
}
