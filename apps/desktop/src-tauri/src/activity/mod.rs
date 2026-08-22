//! Activity log (§48).
//!
//! A universal record of things worth knowing happened: an agent started, a
//! mission was blocked, a verification failed, a quota threshold was crossed.
//!
//! The bar for writing a row is the same as the bar for a notification (§49):
//! **would a person want to know this?** A log that records every keystroke is
//! a log nobody reads, so ordinary chatter — output arriving, files being read
//! — stays in the session stream where it belongs.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;
use crate::session::log::now_ms;
use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Something happened. No action implied.
    Info,
    /// Worth noticing, but nothing is stuck.
    Warning,
    /// Nothing moves until a person acts.
    Attention,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Attention => "attention",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "warning" => Self::Warning,
            "attention" => Self::Attention,
            _ => Self::Info,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    pub id: i64,
    pub ts_ms: i64,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub mission_id: Option<String>,
    /// A stable identifier the UI localises, never prose (§65).
    pub kind: String,
    pub severity: Severity,
    /// Human-readable subject: a mission title, a provider name.
    pub title: String,
    pub detail: Option<String>,
}

/// Record something worth knowing.
///
/// Failures are logged and swallowed: activity is a record *about* the work,
/// and losing a row must never take down the work itself.
pub fn record(
    db: &Database,
    kind: &str,
    severity: Severity,
    title: &str,
    detail: Option<String>,
    project_id: Option<&str>,
    session_id: Option<&str>,
    mission_id: Option<&str>,
) {
    let result = db.with(|conn| {
        conn.execute(
            "INSERT INTO activity
                 (ts_ms, project_id, session_id, mission_id, kind, severity, title, detail)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                now_ms(),
                project_id,
                session_id,
                mission_id,
                kind,
                severity.as_str(),
                title,
                detail
            ],
        )?;
        Ok(())
    });

    if let Err(e) = result {
        tracing::warn!(error = %e, kind, "could not record activity");
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ActivityFilter {
    pub project_id: Option<String>,
    pub mission_id: Option<String>,
    pub severity: Option<Severity>,
    pub limit: Option<u32>,
}

pub fn list(db: &Database, filter: &ActivityFilter) -> crate::db::Result<Vec<ActivityEntry>> {
    let limit = filter.limit.unwrap_or(200).min(1000) as i64;

    db.with(|conn| {
        // Each filter is optional; a NULL parameter disables its clause, which
        // keeps this one prepared statement instead of assembling SQL by hand.
        let mut stmt = conn.prepare(
            "SELECT * FROM activity
              WHERE (?1 IS NULL OR project_id = ?1)
                AND (?2 IS NULL OR mission_id = ?2)
                AND (?3 IS NULL OR severity = ?3)
              ORDER BY ts_ms DESC
              LIMIT ?4",
        )?;
        let rows: rusqlite::Result<Vec<_>> = stmt
            .query_map(
                params![
                    filter.project_id,
                    filter.mission_id,
                    filter.severity.map(|s| s.as_str()),
                    limit
                ],
                |row| {
                    Ok(ActivityEntry {
                        id: row.get("id")?,
                        ts_ms: row.get("ts_ms")?,
                        project_id: row.get("project_id")?,
                        session_id: row.get("session_id")?,
                        mission_id: row.get("mission_id")?,
                        kind: row.get("kind")?,
                        severity: Severity::parse(&row.get::<_, String>("severity")?),
                        title: row.get("title")?,
                        detail: row.get("detail")?,
                    })
                },
            )?
            .collect();
        rows
    })
}

#[tauri::command]
pub fn list_activity(
    state: State<'_, AppState>,
    filter: Option<ActivityFilter>,
) -> Result<Vec<ActivityEntry>, String> {
    list(&state.db, &filter.unwrap_or_default()).map_err(|e| e.to_string())
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
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn records_and_reads_back_an_entry() {
        let db = db();
        record(
            &db,
            "mission.blocked",
            Severity::Attention,
            "Ship the thing",
            Some("Needs production credentials".into()),
            Some("p1"),
            None,
            Some("m1"),
        );

        let entries = list(&db, &ActivityFilter::default()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, "mission.blocked");
        assert_eq!(entries[0].severity, Severity::Attention);
        assert_eq!(entries[0].mission_id.as_deref(), Some("m1"));
        assert!(entries[0].ts_ms > 1_500_000_000_000);
    }

    #[test]
    fn filters_by_project_and_severity() {
        let db = db();
        record(&db, "a", Severity::Info, "one", None, Some("p1"), None, None);
        record(&db, "b", Severity::Attention, "two", None, Some("p1"), None, None);
        record(&db, "c", Severity::Info, "three", None, None, None, None);

        let all = list(&db, &ActivityFilter::default()).unwrap();
        assert_eq!(all.len(), 3);

        let project = list(
            &db,
            &ActivityFilter { project_id: Some("p1".into()), ..Default::default() },
        )
        .unwrap();
        assert_eq!(project.len(), 2);

        let urgent = list(
            &db,
            &ActivityFilter { severity: Some(Severity::Attention), ..Default::default() },
        )
        .unwrap();
        assert_eq!(urgent.len(), 1);
        assert_eq!(urgent[0].title, "two");
    }

    #[test]
    fn returns_newest_first_and_respects_the_limit() {
        let db = db();
        for i in 0..5 {
            record(&db, "k", Severity::Info, &format!("entry {i}"), None, None, None, None);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let entries = list(&db, &ActivityFilter { limit: Some(3), ..Default::default() }).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].title, "entry 4");
        assert!(entries[0].ts_ms >= entries[1].ts_ms);
    }

    /// Activity is a record about the work; it must never be able to stop it.
    #[test]
    fn a_failed_write_does_not_panic() {
        let db = db();
        // A session id that does not exist violates the foreign key.
        record(
            &db,
            "session.started",
            Severity::Info,
            "ghost",
            None,
            None,
            Some("no-such-session"),
            None,
        );
        // The call returned normally and nothing was recorded.
        assert!(list(&db, &ActivityFilter::default()).unwrap().is_empty());
    }
}
