//! Watching one session's terminal for the moment it stops (§49).
//!
//! One thread per agent session, parked on a channel. The session pump sends it
//! what it reads; the pump itself is untouched apart from that send, because
//! the pump is what keeps the terminal and the log alive and nothing here is
//! worth a millisecond of it.
//!
//! ## The conjunction
//!
//! `detect` can recognise a question on a screen. On its own that is not enough
//! to wake somebody: an agent that prints a numbered list has drawn something
//! that looks like one, and being wrong here is expensive — a notification that
//! says an agent is waiting when it is working teaches the person to ignore the
//! next one.
//!
//! So a match becomes a notification only when **all three** hold:
//!
//! 1. the last screenful parses as a live choice list,
//! 2. the terminal has been **quiet** for `SETTLE`, and
//! 3. **nothing has been typed** since that quiet began.
//!
//! A list that scrolls past mid-turn fails (2) — more output follows within
//! milliseconds. A question that has been answered fails (3). A question nobody
//! has answered satisfies all three and keeps satisfying them, which is why the
//! notification is raised once and then held until the screen changes.
//!
//! `SETTLE` is the whole cost of the feature in latency, and it is paid only by
//! the notification: the terminal has already drawn the question, so the person
//! sitting in front of it sees no delay at all.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::session::event::Confidence;

use super::{store, Raise, Reason};

/// How long the terminal must stay quiet before a question is believed.
///
/// Long enough that a numbered list scrolling past inside a turn is never
/// mistaken for one — output during a turn arrives continuously — and short
/// enough that somebody who walked away is told within a couple of seconds.
const SETTLE: Duration = Duration::from_millis(1_400);

/// How often the watcher wakes to check the clock when nothing is arriving.
const TICK: Duration = Duration::from_millis(300);

/// How much of the terminal's recent output is examined.
///
/// One screenful of a full-colour TUI is a few kilobytes; 32 KB covers a wide
/// window drawing a diff above its question, with room to spare. The buffer is
/// a rolling tail, so this is also the watcher's entire memory cost per
/// session.
const TAIL_BYTES: usize = 32 * 1024;

/// What the session pump tells its watcher.
///
/// Deliberately three cases and no state: everything the watcher concludes, it
/// concludes from the timing of these, so there is nothing here that can
/// disagree with `SessionState` or with the log.
pub enum Beat {
    /// Bytes the process wrote to the terminal.
    Output(Vec<u8>),
    /// Bytes sent to the process — a person answering, or the autopilot typing.
    Input,
    /// The process ended.
    Exited(Option<i32>),
}

/// Everything the watcher needs to describe what it sees.
#[derive(Debug, Clone)]
pub struct Watched {
    pub session_id: String,
    pub project_id: String,
    pub provider: String,
    pub mission_id: Option<String>,
}

/// What the watcher does when it decides somebody should be told.
///
/// A callback rather than a direct call into Tauri: the watcher is pure timing
/// and text, and giving it an `AppHandle` would make it untestable without a
/// running application. `spawn` in `lib` supplies the real one.
pub type Announce = Arc<dyn Fn(store::Notification) + Send + Sync>;

/// Deliver notifications to the surface.
///
/// The core decides *whether* to raise; the surface decides how to say it,
/// because the words are translated and the catalogues live in TypeScript
/// (§65). A minimised window keeps its webview alive, so this reaches the
/// listener whether anybody is looking or not — which is the case that matters.
///
/// The handle comes from the command that started the session rather than from
/// `AppState`, so nothing in the core needs a running application to be
/// constructed — which is what keeps `AppState` buildable in a test (§80).
pub fn announce_to(app: tauri::AppHandle) -> Announce {
    Arc::new(move |notification: store::Notification| {
        use tauri::Emitter;
        if let Err(error) = app.emit(store::EVENT, &notification) {
            tracing::warn!(%error, "could not deliver a notification to the surface");
        }
    })
}

/// Start watching a session. Returns the sender the pump writes into.
pub fn spawn(watched: Watched, stop: Arc<AtomicBool>) -> Sender<Beat> {
    let (tx, rx) = std::sync::mpsc::channel::<Beat>();
    let name = format!("notify-watch-{}", watched.session_id);
    let spawned = std::thread::Builder::new()
        .name(name)
        .spawn(move || run(rx, watched, stop));
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start a notification watcher");
    }
    tx
}

/// The rolling state one watcher keeps.
struct Watcher {
    tail: Vec<u8>,
    last_output: Instant,
    /// Whether anything has been typed since the terminal last went quiet.
    answered: bool,
    /// The question already raised, so a screen that has not changed is not
    /// raised again every tick.
    raised: Option<String>,
}

impl Watcher {
    fn new() -> Self {
        Self {
            tail: Vec::with_capacity(TAIL_BYTES),
            last_output: Instant::now(),
            answered: false,
            raised: None,
        }
    }

    fn saw_output(&mut self, bytes: &[u8]) {
        self.tail.extend_from_slice(bytes);
        if self.tail.len() > TAIL_BYTES {
            // Trim from the front. Cutting mid-character is fine and expected:
            // `render` decodes lossily precisely because its input is a tail.
            let drop = self.tail.len() - TAIL_BYTES;
            self.tail.drain(..drop);
        }
        self.last_output = Instant::now();
        self.answered = false;
        // New output means the screen moved on. Whatever was asked before is
        // either answered or gone, so the next question is a new one — including
        // when it happens to be worded identically.
        self.raised = None;
    }

    fn saw_input(&mut self) {
        self.answered = true;
        self.raised = None;
    }

    /// The question to raise now, if there is one.
    fn settled_question(&mut self) -> Option<super::detect::Prompt> {
        if self.answered || self.last_output.elapsed() < SETTLE {
            return None;
        }
        let found = super::detect::prompt(&self.tail)?;
        if self.raised.as_deref() == Some(found.question.as_str()) {
            return None;
        }
        self.raised = Some(found.question.clone());
        Some(found)
    }
}

fn run(rx: Receiver<Beat>, watched: Watched, stop: Arc<AtomicBool>) {
    let mut watcher = Watcher::new();

    loop {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match rx.recv_timeout(TICK) {
            Ok(Beat::Output(bytes)) => watcher.saw_output(&bytes),
            Ok(Beat::Input) => watcher.saw_input(),
            Ok(Beat::Exited(code)) => {
                let reason = match code {
                    Some(0) | None => Reason::SessionEnded,
                    Some(_) => Reason::SessionFailed,
                };
                raise(&watched, reason, Confidence::Official, None);
                break;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if let Some(question) = watcher.settled_question() {
            raise(
                &watched,
                Reason::ProviderPrompt,
                // Read off a screen, not stated by a provider (§28).
                Confidence::Observed,
                Some(question.preview(super::PREVIEW_CHARS)),
            );
        }
    }
    tracing::debug!(session = %watched.session_id, "notification watcher finished");
}

fn raise(watched: &Watched, reason: Reason, confidence: Confidence, preview: Option<String>) {
    super::bus::raise(
        reason,
        confidence,
        Raise {
            session_id: Some(watched.session_id.clone()),
            project_id: Some(watched.project_id.clone()),
            mission_id: watched.mission_id.clone(),
            provider: Some(watched.provider.clone()),
            preview,
            detail_code: None,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUESTION: &[u8] = include_bytes!("prompts/claude-write-prompt.bin");
    const WORKING: &[u8] = include_bytes!("prompts/claude-working.bin");

    /// Pretend `SETTLE` has already elapsed.
    fn make_it_quiet(watcher: &mut Watcher) {
        watcher.last_output = Instant::now() - SETTLE - Duration::from_millis(50);
    }

    #[test]
    fn a_question_on_a_quiet_terminal_is_raised_once() {
        let mut watcher = Watcher::new();
        watcher.saw_output(QUESTION);
        make_it_quiet(&mut watcher);

        let first = watcher.settled_question().expect("the question is raised");
        assert_eq!(first.question, "Do you want to create hello.txt?");
        // The same screen, one tick later, is the same question — not a second
        // one. Without this a question nobody answers notifies every 300ms.
        assert!(watcher.settled_question().is_none());
    }

    /// The conjunction's second term. A list that scrolls past inside a turn
    /// never gets its quiet second.
    #[test]
    fn a_question_that_has_not_settled_yet_is_not_raised() {
        let mut watcher = Watcher::new();
        watcher.saw_output(QUESTION);
        assert!(watcher.settled_question().is_none());
    }

    /// The conjunction's third term.
    #[test]
    fn a_question_that_has_been_answered_is_not_raised() {
        let mut watcher = Watcher::new();
        watcher.saw_output(QUESTION);
        make_it_quiet(&mut watcher);
        watcher.saw_input();
        assert!(watcher.settled_question().is_none());
    }

    #[test]
    fn a_working_agent_on_a_quiet_terminal_is_still_not_asking() {
        let mut watcher = Watcher::new();
        watcher.saw_output(WORKING);
        make_it_quiet(&mut watcher);
        assert!(watcher.settled_question().is_none());
    }

    /// An agent that asks, is answered, and asks the same thing again has asked
    /// twice. The screen moved in between, and that is what makes it a new
    /// question rather than a repeat.
    #[test]
    fn the_same_question_after_the_screen_moved_is_a_new_question() {
        let mut watcher = Watcher::new();
        watcher.saw_output(QUESTION);
        make_it_quiet(&mut watcher);
        assert!(watcher.settled_question().is_some());

        watcher.saw_input();
        watcher.saw_output(b"...working...");
        watcher.saw_output(QUESTION);
        make_it_quiet(&mut watcher);
        assert!(watcher.settled_question().is_some());
    }

    /// The tail is bounded, and a question still has to be found in it after a
    /// long build has scrolled past.
    #[test]
    fn a_question_survives_a_burst_of_output_before_it() {
        let mut watcher = Watcher::new();
        for _ in 0..40 {
            watcher.saw_output(&vec![b'x'; 4096]);
        }
        watcher.saw_output(QUESTION);
        make_it_quiet(&mut watcher);
        assert!(watcher.tail.len() <= TAIL_BYTES);
        assert!(watcher.settled_question().is_some());
    }

    /// A question pushed out of the tail by later output is gone, and must not
    /// be re-raised from a stale buffer.
    #[test]
    fn a_question_scrolled_out_of_the_tail_is_forgotten() {
        let mut watcher = Watcher::new();
        watcher.saw_output(QUESTION);
        for _ in 0..40 {
            watcher.saw_output(&vec![b'x'; 4096]);
        }
        make_it_quiet(&mut watcher);
        assert!(watcher.settled_question().is_none());
    }
}
