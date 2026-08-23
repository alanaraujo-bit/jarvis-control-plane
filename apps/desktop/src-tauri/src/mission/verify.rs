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
    /// English text, always present as the fallback (§65).
    pub summary: String,
    /// A stable code the UI translates, for the sentences we author below.
    pub code: Option<&'static str>,
    /// Arguments for that code, as JSON. `None` when the code takes none.
    pub code_args: Option<serde_json::Value>,
    /// The tool's own output, or an OS error string. Never given a code —
    /// it is the tool speaking, not J.A.R.V.I.S., so it is not ours to
    /// translate (§65).
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
                code: Some(if exists {
                    "evidence.file.exists"
                } else {
                    "evidence.file.missing"
                }),
                code_args: Some(serde_json::json!({ "path": path })),
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
                        code: Some(if found {
                            "evidence.file.contains"
                        } else {
                            "evidence.file.doesNotContain"
                        }),
                        code_args: Some(serde_json::json!({ "path": path })),
                        detail: Some(format!("looked for: {text}")),
                        kind: EvidenceKind::File,
                    }
                }
                Err(e) => Outcome {
                    ok: false,
                    summary: format!("{path} could not be read"),
                    code: Some("evidence.file.unreadable"),
                    code_args: Some(serde_json::json!({ "path": path })),
                    detail: Some(e.to_string()),
                    kind: EvidenceKind::File,
                },
            }
        }

        Verification::Manual => Outcome {
            ok: false,
            summary: "Needs a person to confirm".into(),
            code: Some("evidence.manual.needsConfirmation"),
            code_args: None,
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
                code: Some("evidence.command.timedOut"),
                code_args: Some(
                    serde_json::json!({ "command": command, "seconds": COMMAND_TIMEOUT.as_secs() }),
                ),
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
                code: Some("evidence.command.notRun"),
                code_args: Some(serde_json::json!({ "command": command })),
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

    let seconds = format!("{:.1}", elapsed.as_secs_f32());
    Outcome {
        ok,
        summary: if ok {
            format!("`{command}` exited {code} in {seconds}s")
        } else {
            format!("`{command}` exited {code}, expected {expect_exit}")
        },
        code: Some(if ok {
            "evidence.command.passed"
        } else {
            "evidence.command.failed"
        }),
        code_args: Some(if ok {
            serde_json::json!({ "command": command, "exitCode": code, "seconds": seconds })
        } else {
            serde_json::json!({ "command": command, "exitCode": code, "expectExitCode": expect_exit })
        }),
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
        // The sentence above is ours to author, so a code lets the interface
        // say it in the reader's language (§65). `detail` is the tool's own
        // output and is never given a code — see `Outcome::detail`.
        code: outcome.code.map(str::to_string),
        code_args: outcome.code_args.as_ref().map(|v| v.to_string()),
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
        assert_eq!(outcome.code, Some("evidence.command.passed"));
        assert_eq!(
            outcome.code_args.unwrap()["command"],
            "echo verification-ran"
        );
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
        assert_eq!(outcome.code, Some("evidence.command.failed"));
        assert_eq!(outcome.code_args.unwrap()["exitCode"], 1);
        // The output matters: a human has to be able to see *why* it failed.
        assert!(outcome.detail.unwrap().contains("something-broke"));
    }

    /// A command that never even starts — here, a working directory that does
    /// not exist — is a distinct outcome from one that ran and failed.
    #[test]
    fn a_command_that_cannot_be_spawned_is_reported_as_such() {
        let dir = project();
        let outcome = check(
            &Verification::Command {
                command: "echo hi".into(),
                cwd: Some("this/path/does/not/exist".into()),
                expect_exit: 0,
            },
            dir.path(),
        );
        assert!(!outcome.ok);
        assert_eq!(outcome.code, Some("evidence.command.notRun"));
        assert_eq!(outcome.code_args.unwrap()["command"], "echo hi");
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
        assert_eq!(present.code, Some("evidence.file.exists"));

        let absent = check(
            &Verification::FileExists { path: "missing.txt".into() },
            dir.path(),
        );
        assert!(!absent.ok);
        assert_eq!(absent.code, Some("evidence.file.missing"));
        assert_eq!(absent.code_args.unwrap()["path"], "missing.txt");
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
        assert_eq!(hit.code, Some("evidence.file.contains"));

        let miss = check(
            &Verification::FileContains {
                path: "out.log".into(),
                text: "coverage".into(),
            },
            dir.path(),
        );
        assert!(!miss.ok);
        assert_eq!(miss.code, Some("evidence.file.doesNotContain"));

        let unreadable = check(
            &Verification::FileContains {
                path: "does-not-exist.log".into(),
                text: "anything".into(),
            },
            dir.path(),
        );
        assert!(!unreadable.ok);
        assert_eq!(unreadable.code, Some("evidence.file.unreadable"));
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
        assert_eq!(outcome.code, Some("evidence.manual.needsConfirmation"));
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

        // The code and its arguments survive the trip from Outcome to
        // Evidence — code_args in particular, which crosses a
        // serde_json::Value -> String boundary here (§65).
        assert_eq!(evidence.code.as_deref(), Some("evidence.command.passed"));
        let args: serde_json::Value =
            serde_json::from_str(&evidence.code_args.unwrap()).unwrap();
        assert_eq!(args["command"], "echo hi");
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
