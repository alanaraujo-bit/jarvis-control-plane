use super::*;
use crate::accounts::live::{LiveReading, LiveStatus, LiveWindow};
use crate::accounts::rotation::policy::{set_policy, Mode, Policy, Trigger};

const MIN: i64 = 60_000;
const HOUR: i64 = 60 * MIN;
const NOW: i64 = 2_000_000 * HOUR;

#[derive(Default)]
struct Fake {
    sessions: Mutex<Vec<LiveRef>>,
    relays: Mutex<Vec<(String, String, Reason)>>,
    /// Every relay tried, refused ones included.
    attempts: Mutex<Vec<String>>,
    /// Destinations whose launch is refused.
    refuse: Mutex<HashSet<String>>,
    /// How long a relay takes, like the real one's refusal window.
    relay_ms: std::sync::atomic::AtomicU64,
    nudges: Mutex<Vec<String>>,
    probes: Mutex<Vec<String>>,
    events: Mutex<Vec<RotationEvent>>,
}

impl Host for Arc<Fake> {
    fn live_sessions(&self) -> Vec<LiveRef> {
        self.sessions.lock().clone()
    }
    fn probe(&self, account: &Account) {
        self.probes.lock().push(account.id.clone());
    }
    fn relay(&self, session_id: &str, to: &Account, reason: Reason) -> Result<(), String> {
        self.attempts.lock().push(to.id.clone());
        let ms = self.relay_ms.load(std::sync::atomic::Ordering::SeqCst);
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
        if self.refuse.lock().contains(&to.id) {
            return Err("accounts.destinationRefused".into());
        }
        self.relays.lock().push((session_id.into(), to.id.clone(), reason));
        Ok(())
    }
    fn nudge(&self, session_id: &str) {
        self.nudges.lock().push(session_id.into());
    }
    fn emit(&self, event: RotationEvent) {
        self.events.lock().push(event);
    }
}

fn add_account(db: &Database, id: &str, position: i64, active: bool) {
    add_account_with(db, id, position, active, "claude-code", &format!("C:/nowhere/{id}"));
}

fn add_account_with(db: &Database, id: &str, position: i64, active: bool, provider: &str, dir: &str) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO provider_accounts
                 (id, provider, label, config_dir, adopted, email, account_uuid, signed_in,
                  subscription_since, active, paused, position, created_at, credential_origin)
             VALUES (?1, ?7, ?1, ?2, 0, ?3, ?4, 1, 0, ?5, 0, ?6, 0, 'own')",
            rusqlite::params![
                id,
                dir,
                format!("{id}@example.test"),
                format!("uuid-{id}"),
                active as i64,
                position,
                provider
            ],
        )?;
        Ok(())
    })
    .unwrap();
}

fn reading(db: &Database, id: &str, session: f64, resets_in: Option<i64>) {
    let window = LiveWindow {
        kind: "session".into(),
        raw_kind: "five_hour".into(),
        group: "session".into(),
        scope_label: None,
        percent_used: session,
        resets_at_ms: resets_in.map(|d| NOW + d),
        window_minutes: Some(300),
        binding: true,
        binding_source: "provider".into(),
        severity: if session >= 100.0 { "exhausted".into() } else { "normal".into() },
        severity_source: "provider".into(),
    };
    let status = LiveStatus::Ok {
        reading: LiveReading {
            read_at_ms: NOW,
            source: "claudeGetUsage".into(),
            plan: Some("pro".into()),
            windows: vec![window],
            spend: None,
            reset_credits: 0,
            identity: None,
        },
    };
    crate::accounts::live::store(db, id, &status);
}

fn setup(policy: Policy) -> (Arc<Database>, Arc<Fake>, Engine<Arc<Fake>>) {
    let db = Arc::new(Database::open_in_memory().unwrap());
    set_policy(&db, policy).unwrap();
    add_account(&db, "a", 0, true);
    add_account(&db, "b", 1, false);
    add_account(&db, "c", 2, false);
    reading(&db, "b", 10.0, Some(2 * HOUR));
    reading(&db, "c", 10.0, Some(4 * HOUR));
    let fake = Arc::new(Fake::default());
    let engine = Engine::new(Arc::clone(&db), Arc::clone(&fake));
    (db, fake, engine)
}

fn live(fake: &Fake, session: &str, account: &str, idle: bool) {
    fake.sessions.lock().retain(|s| s.session_id != session);
    fake.sessions.lock().push(LiveRef {
        session_id: session.into(),
        account_id: account.into(),
        project_root: None,
        idle,
    });
}

const AUTO_NEAR: Policy = Policy { mode: Mode::Auto, trigger: Trigger::NearLimit { percent: 90 } };
const AUTO_EXHAUSTED: Policy = Policy { mode: Mode::Auto, trigger: Trigger::Exhausted };

#[test]
fn near_limit_waits_for_turn_end() {
    let (db, fake, engine) = setup(AUTO_NEAR);
    reading(&db, "a", 92.0, Some(HOUR));
    live(&fake, "s1", "a", false);

    engine.tick(NOW);
    assert!(fake.relays.lock().is_empty(), "never mid-turn");
    assert!(engine.holds("s1"), "an autopilot must not send the next turn meanwhile");

    live(&fake, "s1", "a", true);
    engine.turn_ended_at("s1", NOW + MIN);
    assert_eq!(*fake.relays.lock(), vec![("s1".to_string(), "b".to_string(), Reason::NearLimit)]);
    assert!(matches!(fake.events.lock().last(), Some(RotationEvent::Switched { .. })));
    assert!(!engine.holds("s1"));
}

#[test]
fn exhausted_relays_immediately() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    assert_eq!(*fake.relays.lock(), vec![("s1".to_string(), "b".to_string(), Reason::Exhausted)]);
}

#[test]
fn one_decision_per_account_window() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    live(&fake, "s2", "a", false);
    engine.tick(NOW);
    let relays = fake.relays.lock().clone();
    assert_eq!(relays.len(), 2);
    assert!(relays.iter().all(|(_, to, _)| to == "b"), "{relays:?}");
    let switched: Vec<_> = fake
        .events
        .lock()
        .iter()
        .filter_map(|e| match e {
            RotationEvent::Switched { session_ids, .. } => Some(session_ids.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(switched.len(), 1, "one announcement for one decision");
    assert_eq!(switched[0].len(), 2);
}

#[test]
fn ask_mode_emits_ask_and_holds_autopilot() {
    let (db, fake, engine) =
        setup(Policy { mode: Mode::Ask, trigger: Trigger::Exhausted });
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let decision = match fake.events.lock().last().cloned() {
        Some(RotationEvent::Ask { decision_id, to_account_id, .. }) => {
            assert_eq!(to_account_id, "b");
            decision_id
        }
        other => panic!("expected Ask, got {other:?}"),
    };
    assert!(fake.relays.lock().is_empty());
    assert!(engine.holds("s1"));

    engine.answer(&decision, true).unwrap();
    engine.tick(NOW + MIN);
    assert_eq!(fake.relays.lock().len(), 1);
    assert!(!engine.holds("s1"));
}

#[test]
fn declined_window_is_not_asked_again() {
    let (db, fake, engine) =
        setup(Policy { mode: Mode::Ask, trigger: Trigger::Exhausted });
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let decision = match fake.events.lock().last().cloned() {
        Some(RotationEvent::Ask { decision_id, .. }) => decision_id,
        other => panic!("expected Ask, got {other:?}"),
    };
    engine.answer(&decision, false).unwrap();
    engine.tick(NOW + MIN);
    engine.tick(NOW + 2 * MIN);
    let asks = fake.events.lock().iter().filter(|e| matches!(e, RotationEvent::Ask { .. })).count();
    assert_eq!(asks, 1);
    assert!(fake.relays.lock().is_empty());
    assert!(!engine.holds("s1"));
}

#[test]
fn near_limit_asks_once_a_destination_frees_up() {
    let (db, fake, engine) =
        setup(Policy { mode: Mode::Ask, trigger: Trigger::NearLimit { percent: 80 } });
    reading(&db, "a", 92.0, Some(3 * HOUR));
    reading(&db, "b", 100.0, Some(HOUR));
    reading(&db, "c", 100.0, Some(2 * HOUR));
    live(&fake, "s1", "a", true);
    engine.tick(NOW);
    assert!(fake.events.lock().is_empty(), "nowhere to go yet: nothing to ask");

    reading(&db, "b", 0.0, None);
    engine.tick(NOW + HOUR + MIN);
    let last = fake.events.lock().last().cloned();
    match last {
        Some(RotationEvent::Ask { to_account_id, .. }) => assert_eq!(to_account_id, "b"),
        other => panic!("expected Ask once b reset, got {other:?}"),
    }
}

#[test]
fn a_settled_near_limit_window_still_acts_when_it_runs_out() {
    let (db, fake, engine) = setup(AUTO_NEAR);
    reading(&db, "a", 92.0, Some(3 * HOUR));
    reading(&db, "b", 100.0, Some(HOUR));
    reading(&db, "c", 100.0, Some(2 * HOUR));
    live(&fake, "s1", "a", true);
    engine.tick(NOW);
    assert!(fake.events.lock().is_empty());

    reading(&db, "a", 100.0, Some(3 * HOUR));
    engine.tick(NOW + MIN);
    let events = fake.events.lock().clone();
    assert!(
        matches!(events.last(), Some(RotationEvent::AllExhausted { .. })),
        "the same window running out is a new trip: {events:?}"
    );
}

#[test]
fn all_exhausted_then_resume_at_reset() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(3 * HOUR));
    reading(&db, "b", 100.0, Some(HOUR));
    reading(&db, "c", 100.0, Some(2 * HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let (decision, next) = match fake.events.lock().last().cloned() {
        Some(RotationEvent::AllExhausted { decision_id, next_account_id, next_reset_ms, .. }) => {
            assert_eq!(next_account_id.as_deref(), Some("b"));
            (decision_id, next_reset_ms.unwrap())
        }
        other => panic!("expected AllExhausted, got {other:?}"),
    };
    assert_eq!(next, NOW + HOUR);
    assert!(engine.holds("s1"));

    engine.accept_wait(&decision, true).unwrap();
    assert!(matches!(fake.events.lock().last(), Some(RotationEvent::Waiting { .. })));
    engine.tick(NOW + 30 * MIN);
    assert!(fake.relays.lock().is_empty(), "nothing before the reset");

    // b's window resets; the probe at wake-up reads it fresh.
    reading(&db, "b", 0.0, None);
    engine.tick(NOW + HOUR + 2 * MIN);
    assert_eq!(
        *fake.relays.lock(),
        vec![("s1".to_string(), "b".to_string(), Reason::ResumeAfterWait)]
    );
    assert!(matches!(fake.events.lock().last(), Some(RotationEvent::Resumed { .. })));
    assert!(!engine.holds("s1"));
}

#[test]
fn active_moves_only_when_active_account_trips() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "c", 100.0, Some(HOUR));
    live(&fake, "s1", "c", false);
    engine.tick(NOW);
    assert_eq!(fake.relays.lock().len(), 1);
    assert_eq!(crate::accounts::active(&db, "claude-code").unwrap().id, "a");

    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s2", "a", false);
    engine.tick(NOW + MIN);
    assert_eq!(crate::accounts::active(&db, "claude-code").unwrap().id, "b");
}

#[test]
fn off_mode_does_nothing() {
    let (db, fake, engine) = setup(Policy::default());
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    assert!(fake.relays.lock().is_empty());
    assert!(fake.events.lock().is_empty());
    assert!(!engine.holds("s1"));
}

#[test]
fn no_accounts_means_no_engine_work() {
    let db = Arc::new(Database::open_in_memory().unwrap());
    set_policy(&db, AUTO_EXHAUSTED).unwrap();
    let fake = Arc::new(Fake::default());
    let engine = Engine::new(Arc::clone(&db), Arc::clone(&fake));
    engine.tick(NOW);
    assert!(fake.probes.lock().is_empty());
    assert!(fake.events.lock().is_empty());
}

#[test]
fn probes_live_accounts_every_minute_and_others_every_five() {
    let (_db, fake, engine) = setup(Policy::default());
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    assert_eq!(fake.probes.lock().len(), 3);
    engine.tick(NOW + 61_000);
    assert_eq!(fake.probes.lock().iter().filter(|p| *p == "a").count(), 2);
    assert_eq!(fake.probes.lock().iter().filter(|p| *p == "b").count(), 1);
}

// ---- review findings (M26 final review) -----------------------------------

fn asked(fake: &Fake) -> String {
    let last = fake.events.lock().iter().rev().find(|e| matches!(e, RotationEvent::Ask { .. })).cloned();
    match last {
        Some(RotationEvent::Ask { decision_id, .. }) => decision_id,
        other => panic!("expected Ask, got {other:?}"),
    }
}

#[test]
fn concurrent_passes_relay_a_session_once() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    fake.relay_ms.store(300, std::sync::atomic::Ordering::SeqCst);
    std::thread::scope(|scope| {
        scope.spawn(|| engine.tick(NOW));
        scope.spawn(|| engine.tick(NOW + 1));
    });
    let attempts = fake.attempts.lock().clone();
    assert_eq!(attempts.len(), 1, "{attempts:?}");
}

#[test]
fn a_new_turn_cancels_a_stale_turn_end() {
    let (db, fake, engine) = setup(AUTO_NEAR);
    reading(&db, "a", 50.0, Some(HOUR));
    live(&fake, "s1", "a", true);
    engine.turn_ended_at("s1", NOW);
    engine.turn_started("s1");

    // Mid-turn, the 16 ms quiet between spinner frames reads as idle.
    reading(&db, "a", 92.0, Some(HOUR));
    engine.tick(NOW + MIN);
    assert!(fake.relays.lock().is_empty(), "a turn in progress is never cut");
    assert!(engine.holds("s1"));

    engine.turn_ended_at("s1", NOW + 2 * MIN);
    assert_eq!(fake.relays.lock().len(), 1);
}

#[test]
fn a_refused_destination_tries_the_next() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(HOUR));
    fake.refuse.lock().insert("b".into());
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    assert_eq!(*fake.attempts.lock(), vec!["b".to_string(), "c".to_string()]);
    assert_eq!(*fake.relays.lock(), vec![("s1".to_string(), "c".to_string(), Reason::Exhausted)]);
    let switched_to = fake.events.lock().iter().find_map(|e| match e {
        RotationEvent::Switched { to_account_id, .. } => Some(to_account_id.clone()),
        _ => None,
    });
    assert_eq!(switched_to.as_deref(), Some("c"));
}

#[test]
fn every_destination_refusing_an_exhausted_session_waits_for_a_reset() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(HOUR));
    fake.refuse.lock().extend(["b".to_string(), "c".to_string()]);
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let events = fake.events.lock().clone();
    assert!(events.iter().any(|e| matches!(e, RotationEvent::Failed { .. })), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, RotationEvent::AllExhausted { .. })), "{events:?}");
    assert!(engine.holds("s1"));
}

#[test]
fn a_session_moved_by_hand_leaves_the_pending_question() {
    let (db, fake, engine) = setup(Policy { mode: Mode::Ask, trigger: Trigger::Exhausted });
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let decision = asked(&fake);

    let c = crate::accounts::get(&db, "c").unwrap().unwrap();
    engine.switch_now("s1", &c).unwrap();
    live(&fake, "s1", "c", false);
    assert!(!engine.holds("s1"));

    let _ = engine.answer(&decision, true);
    engine.tick(NOW + MIN);
    let relays = fake.relays.lock().clone();
    assert_eq!(relays.len(), 1, "only the manual move: {relays:?}");
}

#[test]
fn pending_lists_open_questions_for_a_reloaded_screen() {
    let (db, fake, engine) = setup(Policy { mode: Mode::Ask, trigger: Trigger::Exhausted });
    reading(&db, "a", 100.0, Some(HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let decision = asked(&fake);
    let pending = engine.pending();
    assert!(
        matches!(pending.as_slice(), [RotationEvent::Ask { decision_id, .. }] if *decision_id == decision),
        "{pending:?}"
    );
}

#[test]
fn an_unanswered_all_exhausted_resumes_after_the_reset() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(3 * HOUR));
    reading(&db, "b", 100.0, Some(HOUR));
    reading(&db, "c", 100.0, Some(2 * HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    assert!(matches!(fake.events.lock().last(), Some(RotationEvent::AllExhausted { .. })));

    // Nobody answered overnight; b's window resets.
    reading(&db, "b", 0.0, None);
    engine.tick(NOW + HOUR + 2 * MIN);
    assert_eq!(
        *fake.relays.lock(),
        vec![("s1".to_string(), "b".to_string(), Reason::ResumeAfterWait)]
    );
    assert!(!engine.holds("s1"));
}

#[test]
fn a_reset_that_jitters_by_milliseconds_is_the_same_window() {
    let (db, fake, engine) = setup(Policy { mode: Mode::Ask, trigger: Trigger::Exhausted });
    reading(&db, "a", 100.0, Some(HOUR - 400));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    engine.answer(&asked(&fake), false).unwrap();

    // The next probe states the same reset with a different sub-second part.
    reading(&db, "a", 100.0, Some(HOUR + 500));
    engine.tick(NOW + MIN);
    let asks = fake.events.lock().iter().filter(|e| matches!(e, RotationEvent::Ask { .. })).count();
    assert_eq!(asks, 1, "a declined window is not asked again");
}

#[test]
fn a_destination_must_trust_every_session_folder() {
    let db = Arc::new(Database::open_in_memory().unwrap());
    set_policy(&db, AUTO_EXHAUSTED).unwrap();
    let trusting = |folders: &[&str]| {
        let dir = tempfile::tempdir().unwrap();
        let projects: serde_json::Map<String, serde_json::Value> = folders
            .iter()
            .map(|f| (f.to_string(), serde_json::json!({ "hasTrustDialogAccepted": true })))
            .collect();
        std::fs::write(
            dir.path().join(".claude.json"),
            serde_json::json!({ "hasCompletedOnboarding": true, "projects": projects }).to_string(),
        )
        .unwrap();
        dir
    };
    let (b_dir, c_dir) = (trusting(&["C:/x"]), trusting(&["C:/x", "C:/y"]));
    add_account(&db, "a", 0, true);
    add_account_with(&db, "b", 1, false, "claude-code", &b_dir.path().to_string_lossy());
    add_account_with(&db, "c", 2, false, "claude-code", &c_dir.path().to_string_lossy());
    reading(&db, "a", 100.0, Some(HOUR));
    reading(&db, "b", 10.0, Some(2 * HOUR));
    reading(&db, "c", 10.0, Some(4 * HOUR));
    let fake = Arc::new(Fake::default());
    let engine = Engine::new(Arc::clone(&db), Arc::clone(&fake));
    for (session, root) in [("s1", "C:/x"), ("s2", "C:/y")] {
        fake.sessions.lock().push(LiveRef {
            session_id: session.into(),
            account_id: "a".into(),
            project_root: Some(root.into()),
            idle: false,
        });
    }
    engine.tick(NOW);
    let relays = fake.relays.lock().clone();
    assert_eq!(relays.len(), 2);
    assert!(relays.iter().all(|(_, to, _)| to == "c"), "b does not trust C:/y: {relays:?}");
}

#[test]
fn a_failed_resume_is_not_announced_as_resumed() {
    let (db, fake, engine) = setup(AUTO_EXHAUSTED);
    reading(&db, "a", 100.0, Some(3 * HOUR));
    reading(&db, "b", 100.0, Some(HOUR));
    reading(&db, "c", 100.0, Some(2 * HOUR));
    live(&fake, "s1", "a", false);
    engine.tick(NOW);
    let decision = match fake.events.lock().last().cloned() {
        Some(RotationEvent::AllExhausted { decision_id, .. }) => decision_id,
        other => panic!("expected AllExhausted, got {other:?}"),
    };
    engine.accept_wait(&decision, true).unwrap();
    reading(&db, "b", 0.0, None);
    fake.refuse.lock().insert("b".into());
    engine.tick(NOW + HOUR + 2 * MIN);
    let events = fake.events.lock().clone();
    assert!(!events.iter().any(|e| matches!(e, RotationEvent::Resumed { .. })), "{events:?}");
    assert!(events.iter().any(|e| matches!(e, RotationEvent::Failed { .. })), "{events:?}");
    assert!(engine.holds("s1"), "still waiting for somewhere to go");
}

#[test]
fn codex_sessions_open_no_decisions_while_resume_is_unverified() {
    let db = Arc::new(Database::open_in_memory().unwrap());
    set_policy(&db, Policy { mode: Mode::Ask, trigger: Trigger::Exhausted }).unwrap();
    add_account_with(&db, "x", 0, true, "codex", "C:/nowhere/x");
    add_account_with(&db, "y", 1, false, "codex", "C:/nowhere/y");
    reading(&db, "x", 100.0, Some(HOUR));
    reading(&db, "y", 10.0, Some(2 * HOUR));
    let fake = Arc::new(Fake::default());
    let engine = Engine::new(Arc::clone(&db), Arc::clone(&fake));
    live(&fake, "s1", "x", false);
    engine.tick(NOW);
    let events = fake.events.lock().clone();
    if crate::accounts::rotation::relay::CODEX_RESUME_VERIFIED {
        assert!(events.iter().any(|e| matches!(e, RotationEvent::Ask { .. })), "{events:?}");
    } else {
        assert!(events.is_empty(), "{events:?}");
        assert!(engine.queue("s1", NOW).is_empty());
    }
}
