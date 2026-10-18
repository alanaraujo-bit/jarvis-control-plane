//! The core-owned rotation engine (M26).
//!
//! It probes quota on a clock, notices when the account under a live session
//! trips the person's trigger, and moves the session — at the end of the turn
//! for *near the limit*, at once for *exhausted*. One decision per account
//! window: every live session on that account goes to the same destination,
//! and the person is told (or asked) once.
//!
//! Nothing here depends on a screen being open. The UI only hears events.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;

use super::policy::{policy, Mode, Trigger};
use super::rank::{candidate_for, rank, Candidate, Why};
use super::relay::Reason;
use crate::accounts::Account;
use crate::db::Database;
use crate::session::event::Confidence;

/// Accounts with a live session are probed this often; the rest less.
const LIVE_PROBE_MS: i64 = 60_000;
const IDLE_PROBE_MS: i64 = 300_000;
/// Margin after a provider's stated reset before work resumes on it.
const RESET_MARGIN_MS: i64 = 60_000;
/// How often the background thread ticks.
const TICK: Duration = Duration::from_secs(15);
pub const EVENT: &str = "accounts:rotation";

/// A live session as the engine needs to see it.
#[derive(Debug, Clone)]
pub struct LiveRef {
    pub session_id: String,
    pub account_id: String,
    /// Where the session works; destinations must trust it (Claude Code).
    pub project_root: Option<std::path::PathBuf>,
    /// The provider has gone quiet — the turn is over, not merely paused.
    pub idle: bool,
}

/// What the engine tells the surface. Stable ids only; the UI writes prose.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RotationEvent {
    #[serde(rename_all = "camelCase")]
    Switched {
        decision_id: String,
        from_account_id: String,
        to_account_id: String,
        session_ids: Vec<String>,
        reason: String,
        percent: Option<f64>,
        confidence: Confidence,
        resets_at_ms: Option<i64>,
    },
    #[serde(rename_all = "camelCase")]
    Ask {
        decision_id: String,
        from_account_id: String,
        to_account_id: String,
        session_ids: Vec<String>,
        reason: String,
        percent: Option<f64>,
        confidence: Confidence,
        resets_at_ms: Option<i64>,
    },
    #[serde(rename_all = "camelCase")]
    AllExhausted {
        decision_id: String,
        session_ids: Vec<String>,
        next_account_id: Option<String>,
        next_reset_ms: Option<i64>,
    },
    #[serde(rename_all = "camelCase")]
    Waiting { decision_id: String, until_ms: i64 },
    #[serde(rename_all = "camelCase")]
    Resumed { decision_id: String, account_id: String },
    #[serde(rename_all = "camelCase")]
    Failed { decision_id: String, code: String },
}

/// The world the engine acts on. A trait so the decisions can be tested
/// without processes, a webview or a clock.
pub trait Host: Send + Sync {
    fn live_sessions(&self) -> Vec<LiveRef>;
    /// Refresh one account's stored live reading.
    fn probe(&self, account: &Account);
    fn relay(&self, session_id: &str, to: &Account, reason: Reason) -> Result<(), String>;
    /// Tell an agent on the same account to go on (its window reset).
    fn nudge(&self, session_id: &str);
    fn emit(&self, event: RotationEvent);
}

#[derive(Debug, Clone, PartialEq)]
enum Stage {
    Asking,
    Approved,
    AllExhausted { next_account_id: Option<String>, next_reset_ms: Option<i64> },
    Waiting { until_ms: i64 },
}

/// (account, window, reset, reason): one trip of one window, decided once. The
/// reason is part of it so a window that was near its limit and then runs out
/// is a new trip.
type Key = (String, String, Option<i64>, &'static str);

#[derive(Debug, Clone)]
struct Decision {
    id: String,
    key: Key,
    from: Account,
    to: Option<Account>,
    why: Option<Why>,
    /// The rest of the ranked list, tried in order when `to` refuses.
    fallbacks: Vec<super::rank::Ranked>,
    reason: Reason,
    percent: Option<f64>,
    confidence: Confidence,
    sessions: Vec<String>,
    moved: Vec<String>,
    failed: Option<String>,
    stage: Stage,
}

#[derive(Default)]
struct Inner {
    turn_ended: HashSet<String>,
    decisions: Vec<Decision>,
    settled: HashSet<Key>,
    last_probe: HashMap<String, i64>,
    seq: u64,
}

pub struct Engine<H: Host> {
    db: Arc<Database>,
    host: H,
    inner: Mutex<Inner>,
    /// One pass at a time. The clock, a finished turn, an answer and a manual
    /// switch each run a pass from their own thread, and a relay takes
    /// seconds: two passes would move the same session twice.
    run: Mutex<()>,
}

/// A window of the session's account that tripped the trigger.
struct Trip {
    window: &'static str,
    percent: Option<f64>,
    resets_at_ms: Option<i64>,
    confidence: Confidence,
    reason: Reason,
}

fn tripped(c: &Candidate, trigger: &Trigger) -> Option<Trip> {
    let windows = [("session", &c.session), ("weekly", &c.weekly)];
    let trip = |name: &'static str, w: &super::rank::WindowState, reason| Trip {
        window: name,
        percent: w.percent_used,
        resets_at_ms: w.resets_at_ms,
        confidence: w.confidence,
        reason,
    };
    for (name, w) in windows {
        if let Some(w) = w.as_ref().filter(|w| w.exhausted) {
            return Some(trip(name, w, Reason::Exhausted));
        }
    }
    if let Trigger::NearLimit { percent } = trigger {
        for (name, w) in windows {
            if let Some(w) = w
                .as_ref()
                .filter(|w| w.percent_used.is_some_and(|p| p >= f64::from(*percent)))
            {
                return Some(trip(name, w, Reason::NearLimit));
            }
        }
    }
    None
}

/// When an account can take work again: the latest reset among the windows
/// that block it, or now when nothing does. `None` when a blocking window has
/// no stated reset.
fn available_at(c: &Candidate, now_ms: i64) -> Option<i64> {
    let blocking: Vec<_> = [&c.session, &c.weekly]
        .into_iter()
        .flatten()
        .filter(|w| w.exhausted || w.percent_used.is_some_and(|p| p >= 90.0))
        .collect();
    if blocking.is_empty() {
        return Some(now_ms);
    }
    blocking.iter().map(|w| w.resets_at_ms).collect::<Option<Vec<_>>>()?.into_iter().max()
}

impl<H: Host> Engine<H> {
    pub fn new(db: Arc<Database>, host: H) -> Self {
        Engine { db, host, inner: Mutex::new(Inner::default()), run: Mutex::new(()) }
    }

    fn accounts(&self) -> Vec<Account> {
        crate::accounts::list(&self.db).unwrap_or_default()
    }

    /// Probe on the clock, then decide and act.
    pub fn tick(&self, now_ms: i64) {
        let _run = self.run.lock();
        let accounts = self.accounts();
        if accounts.is_empty() {
            return;
        }
        let live = self.host.live_sessions();
        for account in accounts.iter().filter(|a| a.signed_in && !a.paused) {
            let has_live = live.iter().any(|s| s.account_id == account.id);
            let interval = if has_live { LIVE_PROBE_MS } else { IDLE_PROBE_MS };
            let due = {
                let inner = self.inner.lock();
                inner.last_probe.get(&account.id).is_none_or(|last| now_ms - last >= interval)
            };
            if due {
                self.host.probe(account);
                self.inner.lock().last_probe.insert(account.id.clone(), now_ms);
            }
        }
        self.advance(now_ms, &live);
    }

    /// A session's agent finished a turn: the safe point for a pending move.
    pub fn turn_ended_at(&self, session_id: &str, now_ms: i64) {
        let _run = self.run.lock();
        self.inner.lock().turn_ended.insert(session_id.to_string());
        let live = self.host.live_sessions();
        self.advance(now_ms, &live);
    }

    /// The agent is working again, so the last finished turn is no longer a
    /// safe point. `Idle` alone cannot say this: it means only a moment of
    /// quiet, which happens between frames of a running turn.
    pub fn turn_started(&self, session_id: &str) {
        self.inner.lock().turn_ended.remove(session_id);
    }

    fn next_id(&self) -> String {
        let mut inner = self.inner.lock();
        inner.seq += 1;
        format!("rotation-{}", inner.seq)
    }

    fn rank_for(&self, from: &Account, root: Option<&std::path::Path>, trigger: &Trigger, now_ms: i64) -> Vec<super::rank::Ranked> {
        let candidates = self
            .accounts()
            .iter()
            .filter(|a| a.provider == from.provider)
            .map(|a| candidate_for(&self.db, a, root, now_ms))
            .collect();
        rank(now_ms, from, trigger, candidates)
    }

    /// The ranked destinations every one of these folders can move to: a
    /// destination that has not trusted one of them would park that session
    /// on Claude Code's trust screen.
    fn rank_for_all(&self, from: &Account, roots: &[Option<std::path::PathBuf>], trigger: &Trigger, now_ms: i64) -> Vec<super::rank::Ranked> {
        let mut ranked = self.rank_for(from, roots.first().and_then(|r| r.as_deref()), trigger, now_ms);
        for root in roots.iter().skip(1) {
            let fits: HashSet<String> = self
                .rank_for(from, root.as_deref(), trigger, now_ms)
                .into_iter()
                .map(|r| r.account.id)
                .collect();
            ranked.retain(|r| fits.contains(&r.account.id));
        }
        ranked
    }

    fn advance(&self, now_ms: i64, live: &[LiveRef]) {
        let current_policy = policy(&self.db);
        if current_policy.mode == Mode::Off {
            return;
        }
        self.open_decisions(now_ms, live, &current_policy.trigger, current_policy.mode);
        self.execute_approved(now_ms, live);
        self.wake_waiting(now_ms, live, &current_policy.trigger);
    }

    fn open_decisions(&self, now_ms: i64, live: &[LiveRef], trigger: &Trigger, mode: Mode) {
        let mut by_account: HashMap<&str, Vec<&LiveRef>> = HashMap::new();
        for session in live {
            by_account.entry(session.account_id.as_str()).or_default().push(session);
        }
        for (account_id, sessions) in by_account {
            let busy = self.inner.lock().decisions.iter().any(|d| d.from.id == account_id);
            if busy {
                continue;
            }
            let Some(from) = crate::accounts::get(&self.db, account_id).ok().flatten() else {
                continue;
            };
            // Asking would only lead to "could not switch".
            if from.provider == "codex" && !super::relay::CODEX_RESUME_VERIFIED {
                continue;
            }
            let root = sessions[0].project_root.as_deref();
            let Some(trip) = tripped(&candidate_for(&self.db, &from, root, now_ms), trigger) else {
                continue;
            };
            let key: Key = (
                from.id.clone(),
                trip.window.to_string(),
                trip.resets_at_ms.map(crate::accounts::quota::reset_key),
                trip.reason.id(),
            );
            if self.inner.lock().settled.contains(&key) {
                continue;
            }
            let mut roots: Vec<Option<std::path::PathBuf>> = sessions.iter().map(|s| s.project_root.clone()).collect();
            roots.dedup();
            let mut ranked = self.rank_for_all(&from, &roots, trigger, now_ms).into_iter();
            let session_ids: Vec<String> = sessions.iter().map(|s| s.session_id.clone()).collect();
            let id = self.next_id();
            let mut decision = Decision {
                id: id.clone(),
                key: key.clone(),
                from: from.clone(),
                to: None,
                why: None,
                fallbacks: Vec::new(),
                reason: trip.reason,
                percent: trip.percent,
                confidence: trip.confidence,
                sessions: session_ids.clone(),
                moved: Vec::new(),
                failed: None,
                stage: Stage::Approved,
            };
            match ranked.next() {
                Some(best) => {
                    decision.to = Some(best.account.clone());
                    decision.why = Some(best.why.clone());
                    decision.fallbacks = ranked.collect();
                    if mode == Mode::Ask {
                        decision.stage = Stage::Asking;
                        self.host.emit(RotationEvent::Ask {
                            decision_id: id,
                            from_account_id: from.id.clone(),
                            to_account_id: best.account.id.clone(),
                            session_ids,
                            reason: trip.reason.id().into(),
                            percent: trip.percent,
                            confidence: trip.confidence,
                            resets_at_ms: reset_of(&best.why),
                        });
                    }
                }
                // Near the limit with nowhere better to go: the account still
                // works, so there is nothing to ask about yet. Not settled, so
                // the next tick asks once another account frees up.
                None if trip.reason == Reason::NearLimit => continue,
                None => {
                    let (next_account_id, next_reset_ms) = self.next_available(&from, root, now_ms);
                    decision.stage = Stage::AllExhausted { next_account_id: next_account_id.clone(), next_reset_ms };
                    self.host.emit(RotationEvent::AllExhausted {
                        decision_id: id,
                        session_ids,
                        next_account_id,
                        next_reset_ms,
                    });
                }
            }
            self.inner.lock().decisions.push(decision);
        }
    }

    /// The account that becomes usable first, and when.
    fn next_available(&self, from: &Account, root: Option<&std::path::Path>, now_ms: i64) -> (Option<String>, Option<i64>) {
        self.accounts()
            .iter()
            .filter(|a| {
                a.provider == from.provider
                    && a.signed_in
                    && !a.paused
                    && a.credential_origin == "own"
                    && (a.id == from.id || !crate::accounts::same_subscription(from, a))
            })
            .filter_map(|a| {
                available_at(&candidate_for(&self.db, a, root, now_ms), now_ms).map(|t| (a.id.clone(), t))
            })
            .min_by_key(|(_, t)| *t)
            .map(|(id, t)| (Some(id), Some(t)))
            .unwrap_or((None, None))
    }

    fn execute_approved(&self, now_ms: i64, live: &[LiveRef]) {
        let approved: Vec<Decision> = self
            .inner
            .lock()
            .decisions
            .iter()
            .filter(|d| d.stage == Stage::Approved)
            .cloned()
            .collect();
        for mut decision in approved {
            let Some(mut to) = decision.to.clone() else { continue };
            let pending: Vec<String> = decision
                .sessions
                .iter()
                .filter(|s| !decision.moved.contains(s))
                .cloned()
                .collect();
            let mut remaining = Vec::new();
            let mut stranded = Vec::new();
            let mut last_error = None;
            for session_id in pending {
                let Some(session) = live.iter().find(|s| s.session_id == session_id) else {
                    continue; // ended meanwhile
                };
                if session.account_id != decision.from.id {
                    continue; // moved by other means meanwhile
                }
                let ready = decision.reason != Reason::NearLimit
                    || (session.idle && self.inner.lock().turn_ended.contains(&session_id));
                if !ready {
                    remaining.push(session_id);
                    continue;
                }
                self.inner.lock().turn_ended.remove(&session_id);
                // A refusal moves on down the ranked list (spec §7.2).
                loop {
                    match self.host.relay(&session_id, &to, decision.reason) {
                        Ok(()) => {
                            decision.moved.push(session_id.clone());
                            break;
                        }
                        Err(code) => {
                            last_error = Some(code);
                            if decision.fallbacks.is_empty() {
                                stranded.push(session_id.clone());
                                break;
                            }
                            let next = decision.fallbacks.remove(0);
                            to = next.account;
                            decision.to = Some(to.clone());
                            decision.why = Some(next.why);
                        }
                    }
                }
            }
            if !stranded.is_empty() {
                decision.failed = last_error;
            }
            if !stranded.is_empty() && decision.reason == Reason::Exhausted {
                // Every destination refused a session that cannot go on where
                // it is: announce what moved, then wait for a reset.
                self.announce(&decision, &to);
                let root = live
                    .iter()
                    .find(|s| stranded.contains(&s.session_id))
                    .and_then(|s| s.project_root.clone());
                let (next_account_id, next_reset_ms) = self.next_available(&decision.from, root.as_deref(), now_ms);
                let id = self.next_id();
                {
                    let mut inner = self.inner.lock();
                    if let Some(slot) = inner.decisions.iter_mut().find(|d| d.id == decision.id) {
                        slot.id = id.clone();
                        slot.sessions = stranded.clone();
                        slot.moved.clear();
                        slot.failed = None;
                        slot.stage = Stage::AllExhausted { next_account_id: next_account_id.clone(), next_reset_ms };
                    }
                }
                self.host.emit(RotationEvent::AllExhausted {
                    decision_id: id,
                    session_ids: stranded,
                    next_account_id,
                    next_reset_ms,
                });
                continue;
            }
            decision.sessions = decision
                .moved
                .iter()
                .cloned()
                .chain(remaining.iter().cloned())
                .collect();
            if remaining.is_empty() {
                self.finish(&decision, &to, now_ms);
            } else {
                let mut inner = self.inner.lock();
                if let Some(slot) = inner.decisions.iter_mut().find(|d| d.id == decision.id) {
                    *slot = decision;
                }
            }
        }
    }

    fn finish(&self, decision: &Decision, to: &Account, _now_ms: i64) {
        {
            let mut inner = self.inner.lock();
            inner.decisions.retain(|d| d.id != decision.id);
            inner.settled.insert(decision.key.clone());
        }
        self.announce(decision, to);
    }

    /// Tell the surface what moved and what could not.
    fn announce(&self, decision: &Decision, to: &Account) {
        if !decision.moved.is_empty() {
            // New work should not start on the account that just ran low.
            if decision.from.active {
                let _ = crate::accounts::switch::set_active(&self.db, &to.id);
            }
            crate::accounts::switch::record_rotation(
                &self.db,
                &decision.from,
                to,
                decision.confidence == Confidence::Estimated,
            );
            self.host.emit(RotationEvent::Switched {
                decision_id: decision.id.clone(),
                from_account_id: decision.from.id.clone(),
                to_account_id: to.id.clone(),
                session_ids: decision.moved.clone(),
                reason: decision.reason.id().into(),
                percent: decision.percent,
                confidence: decision.confidence,
                resets_at_ms: decision.why.as_ref().and_then(reset_of),
            });
        }
        if let Some(code) = &decision.failed {
            // Its own card: a partial success must not be replaced by it.
            self.host.emit(RotationEvent::Failed {
                decision_id: format!("{}-failed", decision.id),
                code: code.clone(),
            });
        }
    }

    fn wake_waiting(&self, now_ms: i64, live: &[LiveRef], trigger: &Trigger) {
        // An unanswered "all out" question resolves itself once quota is back:
        // an unattended autopilot must not stay held past the reset.
        let due: Vec<Decision> = self
            .inner
            .lock()
            .decisions
            .iter()
            .filter(|d| match d.stage {
                Stage::Waiting { until_ms } => now_ms >= until_ms,
                Stage::AllExhausted { next_reset_ms: Some(reset), .. } => now_ms >= reset + RESET_MARGIN_MS,
                _ => false,
            })
            .cloned()
            .collect();
        for decision in due {
            let root = live
                .iter()
                .find(|s| decision.sessions.contains(&s.session_id))
                .and_then(|s| s.project_root.clone());
            for account in self.accounts().iter().filter(|a| a.provider == decision.from.provider) {
                self.host.probe(account);
            }
            let sessions: Vec<String> = decision
                .sessions
                .iter()
                .filter(|s| live.iter().any(|l| &l.session_id == *s))
                .cloned()
                .collect();
            let own = candidate_for(&self.db, &decision.from, root.as_deref(), now_ms);
            let mut failed: Vec<String> = Vec::new();
            let mut last_error = None;
            let resumed_on = if tripped(&own, &Trigger::Exhausted).is_none() {
                for session in &sessions {
                    self.host.nudge(session);
                }
                Some(decision.from.id.clone())
            } else {
                let ranked = self.rank_for(&decision.from, root.as_deref(), &Trigger::Exhausted, now_ms);
                let mut landed = None;
                for session in &sessions {
                    let mut moved = false;
                    for candidate in &ranked {
                        match self.host.relay(session, &candidate.account, Reason::ResumeAfterWait) {
                            Ok(()) => {
                                landed.get_or_insert_with(|| candidate.account.id.clone());
                                moved = true;
                                break;
                            }
                            Err(code) => last_error = Some(code),
                        }
                    }
                    if !moved {
                        failed.push(session.clone());
                    }
                }
                if let Some(account_id) = &landed {
                    if decision.from.active {
                        let _ = crate::accounts::switch::set_active(&self.db, account_id);
                    }
                }
                landed
            };
            let _ = trigger;
            if let Some(code) = last_error.filter(|_| !failed.is_empty()) {
                self.host.emit(RotationEvent::Failed {
                    decision_id: format!("{}-failed", decision.id),
                    code,
                });
            }
            if let Some(account_id) = resumed_on.filter(|_| failed.is_empty()) {
                {
                    let mut inner = self.inner.lock();
                    inner.decisions.retain(|d| d.id != decision.id);
                    inner.settled.insert(decision.key.clone());
                }
                self.host.emit(RotationEvent::Resumed {
                    decision_id: decision.id.clone(),
                    account_id,
                });
                continue;
            }
            // Still nowhere to go (or it refused): wait for the next stated
            // reset, keeping only the sessions that did not move.
            let (_, next) = self.next_available(&decision.from, root.as_deref(), now_ms);
            let until_ms = next.filter(|t| *t > now_ms).unwrap_or(now_ms + IDLE_PROBE_MS) + RESET_MARGIN_MS;
            let mut inner = self.inner.lock();
            if let Some(slot) = inner.decisions.iter_mut().find(|d| d.id == decision.id) {
                if !failed.is_empty() {
                    slot.sessions = failed;
                }
                slot.stage = Stage::Waiting { until_ms };
            }
            drop(inner);
            self.host.emit(RotationEvent::Waiting { decision_id: decision.id.clone(), until_ms });
        }
    }

    /// Whether a pending decision covers this session — an autopilot must not
    /// send its next turn while the person is being asked, the move is waiting
    /// for the end of the turn, or every account is out.
    pub fn holds(&self, session_id: &str) -> bool {
        self.inner
            .lock()
            .decisions
            .iter()
            .any(|d| d.sessions.iter().any(|s| s == session_id) && !d.moved.iter().any(|s| s == session_id))
    }

    /// The person answered an `Ask`.
    pub fn answer(&self, decision_id: &str, accept: bool) -> Result<(), String> {
        let mut inner = self.inner.lock();
        let index = inner
            .decisions
            .iter()
            .position(|d| d.id == decision_id && d.stage == Stage::Asking)
            .ok_or_else(|| "rotation.unknownDecision".to_string())?;
        if accept {
            inner.decisions[index].stage = Stage::Approved;
        } else {
            let decision = inner.decisions.remove(index);
            inner.settled.insert(decision.key);
        }
        Ok(())
    }

    /// The person answered an `AllExhausted`: wait and resume, or not.
    pub fn accept_wait(&self, decision_id: &str, accept: bool) -> Result<(), String> {
        let mut inner = self.inner.lock();
        let index = inner
            .decisions
            .iter()
            .position(|d| d.id == decision_id && matches!(d.stage, Stage::AllExhausted { .. }))
            .ok_or_else(|| "rotation.unknownDecision".to_string())?;
        if !accept {
            let decision = inner.decisions.remove(index);
            inner.settled.insert(decision.key);
            return Ok(());
        }
        let Stage::AllExhausted { next_reset_ms, .. } = inner.decisions[index].stage else {
            unreachable!()
        };
        let until_ms = next_reset_ms.unwrap_or(0) + RESET_MARGIN_MS;
        inner.decisions[index].stage = Stage::Waiting { until_ms };
        drop(inner);
        self.host.emit(RotationEvent::Waiting { decision_id: decision_id.to_string(), until_ms });
        Ok(())
    }

    /// Cancel a wait the person no longer wants.
    pub fn cancel_wait(&self, decision_id: &str) -> Result<(), String> {
        let mut inner = self.inner.lock();
        let index = inner
            .decisions
            .iter()
            .position(|d| d.id == decision_id && matches!(d.stage, Stage::Waiting { .. }))
            .ok_or_else(|| "rotation.unknownDecision".to_string())?;
        let decision = inner.decisions.remove(index);
        inner.settled.insert(decision.key);
        Ok(())
    }

    /// Move one session now, at the person's request. `active` is untouched.
    pub fn switch_now(&self, session_id: &str, to: &Account) -> Result<(), String> {
        let _run = self.run.lock();
        let from = self
            .host
            .live_sessions()
            .into_iter()
            .find(|s| s.session_id == session_id)
            .and_then(|s| crate::accounts::get(&self.db, &s.account_id).ok().flatten())
            .ok_or_else(|| "rotation.sessionNotLive".to_string())?;
        if from.id == to.id {
            return Err("rotation.sameAccount".into());
        }
        let id = self.next_id();
        if let Err(code) = self.host.relay(session_id, to, Reason::Manual) {
            self.host.emit(RotationEvent::Failed { decision_id: id, code: code.clone() });
            return Err(code);
        }
        // The session is no longer where any pending decision found it.
        {
            let mut inner = self.inner.lock();
            for decision in inner.decisions.iter_mut() {
                decision.sessions.retain(|s| s != session_id);
            }
            inner.decisions.retain(|d| !d.sessions.is_empty());
        }
        self.host.emit(RotationEvent::Switched {
            decision_id: id,
            from_account_id: from.id,
            to_account_id: to.id.clone(),
            session_ids: vec![session_id.to_string()],
            reason: Reason::Manual.id().into(),
            percent: None,
            confidence: Confidence::Unknown,
            resets_at_ms: None,
        });
        Ok(())
    }

    /// The destination queue for a session's account, best first.
    pub fn queue(&self, session_id: &str, now_ms: i64) -> Vec<super::rank::Ranked> {
        let Some(session) = self.host.live_sessions().into_iter().find(|s| s.session_id == session_id) else {
            return Vec::new();
        };
        let Some(from) = crate::accounts::get(&self.db, &session.account_id).ok().flatten() else {
            return Vec::new();
        };
        if from.provider == "codex" && !super::relay::CODEX_RESUME_VERIFIED {
            return Vec::new();
        }
        let trigger = policy(&self.db).trigger;
        self.rank_for(&from, session.project_root.as_deref(), &trigger, now_ms)
    }

    /// The questions and waits still open, as the events that opened them —
    /// for a screen that loads after they were emitted.
    pub fn pending(&self) -> Vec<RotationEvent> {
        self.inner
            .lock()
            .decisions
            .iter()
            .filter_map(|d| match &d.stage {
                Stage::Asking => Some(RotationEvent::Ask {
                    decision_id: d.id.clone(),
                    from_account_id: d.from.id.clone(),
                    to_account_id: d.to.as_ref()?.id.clone(),
                    session_ids: d.sessions.clone(),
                    reason: d.reason.id().into(),
                    percent: d.percent,
                    confidence: d.confidence,
                    resets_at_ms: d.why.as_ref().and_then(reset_of),
                }),
                Stage::AllExhausted { next_account_id, next_reset_ms } => Some(RotationEvent::AllExhausted {
                    decision_id: d.id.clone(),
                    session_ids: d.sessions.clone(),
                    next_account_id: next_account_id.clone(),
                    next_reset_ms: *next_reset_ms,
                }),
                Stage::Waiting { until_ms } => Some(RotationEvent::Waiting {
                    decision_id: d.id.clone(),
                    until_ms: *until_ms,
                }),
                Stage::Approved => None,
            })
            .collect()
    }
}

fn reset_of(why: &Why) -> Option<i64> {
    match why {
        Why::ExpiresSoonest { resets_at_ms } => Some(*resets_at_ms),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The running application
// ---------------------------------------------------------------------------

struct AppHost {
    app: tauri::AppHandle,
}

impl Host for AppHost {
    fn live_sessions(&self) -> Vec<LiveRef> {
        use tauri::Manager;
        let state = self.app.state::<crate::AppState>();
        state
            .sessions
            .ids()
            .into_iter()
            .filter_map(|id| {
                let session = state.sessions.get(&id).ok()?;
                if session.exited().is_some() {
                    return None;
                }
                let (account_id, cwd, provider): (Option<String>, String, String) = state
                    .db
                    .with(|conn| {
                        conn.query_row(
                            "SELECT account_id, cwd, provider FROM sessions WHERE id = ?1",
                            [&id],
                            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                        )
                    })
                    .ok()?;
                if !crate::accounts::PROVIDERS.contains(&provider.as_str()) {
                    return None;
                }
                Some(LiveRef {
                    session_id: id,
                    account_id: account_id?,
                    project_root: Some(std::path::PathBuf::from(cwd)),
                    idle: session.state() == crate::session::event::SessionState::Idle,
                })
            })
            .collect()
    }

    fn probe(&self, account: &Account) {
        use tauri::Manager;
        let state = self.app.state::<crate::AppState>();
        crate::accounts::live::refresh_all(&state.db, std::slice::from_ref(account));
    }

    fn relay(&self, session_id: &str, to: &Account, reason: Reason) -> Result<(), String> {
        use tauri::Manager;
        let state = self.app.state::<crate::AppState>();
        super::relay::relay_session(&state, session_id, to, reason)
    }

    fn nudge(&self, session_id: &str) {
        use tauri::Manager;
        let state = self.app.state::<crate::AppState>();
        if let Ok(session) = state.sessions.get(session_id) {
            if crate::session::typing::type_text(&session, super::relay::CONTINUE_INSTRUCTION).is_ok() {
                let _ = crate::session::typing::submit(&session);
            }
        }
    }

    fn emit(&self, event: RotationEvent) {
        use tauri::Emitter;
        let _ = self.app.emit(EVENT, event);
    }
}

static ENGINE: OnceLock<Arc<Engine<AppHost>>> = OnceLock::new();

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Start the engine's clock. Idempotent.
pub fn start(app: tauri::AppHandle, db: Arc<Database>) {
    let engine = Arc::new(Engine::new(db, AppHost { app }));
    if ENGINE.set(Arc::clone(&engine)).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("account-rotation".into())
        .spawn(move || loop {
            engine.tick(now_ms());
            std::thread::sleep(TICK);
        })
        .expect("spawn rotation engine");
}

fn engine() -> Option<&'static Arc<Engine<AppHost>>> {
    ENGINE.get()
}

/// Called by the transcript tailer when the provider reports a finished turn.
/// Moves happen off the tailer's thread.
pub fn turn_ended(session_id: &str) {
    if let Some(engine) = engine() {
        let engine = Arc::clone(engine);
        let session_id = session_id.to_string();
        std::thread::spawn(move || engine.turn_ended_at(&session_id, now_ms()));
    }
}

/// Called by the transcript tailer for anything but a finished turn.
pub fn turn_started(session_id: &str) {
    if let Some(engine) = engine() {
        engine.turn_started(session_id);
    }
}

pub fn pending() -> Vec<RotationEvent> {
    engine().map(|engine| engine.pending()).unwrap_or_default()
}

pub fn holds(session_id: &str) -> bool {
    engine().is_some_and(|engine| engine.holds(session_id))
}

pub fn answer(decision_id: &str, accept: bool) -> Result<(), String> {
    let engine = engine().ok_or("rotation.notRunning")?;
    engine.answer(decision_id, accept)?;
    let engine = Arc::clone(engine);
    std::thread::spawn(move || engine.tick(now_ms()));
    Ok(())
}

pub fn accept_wait(decision_id: &str, accept: bool) -> Result<(), String> {
    engine().ok_or("rotation.notRunning")?.accept_wait(decision_id, accept)
}

pub fn cancel_wait(decision_id: &str) -> Result<(), String> {
    engine().ok_or("rotation.notRunning")?.cancel_wait(decision_id)
}

pub fn switch_now(session_id: &str, to: &Account) -> Result<(), String> {
    engine().ok_or("rotation.notRunning")?.switch_now(session_id, to)
}

pub fn queue(session_id: &str) -> Vec<super::rank::Ranked> {
    engine().map(|engine| engine.queue(session_id, now_ms())).unwrap_or_default()
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
