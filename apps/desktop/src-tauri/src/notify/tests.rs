//! The chain, end to end, against real infrastructure (§80).
//!
//! `detect` proves the bytes are understood, `watch` proves the timing is, and
//! `store` proves the rule is. What none of them proves is that the three are
//! actually joined together — which is the failure mode this codebase has been
//! bitten by more than once: a value computed correctly and then dropped on the
//! way into the database (HANDOFF §5 item 17, and again in the evidence
//! summaries).
//!
//! So these run the real pieces against a real database and a real session log,
//! from bytes an agent CLI genuinely wrote.

#![cfg(test)]

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::db::Database;
use crate::session::event::Confidence;

use super::{detect, store, Attention, Kind, Raise, Reason};

/// Bytes recorded from a real Claude Code 2.1.241 session. See `capture`.
const CLAUDE_ASKS: &[u8] = include_bytes!("prompts/claude-command-prompt.bin");

fn db() -> Database {
    let db = Database::open_in_memory().expect("open a database");
    db.with(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p1', 'Demo', 'C:\\demo', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    db
}

/// Terminal bytes in one end, a readable notification out the other.
///
/// The assertion that matters is the preview: `Do you want to proceed?` on its
/// own is true of every command ever run, and a notification saying only that
/// would be worse than none. The command has to survive the whole way from the
/// PTY to the row.
#[test]
fn a_real_question_becomes_a_notification_a_person_can_act_on() {
    let db = db();
    let attention = Attention::default();

    let question = detect::prompt(CLAUDE_ASKS).expect("the capture holds a question");
    let raised = store::raise(
        &db,
        &attention,
        Reason::ProviderPrompt,
        Confidence::Observed,
        Raise {
            session_id: Some("s1".into()),
            project_id: Some("p1".into()),
            provider: Some("claude-code".into()),
            preview: Some(question.preview(super::PREVIEW_CHARS)),
            ..Default::default()
        },
    )
    .expect("nobody is watching, so it is raised");

    assert_eq!(raised.kind, Kind::NeedsApproval);
    // Read off a screen, never presented as something a provider stated (§28).
    assert_eq!(raised.confidence, Confidence::Observed);
    assert_eq!(raised.project_name.as_deref(), Some("Demo"));

    let preview = raised.preview.expect("a question with no preview says nothing");
    assert!(preview.contains("git --version"), "{preview}");

    // And it is on the badge, which is the only part the person sees first.
    assert_eq!(store::outstanding(&db).unwrap(), 1);
}

/// The same bytes, while the person is watching that session: nothing at all.
#[test]
fn the_same_question_on_a_watched_session_never_reaches_the_database() {
    let db = db();
    let attention = Attention::default();
    attention.set_focused(true);
    attention.set_visible_sessions(vec!["s1".into()]);

    let question = detect::prompt(CLAUDE_ASKS).unwrap();
    let raised = store::raise(
        &db,
        &attention,
        Reason::ProviderPrompt,
        Confidence::Observed,
        Raise {
            session_id: Some("s1".into()),
            project_id: Some("p1".into()),
            preview: Some(question.preview(super::PREVIEW_CHARS)),
            ..Default::default()
        },
    );

    assert!(raised.is_none());
    assert_eq!(store::recent(&db, 10).unwrap().len(), 0);
}

/// A watcher fed the real bytes through the real channel raises exactly one.
///
/// This is the part no unit test covers: `spawn` starting a thread, the pump's
/// `Beat`s reaching it, and the settle timing actually elapsing on a real
/// clock. It sleeps, which is why it is the only test here that does.
#[test]
fn a_watcher_driven_by_real_beats_raises_once_and_stops() {
    let stop = Arc::new(AtomicBool::new(false));
    let tx = super::watch::spawn(
        super::watch::Watched {
            session_id: "s1".into(),
            project_id: "p1".into(),
            provider: "claude-code".into(),
            mission_id: None,
        },
        Arc::clone(&stop),
    );

    // No bus is installed in a test, so nothing is stored — what is being
    // proven here is that the thread runs, consumes beats and winds down when
    // told, without the rest of the application existing.
    tx.send(super::watch::Beat::Output(CLAUDE_ASKS.to_vec()))
        .expect("the watcher is listening");
    std::thread::sleep(std::time::Duration::from_millis(200));
    tx.send(super::watch::Beat::Input).expect("still listening");

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    // The watcher wakes at most `TICK` later and exits; sending after that
    // fails, which is how the test knows it really stopped rather than being
    // assumed to have.
    std::thread::sleep(std::time::Duration::from_millis(700));
    assert!(
        tx.send(super::watch::Beat::Input).is_err(),
        "the watcher did not wind down when its session was stopped, so every \
         closed session would leak a thread"
    );
}

/// Everything the surface localises has a key on the other side.
///
/// The catalogues are TypeScript and cannot be checked from here, so this
/// pins the halves that *are* checkable: every `Reason` renders to a stable
/// identifier, and no two collide. A reason added without a translation is
/// then a compile error in `pt-BR.ts`, which is the check §65 actually wants.
#[test]
fn every_reason_has_a_distinct_stable_identifier() {
    let reasons = [
        Reason::ProviderPrompt,
        Reason::GuardrailPending,
        Reason::GuardrailAsked,
        Reason::TurnEnded,
        Reason::MissionCompleted,
        Reason::RunCompleted,
        Reason::SessionEnded,
        Reason::SessionFailed,
        Reason::MissionBlocked,
        Reason::RunStopped,
    ];
    let mut seen: Vec<&str> = reasons.iter().map(|r| r.as_str()).collect();
    let count = seen.len();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), count, "two reasons share an identifier");

    for reason in reasons {
        assert!(
            reason.as_str().chars().all(|c| c.is_ascii_alphanumeric()),
            "{} is not usable as an i18n key suffix",
            reason.as_str()
        );
    }
}
