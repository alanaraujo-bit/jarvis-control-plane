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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsReport {
    pub by_provider: Vec<UsageBucket>,
    pub by_model: Vec<UsageBucket>,
    pub by_project: Vec<UsageBucket>,
    pub by_day: Vec<UsageBucket>,
    pub leverage: Leverage,
    /// Files touched by agent sessions in the window.
    pub files_changed: i64,
    /// The window these figures cover, in days.
    pub window_days: i64,
}

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
fn usage_by(db: &Database, group_sql: &str, since_ms: i64) -> Result<Vec<UsageBucket>> {
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
          WHERE u.ts_ms >= ?1
          GROUP BY label
          ORDER BY (input + output) DESC"
    );

    db.with(|conn| {
        let mut stmt = conn.prepare(&sql)?;
        let rows: rusqlite::Result<Vec<_>> = stmt
            .query_map([since_ms], |row| {
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

fn leverage(db: &Database, since_ms: i64) -> Result<Leverage> {
    db.with(|conn| {
        let human_active_minutes: i64 = conn.query_row(
            "SELECT COUNT(DISTINCT minute) FROM interaction_minutes WHERE minute * 60000 >= ?1",
            [since_ms],
            |row| row.get(0),
        )?;

        // A session still running counts up to now, so the figure is live
        // rather than only updating when something ends.
        let agent_runtime_ms: i64 = conn.query_row(
            "SELECT COALESCE(SUM(COALESCE(ended_at, ?2) - created_at), 0)
               FROM sessions
              WHERE created_at >= ?1 AND provider != 'shell'",
            params![since_ms, now_ms()],
            |row| row.get(0),
        )?;

        let sessions: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE created_at >= ?1",
            [since_ms],
            |row| row.get(0),
        )?;

        Ok(Leverage {
            human_active_minutes,
            agent_runtime_minutes: agent_runtime_ms / 60_000,
            sessions,
        })
    })
}

pub fn report(db: &Database, window_days: i64) -> Result<AnalyticsReport> {
    let window_days = window_days.clamp(1, 365);
    let since_ms = now_ms() - window_days * 86_400_000;

    let files_changed: i64 = db.with(|conn| {
        conn.query_row(
            "SELECT COUNT(DISTINCT path) FROM file_changes WHERE ts_ms >= ?1",
            [since_ms],
            |row| row.get(0),
        )
    })?;

    Ok(AnalyticsReport {
        by_provider: usage_by(db, "u.provider", since_ms)?,
        by_model: usage_by(db, "COALESCE(u.model, '—')", since_ms)?,
        by_project: usage_by(db, "COALESCE(p.name, '—')", since_ms)?,
        // SQLite stores epoch millis; this bucket is the calendar day in UTC.
        by_day: usage_by(db, "date(u.ts_ms / 1000, 'unixepoch')", since_ms)?,
        leverage: leverage(db, since_ms)?,
        files_changed,
        window_days,
    })
}

#[tauri::command]
pub fn analytics_report(
    state: State<'_, AppState>,
    window_days: Option<i64>,
) -> std::result::Result<AnalyticsReport, String> {
    report(&state.db, window_days.unwrap_or(30)).map_err(|e| e.to_string())
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

        let report = report(&db, 30).unwrap();

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

        let report = report(&db, 30).unwrap();
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

        let report = report(&db, 30).unwrap();
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
        let report = report(&db, 30).unwrap();
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

        let recent = report(&db, 7).unwrap();
        assert_eq!(recent.by_provider[0].input, 10, "old samples must not leak in");

        let wide = report(&db, 365).unwrap();
        assert_eq!(wide.by_provider[0].input, 1_000_009);
    }
}
