//! Analytics (§52, §53).
//!
//! Every figure here answers a question someone would actually ask. Nothing is
//! included to fill a dashboard, and nothing is turned into a score (§52 is
//! explicit that metrics are information, not gamification).
//!
//! Two rules the queries follow:
//!
//! * **Only what was measured.** Metrics that would need data this build does
//!   not collect are absent rather than estimated. There are no commit counts
//!   here because nothing counts commits yet.
//! * **Confidence travels with the number.** Token figures are grouped by the
//!   confidence the provider's adapter stamped on them, so an estimate can
//!   never be displayed as something a provider reported (§28).

pub mod backfill;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::{Database, Result};
use crate::session::log::now_ms;
use crate::AppState;

/// Tokens, grouped by whatever dimension the caller asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageBucket {
    /// Provider id, model name, project name, or an ISO date — see the query.
    pub label: String,
    pub input: i64,
    pub output: i64,
    pub cache_read: i64,
    pub cache_write: i64,
    /// Distinct sessions contributing to this bucket.
    pub sessions: i64,
    /// The weakest confidence of any sample in the bucket.
    ///
    /// Deliberately the weakest, not the most common: a total that mixes
    /// reported and derived numbers is only as trustworthy as its weakest part.
    pub confidence: String,
}

/// How much of the work happened while nobody was watching (§53).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leverage {
    /// Minutes in which a person actually typed into a session.
    pub human_active_minutes: i64,
    /// Wall-clock minutes that agent sessions were alive.
    pub agent_runtime_minutes: i64,
    pub sessions: i64,
    /// The earliest day this figure could have been measured from.
    ///
    /// Load-bearing rather than a footnote. Tokens go back as far as the
    /// provider's transcripts do — twenty days on this machine — but keystrokes
    /// and session lifetimes were only ever recorded by J.A.R.V.I.S. itself,
    /// which on the same machine is two days. Printing a 30-day leverage ratio
    /// computed from two days of attention would be the most flattering number
    /// on the screen and the least true. The surface says which span it covers.
    pub observed_from: Option<String>,
}

/// One calendar day, whether or not anything happened on it.
///
/// Zero-filled across the whole window on purpose: a calendar with the quiet
/// days missing is not a calendar, and the gaps are half of what the shape
/// says. This machine's own corpus has three of them — 19, 20 and 21 August —
/// and they are the reason the streak below means anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCell {
    /// `YYYY-MM-DD` in **local** time. See `LOCAL_DAY`.
    pub date: String,
    pub tokens: i64,
    /// Assistant turns — the unit of "something happened", see `ACTIVE_DAY`.
    pub turns: i64,
    /// Distinct hours of the day that carried a turn. A cheap, honest proxy for
    /// "how spread out was this day" that needs no keystroke data, so it works
    /// across recovered history as well as watched history.
    pub hours: i64,
    pub projects: i64,
}

/// Days worked, without turning them into a score (§52).
///
/// Alan asked for a streak *and* for the screen to be healthy, and those pull
/// against each other. The resolution here is deliberate: current and longest
/// are reported as **facts, with no target, no goal, and no warning about
/// breaking one**. A quiet day is a day off. The product does not have an
/// opinion about how many days in a row anyone should work.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Streaks {
    /// Consecutive active days ending today — or yesterday, so that a streak
    /// does not appear broken every morning before the first turn of the day.
    pub current: i64,
    pub longest: i64,
    /// `YYYY-MM-DD` of the longest run's first and last day, so it can be named.
    pub longest_from: Option<String>,
    pub longest_to: Option<String>,
    /// Active days in the window, and days in the window. Shown together
    /// because "12 of 30" says something a percentage hides.
    pub active_days: i64,
    pub window_days: i64,
}

/// When in the day the work actually happens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourBucket {
    /// 0–23, local time.
    pub hour: i64,
    pub tokens: i64,
    pub turns: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsReport {
    pub by_provider: Vec<UsageBucket>,
    pub by_model: Vec<UsageBucket>,
    pub by_project: Vec<UsageBucket>,
    pub by_day: Vec<UsageBucket>,
    /// Every day in the window, oldest first, zero-filled.
    pub calendar: Vec<DayCell>,
    pub streaks: Streaks,
    pub by_hour: Vec<HourBucket>,
    pub leverage: Leverage,
    /// Files touched by agent sessions in the window.
    pub files_changed: i64,
    /// The window these figures cover, in days.
    pub window_days: i64,
    /// The single day everything except `calendar` is scoped to, if any.
    ///
    /// The calendar deliberately stays whole while a day is selected: it is
    /// both the picture and the control, and a control that erases itself when
    /// used leaves no way back.
    pub day: Option<String>,
    /// The earliest day any usage is recorded for, at any window.
    ///
    /// What lets the surface say "history starts 4 August" instead of drawing
    /// ninety empty cells and letting them read as ninety idle days.
    pub history_from: Option<String>,
}

/// How a timestamp becomes a calendar day.
///
/// **Local time, not UTC**, and this is a correction rather than a preference.
/// The rest of the product buckets by UTC, which is right for a provider's
/// quota window — those are absolute instants — and wrong for a calendar a
/// person reads. Measured on this machine: work done at 21:04 on 26 August
/// appeared under *27 August*, because that is 00:04 UTC. On an unlabelled
/// sparkline nobody can see that; on a dated calendar it is simply wrong, and
/// the person whose evening it was is the one reading it.
///
/// `accounts::quota::daily_tokens` still buckets in UTC deliberately: it feeds
/// a fourteen-bar sparkline with no dates on it, where the convention is
/// invisible and consistency with the quota windows matters more.
const LOCAL_DAY: &str = "date(u.ts_ms / 1000, 'unixepoch', 'localtime')";

/// What makes a day count as worked.
///
/// One assistant turn. Not a token threshold, not a minimum session length:
/// any rule with a number in it is this product deciding how much work counts
/// as work, which is exactly what §52 rules out. The magnitude is not thrown
/// away — it is what the calendar's intensity carries — so the streak can
/// afford to be a plain yes-or-no.
const ACTIVE_DAY_MIN_TURNS: i64 = 1;

/// Rank confidence so a bucket can report its weakest sample.
fn confidence_rank(value: &str) -> u8 {
    match value {
        "official" => 0,
        "observed" => 1,
        "estimated" => 2,
        _ => 3,
    }
}

fn weakest(a: &str, b: &str) -> String {
    if confidence_rank(a) >= confidence_rank(b) {
        a.to_string()
    } else {
        b.to_string()
    }
}

/// Aggregate token usage grouped by an expression.
///
/// `group_sql` is a fixed fragment chosen by the caller from the set below —
/// never anything derived from user input.
fn usage_by(
    db: &Database,
    group_sql: &str,
    since_ms: i64,
    day: Option<&str>,
) -> Result<Vec<UsageBucket>> {
    let sql = format!(
        "SELECT {group_sql} AS label,
                COALESCE(SUM(u.input_tokens), 0)       AS input,
                COALESCE(SUM(u.output_tokens), 0)      AS output,
                COALESCE(SUM(u.cache_read_tokens), 0)  AS cache_read,
                COALESCE(SUM(u.cache_write_tokens), 0) AS cache_write,
                COUNT(DISTINCT u.session_id)           AS sessions,
                GROUP_CONCAT(DISTINCT u.confidence)    AS confidences
           FROM usage_samples u
           LEFT JOIN projects p ON p.id = u.project_id
          WHERE u.ts_ms >= ?1 AND (?2 IS NULL OR {LOCAL_DAY} = ?2)
          GROUP BY label
          ORDER BY (input + output) DESC"
    );

    db.with(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows: rusqlite::Result<Vec<_>> = stmt
            .query_map(params![since_ms, day], |row| {
                let confidences: Option<String> = row.get("confidences")?;
                let confidence = confidences
                    .unwrap_or_default()
                    .split(',')
                    .fold("official".to_string(), |acc, c| weakest(&acc, c.trim()));

                Ok(UsageBucket {
                    label: row
                        .get::<_, Option<String>>("label")?
                        .unwrap_or_else(|| "—".into()),
                    input: row.get("input")?,
                    output: row.get("output")?,
                    cache_read: row.get("cache_read")?,
                    cache_write: row.get("cache_write")?,
                    sessions: row.get("sessions")?,
                    confidence,
                })
            })?
            .collect();
        rows
    })
}

fn leverage(db: &Database, since_ms: i64, day: Option<&str>) -> Result<Leverage> {
    db.with(|conn| {
        let human_active_minutes: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT minute) FROM interaction_minutes
              WHERE minute * 60000 >= ?1
                AND (?2 IS NULL
                     OR date(minute * 60, 'unixepoch', 'localtime') = ?2)",
            params![since_ms, day],
            |row| row.get(0),
        )?;

        // A session still running counts up to now, so the figure is live
        // rather than only updating when something ends.
        let agent_runtime_ms: i64 = conn.query_row(
            "SELECT COALESCE(SUM(COALESCE(ended_at, ?2) - created_at), 0)
               FROM sessions
              WHERE created_at >= ?1 AND provider != 'shell'
                AND (?3 IS NULL
                     OR date(created_at / 1000, 'unixepoch', 'localtime') = ?3)",
            params![since_ms, now_ms(), day],
            |row| row.get(0),
        )?;

        let sessions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions
              WHERE created_at >= ?1
                AND (?2 IS NULL
                     OR date(created_at / 1000, 'unixepoch', 'localtime') = ?2)",
            params![since_ms, day],
            |row| row.get(0),
        )?;

        // The earliest moment either half of this ratio could have been seen.
        let observed_from: Option<String> = conn
            .query_row(
                "SELECT MIN(d) FROM (
                     SELECT date(MIN(created_at) / 1000, 'unixepoch', 'localtime') d FROM sessions
                     UNION ALL
                     SELECT date(MIN(minute) * 60, 'unixepoch', 'localtime') d
                       FROM interaction_minutes)",
                [],
                |row| row.get(0),
            )
            .unwrap_or(None);

        Ok(Leverage {
            human_active_minutes,
            agent_runtime_minutes: agent_runtime_ms / 60_000,
            sessions,
            observed_from,
        })
    })
}

/// Days that carried at least one turn, as `YYYY-MM-DD` in local time.
///
/// Read over **all** history rather than the window, because the streak the
/// person is on does not begin when the filter does. A 7-day view that reported
/// "current streak: 7" for someone twenty days in would be arithmetic dressed
/// as a fact.
fn active_days(db: &Database) -> Result<Vec<String>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT {LOCAL_DAY} AS d, COUNT(*) AS turns
               FROM usage_samples u
              GROUP BY d HAVING turns >= {ACTIVE_DAY_MIN_TURNS}
              ORDER BY d"
        ))?;
        let rows: rusqlite::Result<Vec<String>> =
            stmt.query_map([], |row| row.get(0))?.collect();
        Ok(rows?)
    })
}

/// Days since the epoch, for a `YYYY-MM-DD`. Used only to measure adjacency.
///
/// Deliberately arithmetic on the civil date rather than on a timestamp: the
/// question "were these two days next to each other" must not change answer
/// because one of them contained a daylight-saving transition.
fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    // Howard Hinnant's days-from-civil, which is exact for the proleptic
    // Gregorian calendar and needs no date library.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn day_string(day_number: i64) -> String {
    // Inverse of the above, same source.
    let z = day_number + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Today, in the same local calendar the days are bucketed in.
fn today(db: &Database) -> Result<String> {
    db.with(|conn| {
        conn.query_row("SELECT date('now', 'localtime')", [], |row| row.get(0))
    })
}

fn streaks(db: &Database, window_days: i64) -> Result<Streaks> {
    let days = active_days(db)?;
    let numbers: Vec<i64> = days.iter().filter_map(|d| day_number(d)).collect();

    let mut longest = 0i64;
    let mut longest_end_index = 0usize;
    let mut run = 0i64;
    for (index, number) in numbers.iter().enumerate() {
        run = if index > 0 && number - numbers[index - 1] == 1 {
            run + 1
        } else {
            1
        };
        if run > longest {
            longest = run;
            longest_end_index = index;
        }
    }

    // The current run counts back from today, and accepts yesterday as its end.
    //
    // Without that grace every streak reads as broken between midnight and the
    // first turn of the morning — the product announcing a loss for the crime
    // of being early in the day. It is the one place a streak is allowed an
    // opinion, and the opinion is generous.
    let today_number = day_number(&today(db)?).unwrap_or(0);
    let current = match numbers.last() {
        Some(&last) if today_number - last <= 1 => {
            let mut count = 1i64;
            for pair in numbers.windows(2).rev() {
                if pair[1] - pair[0] == 1 {
                    count += 1;
                } else {
                    break;
                }
            }
            count
        }
        _ => 0,
    };

    let first_day = today_number - window_days + 1;
    let active_in_window = numbers.iter().filter(|n| **n >= first_day).count() as i64;

    Ok(Streaks {
        current,
        longest,
        longest_from: (longest > 0)
            .then(|| day_string(numbers[longest_end_index] - longest + 1)),
        longest_to: (longest > 0).then(|| days[longest_end_index].clone()),
        active_days: active_in_window,
        window_days,
    })
}

/// Every day in the window, zero-filled, oldest first.
fn calendar(db: &Database, window_days: i64, since_ms: i64) -> Result<Vec<DayCell>> {
    let measured: Vec<DayCell> = db.with(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT {LOCAL_DAY} AS d,
                    COALESCE(SUM(u.input_tokens + u.output_tokens + u.cache_write_tokens), 0),
                    COUNT(*),
                    COUNT(DISTINCT strftime('%H', u.ts_ms / 1000, 'unixepoch', 'localtime')),
                    COUNT(DISTINCT COALESCE(p.name, u.project_label))
               FROM usage_samples u
               LEFT JOIN projects p ON p.id = u.project_id
              WHERE u.ts_ms >= ?1
              GROUP BY d ORDER BY d"
        ))?;
        let rows: rusqlite::Result<Vec<DayCell>> = stmt
            .query_map([since_ms], |row| {
                Ok(DayCell {
                    date: row.get(0)?,
                    tokens: row.get(1)?,
                    turns: row.get(2)?,
                    hours: row.get(3)?,
                    projects: row.get(4)?,
                })
            })?
            .collect();
        Ok(rows?)
    })?;

    let today_number = day_number(&today(db)?).unwrap_or(0);
    let first = today_number - window_days + 1;
    let found: std::collections::HashMap<String, DayCell> = measured
        .into_iter()
        .map(|cell| (cell.date.clone(), cell))
        .collect();

    Ok((first..=today_number)
        .map(|number| {
            let date = day_string(number);
            found.get(&date).cloned().unwrap_or(DayCell {
                date,
                tokens: 0,
                turns: 0,
                hours: 0,
                projects: 0,
            })
        })
        .collect())
}

/// Tokens and turns by hour of the local day, zero-filled across all 24.
fn by_hour(db: &Database, since_ms: i64, day: Option<&str>) -> Result<Vec<HourBucket>> {
    let mut buckets: Vec<HourBucket> = (0..24)
        .map(|hour| HourBucket {
            hour,
            tokens: 0,
            turns: 0,
        })
        .collect();

    db.with(|conn| {
        let mut stmt = conn.prepare(&format!(
            "SELECT CAST(strftime('%H', u.ts_ms / 1000, 'unixepoch', 'localtime') AS INTEGER) AS h,
                    COALESCE(SUM(u.input_tokens + u.output_tokens + u.cache_write_tokens), 0),
                    COUNT(*)
               FROM usage_samples u
              WHERE u.ts_ms >= ?1 AND (?2 IS NULL OR {LOCAL_DAY} = ?2)
              GROUP BY h"
        ))?;
        let rows = stmt.query_map(params![since_ms, day], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?))
        })?;
        for row in rows.flatten() {
            if let Some(bucket) = buckets.get_mut(row.0 as usize) {
                bucket.tokens = row.1;
                bucket.turns = row.2;
            }
        }
        Ok(())
    })?;

    Ok(buckets)
}

fn history_from(db: &Database) -> Result<Option<String>> {
    db.with(|conn| {
        Ok(conn
            .query_row(
                &format!("SELECT MIN({LOCAL_DAY}) FROM usage_samples u"),
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .unwrap_or(None))
    })
}

pub fn report(db: &Database, window_days: i64, day: Option<&str>) -> Result<AnalyticsReport> {
    let window_days = window_days.clamp(1, 365);
    // A selected day is scoped by the day expression itself, not by moving the
    // window: the day may sit anywhere inside it, and narrowing `since_ms` to
    // "the last 24 hours" would answer a different question.
    let since_ms = if day.is_some() {
        0
    } else {
        now_ms() - window_days * 86_400_000
    };

    let files_changed: i64 = db.with(|conn| {
        conn.query_row(
            "SELECT COUNT(DISTINCT path) FROM file_changes
              WHERE ts_ms >= ?1
                AND (?2 IS NULL
                     OR date(ts_ms / 1000, 'unixepoch', 'localtime') = ?2)",
            params![since_ms, day],
            |row| row.get(0),
        )
    })?;

    Ok(AnalyticsReport {
        by_provider: usage_by(db, "u.provider", since_ms, day)?,
        by_model: usage_by(db, "COALESCE(u.model, '—')", since_ms, day)?,
        // History recovered from disk has no `projects` row and never will —
        // see `backfill`. Its label travels on the sample instead, so twenty
        // days of work across thirty-five folders is attributed rather than
        // collapsed into one nameless heap.
        by_project: usage_by(db, "COALESCE(p.name, u.project_label, '—')", since_ms, day)?,
        by_day: usage_by(db, LOCAL_DAY, since_ms, day)?,
        // The calendar always spans the whole window, selected day or not.
        calendar: calendar(db, window_days, now_ms() - window_days * 86_400_000)?,
        streaks: streaks(db, window_days)?,
        by_hour: by_hour(db, since_ms, day)?,
        leverage: leverage(db, since_ms, day)?,
        files_changed,
        window_days,
        day: day.map(str::to_string),
        history_from: history_from(db)?,
    })
}

#[tauri::command]
pub fn analytics_report(
    state: State<'_, AppState>,
    window_days: Option<i64>,
    day: Option<String>,
) -> std::result::Result<AnalyticsReport, String> {
    report(&state.db, window_days.unwrap_or(30), day.as_deref()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'Demo', 'C:\\demo', 0, 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at, ended_at)
                 VALUES ('s1', 'p1', 'claude-code', 'C:\\demo', 'completed', 'l', ?1, ?2)",
                params![now_ms() - 600_000, now_ms()],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn sample(db: &Database, provider: &str, model: &str, input: i64, output: i64, confidence: &str) {
        db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_samples
                     (session_id, project_id, provider, model, ts_ms,
                      input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, confidence)
                 VALUES ('s1', 'p1', ?1, ?2, ?3, ?4, ?5, 0, 0, ?6)",
                params![provider, model, now_ms(), input, output, confidence],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn aggregates_tokens_by_provider_and_model() {
        let db = db();
        sample(&db, "claude-code", "claude-opus-5", 100, 20, "official");
        sample(&db, "claude-code", "claude-opus-5", 50, 10, "official");
        sample(&db, "codex", "gpt-5", 30, 5, "official");

        let report = report(&db, 30, None).unwrap();

        let claude = report
            .by_provider
            .iter()
            .find(|b| b.label == "claude-code")
            .expect("claude bucket");
        assert_eq!(claude.input, 150);
        assert_eq!(claude.output, 30);
        assert_eq!(claude.sessions, 1);

        assert_eq!(report.by_model.len(), 2);
        assert_eq!(report.by_project[0].label, "Demo");
    }

    /// §28 — a total mixing reported and derived numbers is only as trustworthy
    /// as its weakest part, so the bucket reports that.
    #[test]
    fn a_bucket_reports_its_weakest_confidence() {
        let db = db();
        sample(&db, "claude-code", "m", 100, 10, "official");
        sample(&db, "claude-code", "m", 100, 10, "estimated");

        let report = report(&db, 30, None).unwrap();
        let bucket = &report.by_provider[0];
        assert_eq!(
            bucket.confidence, "estimated",
            "mixing an estimate into a total must downgrade the whole total"
        );
    }

    #[test]
    fn confidence_ordering_is_official_to_unknown() {
        assert_eq!(weakest("official", "observed"), "observed");
        assert_eq!(weakest("observed", "estimated"), "estimated");
        assert_eq!(weakest("estimated", "unknown"), "unknown");
        assert_eq!(weakest("official", "official"), "official");
    }

    /// §53 — the distinctive figure: work that happened while nobody watched.
    #[test]
    fn human_attention_is_counted_in_minutes_actually_worked() {
        let db = db();
        let minute = now_ms() / 60_000;

        db.with(|conn| {
            // Three keystroke bursts inside the same minute, plus one later.
            for m in [minute, minute, minute, minute - 5] {
                conn.execute(
                    "INSERT OR IGNORE INTO interaction_minutes (session_id, project_id, minute)
                     VALUES ('s1', 'p1', ?1)",
                    [m],
                )?;
            }
            Ok(())
        })
        .unwrap();

        let report = report(&db, 30, None).unwrap();
        assert_eq!(
            report.leverage.human_active_minutes, 2,
            "a burst of typing within one minute is one minute of attention"
        );
        // The session ran for ten minutes with two minutes of human attention.
        assert_eq!(report.leverage.agent_runtime_minutes, 10);
    }

    #[test]
    fn an_empty_database_reports_zeroes_rather_than_failing() {
        let db = Database::open_in_memory().unwrap();
        let report = report(&db, 30, None).unwrap();
        assert!(report.by_provider.is_empty());
        assert_eq!(report.leverage.human_active_minutes, 0);
        assert_eq!(report.files_changed, 0);
    }

    #[test]
    fn the_window_bounds_what_is_counted() {
        let db = db();
        // A sample from well outside the window.
        db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_samples
                     (session_id, project_id, provider, model, ts_ms, input_tokens, output_tokens, confidence)
                 VALUES ('s1', 'p1', 'claude-code', 'm', ?1, 999999, 999999, 'official')",
                params![now_ms() - 90 * 86_400_000i64],
            )?;
            Ok(())
        })
        .unwrap();
        sample(&db, "claude-code", "m", 10, 1, "official");

        let recent = report(&db, 7, None).unwrap();
        assert_eq!(recent.by_provider[0].input, 10, "old samples must not leak in");

        let wide = report(&db, 365, None).unwrap();
        assert_eq!(wide.by_provider[0].input, 1_000_009);
    }

    // -----------------------------------------------------------------------
    // Calendar, streaks and the day filter (M22)
    // -----------------------------------------------------------------------

    /// Civil-date arithmetic, which every streak answer rests on.
    #[test]
    fn day_numbers_round_trip_and_measure_adjacency() {
        for date in ["2026-08-04", "2026-01-01", "2026-12-31", "2024-02-29"] {
            assert_eq!(day_string(day_number(date).unwrap()), date, "{date}");
        }
        // Adjacent across a month boundary, and across a leap day.
        assert_eq!(
            day_number("2026-09-01").unwrap() - day_number("2026-08-31").unwrap(),
            1
        );
        assert_eq!(
            day_number("2024-03-01").unwrap() - day_number("2024-02-29").unwrap(),
            1
        );
        assert_eq!(day_number("not-a-date"), None);
    }

    /// Put a turn on a specific local day, `back` days before today.
    fn turn_on(db: &Database, days_back: i64, tokens: i64) {
        // Midday local, so the row cannot drift across a boundary whatever the
        // machine's offset is — the bug this whole local-time change exists to
        // fix would otherwise reappear inside its own test.
        // The `'utc'` modifier is what makes this *local* midday: without it
        // SQLite reads a bare datetime string as UTC, and on this machine the
        // row would land at 09:00 local — the test would then be asserting the
        // very off-by-a-timezone this change exists to remove.
        let ts: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT unixepoch(date('now','localtime',?1) || ' 12:00:00', 'utc') * 1000",
                    [format!("-{days_back} days")],
                    |row| row.get(0),
                )
            })
            .unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_samples
                     (session_id, project_id, provider, model, ts_ms,
                      input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                      confidence)
                 VALUES ('s1','p1','claude-code','m',?1,?2,0,0,0,'official')",
                params![ts, tokens],
            )?;
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn a_streak_counts_consecutive_days_and_survives_a_quiet_today() {
        let db = db();
        // Yesterday and the two days before it: a run of three that has not
        // been continued yet today.
        for back in [1, 2, 3] {
            turn_on(&db, back, 10);
        }

        let report = report(&db, 30, None).unwrap();
        assert_eq!(
            report.streaks.current, 3,
            "a run ending yesterday is still the run you are on — a streak that \
             reports itself broken every midnight is telling the person off for \
             the crime of being early in the day"
        );
        assert_eq!(report.streaks.longest, 3);
        assert_eq!(report.streaks.active_days, 3);
    }

    #[test]
    fn a_gap_ends_the_current_streak_but_not_the_longest() {
        let db = db();
        // A long run well in the past, a gap, then a short recent one.
        for back in [10, 11, 12, 13, 14] {
            turn_on(&db, back, 10);
        }
        for back in [1, 2] {
            turn_on(&db, back, 10);
        }

        let report = report(&db, 30, None).unwrap();
        assert_eq!(report.streaks.current, 2, "the gap ended the older run");
        assert_eq!(report.streaks.longest, 5);
        assert!(report.streaks.longest_from.is_some());
        assert!(report.streaks.longest_to.is_some());
        assert_eq!(report.streaks.active_days, 7);

        // A day off is a day off. Nothing here reports it as a loss.
        assert_eq!(report.streaks.window_days, 30);
    }

    #[test]
    fn a_streak_broken_before_yesterday_is_zero_not_stale() {
        let db = db();
        for back in [5, 6, 7] {
            turn_on(&db, back, 10);
        }
        let report = report(&db, 30, None).unwrap();
        assert_eq!(
            report.streaks.current, 0,
            "the run ended five days ago; reporting it as current would be the \
             screen flattering the person with a number that stopped being true"
        );
        assert_eq!(report.streaks.longest, 3);
    }

    #[test]
    fn the_calendar_is_zero_filled_across_the_whole_window() {
        let db = db();
        turn_on(&db, 3, 100);

        let report = report(&db, 14, None).unwrap();
        assert_eq!(
            report.calendar.len(),
            14,
            "a calendar with the quiet days missing is not a calendar"
        );
        assert!(
            report.calendar.windows(2).all(|w| w[0].date < w[1].date),
            "oldest first, so it reads left to right like a calendar"
        );
        let busy: Vec<_> = report.calendar.iter().filter(|c| c.turns > 0).collect();
        assert_eq!(busy.len(), 1);
        assert_eq!(busy[0].tokens, 100);
        assert_eq!(busy[0].hours, 1);
    }

    #[test]
    fn selecting_a_day_scopes_the_figures_but_never_the_calendar() {
        let db = db();
        turn_on(&db, 1, 100);
        turn_on(&db, 2, 500);

        let all = report(&db, 30, None).unwrap();
        assert_eq!(all.by_provider[0].input, 600);

        let yesterday = all.calendar.iter().rev().find(|c| c.turns > 0).unwrap();
        let scoped = report(&db, 30, Some(&yesterday.date)).unwrap();

        assert_eq!(
            scoped.by_provider[0].input, 100,
            "the figures follow the selected day"
        );
        assert_eq!(
            scoped.calendar.len(),
            30,
            "the calendar is both the picture and the control — one that erased \
             itself when used would leave no way back"
        );
        assert_eq!(scoped.day.as_deref(), Some(yesterday.date.as_str()));
        // The streak is a fact about the person, not about the filter.
        assert_eq!(scoped.streaks.current, all.streaks.current);
    }

    #[test]
    fn the_hour_histogram_covers_all_twenty_four_hours() {
        let db = db();
        turn_on(&db, 1, 42);

        let report = report(&db, 30, None).unwrap();
        assert_eq!(report.by_hour.len(), 24);
        assert!(report.by_hour.iter().enumerate().all(|(i, b)| b.hour == i as i64));
        assert_eq!(report.by_hour.iter().map(|b| b.tokens).sum::<i64>(), 42);
        assert_eq!(report.by_hour[12].turns, 1, "recorded at midday local");
    }

    /// Recovered history has no project row; its label must still be used.
    #[test]
    fn history_recovered_from_disk_is_attributed_by_its_own_label() {
        let db = db();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_samples
                     (session_id, project_id, provider, model, ts_ms,
                      input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                      confidence, origin_uuid, project_label)
                 VALUES (NULL, NULL, 'claude-code', 'm', ?1, 7, 0, 0, 0,
                         'official', 'u-1', 'estoca')",
                params![now_ms() - 3_600_000],
            )?;
            Ok(())
        })
        .unwrap();

        let report = report(&db, 30, None).unwrap();
        assert!(
            report.by_project.iter().any(|b| b.label == "estoca"),
            "twenty days across thirty-five folders must not collapse into one \
             nameless heap just because those folders were never opened here"
        );
        assert!(report.history_from.is_some());
    }

    /// The leverage ratio must say how far back it could actually see.
    #[test]
    fn leverage_reports_the_span_it_was_measured_over() {
        let db = db();
        let report = report(&db, 30, None).unwrap();
        assert!(
            report.leverage.observed_from.is_some(),
            "tokens reach back as far as the transcripts do, attention only as \
             far as this product was running — a 30-day ratio computed from two \
             days of attention is the most flattering number on the screen and \
             the least true"
        );
    }
}
