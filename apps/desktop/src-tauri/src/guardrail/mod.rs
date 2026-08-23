//! Guardrails for sensitive operations (§35).
//!
//! ## What a guardrail actually is here
//!
//! A policy that a class of operation — force push, recursive delete, deploying
//! to production, reading a credential file — must be allowed, refused, or put
//! to a human, resolved per project and per operation.
//!
//! ## Where it can be enforced, honestly
//!
//! This is the part worth reading carefully, because the temptation is to
//! present one mechanism as if it covered everything. It does not. There are
//! three distinct situations and they are genuinely different in kind — the
//! same reason the provider capability model exists (§26).
//!
//! | Situation | What J.A.R.V.I.S. can do |
//! |---|---|
//! | A command **J.A.R.V.I.S. itself runs** — a mission's verification (§30) | Real enforcement. The command does not run. |
//! | A **Claude Code** agent session | Real enforcement. Claude Code calls a hook before running a tool and honours a refusal; see `guard`. |
//! | A **Codex** agent session | The same mechanism exists — 0.149.0 has `PreToolUse` hooks with the same wire shape — but Codex will not run a hook until the person has reviewed and trusted it in its own interface. So the file is written and waits; until then the session is observed, not guarded. |
//! | A **human typing in a terminal** | Nothing, deliberately. Guardrails govern agents. It is the user's machine. |
//!
//! `ProviderCapabilities::guardrails` reports which of these applies, so the UI
//! renders the truth instead of a promise (§26).
//!
//! ## The Unattended case is the one that matters
//!
//! Under Guided or Autonomous, "ask" can be answered: someone is looking at the
//! terminal and the provider's own prompt reaches them. Under Unattended (§32)
//! nobody is, and a prompt would wait forever — the indefinite resource
//! consumption §34 exists to forbid. So when a rule says *ask* and there is
//! nobody who can answer, the guard **refuses** and the mission goes to Waiting
//! with a reason. Stopping and saying why beats hanging quietly.
//!
//! "Nobody who can answer" is deliberately not the same as "nobody is looking".
//! A driven session usually *does* have its terminal open with a person reading
//! along, and that person is not the one the provider's prompt reaches — the
//! autopilot is in the seat. See `Snapshot::can_ask_a_person`.
//!
//! This is also why guardrails had to land before agents can drive missions
//! unattended, rather than after.

pub mod classify;
pub mod commands;
pub mod guard;
pub mod policy;
pub mod sessions;
pub mod surface;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::session::log::now_ms;

pub use classify::{classify, Match, Operation};
pub use policy::Decision;

/// Where a guardrail event came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// An agent's tool call, intercepted before it ran.
    Agent,
    /// A command J.A.R.V.I.S. was about to run to verify a mission (§30).
    Verification,
    /// An operation the user asked a surface to perform — staging a file,
    /// discarding a change, removing a worktree (§44/§45).
    ///
    /// Distinct from `Agent` because nothing was intercepted: the product was
    /// asked to do something destructive and asked itself first. Distinct from
    /// `Verification` because there is nothing to resume — the person is at the
    /// screen, so the question is answered there and then. See `surface`.
    Surface,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Agent => "agent",
            Self::Verification => "verification",
            Self::Surface => "surface",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "verification" => Self::Verification,
            "surface" => Self::Surface,
            _ => Self::Agent,
        }
    }
}

/// What became of a matched operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    /// Held, waiting for a person. Only verification can be held — an agent's
    /// tool call cannot be paused mid-flight and resumed later.
    Pending,
    Allowed,
    Denied,
    /// Put to the person at the terminal by the provider's own prompt. What
    /// they answered is theirs to know; we did not intercept it.
    Asked,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Allowed => "allowed",
            Self::Denied => "denied",
            Self::Asked => "asked",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "pending" => Self::Pending,
            "allowed" => Self::Allowed,
            "denied" => Self::Denied,
            _ => Self::Asked,
        }
    }
}

/// One occasion on which a guardrail had something to say.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardrailEvent {
    pub id: String,
    pub ts_ms: i64,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub mission_id: Option<String>,
    pub criterion_id: Option<String>,
    pub origin: Origin,
    pub operation: Operation,
    /// The text that matched, verbatim.
    pub fragment: String,
    pub command: String,
    pub status: Status,
    /// A stable code the UI localises (§65).
    pub reason: String,
    pub decided_at: Option<i64>,
    pub decided_by: Option<String>,
}

/// What to record about a match, before it is stored.
pub struct NewEvent<'a> {
    pub project_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub mission_id: Option<&'a str>,
    pub criterion_id: Option<&'a str>,
    pub origin: Origin,
    pub operation: Operation,
    pub fragment: &'a str,
    pub command: &'a str,
    pub status: Status,
    pub reason: &'a str,
}

/// Store a guardrail event and return its id.
pub fn record(db: &Database, event: NewEvent<'_>) -> crate::db::Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    let stored = id.clone();
    db.with(move |conn| {
        conn.execute(
            "INSERT INTO guardrail_events
                 (id, ts_ms, project_id, session_id, mission_id, criterion_id,
                  origin, operation, fragment, command, status, reason)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                stored,
                now_ms(),
                event.project_id,
                event.session_id,
                event.mission_id,
                event.criterion_id,
                event.origin.as_str(),
                event.operation.as_str(),
                event.fragment,
                // The command is kept whole up to a bound. A truncated command
                // that hides the interesting part would make the record useless
                // for exactly the reviews it exists to support.
                event.command.chars().take(4000).collect::<String>(),
                event.status.as_str(),
                event.reason,
            ],
        )?;
        Ok(())
    })?;
    Ok(id)
}

fn row_to_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<GuardrailEvent> {
    Ok(GuardrailEvent {
        id: row.get("id")?,
        ts_ms: row.get("ts_ms")?,
        project_id: row.get("project_id")?,
        session_id: row.get("session_id")?,
        mission_id: row.get("mission_id")?,
        criterion_id: row.get("criterion_id")?,
        origin: Origin::parse(&row.get::<_, String>("origin")?),
        // A row written by a newer build may name an operation this one does
        // not know. Falling back keeps the history readable rather than making
        // the whole query fail.
        operation: Operation::parse(&row.get::<_, String>("operation")?)
            .unwrap_or(Operation::GitForcePush),
        fragment: row.get("fragment")?,
        command: row.get("command")?,
        status: Status::parse(&row.get::<_, String>("status")?),
        reason: row.get("reason")?,
        decided_at: row.get("decided_at")?,
        decided_by: row.get("decided_by")?,
    })
}

/// Guardrail history, newest first.
pub fn list(
    db: &Database,
    project_id: Option<&str>,
    mission_id: Option<&str>,
    limit: u32,
) -> crate::db::Result<Vec<GuardrailEvent>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT * FROM guardrail_events
              WHERE (?1 IS NULL OR project_id = ?1)
                AND (?2 IS NULL OR mission_id = ?2)
              ORDER BY ts_ms DESC
              LIMIT ?3",
        )?;
        let rows: rusqlite::Result<Vec<_>> = stmt
            .query_map(params![project_id, mission_id, limit.min(500)], row_to_event)?
            .collect();
        rows
    })
}

/// Approvals still waiting for a person.
pub fn pending(db: &Database, mission_id: Option<&str>) -> crate::db::Result<Vec<GuardrailEvent>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT * FROM guardrail_events
              WHERE status = 'pending' AND (?1 IS NULL OR mission_id = ?1)
              ORDER BY ts_ms ASC",
        )?;
        let rows: rusqlite::Result<Vec<_>> =
            stmt.query_map(params![mission_id], row_to_event)?.collect();
        rows
    })
}

pub fn get(db: &Database, id: &str) -> crate::db::Result<Option<GuardrailEvent>> {
    db.with(|conn| {
        let mut stmt = conn.prepare("SELECT * FROM guardrail_events WHERE id = ?1")?;
        let mut rows = stmt.query_map([id], row_to_event)?;
        rows.next().transpose()
    })
}

/// Close out a pending approval.
pub fn settle(
    db: &Database,
    id: &str,
    status: Status,
    by: &str,
    reason: &str,
) -> crate::db::Result<()> {
    let (id, by, reason) = (id.to_string(), by.to_string(), reason.to_string());
    db.with(move |conn| {
        conn.execute(
            "UPDATE guardrail_events
                SET status = ?2, reason = ?3, decided_at = ?4, decided_by = ?5
              WHERE id = ?1",
            params![id, status.as_str(), reason, now_ms(), by],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'demo', 'C:/demo', 1, 1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn event<'a>(status: Status) -> NewEvent<'a> {
        NewEvent {
            project_id: Some("p1"),
            session_id: None,
            mission_id: Some("m1"),
            criterion_id: Some("c1"),
            origin: Origin::Verification,
            operation: Operation::GitForcePush,
            fragment: "git push --force",
            command: "git push --force origin main",
            status,
            reason: policy::reason::NOBODY_TO_ASK,
        }
    }

    #[test]
    fn an_event_survives_storage_with_the_fragment_that_matched() {
        let db = db();
        let id = record(&db, event(Status::Pending)).unwrap();

        let stored = get(&db, &id).unwrap().unwrap();
        assert_eq!(stored.operation, Operation::GitForcePush);
        assert_eq!(stored.fragment, "git push --force");
        assert_eq!(stored.status, Status::Pending);
        assert_eq!(stored.origin, Origin::Verification);
        assert!(stored.decided_at.is_none());
    }

    #[test]
    fn only_pending_events_are_waiting_for_someone() {
        let db = db();
        record(&db, event(Status::Pending)).unwrap();
        record(&db, event(Status::Denied)).unwrap();
        record(&db, event(Status::Asked)).unwrap();

        assert_eq!(pending(&db, None).unwrap().len(), 1);
        assert_eq!(pending(&db, Some("m1")).unwrap().len(), 1);
        assert_eq!(pending(&db, Some("other")).unwrap().len(), 0);
        assert_eq!(list(&db, Some("p1"), None, 100).unwrap().len(), 3);
    }

    #[test]
    fn settling_records_who_decided_and_when() {
        let db = db();
        let id = record(&db, event(Status::Pending)).unwrap();
        settle(&db, &id, Status::Allowed, "alan", "allowedOnce").unwrap();

        let stored = get(&db, &id).unwrap().unwrap();
        assert_eq!(stored.status, Status::Allowed);
        assert_eq!(stored.decided_by.as_deref(), Some("alan"));
        assert_eq!(stored.reason, "allowedOnce");
        assert!(stored.decided_at.is_some());
        assert!(pending(&db, None).unwrap().is_empty());
    }

    #[test]
    fn statuses_and_origins_round_trip() {
        for status in [Status::Pending, Status::Allowed, Status::Denied, Status::Asked] {
            assert_eq!(Status::parse(status.as_str()), status);
        }
        for origin in [Origin::Agent, Origin::Verification, Origin::Surface] {
            assert_eq!(Origin::parse(origin.as_str()), origin);
        }
    }
}
