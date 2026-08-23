//! The loop that takes the human's seat (§32).
//!
//! One thread per driven session. It follows that session's own event log,
//! waits for the provider to report a finished turn, verifies the mission, and
//! either sends the next instruction or stops with a reason.
//!
//! Everything about *what to do* lives in `plan`, as a pure function. This file
//! is the part that touches processes, the database and the clock.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::mission::model::{CriterionStatus, MissionStatus};
use crate::providers::conversation::ConversationItem;
use crate::session::event::EventKind;
use crate::session::manager::LiveSession;
use crate::session::SessionLogReader;

use super::plan::{self, next_instruction, Progress, Step, DEFAULT_TURN_BUDGET};

/// How often the session log is polled for a finished turn.
///
/// The same order as the transcript tailer that feeds it. Nothing here is
/// latency-critical — a second either way on a turn that took minutes is not
/// perceptible — and polling costs a file length check.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// How long to wait after a turn ends before acting on it.
///
/// A transcript is written in pieces. `TurnEnded` can land while the last tool
/// results are still being flushed, and verifying against a half-written state
/// would judge work that is still arriving.
const SETTLE: Duration = Duration::from_millis(1200);

/// How long to let an agent CLI draw its prompt before typing into it.
///
/// These are full-screen terminal programs: they clear the screen, render a
/// banner and only then start reading. Input sent before that is dropped or
/// mangled, which looked exactly like the autopilot doing nothing.
const FIRST_PROMPT: Duration = Duration::from_secs(4);

/// Pause between chunks of an instruction as it is typed in.
///
/// An agent CLI's line editor re-renders on every keystroke; writing a long
/// instruction in one go outruns it and characters are lost. See `send`.
const TYPING_PACE: Duration = Duration::from_millis(30);

/// Pause between the last of the text and the key that submits it.
///
/// Without this the editor is still catching up when the carriage return
/// arrives and swallows it, leaving the instruction in the prompt unsent — a
/// failure that looks entirely correct on screen. See `send`.
const SUBMIT_PAUSE: Duration = Duration::from_millis(400);

/// Where a driven run has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    /// Waiting for the agent to finish a turn.
    Working,
    /// Between turns: verifying and deciding.
    Deciding,
    /// Finished, for any of the three §34 reasons.
    Finished,
}

/// A driven run.
pub struct Autopilot {
    pub session_id: String,
    pub mission_id: String,
    state: Mutex<RunState>,
    turns: AtomicU32,
    stop: Arc<AtomicBool>,
}

impl Autopilot {
    pub fn state(&self) -> RunState {
        *self.state.lock()
    }

    pub fn turns(&self) -> u32 {
        self.turns.load(Ordering::Relaxed)
    }

    /// Ask the run to stop at the next opportunity.
    ///
    /// Deliberately does not kill the agent. A person stopping an autopilot
    /// means "stop driving it", not "destroy whatever it is in the middle of" —
    /// the session stays alive and they can take it over by typing.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Every driven run in the application.
#[derive(Default)]
pub struct Autopilots {
    runs: Mutex<std::collections::HashMap<String, Arc<Autopilot>>>,
}

impl Autopilots {
    pub fn get(&self, session_id: &str) -> Option<Arc<Autopilot>> {
        self.runs.lock().get(session_id).cloned()
    }

    pub fn for_mission(&self, mission_id: &str) -> Option<Arc<Autopilot>> {
        self.runs
            .lock()
            .values()
            .find(|a| a.mission_id == mission_id)
            .cloned()
    }

    pub fn insert(&self, run: Arc<Autopilot>) {
        self.runs.lock().insert(run.session_id.clone(), run);
    }

    pub fn remove(&self, session_id: &str) {
        self.runs.lock().remove(session_id);
    }

    /// Stop every run. Used when the application is shutting down.
    pub fn stop_all(&self) {
        for run in self.runs.lock().values() {
            run.stop();
        }
    }
}

/// Stop driving a session.
pub fn stop(pilots: &Autopilots, session_id: &str) {
    if let Some(run) = pilots.get(session_id) {
        run.stop();
    }
}

/// Start driving a session towards a mission (§32).
///
/// The caller has already started the agent session; this takes over the seat
/// in front of it.
pub fn start(
    session: Arc<LiveSession>,
    db: Arc<Database>,
    log_dir: std::path::PathBuf,
    mission_id: String,
    project_id: String,
) -> Arc<Autopilot> {
    let stop_flag = Arc::new(AtomicBool::new(false));
    let run = Arc::new(Autopilot {
        session_id: session.id.clone(),
        mission_id: mission_id.clone(),
        state: Mutex::new(RunState::Working),
        turns: AtomicU32::new(0),
        stop: Arc::clone(&stop_flag),
    });

    let driven = Arc::clone(&run);
    let session_stop = session.stop_flag();

    std::thread::Builder::new()
        .name(format!("autopilot-{}", session.id))
        .spawn(move || {
            // Start reading from the end of what already exists: a session
            // resumed or restarted must not replay turns that were driven
            // before, or the agent is told the same thing twice.
            let mut cursor = SessionLogReader::open(&log_dir)
                .ok()
                .and_then(|reader| reader.read_from(0).ok())
                .map(|events| events.len() as u64)
                .unwrap_or(0);

            let mut last_failing: Vec<String> = Vec::new();
            let mut stalled_rounds = 0u32;

            // The opening move.
            //
            // The loop reacts to a turn ending, and a freshly started agent has
            // not spoken yet — it is sitting at an empty prompt waiting to be
            // told something. Without this the run would wait forever for a
            // turn that can never end, showing "turn 0" and a working agent
            // doing nothing. Found by starting a real run and reading the
            // session log, not by any test.
            //
            // The agent CLI needs a moment to draw its prompt before it will
            // accept input; typing into it earlier loses the first characters.
            std::thread::sleep(FIRST_PROMPT);
            if !driven.stop.load(Ordering::SeqCst) {
                match crate::mission::store::detail(&db, &mission_id) {
                    Ok(detail) => {
                        let opening = super::plan::opening_instruction(&detail);
                        if send(&session, &opening).is_err() {
                            *driven.state.lock() = RunState::Finished;
                            return;
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "autopilot could not read the mission");
                        *driven.state.lock() = RunState::Finished;
                        return;
                    }
                }
            }

            loop {
                if driven.stop.load(Ordering::SeqCst) || session_stop.load(Ordering::Relaxed) {
                    break;
                }

                let Some((turn_ended, next_cursor)) = poll_for_turn_end(&log_dir, cursor) else {
                    std::thread::sleep(POLL_INTERVAL);
                    continue;
                };
                cursor = next_cursor;
                if !turn_ended {
                    std::thread::sleep(POLL_INTERVAL);
                    continue;
                }

                // Let the last of the turn land before judging it.
                std::thread::sleep(SETTLE);
                if driven.stop.load(Ordering::SeqCst) {
                    break;
                }

                *driven.state.lock() = RunState::Deciding;
                let turns = driven.turns.fetch_add(1, Ordering::Relaxed) + 1;

                // Establish facts before deciding anything (§30). This runs the
                // real checks, and is also where a guardrail may hold one.
                let verified = crate::mission::store::verify_mission(&db, &mission_id);
                let Ok(detail) = verified else {
                    tracing::warn!(mission = %mission_id, "autopilot could not verify; stopping");
                    break;
                };

                // Are the same things failing as last time, with nothing new
                // passing? That is a circle, not progress.
                let failing: Vec<String> = detail
                    .criteria
                    .iter()
                    .filter(|c| c.is_active() && c.required && c.status != CriterionStatus::Verified)
                    .map(|c| c.id.clone())
                    .collect();
                if !failing.is_empty() && failing == last_failing {
                    stalled_rounds += 1;
                } else {
                    stalled_rounds = 0;
                }
                last_failing = failing;

                let approval_pending = crate::guardrail::pending(&db, Some(&mission_id))
                    .map(|p| !p.is_empty())
                    .unwrap_or(false);

                let progress = Progress {
                    turns,
                    budget: DEFAULT_TURN_BUDGET,
                    stalled_rounds,
                    approval_pending,
                };

                match next_instruction(&detail, &progress) {
                    Step::Say(text) => {
                        record(
                            &db,
                            "autopilot.turn",
                            crate::activity::Severity::Info,
                            &detail.mission.title,
                            Some(format!("turn {turns}")),
                            &project_id,
                            &driven.session_id,
                            &mission_id,
                        );
                        *driven.state.lock() = RunState::Working;
                        if send(&session, &text).is_err() {
                            break;
                        }
                    }

                    Step::Complete => {
                        // The core still refuses if the evidence does not hold;
                        // this asks, it does not assert (§30).
                        let outcome = crate::mission::store::set_status(
                            &db,
                            &mission_id,
                            MissionStatus::Completed,
                            None,
                        );
                        if let Err(e) = outcome {
                            tracing::warn!(error = %e, "autopilot completion was refused");
                        }
                        finish(&driven, &db, &project_id, &detail.mission.title, "autopilot.completed");
                        break;
                    }

                    Step::NeedsHuman { code } => {
                        let _ = crate::mission::store::set_status(
                            &db,
                            &mission_id,
                            MissionStatus::Waiting,
                            Some(code.to_string()),
                        );
                        finish(&driven, &db, &project_id, &detail.mission.title, code);
                        break;
                    }

                    Step::Fail { code } => {
                        let _ = crate::mission::store::set_status(
                            &db,
                            &mission_id,
                            MissionStatus::Failed,
                            Some(code.to_string()),
                        );
                        finish(&driven, &db, &project_id, &detail.mission.title, code);
                        break;
                    }
                }
            }

            *driven.state.lock() = RunState::Finished;
            tracing::info!(session = %driven.session_id, "autopilot finished");
        })
        .expect("spawn autopilot");

    run
}

/// Read new frames and report whether a turn ended among them.
///
/// Returns `None` only when the log cannot be read at all.
fn poll_for_turn_end(log_dir: &std::path::Path, cursor: u64) -> Option<(bool, u64)> {
    let reader = SessionLogReader::open(log_dir).ok()?;
    let events = reader.read_from(cursor).ok()?;
    if events.is_empty() {
        return Some((false, cursor));
    }

    let next = cursor + events.len() as u64;
    let ended = events
        .iter()
        .filter(|e| e.kind == EventKind::Message)
        .filter_map(|e| serde_json::from_slice::<ConversationItem>(&e.payload).ok())
        .any(|item| matches!(item, ConversationItem::TurnEnded { .. }));

    Some((ended, next))
}

/// Send an instruction to the agent, as if typed.
///
/// The trailing newline is what submits it — this is a terminal, and the agent
/// is reading a line. Sent through the session so it lands in the log as
/// `PtyInput`, which is what keeps the record honest: a replay shows what the
/// autopilot said, indistinguishable in kind from what a person would have said,
/// because it occupies exactly the same seat.
fn send(session: &LiveSession, text: &str) -> Result<(), ()> {
    // Newlines inside the text would submit it a fragment at a time.
    let single_line = text.replace('\n', " ").replace('\r', " ");

    // Typed, not pasted.
    //
    // Both halves of this were found by driving a real agent and reading the
    // session log, not by reasoning about it:
    //
    // 1. Sending the whole line in one write **drops characters**. The observed
    //    result was "so tere is no" — an agent CLI reads its input through a
    //    line editor that re-renders as it goes, and a large burst outruns it.
    //    Writing in small pieces with a breath between them arrives intact.
    //
    // 2. The submit key had to be sent **separately, after a pause**. Appending
    //    `\r` to the text left the instruction sitting in the prompt as an
    //    unsent draft: the editor was still processing the paste when the
    //    carriage return arrived, and swallowed it. That failure is especially
    //    nasty because the terminal *looks* right — the words are on screen —
    //    while the agent has been told nothing at all.
    const CHUNK: usize = 48;
    let bytes = single_line.as_bytes();
    for chunk in bytes.chunks(CHUNK) {
        session.write(chunk).map_err(|e| {
            tracing::warn!(error = %e, "autopilot could not write to the session");
        })?;
        std::thread::sleep(TYPING_PACE);
    }

    std::thread::sleep(SUBMIT_PAUSE);
    session.write(b"\r").map_err(|e| {
        tracing::warn!(error = %e, "autopilot could not submit to the session");
    })
}

fn finish(
    run: &Autopilot,
    db: &Database,
    project_id: &str,
    title: &str,
    code: &str,
) {
    *run.state.lock() = RunState::Finished;
    record(
        db,
        code,
        // A run that ended because it needs a person is exactly what §49 means
        // by worth a notification; one that completed is information.
        if code == "autopilot.completed" {
            crate::activity::Severity::Info
        } else {
            crate::activity::Severity::Attention
        },
        title,
        None,
        project_id,
        &run.session_id,
        &run.mission_id,
    );
}

#[allow(clippy::too_many_arguments)]
fn record(
    db: &Database,
    kind: &str,
    severity: crate::activity::Severity,
    title: &str,
    detail: Option<String>,
    project_id: &str,
    session_id: &str,
    mission_id: &str,
) {
    crate::activity::record(
        db,
        kind,
        severity,
        title,
        detail,
        Some(project_id),
        Some(session_id),
        Some(mission_id),
    );
}

/// Reason codes this module records, re-exported for the UI catalogue.
pub use plan::reason;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::log::SessionLog;
    use crate::providers::conversation::Role;

    fn write_items(dir: &std::path::Path, items: &[ConversationItem]) {
        let mut log = SessionLog::open(dir).unwrap();
        for item in items {
            log.append(EventKind::Message, &serde_json::to_vec(item).unwrap())
                .unwrap();
        }
    }

    #[test]
    fn a_turn_end_frame_is_noticed() {
        let dir = tempfile::tempdir().unwrap();
        write_items(
            dir.path(),
            &[
                ConversationItem::Message {
                    role: Role::Assistant,
                    text: "working on it".into(),
                    ts_ms: 1,
                    usage: None,
                },
                ConversationItem::TurnEnded {
                    reason: "end_turn".into(),
                    ts_ms: 2,
                },
            ],
        );

        let (ended, cursor) = poll_for_turn_end(dir.path(), 0).unwrap();
        assert!(ended, "a finished turn must be seen");
        assert_eq!(cursor, 2);

        // Reading again from the new cursor finds nothing, so one turn is
        // acted on exactly once.
        let (again, _) = poll_for_turn_end(dir.path(), cursor).unwrap();
        assert!(!again);
    }

    #[test]
    fn ordinary_output_is_not_mistaken_for_a_finished_turn() {
        let dir = tempfile::tempdir().unwrap();
        write_items(
            dir.path(),
            &[ConversationItem::Message {
                role: Role::Assistant,
                text: "still thinking".into(),
                ts_ms: 1,
                usage: None,
            }],
        );
        let (ended, _) = poll_for_turn_end(dir.path(), 0).unwrap();
        assert!(!ended);
    }

    /// The instruction is one line: a multi-line one typed into a terminal
    /// submits on the first newline, handing the agent a fragment and leaving
    /// the rest sitting as a second prompt.
    #[test]
    fn an_instruction_is_flattened_to_one_line() {
        let text = "first line\nsecond line\r\nthird";
        let flattened = text.replace('\n', " ").replace('\r', " ");
        assert!(!flattened.contains('\n'));
        assert!(!flattened.contains('\r'));
        assert!(flattened.starts_with("first line second line"));
    }

    /// Long instructions go out in pieces.
    ///
    /// One large write loses characters to the agent CLI's line editor, which
    /// re-renders on every keystroke. Observed in a real run as "so tere is no".
    #[test]
    fn a_long_instruction_is_written_in_pieces_that_survive() {
        let long = "x".repeat(500);
        let chunks: Vec<_> = long.as_bytes().chunks(48).collect();
        assert!(chunks.len() > 1, "a long line must not go out in one write");
        // The chunking itself loses nothing.
        assert_eq!(chunks.concat(), long.as_bytes());
    }

    /// The submit key is a separate write, after a pause.
    ///
    /// Appending it to the text left the instruction unsent in the prompt: the
    /// editor was still catching up and swallowed the carriage return. On
    /// screen it looked completely correct, which is what made it worth a test.
    #[test]
    fn the_submit_key_gets_its_own_write_and_a_pause() {
        assert!(
            SUBMIT_PAUSE >= Duration::from_millis(200),
            "the editor needs time to settle before the return arrives"
        );
        assert!(TYPING_PACE > Duration::ZERO, "typing must be paced");
    }

    #[test]
    fn stopping_a_run_does_not_end_the_session() {
        // Stopping the autopilot means "stop driving", not "kill the agent" —
        // the person taking over needs the session to still be there.
        let run = Autopilot {
            session_id: "s1".into(),
            mission_id: "m1".into(),
            state: Mutex::new(RunState::Working),
            turns: AtomicU32::new(3),
            stop: Arc::new(AtomicBool::new(false)),
        };
        run.stop();
        assert!(run.stop.load(Ordering::SeqCst));
        assert_eq!(run.turns(), 3);
    }

    #[test]
    fn runs_are_found_by_session_and_by_mission() {
        let pilots = Autopilots::default();
        pilots.insert(Arc::new(Autopilot {
            session_id: "s1".into(),
            mission_id: "m1".into(),
            state: Mutex::new(RunState::Working),
            turns: AtomicU32::new(0),
            stop: Arc::new(AtomicBool::new(false)),
        }));

        assert!(pilots.get("s1").is_some());
        assert!(pilots.for_mission("m1").is_some());
        assert!(pilots.for_mission("nope").is_none());

        pilots.remove("s1");
        assert!(pilots.get("s1").is_none());
    }
}
