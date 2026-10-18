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
    db.with(|conn| {
        conn.execute(
            "INSERT INTO provider_accounts
                 (id, provider, label, config_dir, adopted, email, account_uuid, signed_in,
                  subscription_since, active, paused, position, created_at, credential_origin)
             VALUES (?1, 'claude-code', ?1, ?2, 0, ?3, ?4, 1, 0, ?5, 0, ?6, 0, 'own')",
            rusqlite::params![
                id,
                format!("C:/nowhere/{id}"),
                format!("{id}@example.test"),
                format!("uuid-{id}"),
                active as i64,
                position
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
