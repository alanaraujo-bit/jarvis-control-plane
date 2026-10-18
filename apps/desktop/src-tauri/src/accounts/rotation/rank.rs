//! Where work goes when an account runs low: "use it or lose it".
//!
//! Allowance that resets unused is allowance thrown away, so the account whose
//! running five-hour window resets soonest is spent first; untouched accounts
//! are the reserve, because using one starts its clock.

use std::path::Path;

use serde::Serialize;

use super::policy::Trigger;
use crate::accounts::{same_subscription, Account};
use crate::db::Database;
use crate::session::event::Confidence;

const MINUTE_MS: i64 = 60_000;
/// Two resets this close are the same moment for planning purposes.
const RESET_TIE_MS: i64 = 15 * MINUTE_MS;
/// Points under the trigger a destination must sit, so a move is not followed
/// by another one minutes later.
const HEADROOM: f64 = 10.0;
/// Under the exhausted trigger there is no threshold; a destination still
/// needs some room.
const EXHAUSTED_CEILING: f64 = 90.0;

/// One allowance window, reduced to what ranking needs.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowState {
    pub percent_used: Option<f64>,
    pub resets_at_ms: Option<i64>,
    pub exhausted: bool,
    pub confidence: Confidence,
}

#[derive(Debug, Clone)]
pub struct Candidate {
    pub account: Account,
    /// The five-hour window.
    pub session: Option<WindowState>,
    /// The fullest weekly window.
    pub weekly: Option<WindowState>,
    /// `Some(false)` when this account has not trusted the project folder.
    pub folder_trusted: Option<bool>,
    /// `Some(false)` when the CLI in this directory never finished its
    /// first-run screens (Claude Code's `hasCompletedOnboarding`).
    pub onboarded: Option<bool>,
}

/// Why a destination is where it is in the queue, for the modal to say.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Why {
    /// Its running window resets first: allowance that would otherwise expire.
    #[serde(rename_all = "camelCase")]
    ExpiresSoonest { resets_at_ms: i64 },
    /// No window running; using it starts its clock.
    Reserve,
    /// No numbers at all.
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    pub account: Account,
    pub why: Why,
}

fn windows(c: &Candidate) -> impl Iterator<Item = &WindowState> {
    c.session.iter().chain(c.weekly.iter())
}

pub fn eligible(c: &Candidate, current: &Account, trigger: &Trigger) -> bool {
    let a = &c.account;
    if a.provider != current.provider
        || a.id == current.id
        || same_subscription(current, a)
        || !a.signed_in
        || a.paused
        || a.credential_origin != "own"
        || c.folder_trusted == Some(false)
        || c.onboarded == Some(false)
        || windows(c).any(|w| w.exhausted)
    {
        return false;
    }
    let ceiling = match trigger {
        Trigger::NearLimit { percent } => f64::from(*percent) - HEADROOM,
        Trigger::Exhausted => EXHAUSTED_CEILING,
    };
    windows(c).filter_map(|w| w.percent_used).all(|p| p < ceiling)
}

/// Weekly allowance per hour that is lost if nobody uses it before its reset.
fn weekly_urgency(c: &Candidate, now_ms: i64) -> f64 {
    let Some(weekly) = &c.weekly else { return 0.0 };
    let (Some(used), Some(resets)) = (weekly.percent_used, weekly.resets_at_ms) else {
        return 0.0;
    };
    let hours = ((resets - now_ms) as f64 / 3_600_000.0).max(0.25);
    (100.0 - used).max(0.0) / hours
}

fn why(c: &Candidate, now_ms: i64) -> Why {
    match c.session.as_ref().and_then(|s| s.resets_at_ms).filter(|&r| r > now_ms) {
        Some(resets_at_ms) => Why::ExpiresSoonest { resets_at_ms },
        None if windows(c).any(|w| w.percent_used.is_some()) => Why::Reserve,
        None => Why::Unknown,
    }
}

pub fn rank(
    now_ms: i64,
    current: &Account,
    trigger: &Trigger,
    candidates: Vec<Candidate>,
) -> Vec<Ranked> {
    let mut keyed: Vec<_> = candidates
        .into_iter()
        .filter(|c| eligible(c, current, trigger))
        .map(|c| {
            let why = why(&c, now_ms);
            let (tier, bucket) = match why {
                Why::ExpiresSoonest { resets_at_ms } => (0, (resets_at_ms - now_ms) / RESET_TIE_MS),
                Why::Reserve => (1, 0),
                Why::Unknown => (2, 0),
            };
            let urgency = weekly_urgency(&c, now_ms);
            (tier, bucket, urgency, c, why)
        })
        .collect();
    keyed.sort_by(|a, b| {
        (a.0, a.1)
            .cmp(&(b.0, b.1))
            .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.3.account.position.cmp(&b.3.account.position))
    });
    keyed
        .into_iter()
        .map(|(_, _, _, c, why)| Ranked { account: c.account, why })
        .collect()
}

/// Build a candidate from the stored live reading, falling back to the
/// Observed/Estimated quota model when no live reading exists.
pub fn candidate_for(
    db: &Database,
    account: &Account,
    project_root: Option<&Path>,
    now_ms: i64,
) -> Candidate {
    let folder_trusted = match (account.provider.as_str(), project_root) {
        ("claude-code", Some(root)) => crate::providers::claude::folder_is_trusted_in(
            Path::new(&account.config_dir),
            account.adopted,
            root,
        ),
        _ => None,
    };
    // Read, never written: finishing first-run is the person's step in the
    // provider's own interface, like trust (M13 §5.4).
    let onboarded = (account.provider == "claude-code").then(|| {
        std::fs::read_to_string(Path::new(&account.config_dir).join(".claude.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .map(|value| value.get("hasCompletedOnboarding").and_then(|v| v.as_bool()).unwrap_or(false))
    }).flatten();
    let live = crate::accounts::live::stored(db, &account.id);
    // A refusal seen in the transcript is official even when the stored live
    // reading predates it: an exhausted CLI may stop answering probes.
    let quota = crate::accounts::quota::for_account(db, account);
    let refused = |name: &str| quota.windows.iter().any(|w| w.window == name && w.exhausted);
    let (session, weekly) = match live.as_ref().and_then(|s| s.reading()) {
        Some(reading) => {
            let state = |w: &crate::accounts::live::LiveWindow| WindowState {
                percent_used: Some(w.percent_used),
                resets_at_ms: w.resets_at_ms,
                exhausted: w.exhausted(),
                confidence: Confidence::Official,
            };
            let session = reading.windows.iter().find(|w| w.group == "session").map(state);
            let weekly = reading
                .windows
                .iter()
                .filter(|w| w.group == "weekly")
                .max_by(|a, b| a.percent_used.partial_cmp(&b.percent_used).unwrap_or(std::cmp::Ordering::Equal))
                .map(state);
            (session, weekly)
        }
        None => {
            let state = |name: &str| {
                quota.windows.iter().find(|w| w.window == name).map(|w| WindowState {
                    percent_used: w.percent,
                    resets_at_ms: w.resets_at_ms,
                    exhausted: w.exhausted,
                    confidence: w.confidence,
                })
            };
            (state("five_hour"), state("weekly"))
        }
    };
    // A window whose stated reset has passed is fresh, whatever the last
    // reading said — the provider told us when it comes back (M13 §5.2).
    let settle = |w: Option<WindowState>, refused: bool| {
        w.map(|mut w| {
            if w.resets_at_ms.is_some_and(|r| r <= now_ms) {
                w.percent_used = Some(0.0);
                w.exhausted = false;
                w.resets_at_ms = None;
            } else if refused {
                w.exhausted = true;
            }
            w
        })
    };
    Candidate {
        account: account.clone(),
        session: settle(session, refused("five_hour")),
        weekly: settle(weekly, refused("weekly")),
        folder_trusted,
        onboarded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;
    const NOW: i64 = 1_000 * HOUR;

    fn acct(id: &str, position: i64) -> Account {
        Account {
            id: id.into(),
            provider: "claude-code".into(),
            label: id.into(),
            config_dir: format!("C:/accounts/{id}"),
            adopted: false,
            email: Some(format!("{id}@example.test")),
            account_uuid: Some(format!("uuid-{id}")),
            org_id: None,
            org_name: None,
            plan: Some("pro".into()),
            signed_in: true,
            checked_at: Some(1),
            identity_attempted_at: Some(1),
            subscription_since: 0,
            active: false,
            paused: false,
            position,
            created_at: 1,
            last_used_at: None,
            credential_origin: "own".into(),
        }
    }

    fn win(percent: f64, resets_in: Option<i64>) -> Option<WindowState> {
        Some(WindowState {
            percent_used: Some(percent),
            resets_at_ms: resets_in.map(|d| NOW + d),
            exhausted: percent >= 100.0,
            confidence: Confidence::Official,
        })
    }

    fn cand(id: &str, position: i64, session: Option<WindowState>, weekly: Option<WindowState>) -> Candidate {
        Candidate { account: acct(id, position), session, weekly, folder_trusted: Some(true), onboarded: Some(true) }
    }

    const NEAR: Trigger = Trigger::NearLimit { percent: 90 };

    fn ids(ranked: &[Ranked]) -> Vec<&str> {
        ranked.iter().map(|r| r.account.id.as_str()).collect()
    }

    #[test]
    fn soonest_reset_wins() {
        let current = acct("cur", 0);
        let ranked = rank(
            NOW,
            &current,
            &NEAR,
            vec![
                cand("b", 2, win(10.0, Some(4 * HOUR)), win(10.0, Some(100 * HOUR))),
                cand("a", 1, win(30.0, Some(40 * MIN)), win(10.0, Some(100 * HOUR))),
            ],
        );
        assert_eq!(ids(&ranked), ["a", "b"]);
        assert_eq!(ranked[0].why, Why::ExpiresSoonest { resets_at_ms: NOW + 40 * MIN });
    }

    #[test]
    fn running_window_beats_reserve() {
        let current = acct("cur", 0);
        let ranked = rank(
            NOW,
            &current,
            &NEAR,
            vec![
                cand("fresh", 1, win(0.0, None), win(5.0, Some(100 * HOUR))),
                cand("running", 2, win(50.0, Some(3 * HOUR)), win(20.0, Some(100 * HOUR))),
            ],
        );
        assert_eq!(ids(&ranked), ["running", "fresh"]);
        assert_eq!(ranked[1].why, Why::Reserve);
    }

    #[test]
    fn unknown_ranks_last() {
        let current = acct("cur", 0);
        let ranked = rank(
            NOW,
            &current,
            &NEAR,
            vec![cand("dark", 1, None, None), cand("fresh", 2, win(0.0, None), None)],
        );
        assert_eq!(ids(&ranked), ["fresh", "dark"]);
        assert_eq!(ranked[1].why, Why::Unknown);
    }

    #[test]
    fn weekly_urgency_breaks_reset_ties() {
        let current = acct("cur", 0);
        // Same five-hour reset (5 min apart); `b` has weekly allowance that
        // expires in a day, `a` has the same allowance with six days left.
        let ranked = rank(
            NOW,
            &current,
            &NEAR,
            vec![
                cand("a", 1, win(20.0, Some(2 * HOUR)), win(40.0, Some(144 * HOUR))),
                cand("b", 2, win(20.0, Some(2 * HOUR + 5 * MIN)), win(40.0, Some(24 * HOUR))),
            ],
        );
        assert_eq!(ids(&ranked), ["b", "a"]);
    }

    #[test]
    fn twins_and_copied_credentials_never_rank() {
        let current = acct("cur", 0);
        let mut twin = cand("twin", 1, win(0.0, None), None);
        twin.account.account_uuid = current.account_uuid.clone();
        let mut copied = cand("copied", 2, win(0.0, None), None);
        copied.account.credential_origin = "copied".into();
        assert!(rank(NOW, &current, &NEAR, vec![twin, copied]).is_empty());
    }

    #[test]
    fn headroom_excludes_near_full_destination() {
        let current = acct("cur", 0);
        let ranked = rank(
            NOW,
            &current,
            &NEAR,
            vec![
                cand("full", 1, win(81.0, Some(HOUR)), None),
                cand("ok", 2, win(79.0, Some(HOUR)), None),
                cand("weekly-full", 3, win(10.0, Some(HOUR)), win(85.0, Some(HOUR))),
            ],
        );
        assert_eq!(ids(&ranked), ["ok"]);
    }

    #[test]
    fn exhausted_trigger_uses_ninety_ceiling() {
        let current = acct("cur", 0);
        let ranked = rank(
            NOW,
            &current,
            &Trigger::Exhausted,
            vec![
                cand("high", 1, win(89.0, Some(HOUR)), None),
                cand("over", 2, win(91.0, Some(HOUR)), None),
                cand("gone", 3, win(100.0, Some(HOUR)), None),
            ],
        );
        assert_eq!(ids(&ranked), ["high"]);
    }

    #[test]
    fn unonboarded_destination_excluded() {
        // A directory whose CLI never finished its first-run screens would park
        // the resumed conversation on them, with nobody to press Enter.
        let current = acct("cur", 0);
        let mut c = cand("a", 1, win(0.0, None), None);
        c.onboarded = Some(false);
        assert!(rank(NOW, &current, &NEAR, vec![c]).is_empty());
    }

    #[test]
    fn untrusted_folder_excluded() {
        let current = acct("cur", 0);
        let mut c = cand("a", 1, win(0.0, None), None);
        c.folder_trusted = Some(false);
        assert!(rank(NOW, &current, &NEAR, vec![c]).is_empty());
    }
}
