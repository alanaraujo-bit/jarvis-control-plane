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
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::mission::model::{CriterionStatus, MissionStatus};
use crate::providers::conversation::{ConversationItem, Role};
use crate::session::event::EventKind;
use crate::session::manager::LiveSession;
use crate::session::SessionLogReader;

use super::plan::{self, next_instruction, turn_budget, Progress, Step};

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

/// How long to wait for an answer to the reflection question asked at the end
/// of a completed run (§36–§38, D27).
///
/// The mission is already `Completed` by the time this runs, so nothing
/// downstream is blocked on it — this bound exists only so the thread does
/// not sit parked indefinitely if the agent ignores the question or wanders
/// off exploring instead of answering it.
const REFLECT_TIMEOUT: Duration = Duration::from_secs(90);

/// The exact phrase that means "nothing worth recording."
///
/// Checked as a substring, not an exact match — a reply that wraps it in a
/// sentence still counts, and skipping is always the safe read: an empty
/// Brain entry costs nothing, a polluted one is briefed to every agent that
/// starts here afterward.
const REFLECT_SENTINEL: &str = "NOTHING TO RECORD";

/// The longest an agent-written knowledge entry is allowed to be.
///
/// Forces the one-or-two-sentence answer the prompt asks for. A reply that
/// ignores the length hint is truncated rather than discarded — an over-long
/// but genuinely durable fact still beats nothing.
const REFLECT_MAX_CHARS: usize = 500;

/// Asked once, right after a mission completes unattended.
///
/// Addressed the same way `plan::instruction_for` addresses every other
/// instruction: as a brief to a competent colleague. Three things are
/// deliberate: it asks for what would still matter *next time*, not a
/// restatement of this task (the risk Alan named before this was built); it
/// forbids touching anything, because completion has already been set and a
/// stray edit here could be the reason a later re-verify revokes it (§30);
/// and it names the four kinds up front so a plain answer can still be filed
/// correctly without a second round trip.
const REFLECT_PROMPT: &str = "One more thing before we're done. Is there anything about \
     this project that is not obvious from reading the code, and would still matter to \
     whoever touches it next -- a gotcha, a constraint, a convention, what a term means \
     here? Not a summary of this task, only something durable. Do not run anything or \
     change any file, just answer. Reply in one or two sentences, starting with one of \
     WHAT:, CONVENTION:, GOTCHA: or GLOSSARY:. If there is nothing durable to say, reply \
     with exactly: NOTHING TO RECORD.";

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

            // Read once, at the start, and hold it for the run.
            //
            // Re-reading every turn would let a change in Settings move the
            // finish line under a run that is already going — a mission could
            // pass the budget it started with and fail on a number nobody
            // applied to it. A budget is part of the terms this run began
            // under; the next one gets the new value.
            let budget = turn_budget(&db);

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
                    budget,
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
                        match &outcome {
                            Ok(_) => reflect_and_record(
                                &driven,
                                &session,
                                &db,
                                &log_dir,
                                cursor,
                                &project_id,
                                &driven.session_id,
                                &mission_id,
                                &detail.mission.title,
                            ),
                            Err(e) => {
                                tracing::warn!(error = %e, "autopilot completion was refused")
                            }
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

/// After a mission completes unattended, ask whether the agent learned
/// anything durable enough to outlive the session, and record it in the
/// Brain if so (§36–§38, D27).
///
/// This only ever runs here, immediately after `Step::Complete`, because this
/// is the one place a driven run holds the seat with nobody else in it (D15).
/// An attended completion is watched by a person, and typing a question into
/// that terminal would be interrupting a conversation that is not this
/// run's to have — so a manually completed mission is never asked.
#[allow(clippy::too_many_arguments)]
fn reflect_and_record(
    run: &Autopilot,
    session: &LiveSession,
    db: &Database,
    log_dir: &std::path::Path,
    cursor: u64,
    project_id: &str,
    session_id: &str,
    mission_id: &str,
    mission_title: &str,
) {
    // `Deciding` renders as "Verifying" in the UI, and would sit there,
    // stale, for up to REFLECT_TIMEOUT once this starts. The agent is
    // working again, just on a different question.
    *run.state.lock() = RunState::Working;

    // SETTLE exists because a turn's last frames can still be arriving when
    // TurnEnded is seen (see that constant's own doc comment). Baseline here,
    // immediately before the question is sent, rather than trusting `cursor`
    // as handed in — otherwise the tail of the work turn that just finished
    // is read back as if it were part of the answer, and the recorded
    // knowledge opens with the agent restating what it just did: exactly the
    // restatement risk this feature exists to avoid.
    let cursor = drain_to_end(log_dir, cursor);

    if send(session, REFLECT_PROMPT).is_err() {
        return;
    }

    let deadline = Instant::now() + REFLECT_TIMEOUT;
    let session_stop = session.stop_flag();
    let Some(reply) = await_reflection(log_dir, cursor, deadline, &run.stop, &session_stop) else {
        tracing::info!(mission = %mission_id, "reflection: no usable answer in time");
        return;
    };

    let Some((kind, body)) = interpret_reflection(&reply) else {
        return;
    };

    // An exact repeat of something already known does not need saying again
    // — most likely across several missions in the same project each
    // rediscovering the same fact. Similarity matching is deliberately not
    // attempted here; an exact match is cheap and catches the common case.
    let already_known = crate::brain::knowledge(db, project_id)
        .map(|existing| existing.iter().any(|k| k.body.eq_ignore_ascii_case(&body)))
        .unwrap_or(false);
    if already_known {
        return;
    }

    match crate::brain::add_knowledge(
        db,
        project_id,
        kind,
        &body,
        crate::brain::Source::Agent,
        Some(session_id),
        Some(mission_id),
    ) {
        Ok(_) => record(
            db,
            "brain.agentRecorded",
            crate::activity::Severity::Info,
            mission_title,
            None,
            project_id,
            session_id,
            mission_id,
        ),
        Err(e) => tracing::warn!(error = %e, "could not record the agent's reflection"),
    }
}

/// Read to the current end of the log without acting on anything found there.
fn drain_to_end(log_dir: &std::path::Path, cursor: u64) -> u64 {
    SessionLogReader::open(log_dir)
        .and_then(|reader| reader.read_from(cursor))
        .map(|events| cursor + events.len() as u64)
        .unwrap_or(cursor)
}

/// Wait for the agent's turn to end and return what it said.
///
/// `None` means: told to stop, the session ended, or nothing arrived before
/// the deadline. All three are the same outcome from the caller's side —
/// nothing gets recorded — because this runs after the mission is already
/// complete, so there is nothing here worth failing loudly over.
fn await_reflection(
    log_dir: &std::path::Path,
    mut cursor: u64,
    deadline: Instant,
    stop: &Arc<AtomicBool>,
    session_stop: &Arc<AtomicBool>,
) -> Option<String> {
    let mut pieces: Vec<String> = Vec::new();
    loop {
        if stop.load(Ordering::SeqCst) || session_stop.load(Ordering::Relaxed) {
            return None;
        }
        if Instant::now() >= deadline {
            return None;
        }

        let reader = SessionLogReader::open(log_dir).ok()?;
        let events = reader.read_from(cursor).ok()?;
        if events.is_empty() {
            std::thread::sleep(POLL_INTERVAL);
            continue;
        }
        cursor += events.len() as u64;

        let mut ended = false;
        for event in &events {
            if event.kind != EventKind::Message {
                continue;
            }
            match serde_json::from_slice::<ConversationItem>(&event.payload) {
                Ok(ConversationItem::Message {
                    role: Role::Assistant,
                    text,
                    ..
                }) => {
                    if !text.trim().is_empty() {
                        pieces.push(text);
                    }
                }
                Ok(ConversationItem::TurnEnded { .. }) => ended = true,
                _ => {}
            }
        }

        if ended {
            std::thread::sleep(SETTLE);
            return Some(pieces.join(" "));
        }
    }
}

/// Turn the agent's raw reply into a knowledge entry, or `None` if there is
/// nothing worth keeping.
fn interpret_reflection(raw: &str) -> Option<(crate::brain::Kind, String)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.to_ascii_uppercase().contains(REFLECT_SENTINEL) {
        return None;
    }

    let prefixes: [(&str, crate::brain::Kind); 4] = [
        ("WHAT:", crate::brain::Kind::What),
        ("CONVENTION:", crate::brain::Kind::Convention),
        ("GOTCHA:", crate::brain::Kind::Gotcha),
        ("GLOSSARY:", crate::brain::Kind::Glossary),
    ];
    let (kind, body) = prefixes
        .iter()
        .find(|(prefix, _)| {
            trimmed.len() >= prefix.len() && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix)
        })
        .map(|(prefix, kind)| (*kind, trimmed[prefix.len()..].trim().to_string()))
        // No recognised prefix is not a parse failure — an agent that
        // answers plainly still gets recorded, filed under the kind the
        // module doc already calls the most valuable one when nothing more
        // specific was said.
        .unwrap_or((crate::brain::Kind::Gotcha, trimmed.to_string()));

    if body.is_empty() {
        return None;
    }

    let body = if body.chars().count() > REFLECT_MAX_CHARS {
        let mut truncated: String = body.chars().take(REFLECT_MAX_CHARS).collect();
        truncated.push('…');
        truncated
    } else {
        body
    };

    Some((kind, body))
}

/// Send an instruction to the agent, as if typed.
///
/// Sent through the session so it lands in the log as `PtyInput`, which is
/// what keeps the record honest: a replay shows what the autopilot said,
/// indistinguishable in kind from what a person would have said, because it
/// occupies exactly the same seat. The typing itself — paced chunks, then the
/// submit key as its own write after a pause — is shared with voice dictation
/// (§54); see `session::typing` for why both halves are the way they are.
fn send(session: &LiveSession, text: &str) -> Result<(), ()> {
    crate::session::typing::type_text(session, text)?;
    crate::session::typing::submit(session)
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

    // The typing itself — flattening, chunking, the separate paced submit —
    // lives in `session::typing` and is tested there against a real PTY (§54
    // needs the same path for voice dictation), not duplicated here.

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

    /// A straggler frame from the just-finished work turn — written after
    /// `cursor` was captured but before the reflection question is asked —
    /// must not be read back as part of the answer.
    ///
    /// This is the same risk `SETTLE` exists for one turn earlier: a turn's
    /// last frames can still be arriving when `TurnEnded` is seen. Without
    /// re-baselining right before `send()`, that straggler text would be
    /// folded into the recorded knowledge, opening every agent-written entry
    /// with a restatement of the task just finished — the exact failure mode
    /// this feature exists to avoid.
    #[test]
    fn a_stray_frame_from_the_finished_turn_never_reaches_the_reflection() {
        let dir = tempfile::tempdir().unwrap();
        write_items(
            dir.path(),
            &[
                ConversationItem::Message {
                    role: Role::Assistant,
                    text: "finished the task".into(),
                    ts_ms: 1,
                    usage: None,
                },
                ConversationItem::TurnEnded {
                    reason: "end_turn".into(),
                    ts_ms: 2,
                },
            ],
        );
        let (_, cursor) = poll_for_turn_end(dir.path(), 0).unwrap();

        // Still part of the work turn's own flush, landing after the cursor
        // above was captured but before reflection ever runs.
        write_items(
            dir.path(),
            &[ConversationItem::Message {
                role: Role::Assistant,
                text: "(late flush from the finished turn)".into(),
                ts_ms: 3,
                usage: None,
            }],
        );

        let baseline = drain_to_end(dir.path(), cursor);

        write_items(
            dir.path(),
            &[
                ConversationItem::Message {
                    role: Role::Assistant,
                    text: "GOTCHA: the seed depends on insertion order.".into(),
                    ts_ms: 4,
                    usage: None,
                },
                ConversationItem::TurnEnded {
                    reason: "end_turn".into(),
                    ts_ms: 5,
                },
            ],
        );

        let stop = Arc::new(AtomicBool::new(false));
        let reply = await_reflection(
            dir.path(),
            baseline,
            Instant::now() + Duration::from_secs(5),
            &stop,
            &stop,
        )
        .expect("a reply was written");

        assert_eq!(reply, "GOTCHA: the seed depends on insertion order.");
        assert!(!reply.contains("late flush"));
    }

    #[test]
    fn a_stopped_run_does_not_wait_for_a_reflection_answer() {
        let dir = tempfile::tempdir().unwrap();
        let stop = Arc::new(AtomicBool::new(true));
        let never = Arc::new(AtomicBool::new(false));
        let reply = await_reflection(
            dir.path(),
            0,
            Instant::now() + Duration::from_secs(30),
            &stop,
            &never,
        );
        assert!(reply.is_none(), "a stopped run must not wait on an answer");
    }

    #[test]
    fn the_sentinel_records_nothing() {
        assert!(interpret_reflection("NOTHING TO RECORD").is_none());
        assert!(interpret_reflection("  nothing to record ").is_none());
        assert!(interpret_reflection("Honestly, nothing to record here.").is_none());
    }

    #[test]
    fn an_empty_reply_records_nothing() {
        assert!(interpret_reflection("").is_none());
        assert!(interpret_reflection("   ").is_none());
    }

    #[test]
    fn a_recognised_prefix_selects_its_kind() {
        let (kind, body) =
            interpret_reflection("GOTCHA: the port is hardcoded to 4173.").unwrap();
        assert_eq!(kind, crate::brain::Kind::Gotcha);
        assert_eq!(body, "the port is hardcoded to 4173.");

        let (kind, _) =
            interpret_reflection("convention: tests live beside the code.").unwrap();
        assert_eq!(kind, crate::brain::Kind::Convention);

        let (kind, _) = interpret_reflection("WHAT: this is a CLI, not a library.").unwrap();
        assert_eq!(kind, crate::brain::Kind::What);

        let (kind, _) =
            interpret_reflection("GLOSSARY: a \"run\" means one CI job.").unwrap();
        assert_eq!(kind, crate::brain::Kind::Glossary);
    }

    /// A plain answer with no prefix is still recorded rather than discarded
    /// — the model missing the formatting hint is not the same as having
    /// nothing to say.
    #[test]
    fn a_reply_with_no_prefix_defaults_to_gotcha() {
        let (kind, body) =
            interpret_reflection("the CI runner is pinned to an old Node.").unwrap();
        assert_eq!(kind, crate::brain::Kind::Gotcha);
        assert_eq!(body, "the CI runner is pinned to an old Node.");
    }

    #[test]
    fn an_overlong_reply_is_truncated_not_dropped() {
        let long = "x".repeat(REFLECT_MAX_CHARS + 50);
        let (_, body) = interpret_reflection(&format!("GOTCHA: {long}")).unwrap();
        assert!(body.chars().count() <= REFLECT_MAX_CHARS + 1, "the ellipsis adds one char");
        assert!(body.ends_with('…'));
    }

    #[test]
    fn the_reflection_prompt_forbids_touching_anything() {
        assert!(REFLECT_PROMPT.contains("Do not run anything or change any file"));
        assert!(REFLECT_PROMPT.contains(REFLECT_SENTINEL));
    }
}
