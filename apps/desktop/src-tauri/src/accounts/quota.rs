//! What is left of an account's allowance, and how sure we are (§28, §66).
//!
//! ## The two providers are not equal here either
//!
//! **Codex states its own consumption.** Every `token_count` event carries
//! `rate_limits.primary.used_percent`, the window length in minutes and when it
//! resets. That is Official and needs no interpretation.
//!
//! **Claude Code does not.** This was established by reading every transcript
//! on this machine — 115 files — and finding exactly one shape of quota data in
//! them: a `quotaLimits` object attached to the assistant turn that *was
//! refused*, carrying `status: "rejected"`, `rateLimitType` and `resetsAt`.
//! There is no running gauge anywhere in the transcript. So for Claude Code:
//!
//! * the moment an account is exhausted, and the exact moment it recovers, are
//!   **Official** — the provider said so, to the second;
//! * how full the window is before that, is **Observed** at best — a sum of the
//!   token counts the provider itself reported for turns inside the window;
//! * and a percentage of that sum requires knowing the allowance, which nobody
//!   publishes. We learn it from this machine's own history: every rejection
//!   tells us "this account was refused after roughly N tokens in its window",
//!   and that is the calibration. Until an account has been refused at least
//!   once, its percentage is **Unknown** and the surface says so rather than
//!   drawing a confident bar over a number nobody has.
//!
//! That last point is the whole discipline of §28 applied to the feature Alan
//! cares most about: an automatic switch fired on a guess must announce itself
//! as a guess, and a bar that looks Official must be Official.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Database;
use crate::session::event::Confidence;

use super::{Account, Result};

/// The five-hour window every Claude subscription is rationed by.
pub const FIVE_HOUR_MS: i64 = 5 * 60 * 60 * 1000;
/// The weekly window that sits above it.
pub const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// A limit observation, exactly as the provider stated it.
#[derive(Debug, Clone, PartialEq)]
pub struct LimitObservation {
    /// The provider's own name for the window, kept verbatim.
    pub window: String,
    /// ok | warning | rejected
    pub status: String,
    pub resets_at_ms: Option<i64>,
    pub percent: Option<f64>,
    pub detail: Option<String>,
}

/// How long a named window lasts, when we know.
pub fn window_length_ms(window: &str) -> Option<i64> {
    match window {
        "five_hour" => Some(FIVE_HOUR_MS),
        "weekly" | "seven_day" | "opus_weekly" | "seven_day_opus" => Some(WEEK_MS),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Reading what a provider said
// ---------------------------------------------------------------------------

/// Pull a limit observation out of one raw Claude Code transcript line.
///
/// Returns `None` for the overwhelming majority of lines, which carry no quota
/// information at all — the caller guards on a substring first so this is not
/// a second JSON parse of every line in a busy session.
pub fn claude_observation(line: &str) -> Option<LimitObservation> {
    let value: Value = serde_json::from_str(line).ok()?;
    let quota = value.get("quotaLimits")?;

    let status = quota
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

    // `resetsAt` is unix **seconds**, not milliseconds. Reading it as
    // milliseconds puts the reset in 1970 and makes an exhausted account look
    // permanently recovered, which is the failure that silently disables the
    // entire feature.
    let resets_at_ms = quota
        .get("resetsAt")
        .and_then(Value::as_i64)
        .map(|secs| secs * 1000);

    // The sentence Claude Code itself showed the user, kept so the panel can
    // quote the provider rather than paraphrase it.
    let detail = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
        .and_then(|blocks| {
            blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .next()
        })
        .map(str::to_string);

    Some(LimitObservation {
        window: quota
            .get("rateLimitType")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        status,
        resets_at_ms,
        // Claude Code states no percentage anywhere. Inventing one here would
        // put an Estimated number behind an Official label.
        percent: None,
        detail,
    })
}

/// Pull limit observations out of one raw Codex rollout line.
///
/// Codex reports two windows at once — a short `primary` and a longer
/// `secondary` — and both are returned. Folding them into one, as an earlier
/// version of the provider adapter did, throws away whichever window is not
/// currently the binding one, which is the one a person planning their week
/// actually wants to see.
pub fn codex_observations(line: &str) -> Vec<LimitObservation> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let Some(limits) = value.get("payload").and_then(|p| p.get("rate_limits")) else {
        return Vec::new();
    };
    let ts_ms = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(crate::providers::conversation::parse_timestamp)
        .unwrap_or_else(now_ms);

    let mut out = Vec::new();
    for (key, fallback_name) in [("primary", "primary"), ("secondary", "secondary")] {
        let Some(window) = limits.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let percent = window.get("used_percent").and_then(Value::as_f64);

        // Two spellings exist in the wild and only one is present per build:
        // `resets_at` (absolute unix seconds — what this machine's Codex
        // writes) and `resets_in_seconds` (relative). Reading only the second,
        // which is what the provider adapter did, silently produced no reset
        // time at all on this machine.
        let resets_at_ms = window
            .get("resets_at")
            .and_then(Value::as_i64)
            .map(|secs| secs * 1000)
            .or_else(|| {
                window
                    .get("resets_in_seconds")
                    .and_then(Value::as_i64)
                    .map(|secs| ts_ms + secs * 1000)
            });

        // Name the window by its length when Codex gives one, so a weekly
        // allowance is not filed under a name that means "the short one".
        let name = match window.get("window_minutes").and_then(Value::as_i64) {
            Some(m) if m >= 6 * 24 * 60 => "weekly".to_string(),
            Some(m) if m <= 6 * 60 => "five_hour".to_string(),
            _ => fallback_name.to_string(),
        };

        out.push(LimitObservation {
            window: name,
            status: match percent {
                Some(p) if p >= 100.0 => "rejected".to_string(),
                Some(p) if p >= 90.0 => "warning".to_string(),
                _ => "ok".to_string(),
            },
            resets_at_ms,
            percent,
            detail: None,
        });
    }
    out
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/// Record what a provider said about an account's allowance.
///
/// Deliberately not deduplicated on the way in. Codex restates its percentage
/// on every turn, and those repeats are the only running record of how a window
/// filled up — collapsing them would leave a history that can say when an
/// account was refused and nothing about how it got there.
pub fn record(
    db: &Database,
    account_id: &str,
    session_id: Option<&str>,
    observation: &LimitObservation,
) {
    let _ = db.with(|conn| {
        conn.execute(
            "INSERT INTO account_limit_events
                 (account_id, session_id, ts_ms, window, status, resets_at_ms, percent, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                account_id,
                session_id,
                now_ms(),
                observation.window,
                observation.status,
                observation.resets_at_ms,
                observation.percent,
                observation.detail,
            ],
        )?;
        Ok(())
    });
}

/// Read a transcript line for quota news and record anything it carries.
///
/// The substring guard is what keeps this affordable: it runs on every line of
/// every live transcript, and all but a handful of them have nothing to say.
pub fn observe_line(
    db: &Database,
    account_id: &str,
    session_id: &str,
    provider: &str,
    line: &str,
) -> Vec<LimitObservation> {
    let found = match provider {
        "claude-code" if line.contains("quotaLimits") => {
            claude_observation(line).into_iter().collect()
        }
        "codex" if line.contains("rate_limits") => codex_observations(line),
        _ => Vec::new(),
    };
    for observation in &found {
        record(db, account_id, Some(session_id), observation);
    }
    found
}

// ---------------------------------------------------------------------------
// The picture the panel draws
// ---------------------------------------------------------------------------

/// One allowance window, as the surface renders it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// Stable id the UI localises. Never prose (§65).
    pub window: String,
    /// 0–100, or `None` when nothing honest can be said.
    pub percent: Option<f64>,
    /// Where that percentage came from. An Observed figure must never be drawn
    /// the way an Official one is.
    pub confidence: Confidence,
    /// When the provider says the window resets.
    pub resets_at_ms: Option<i64>,
    /// True while the provider is refusing work on this window.
    pub exhausted: bool,
    /// Tokens this account spent inside the current window, from the counts the
    /// provider itself reported per turn. Always Observed.
    pub tokens: i64,
    /// Tokens this account was refused at, learned from its own past
    /// rejections. `None` until it has been refused at least once.
    pub calibration_tokens: Option<i64>,
    /// How many past rejections that calibration rests on, so the surface can
    /// say "learned from one refusal" rather than implying a measurement.
    pub calibration_samples: i64,
}

/// Whether an account can take work right now.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountHealth {
    /// Signed in, in the rotation, and not near a limit.
    Ready,
    /// Signed in and usable, but close enough to a limit to plan around.
    Nearing,
    /// The provider is refusing work until the window resets.
    Exhausted,
    /// Taken out of the rotation by the user.
    Paused,
    /// No working credentials in this account's configuration directory.
    SignedOut,
}

/// Everything the Accounts surface shows for one account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountQuota {
    pub account_id: String,
    pub health: AccountHealth,
    pub windows: Vec<QuotaWindow>,
    /// The earliest moment this account can work again, when it cannot now.
    pub recovers_at_ms: Option<i64>,
    /// The provider's own sentence about the refusal, quoted rather than
    /// paraphrased, so the panel never puts words in the provider's mouth.
    pub refusal_detail: Option<String>,
    /// Tokens spent on this account in the last twenty-four hours. Context for
    /// the windows above, and the one number that is meaningful even for an
    /// account whose allowance nobody can express as a percentage.
    pub tokens_today: i64,
    /// Live sessions currently running on this account. A switch never touches
    /// them, and the surface has to be able to say so.
    pub live_sessions: i64,
}

/// The latest thing the provider said about one window.
fn latest_event(
    db: &Database,
    account_id: &str,
    window: &str,
) -> Option<(i64, String, Option<i64>, Option<f64>, Option<String>)> {
    db.with(|conn| {
        Ok(conn
            .query_row(
                "SELECT ts_ms, status, resets_at_ms, percent, detail
                   FROM account_limit_events
                  WHERE account_id = ?1 AND window = ?2
                  ORDER BY ts_ms DESC LIMIT 1",
                params![account_id, window],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?)
    })
    .ok()
    .flatten()
}

/// Tokens this account reported spending in `[from, to)`.
///
/// Cache reads are excluded on purpose: they are the cheap half of a turn, they
/// dwarf everything else in a long session, and counting them would make every
/// account look near its limit after twenty minutes. Cache *writes* are counted
/// because they are billed as input.
fn tokens_between(db: &Database, account_id: &str, from: i64, to: i64) -> i64 {
    db.with(|conn| {
        Ok(conn
            .query_row(
                "SELECT COALESCE(SUM(COALESCE(input_tokens,0) + COALESCE(output_tokens,0)
                                    + COALESCE(cache_write_tokens,0)), 0)
                   FROM usage_samples
                  WHERE account_id = ?1 AND ts_ms >= ?2 AND ts_ms < ?3",
                params![account_id, from, to],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0))
    })
    .unwrap_or(0)
}

/// When the window an account is currently inside began.
///
/// A five-hour allowance is not a rolling average: it starts at the first
/// request after the last reset and ends five hours later. Whenever the
/// provider has told us a reset time we anchor to it exactly, because that is
/// the truth and a rolling window would smear spend across a boundary the
/// provider treats as absolute. With no reset ever reported, a rolling window
/// is the honest approximation — and everything computed from it is stamped
/// Observed, never Official.
fn window_start(resets_at_ms: Option<i64>, length_ms: i64, now: i64) -> i64 {
    match resets_at_ms {
        // The window we are inside ends at the announced reset.
        Some(reset) if reset > now => reset - length_ms,
        // The announced reset has already passed: the current window began
        // there and runs forward from it.
        Some(reset) => {
            let elapsed = now - reset;
            reset + (elapsed / length_ms) * length_ms
        }
        None => now - length_ms,
    }
}

/// What this account has historically been refused at, in tokens.
///
/// Every past rejection is one measurement: sum the account's own reported
/// tokens over the window that ended at the refusal. The **smallest** such
/// measurement is used rather than the largest or the mean, because the number
/// is used to decide when to switch away, and switching a little early costs
/// nothing while switching late costs a refused turn in the middle of a run.
fn calibration(db: &Database, account_id: &str, window: &str, length_ms: i64) -> (Option<i64>, i64) {
    let rejections: Vec<i64> = db
        .with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT ts_ms FROM account_limit_events
                  WHERE account_id = ?1 AND window = ?2 AND status = 'rejected'
                  ORDER BY ts_ms DESC LIMIT 12",
            )?;
            let rows: rusqlite::Result<Vec<i64>> =
                stmt.query_map(params![account_id, window], |r| r.get(0))?.collect();
            Ok(rows?)
        })
        .unwrap_or_default();

    let mut totals: Vec<i64> = rejections
        .iter()
        .map(|at| tokens_between(db, account_id, at - length_ms, *at))
        .filter(|t| *t > 0)
        .collect();
    totals.sort_unstable();

    (totals.first().copied(), totals.len() as i64)
}

/// Build one window's picture from everything known about it.
fn build_window(db: &Database, account: &Account, window: &str, now: i64) -> QuotaWindow {
    let length = window_length_ms(window).unwrap_or(FIVE_HOUR_MS);
    let latest = latest_event(db, &account.id, window);

    let (event_ts, status, resets_at_ms, official_percent, _detail) = match latest {
        Some(v) => (Some(v.0), v.1, v.2, v.3, v.4),
        None => (None, "ok".to_string(), None, None, None),
    };

    // A rejection only stands until its own reset time. After that the account
    // is presumed recovered — the provider said when, and waiting for it to say
    // so a second time would leave an account parked as exhausted forever, since
    // nothing runs on it to produce a new observation.
    let exhausted = status == "rejected" && resets_at_ms.map(|r| r > now).unwrap_or(false);

    let start = window_start(resets_at_ms, length, now);
    let tokens = tokens_between(db, &account.id, start, now + 1);
    let (calibration_tokens, calibration_samples) = calibration(db, &account.id, window, length);

    // Order matters and encodes §28. An official percentage wins; a stale one
    // is not used to describe the window we are in now.
    let official_is_current = official_percent.is_some()
        && event_ts.map(|ts| ts >= start).unwrap_or(false);

    let (percent, confidence) = if exhausted {
        (Some(100.0), Confidence::Official)
    } else if official_is_current {
        (official_percent, Confidence::Official)
    } else {
        match calibration_tokens {
            Some(limit) if limit > 0 => (
                Some(((tokens as f64 / limit as f64) * 100.0).clamp(0.0, 100.0)),
                Confidence::Estimated,
            ),
            // Tokens are known and the allowance is not. Saying nothing about
            // the percentage is the honest answer, and the surface renders the
            // token count instead of an empty bar.
            _ => (None, Confidence::Unknown),
        }
    };

    QuotaWindow {
        window: window.to_string(),
        percent,
        confidence,
        resets_at_ms,
        exhausted,
        tokens,
        calibration_tokens,
        calibration_samples,
    }
}

/// Which windows a provider is rationed by.
fn windows_for(provider: &str) -> &'static [&'static str] {
    match provider {
        "claude-code" => &["five_hour", "weekly"],
        "codex" => &["five_hour", "weekly"],
        _ => &[],
    }
}

/// The percentage at which an account is considered to be nearing its limit.
///
/// Also the default trigger for an automatic switch, which is why it lives here
/// rather than in the switch policy: the bar the user sees turning amber and the
/// point work moves away are the same number, and two constants would drift.
pub const NEARING_PERCENT: f64 = 85.0;

pub fn for_account(db: &Database, account: &Account) -> AccountQuota {
    let now = now_ms();
    let windows: Vec<QuotaWindow> = windows_for(&account.provider)
        .iter()
        .map(|w| build_window(db, account, w, now))
        .collect();

    let recovers_at_ms = windows
        .iter()
        .filter(|w| w.exhausted)
        .filter_map(|w| w.resets_at_ms)
        .max();

    let refusal_detail = recovers_at_ms.and_then(|_| {
        db.with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT detail FROM account_limit_events
                      WHERE account_id = ?1 AND status = 'rejected' AND detail IS NOT NULL
                      ORDER BY ts_ms DESC LIMIT 1",
                    [&account.id],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten())
        })
        .ok()
        .flatten()
    });

    let live_sessions: i64 = db
        .with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT COUNT(*) FROM sessions
                      WHERE account_id = ?1 AND ended_at IS NULL",
                    [&account.id],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0))
        })
        .unwrap_or(0);

    // Order matters: a paused account that is also exhausted is reported as
    // exhausted, because that is the fact that decides whether it could take
    // work if the user un-paused it.
    let health = if windows.iter().any(|w| w.exhausted) {
        AccountHealth::Exhausted
    } else if !account.signed_in {
        AccountHealth::SignedOut
    } else if account.paused {
        AccountHealth::Paused
    } else if windows
        .iter()
        .any(|w| w.percent.map(|p| p >= NEARING_PERCENT).unwrap_or(false))
    {
        AccountHealth::Nearing
    } else {
        AccountHealth::Ready
    };

    AccountQuota {
        account_id: account.id.clone(),
        health,
        windows,
        recovers_at_ms,
        refusal_detail,
        tokens_today: tokens_between(db, &account.id, now - 24 * 60 * 60 * 1000, now + 1),
        live_sessions,
    }
}

/// Every account on a provider, with its quota, in rotation order.
pub fn report(db: &Database, provider: &str) -> Result<Vec<(Account, AccountQuota)>> {
    Ok(super::list(db)?
        .into_iter()
        .filter(|a| a.provider == provider)
        .map(|a| {
            let quota = for_account(db, &a);
            (a, quota)
        })
        .collect())
}
