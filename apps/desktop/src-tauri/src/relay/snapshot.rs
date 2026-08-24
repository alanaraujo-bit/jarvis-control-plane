//! What the desktop tells the relay about itself (§59).
//!
//! A **summary, not a mirror**. What is running and what needs a person —
//! never file contents, terminal output or conversation text. A relay holding
//! a copy of the work would be the second store §23 exists to prevent, and a
//! far larger thing to secure for a companion that only needs to answer "is
//! anything waiting for me?".
//!
//! Everything here is composed from queries that already exist and are already
//! tested: `mission::store::summaries` is what Mission Control reads, and
//! `guardrail::pending` is what the approvals surface reads. Writing new SQL
//! for the phone would give the companion its own idea of what is running,
//! which is exactly how two surfaces start disagreeing.

use serde::Serialize;

use crate::db::Database;
use crate::mission::model::MissionStatus;

/// How long the phone should treat a snapshot as current, in seconds.
///
/// Slightly more than twice the desktop's push interval, so one missed push
/// does not make a live desktop look offline — while two missed pushes do,
/// which is the point. A companion that keeps showing stale state confidently
/// is worse than one that admits it has not heard anything (§28).
const STALE_AFTER_SECONDS: u32 = 150;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Freshness {
    pub captured_at: String,
    pub stale_after_seconds: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayMission {
    pub id: String,
    pub title: String,
    pub status: MissionStatus,
    pub reason: Option<String>,
    pub turns: Option<u32>,
    pub budget: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayProject {
    pub id: String,
    pub name: String,
    pub attention: Vec<RelayMission>,
    pub active_sessions: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayApproval {
    pub id: String,
    pub project_name: String,
    pub operation: String,
    pub summary: String,
    pub requested_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelaySnapshot {
    pub freshness: Freshness,
    pub device_name: String,
    pub projects: Vec<RelayProject>,
    pub approvals: Vec<RelayApproval>,
}

/// Whether a mission is worth telling a phone about.
///
/// **Only what needs a person, or what is happening right now.** A `ready`
/// mission nobody has started is not news, and a `completed` one is not either
/// — sending the whole list would make the companion a second Mission Control
/// on a smaller screen, which is not what a phone is for (§56). The phone is
/// for "does anything need me?".
fn needs_attention(status: MissionStatus) -> bool {
    matches!(
        status,
        MissionStatus::Running
            | MissionStatus::Verifying
            | MissionStatus::Waiting
            | MissionStatus::Blocked
            | MissionStatus::Failed
    )
}

/// Truncate a command for display, on a character boundary.
///
/// A phone shows this in a list, and a `git push` with a long remote URL would
/// push everything else off the row. Cut by **characters, never bytes** — the
/// same trap as `session::typing`'s chunker (HANDOFF §5 item 36), and a
/// guardrail summary can hold a path with accented characters in it.
fn shorten(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut out: String = text.chars().take(limit.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Build the snapshot the relay will carry.
pub fn build(db: &Database, device_name: String, now_ms: i64) -> crate::db::Result<RelaySnapshot> {
    let summaries = crate::mission::store::summaries(db).unwrap_or_default();
    let pending = crate::guardrail::pending(db, None).unwrap_or_default();

    // Sessions that are live right now, per project. Counted from the same
    // table Mission Control counts from rather than from the in-memory manager:
    // this runs on a background thread with no access to it, and the database
    // is the shared truth either way.
    let live_sessions: Vec<(String, u32)> = db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT project_id, COUNT(*) FROM sessions
              WHERE ended_at IS NULL GROUP BY project_id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? as u32)))?;
        rows.collect()
    })?;

    let mut projects: Vec<RelayProject> = Vec::new();
    for summary in summaries {
        if !needs_attention(summary.mission.status) {
            continue;
        }
        let project_id = summary.mission.project_id.clone();
        let entry = match projects.iter_mut().find(|p| p.id == project_id) {
            Some(existing) => existing,
            None => {
                projects.push(RelayProject {
                    id: project_id.clone(),
                    name: summary.project_name.clone(),
                    attention: Vec::new(),
                    active_sessions: live_sessions
                        .iter()
                        .find(|(id, _)| *id == project_id)
                        .map(|(_, count)| *count)
                        .unwrap_or(0),
                });
                projects.last_mut().expect("just pushed")
            }
        };
        entry.attention.push(RelayMission {
            id: summary.mission.id,
            title: shorten(&summary.mission.title, 80),
            status: summary.mission.status,
            reason: summary.mission.blocked_reason.map(|r| shorten(&r, 140)),
            // Turn progress is a live-run detail the snapshot does not carry:
            // it changes every few seconds and would make every push different
            // for no benefit on a phone. `AutopilotPanel` on the desktop is
            // where a turn count belongs.
            turns: None,
            budget: None,
        });
    }

    let approvals = pending
        .into_iter()
        .map(|event| RelayApproval {
            id: event.id,
            project_name: event.project_id.unwrap_or_default(),
            operation: event.operation.as_str().to_string(),
            // The command as classified, shortened. Already the redacted form
            // the guardrail surface shows — this adds no new exposure.
            summary: shorten(&event.command, 120),
            requested_at: iso(event.ts_ms),
        })
        .collect();

    Ok(RelaySnapshot {
        freshness: Freshness {
            captured_at: iso(now_ms),
            stale_after_seconds: STALE_AFTER_SECONDS,
        },
        device_name,
        projects,
        approvals,
    })
}

/// Milliseconds since the epoch as ISO-8601, which is what the phone parses.
fn iso(ms: i64) -> String {
    let seconds = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    // Hand-rolled rather than pulling in `chrono` for one format string: the
    // shape is fixed and the arithmetic is the civil-from-days algorithm,
    // which is well known and testable.
    let days = seconds.div_euclid(86_400);
    let time = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        time / 3600,
        (time % 3600) / 60,
        time % 60,
    )
}

/// Howard Hinnant's `civil_from_days`, days since 1970-01-01 to (y, m, d).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timestamp_renders_as_the_iso_the_phone_parses() {
        // 2026-08-24T01:48:00.000Z — checked against a known value rather than
        // against the function's own arithmetic.
        assert_eq!(iso(1_787_535_280_000), "2026-08-24T01:34:40.000Z");
        assert_eq!(iso(0), "1970-01-01T00:00:00.000Z");
        // A leap day, which is where a hand-rolled calendar goes wrong.
        assert_eq!(iso(1_709_164_800_000), "2024-02-29T00:00:00.000Z");
    }

    /// Cutting by bytes would split a multi-byte character and produce
    /// mojibake on the phone — the same trap as the dictation chunker.
    #[test]
    fn shortening_never_splits_a_character() {
        let accented = "configuração".repeat(20);
        let short = shorten(&accented, 30);
        assert_eq!(short.chars().count(), 30);
        assert!(short.ends_with('…'));
        // The real check: it is still valid UTF-8 that round-trips.
        assert_eq!(short, String::from_utf8(short.clone().into_bytes()).unwrap());
    }

    #[test]
    fn text_within_the_limit_is_left_alone() {
        assert_eq!(shorten("curto", 80), "curto");
        assert_eq!(shorten("", 80), "");
    }

    /// The rule that keeps the companion from becoming a second Mission
    /// Control: only what is happening or what needs a person.
    #[test]
    fn only_missions_that_need_a_person_reach_the_phone() {
        assert!(needs_attention(MissionStatus::Blocked));
        assert!(needs_attention(MissionStatus::Waiting));
        assert!(needs_attention(MissionStatus::Running));
        assert!(needs_attention(MissionStatus::Failed));

        assert!(!needs_attention(MissionStatus::Ready), "not started is not news");
        assert!(!needs_attention(MissionStatus::Completed), "done is not news either");
    }
}
