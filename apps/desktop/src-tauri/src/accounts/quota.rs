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
//!
//! ## What M16 changed, and what it deliberately did not
//!
//! Everything above is still true of *transcripts*, which is where it was
//! measured. It is not true of the providers as a whole: both of them answer a
//! live, official usage question on their own CLI protocols, and
//! [`super::live`] asks it. That reading is folded straight into
//! `account_limit_events` — the same table a refusal writes to — so this module
//! did not have to learn that probing exists. An official percentage now
//! usually *is* present, and the Observed/Estimated ladder below became the
//! fallback for when a probe cannot run rather than the normal case.
//!
//! The ladder was kept rather than deleted, and sharpened: see
//! [`implied_allowance`], which learns the invisible allowance from any
//! official percentage instead of only from a refusal.

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
    /// Tokens this account spent on each of the last [`HISTORY_DAYS`] days,
    /// oldest first, with today last.
    ///
    /// Always present and always the same length, including the zeroes: a
    /// sparkline with the quiet days removed is a different shape from the one
    /// the days actually made, and the flat stretch before a spike is the part
    /// that tells you the spike was unusual.
    ///
    /// Observed, never Official — these are our own sums of what the provider
    /// reported per turn, and they are a record of *this machine's* work on the
    /// account, which is not the same as everything the account has done.
    pub daily_tokens: Vec<i64>,
    /// Tokens over that whole window, so the sparkline has a magnitude.
    pub window_tokens: i64,
    /// What the provider itself priced that at, when it prices anything.
    ///
    /// `None` for a subscription, where per-turn cost is not reported and
    /// inventing one from public rates would be a guess wearing the same
    /// typeface as a fact (§28). A subscription's real cost is the plan, and
    /// the panel already shows what is left of it.
    pub window_cost_usd: Option<f64>,
    /// The newest thing the provider said when asked directly (M16).
    ///
    /// `None` only before an account has ever been probed. Everything else —
    /// signed out, no live limits on this plan, the CLI missing — is a *stated*
    /// outcome inside [`live::LiveStatus`], because "we have not asked yet" and
    /// "we asked and there is nothing" are different sentences to read and the
    /// surface must not collapse them into one grey card.
    pub live: Option<super::live::LiveStatus>,
    /// Whether that reading is old enough that a fresh probe is worth running.
    pub live_stale: bool,
}

/// The latest thing the provider said about one window.
///
/// `since` is the account's `subscription_since`: a directory that has been
/// signed into a different account since has a history that belongs to somebody
/// else, and reading a refusal from it would park a perfectly healthy account
/// as exhausted on the strength of a stranger's window.
fn latest_event(
    db: &Database,
    account_id: &str,
    window: &str,
    since: i64,
) -> Option<(i64, String, Option<i64>, Option<f64>, Option<String>)> {
    db.with(|conn| {
        Ok(conn
            .query_row(
                "SELECT ts_ms, status, resets_at_ms, percent, detail
                   FROM account_limit_events
                  WHERE account_id = ?1 AND window = ?2 AND ts_ms >= ?3
                  ORDER BY ts_ms DESC LIMIT 1",
                params![account_id, window, since],
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
///
/// `from` is clamped to the account's `subscription_since` by every caller, so
/// spend recorded while the directory belonged to another account never lands
/// in this one's window.
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
fn calibration(
    db: &Database,
    account_id: &str,
    window: &str,
    length_ms: i64,
    since: i64,
) -> (Option<i64>, i64) {
    let rejections: Vec<i64> = db
        .with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT ts_ms FROM account_limit_events
                  WHERE account_id = ?1 AND window = ?2 AND status = 'rejected'
                    AND ts_ms >= ?3
                  ORDER BY ts_ms DESC LIMIT 12",
            )?;
            let rows: rusqlite::Result<Vec<i64>> = stmt
                .query_map(params![account_id, window, since], |r| r.get(0))?
                .collect();
            Ok(rows?)
        })
        .unwrap_or_default();

    let mut totals: Vec<i64> = rejections
        .iter()
        .map(|at| tokens_between(db, account_id, (at - length_ms).max(since), *at))
        .filter(|t| *t > 0)
        .collect();
    totals.sort_unstable();

    (totals.first().copied(), totals.len() as i64)
}

/// The allowance implied by any official percentage, not only by a refusal.
///
/// A live reading says "this window is 42% used". The tokens this machine saw
/// the account spend inside that same window are already known. Those two
/// numbers give the allowance the provider never publishes:
/// `tokens / (percent / 100)`.
///
/// Why it is worth having when a live percentage is usually available anyway:
/// the probe needs the CLI on PATH and the account signed in. When it cannot
/// run — an account mid-login, a CLI being upgraded, a machine offline — the
/// Estimated tier is all that is left, and before this it could only be
/// calibrated by a *refusal*, meaning the number it needed most only arrived
/// after the failure it existed to prevent.
///
/// Readings below ten percent are skipped: dividing by a small percentage
/// multiplies its rounding error by ten or more, and a provider that reports
/// whole numbers makes "1%" mean anything from 0.5 to 1.5. The **smallest**
/// implied allowance wins, for the same reason `calibration` takes the smallest
/// refusal — the number decides when to leave an account, and leaving early is
/// free while leaving late costs a refused turn.
fn implied_allowance(
    db: &Database,
    account_id: &str,
    window: &str,
    length_ms: i64,
    since: i64,
) -> Option<i64> {
    let samples: Vec<(i64, f64, Option<i64>)> = db
        .with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT ts_ms, percent, resets_at_ms FROM account_limit_events
                  WHERE account_id = ?1 AND window = ?2 AND percent >= 10
                    AND ts_ms >= ?3
                  ORDER BY ts_ms DESC LIMIT 16",
            )?;
            let rows: rusqlite::Result<Vec<(i64, f64, Option<i64>)>> = stmt
                .query_map(params![account_id, window, since], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?
                .collect();
            Ok(rows?)
        })
        .unwrap_or_default();

    samples
        .into_iter()
        .filter_map(|(ts, percent, resets_at)| {
            // The window that reading described, anchored the same way the
            // displayed window is — otherwise the token sum and the percentage
            // would be describing different spans of time.
            let start = window_start(resets_at, length_ms, ts);
            let tokens = tokens_between(db, account_id, start.max(since), ts + 1);
            (tokens > 0).then(|| (tokens as f64 / (percent / 100.0)) as i64)
        })
        .filter(|allowance| *allowance > 0)
        .min()
}

/// How many days of usage history a card carries.
///
/// Two weeks: long enough to contain a whole weekly allowance and the one
/// before it, so "is this week heavier than usual" is a question the shape can
/// answer, and short enough to stay legible at the width of a card.
pub const HISTORY_DAYS: i64 = 14;

/// Tokens per day for one account, oldest first, zero-filled.
///
/// Bucketed by **UTC** calendar day, matching `analytics::report`. That is one
/// day-boundary convention across the product rather than two that disagree by
/// a few hours; a sparkline is read for its shape, and the shape does not
/// change when the boundary moves.
fn daily_tokens(db: &Database, account_id: &str, now: i64, owned_since: i64) -> Vec<i64> {
    let since = now - HISTORY_DAYS * 86_400_000;
    let day_of = |ts: i64| ((ts - since) / 86_400_000).clamp(0, HISTORY_DAYS - 1) as usize;

    let mut days = vec![0i64; HISTORY_DAYS as usize];
    let _ = db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT ts_ms,
                    COALESCE(input_tokens,0) + COALESCE(output_tokens,0)
                  + COALESCE(cache_write_tokens,0)
               FROM usage_samples
              WHERE account_id = ?1 AND ts_ms >= ?2",
        )?;
        let rows = stmt.query_map(params![account_id, since.max(owned_since)], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?;
        for row in rows.flatten() {
            days[day_of(row.0)] += row.1;
        }
        Ok(())
    });
    days
}

/// What the provider charged for this account's work in the window, if it says.
///
/// `NULL` and `0` are both "nothing was priced" and neither becomes a zero on
/// screen: a subscription reports no per-turn cost, and printing "$0.00" over
/// a fortnight of real work states something false about a real number.
fn window_cost(db: &Database, account_id: &str, since: i64) -> Option<f64> {
    db.with(|conn| {
        Ok(conn
            .query_row(
                "SELECT SUM(cost_usd) FROM usage_samples
                  WHERE account_id = ?1 AND ts_ms >= ?2 AND cost_usd IS NOT NULL",
                params![account_id, since],
                |row| row.get::<_, Option<f64>>(0),
            )
            .optional()?
            .flatten())
    })
    .ok()
    .flatten()
    .filter(|cost| *cost > 0.0)
}

/// Build one window's picture from everything known about it.
fn build_window(db: &Database, account: &Account, window: &str, now: i64) -> QuotaWindow {
    let length = window_length_ms(window).unwrap_or(FIVE_HOUR_MS);
    // Everything this window is built from is bounded below by the moment this
    // directory started belonging to the subscription it is signed into now.
    let owned_since = account.subscription_since;
    let latest = latest_event(db, &account.id, window, owned_since);

    let (event_ts, status, resets_at_ms, official_percent, _detail) = match latest {
        Some(v) => (Some(v.0), v.1, v.2, v.3, v.4),
        None => (None, "ok".to_string(), None, None, None),
    };

    // A rejection only stands until its own reset time. After that the account
    // is presumed recovered — the provider said when, and waiting for it to say
    // so a second time would leave an account parked as exhausted forever, since
    // nothing runs on it to produce a new observation.
    let exhausted = status == "rejected" && resets_at_ms.map(|r| r > now).unwrap_or(true);

    let start = window_start(resets_at_ms, length, now);
    let tokens = tokens_between(db, &account.id, start.max(owned_since), now + 1);
    let (refusal_calibration, calibration_samples) =
        calibration(db, &account.id, window, length, owned_since);
    // A refusal is the sharper measurement — it is the allowance being hit, not
    // divided into — so it wins where both exist.
    let calibration_tokens = refusal_calibration
        .or_else(|| implied_allowance(db, &account.id, window, length, owned_since));

    // Order matters and encodes §28. An official percentage wins; a stale one
    // is not used to describe the window we are in now.
    let official_is_current =
        official_percent.is_some() && event_ts.map(|ts| ts >= start).unwrap_or(false);

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
        // A past provider timestamp still anchors the token window and its
        // calibration, but it is not the reset of the window currently shown.
        // Advancing it by `length` would be our inference presented through a
        // field documented as provider-reported; keeping the stale value makes
        // the UI promise "resetting now" forever. Until a current observation
        // arrives, the honest countdown is no countdown.
        resets_at_ms: resets_at_ms.filter(|reset| *reset > now),
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
                        AND ts_ms >= ?2
                      ORDER BY ts_ms DESC LIMIT 1",
                    params![&account.id, account.subscription_since],
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

    let history = daily_tokens(db, &account.id, now, account.subscription_since);
    let live = super::live::stored(db, &account.id);

    // A live reading is the sharpest statement of exhaustion there is: the
    // provider was asked a moment ago and answered. It is read alongside the
    // window history rather than instead of it, because a refusal recorded
    // mid-session can be newer than the last probe.
    let live_exhausted = live
        .as_ref()
        .and_then(super::live::LiveStatus::reading)
        .map(|reading| reading.windows.iter().any(super::live::LiveWindow::exhausted))
        .unwrap_or(false);

    // Order matters: a paused account that is also exhausted is reported as
    // exhausted, because that is the fact that decides whether it could take
    // work if the user un-paused it.
    let health = if windows.iter().any(|w| w.exhausted) || live_exhausted {
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
        tokens_today: tokens_between(
            db,
            &account.id,
            (now - 24 * 60 * 60 * 1000).max(account.subscription_since),
            now + 1,
        ),
        live_sessions,
        window_tokens: history.iter().sum(),
        daily_tokens: history,
        window_cost_usd: window_cost(
            db,
            &account.id,
            (now - HISTORY_DAYS * 86_400_000).max(account.subscription_since),
        ),
        live_stale: super::live::is_stale(live.as_ref(), now),
        live,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn insert_account(db: &Database, id: &str, provider: &str) -> Account {
        db.with(|conn| {
            conn.execute(
                "INSERT INTO provider_accounts
                     (id, provider, label, config_dir, adopted, signed_in, active, paused,
                      position, created_at, checked_at)
                 VALUES (?1, ?2, ?1, ?3, 0, 1, 1, 0, 0, 1, 1)",
                params![id, provider, format!("C:/accounts/{id}")],
            )?;
            Ok(())
        })
        .unwrap();
        super::super::get(db, id).unwrap().unwrap()
    }

    fn usage(db: &Database, account_id: Option<&str>, ts_ms: i64, input: i64) {
        db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_samples
                     (provider, ts_ms, input_tokens, confidence, account_id)
                 VALUES ('claude-code', ?1, ?2, 'official', ?3)",
                params![ts_ms, input, account_id],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn claude_reset_is_unix_seconds_and_never_invents_a_percentage() {
        let line = r#"{"quotaLimits":{"status":"rejected","resetsAt":1787556000,"rateLimitType":"five_hour"},"message":{"content":[{"text":"You've hit your limit"}]}}"#;
        let observation = claude_observation(line).unwrap();
        assert_eq!(observation.resets_at_ms, Some(1_787_556_000_000));
        assert_eq!(observation.percent, None);
        assert_eq!(observation.detail.as_deref(), Some("You've hit your limit"));
    }

    #[test]
    fn codex_preserves_both_reported_windows() {
        let line = r#"{"timestamp":"2026-08-24T00:00:00Z","payload":{"rate_limits":{"primary":{"used_percent":18.0,"window_minutes":10080,"resets_at":1788050255},"secondary":{"used_percent":72.0,"window_minutes":300,"resets_in_seconds":600}}}}"#;
        let observations = codex_observations(line);
        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].window, "weekly");
        assert_eq!(observations[0].resets_at_ms, Some(1_788_050_255_000));
        assert_eq!(observations[1].window, "five_hour");
        assert_eq!(observations[1].percent, Some(72.0));
    }

    #[test]
    fn an_uncalibrated_claude_window_reports_tokens_without_a_fake_bar() {
        let db = Database::open_in_memory().unwrap();
        let account = insert_account(&db, "a", "claude-code");
        usage(&db, Some("a"), now_ms() - 1_000, 420);

        let quota = for_account(&db, &account);
        let window = quota
            .windows
            .iter()
            .find(|window| window.window == "five_hour")
            .unwrap();
        assert_eq!(window.tokens, 420);
        assert_eq!(window.percent, None);
        assert_eq!(window.confidence, Confidence::Unknown);
    }

    #[test]
    fn pre_account_usage_is_never_folded_into_the_machine_account() {
        let db = Database::open_in_memory().unwrap();
        let account = insert_account(&db, "a", "claude-code");
        let now = now_ms();
        usage(&db, None, now - 2_000, 9_000);
        usage(&db, Some("a"), now - 1_000, 100);

        let quota = for_account(&db, &account);
        let window = quota
            .windows
            .iter()
            .find(|window| window.window == "five_hour")
            .unwrap();
        assert_eq!(window.tokens, 100);
    }

    #[test]
    fn usage_history_is_zero_filled_and_belongs_to_one_account() {
        let db = Database::open_in_memory().unwrap();
        let account = insert_account(&db, "a", "claude-code");
        insert_account(&db, "b", "claude-code");
        let now = now_ms();

        usage(&db, Some("a"), now - 1_000, 500); // today
        usage(&db, Some("a"), now - 2 * 86_400_000, 300); // two days ago
        usage(&db, Some("b"), now - 1_000, 9_999); // the other account
        usage(&db, None, now - 1_000, 7_777); // recorded before accounts existed
        usage(&db, Some("a"), now - 30 * 86_400_000, 4_444); // outside the window

        let quota = for_account(&db, &account);
        assert_eq!(
            quota.daily_tokens.len(),
            HISTORY_DAYS as usize,
            "the quiet days are the shape — a series with them removed is a \
             different picture from the one the days actually made"
        );
        assert_eq!(*quota.daily_tokens.last().unwrap(), 500, "today");
        assert_eq!(quota.window_tokens, 800, "only this account, only the window");
        assert_eq!(
            quota.window_cost_usd, None,
            "a subscription prices no turn, and $0.00 over a fortnight of real \
             work states something false about a real number"
        );
    }

    #[test]
    fn a_past_refusal_calibrates_an_estimate_but_stops_exhausting_after_reset() {
        let db = Database::open_in_memory().unwrap();
        let account = insert_account(&db, "a", "claude-code");
        let now = now_ms();
        let refused_at = now - 5 * 60 * 60 * 1_000;
        usage(&db, Some("a"), refused_at - 60 * 60 * 1_000, 1_000);
        usage(&db, Some("a"), now - 60 * 60 * 1_000, 850);
        db.with(|conn| {
            conn.execute(
                "INSERT INTO account_limit_events
                     (account_id, ts_ms, window, status, resets_at_ms)
                 VALUES ('a', ?1, 'five_hour', 'rejected', ?2)",
                params![refused_at, now - 4 * 60 * 60 * 1_000],
            )?;
            Ok(())
        })
        .unwrap();

        let quota = for_account(&db, &account);
        let window = quota
            .windows
            .iter()
            .find(|window| window.window == "five_hour")
            .unwrap();
        assert!(
            !window.exhausted,
            "a refusal expires at the provider's reset"
        );
        assert_eq!(window.calibration_tokens, Some(1_000));
        assert_eq!(window.percent, Some(85.0));
        assert_eq!(window.confidence, Confidence::Estimated);
        assert_eq!(window.resets_at_ms, None);
    }
}
