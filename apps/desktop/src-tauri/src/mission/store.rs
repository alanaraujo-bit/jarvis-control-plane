//! Mission persistence and the rules that guard it.

use std::path::PathBuf;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use super::model::*;
use super::verify;
use super::{MissionError, Result};
use crate::db::Database;
use crate::session::log::now_ms;
use crate::AppState;

const GLOBAL_AUTONOMY_KEY: &str = "autonomy.global";

// ---- Row mapping ------------------------------------------------------------

fn row_to_mission(row: &rusqlite::Row<'_>) -> rusqlite::Result<Mission> {
    Ok(Mission {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        goal: row.get("goal")?,
        description: row.get("description")?,
        status: MissionStatus::parse(&row.get::<_, String>("status")?),
        autonomy: row
            .get::<_, Option<String>>("autonomy")?
            .and_then(|t| Autonomy::parse(&t)),
        blocked_reason: row.get("blocked_reason")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        started_at: row.get("started_at")?,
        completed_at: row.get("completed_at")?,
    })
}

fn row_to_criterion(row: &rusqlite::Row<'_>) -> rusqlite::Result<AcceptanceCriterion> {
    let raw: String = row.get("verification")?;
    Ok(AcceptanceCriterion {
        id: row.get("id")?,
        mission_id: row.get("mission_id")?,
        description: row.get("description")?,
        required: row.get::<_, i64>("required")? != 0,
        // A criterion whose stored verification cannot be parsed falls back to
        // Manual rather than vanishing: it must still block completion.
        verification: serde_json::from_str(&raw).unwrap_or(Verification::Manual),
        status: CriterionStatus::parse(&row.get::<_, String>("status")?),
        position: row.get("position")?,
        removed_at: row.get("removed_at")?,
        removed_reason: row.get("removed_reason")?,
        removed_by: row.get("removed_by")?,
    })
}

fn row_to_evidence(row: &rusqlite::Row<'_>) -> rusqlite::Result<Evidence> {
    Ok(Evidence {
        id: row.get("id")?,
        mission_id: row.get("mission_id")?,
        criterion_id: row.get("criterion_id")?,
        session_id: row.get("session_id")?,
        kind: EvidenceKind::parse(&row.get::<_, String>("kind")?),
        ok: row.get::<_, i64>("ok")? != 0,
        summary: row.get("summary")?,
        code: row.get("code")?,
        code_args: row.get("code_args")?,
        detail: row.get("detail")?,
        ts_ms: row.get("ts_ms")?,
    })
}

// ---- Input shapes -----------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewCriterion {
    pub description: String,
    #[serde(default = "yes")]
    pub required: bool,
    pub verification: Verification,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMission {
    pub project_id: String,
    pub title: String,
    pub goal: Option<String>,
    pub description: Option<String>,
    #[serde(default)]
    pub tasks: Vec<String>,
    #[serde(default)]
    pub criteria: Vec<NewCriterion>,
    pub autonomy: Option<Autonomy>,
}

// ---- Reads ------------------------------------------------------------------

pub fn global_autonomy(conn: &Connection) -> Option<Autonomy> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        [GLOBAL_AUTONOMY_KEY],
        |row| row.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
    .and_then(|t| Autonomy::parse(&t))
}

fn project_autonomy(conn: &Connection, project_id: &str) -> Option<Autonomy> {
    conn.query_row(
        "SELECT autonomy FROM projects WHERE id = ?1",
        [project_id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()
    .ok()
    .flatten()
    .flatten()
    .and_then(|t| Autonomy::parse(&t))
}

pub fn list(db: &Database, project_id: Option<&str>) -> Result<Vec<Mission>> {
    Ok(db.with(|conn| match project_id {
        Some(id) => {
            let mut stmt = conn.prepare(
                "SELECT * FROM missions WHERE project_id = ?1 ORDER BY updated_at DESC",
            )?;
            // Collected before `stmt` goes out of scope: the row iterator
            // borrows the statement.
            let rows = stmt.query_map([id], row_to_mission)?.collect();
            rows
        }
        None => {
            let mut stmt = conn.prepare("SELECT * FROM missions ORDER BY updated_at DESC")?;
            let rows = stmt.query_map([], row_to_mission)?.collect();
            rows
        }
    })?)
}

pub fn detail(db: &Database, mission_id: &str) -> Result<MissionDetail> {
    let id = mission_id.to_string();
    db.with(move |conn| {
        let mission: Option<Mission> = conn
            .query_row("SELECT * FROM missions WHERE id = ?1", [&id], row_to_mission)
            .optional()?;
        let Some(mission) = mission else {
            return Ok(None);
        };

        let tasks = {
            let mut stmt = conn.prepare(
                "SELECT * FROM mission_tasks WHERE mission_id = ?1 ORDER BY position",
            )?;
            let rows: rusqlite::Result<Vec<_>> = stmt
                .query_map([&id], |row| {
                    Ok(MissionTask {
                        id: row.get("id")?,
                        mission_id: row.get("mission_id")?,
                        description: row.get("description")?,
                        done: row.get::<_, i64>("done")? != 0,
                        position: row.get("position")?,
                    })
                })?
                .collect();
            rows
        }?;

        let criteria = {
            let mut stmt = conn.prepare(
                "SELECT * FROM acceptance_criteria WHERE mission_id = ?1 ORDER BY position",
            )?;
            let rows: rusqlite::Result<Vec<_>> =
                stmt.query_map([&id], row_to_criterion)?.collect();
            rows
        }?;

        let evidence = {
            let mut stmt = conn.prepare(
                "SELECT * FROM evidence WHERE mission_id = ?1 ORDER BY ts_ms DESC",
            )?;
            let rows: rusqlite::Result<Vec<_>> =
                stmt.query_map([&id], row_to_evidence)?.collect();
            rows
        }?;

        let effective_autonomy = resolve_autonomy(
            mission.autonomy,
            project_autonomy(conn, &mission.project_id),
            global_autonomy(conn),
        );

        Ok(Some(MissionDetail {
            mission,
            tasks,
            criteria,
            evidence,
            effective_autonomy,
        }))
    })?
    .ok_or_else(|| MissionError::Unknown(mission_id.to_string()))
}

// ---- Writes -----------------------------------------------------------------

pub fn create(db: &Database, input: NewMission) -> Result<Mission> {
    if input.title.trim().is_empty() {
        return Err(MissionError::MissingTitle);
    }

    let id = uuid::Uuid::now_v7().to_string();
    let now = now_ms();

    db.with(|conn| {
        conn.execute(
            "INSERT INTO missions
                 (id, project_id, title, goal, description, status, autonomy, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'ready', ?6, ?7, ?7)",
            params![
                id,
                input.project_id,
                input.title.trim(),
                input.goal,
                input.description,
                input.autonomy.map(|a| a.as_str()),
                now
            ],
        )?;

        for (index, task) in input.tasks.iter().enumerate() {
            conn.execute(
                "INSERT INTO mission_tasks (id, mission_id, description, position)
                 VALUES (?1, ?2, ?3, ?4)",
                params![uuid::Uuid::now_v7().to_string(), id, task, index as i64],
            )?;
        }

        for (index, criterion) in input.criteria.iter().enumerate() {
            conn.execute(
                "INSERT INTO acceptance_criteria
                     (id, mission_id, description, required, verification, position)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    uuid::Uuid::now_v7().to_string(),
                    id,
                    criterion.description,
                    criterion.required as i64,
                    serde_json::to_string(&criterion.verification).unwrap_or_default(),
                    index as i64
                ],
            )?;
        }
        Ok(())
    })?;

    Ok(detail(db, &id)?.mission)
}

/// Move a mission to a new status.
///
/// The one transition this refuses is the one §30 exists for: a mission cannot
/// become `Completed` while a required criterion is unverified, no matter who
/// asks. Everything else is allowed — including moving to `Blocked`, which must
/// always be reachable so a stuck mission can say so (§34).
pub fn set_status(
    db: &Database,
    mission_id: &str,
    status: MissionStatus,
    reason: Option<String>,
) -> Result<Mission> {
    if status == MissionStatus::Completed {
        let current = detail(db, mission_id)?;
        let blockers = completion_blockers(&current.criteria);
        if !blockers.is_empty() {
            return Err(MissionError::NotVerified {
                count: blockers.len(),
            });
        }
    }

    let now = now_ms();
    let id = mission_id.to_string();
    db.with(move |conn| {
        conn.execute(
            "UPDATE missions
                SET status = ?2,
                    blocked_reason = ?3,
                    updated_at = ?4,
                    started_at = COALESCE(started_at, CASE WHEN ?2 = 'running' THEN ?4 END),
                    completed_at = CASE WHEN ?2 = 'completed' THEN ?4 ELSE completed_at END
              WHERE id = ?1",
            params![id, status.as_str(), reason, now],
        )?;
        Ok(())
    })?;

    let mission = detail(db, mission_id)?.mission;

    // Worth a person knowing: a mission that stopped, finished, or failed.
    // Ordinary transitions are not logged — a log of everything is a log nobody
    // reads (§48/§49).
    let severity = match status {
        MissionStatus::Blocked | MissionStatus::Failed => crate::activity::Severity::Attention,
        MissionStatus::Waiting => crate::activity::Severity::Warning,
        _ => crate::activity::Severity::Info,
    };
    if status.needs_attention() || status == MissionStatus::Completed {
        crate::activity::record(
            db,
            &format!("mission.{}", status.as_str()),
            severity,
            &mission.title,
            mission.blocked_reason.clone(),
            Some(&mission.project_id),
            None,
            Some(mission_id),
        );
    }

    Ok(mission)
}

/// Set a mission's own autonomy, or clear it so it inherits again (§33).
///
/// `None` clears rather than writing a value: "follow the project" is a
/// different intention from "be Guided here regardless of the project", and
/// collapsing the two would silently pin a mission the first time anyone
/// looked at the setting.
pub fn set_autonomy(
    db: &Database,
    mission_id: &str,
    autonomy: Option<Autonomy>,
) -> Result<Mission> {
    let id = mission_id.to_string();
    let value = autonomy.map(|a| a.as_str().to_string());
    db.with(move |conn| {
        conn.execute(
            "UPDATE missions SET autonomy = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, value, now_ms()],
        )?;
        Ok(())
    })?;
    Ok(detail(db, mission_id)?.mission)
}

/// Withdraw a criterion, with a reason.
///
/// Deliberately not a delete (§31). An agent may propose that a requirement no
/// longer applies, but the record of it — and of who withdrew it and why — has
/// to survive, otherwise a mission can be simplified until it succeeds.
pub fn withdraw_criterion(
    db: &Database,
    criterion_id: &str,
    reason: String,
    by: String,
) -> Result<()> {
    let id = criterion_id.to_string();
    let now = now_ms();
    db.with(move |conn| {
        conn.execute(
            "UPDATE acceptance_criteria
                SET removed_at = ?2, removed_reason = ?3, removed_by = ?4
              WHERE id = ?1 AND removed_at IS NULL",
            params![id, now, reason, by],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Run every automatic criterion and record what happened.
///
/// Returns the mission's detail afterwards. This never marks the mission
/// complete by itself — it establishes facts, and `set_status` decides whether
/// those facts are enough.
pub fn verify_mission(db: &Database, mission_id: &str) -> Result<MissionDetail> {
    let current = detail(db, mission_id)?;

    let project_dir: String = db.with(|conn| {
        conn.query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [&current.mission.project_id],
            |row| row.get(0),
        )
    })?;
    let project_dir = PathBuf::from(project_dir);

    let mut held = 0usize;
    for criterion in current.criteria.iter().filter(|c| c.is_active()) {
        // A manual criterion is left alone rather than being run and failed
        // every time: it is waiting for a person, not for a check.
        if !criterion.verification.is_automatic() {
            continue;
        }

        // Guardrails apply to what J.A.R.V.I.S. runs, not only to what agents
        // run (§35). A verification command is a command like any other, and
        // this is the one place where enforcement is unconditional: we own the
        // process, so a refusal here means it genuinely does not execute.
        if hold_for_guardrail(db, &current.mission, criterion)? {
            held += 1;
            continue;
        }

        run_and_record(db, mission_id, criterion, &project_dir)?;
    }

    // A mission with a check waiting on a person needs a person (§34). Saying
    // so is the difference between a mission that is stuck and one that is
    // stuck and silent.
    if held > 0 {
        let reason =
            format!("{held} acceptance criteria need approval before they can be checked.");
        set_status(db, mission_id, MissionStatus::Waiting, Some(reason))?;
        return detail(db, mission_id);
    }

    db.with(|conn| {
        conn.execute(
            "UPDATE missions SET updated_at = ?2 WHERE id = ?1",
            params![mission_id, now_ms()],
        )?;
        Ok(())
    })?;

    // Completion is revocable.
    //
    // §30 says a mission is complete when its criteria are verified — which is
    // a statement about the present, not about the moment someone pressed a
    // button. If a criterion that once passed no longer does, the mission is no
    // longer verifiably complete, and continuing to display it as Completed
    // would be exactly the false claim this whole mechanism exists to prevent.
    let after = detail(db, mission_id)?;
    if after.mission.status == MissionStatus::Completed {
        let blockers = completion_blockers(&after.criteria);
        if !blockers.is_empty() {
            let reason = format!(
                "Acceptance criteria that previously passed no longer hold ({}).",
                blockers.len()
            );
            return set_status(db, mission_id, MissionStatus::Failed, Some(reason))
                .and_then(|_| detail(db, mission_id));
        }
    }

    Ok(after)
}

/// Run one criterion check and record the evidence.
fn run_and_record(
    db: &Database,
    mission_id: &str,
    criterion: &AcceptanceCriterion,
    project_dir: &std::path::Path,
) -> Result<()> {
    let outcome = verify::check(&criterion.verification, project_dir);
    let record = verify::evidence_from(mission_id, Some(&criterion.id), None, &outcome);
    let status = if outcome.ok {
        CriterionStatus::Verified
    } else {
        CriterionStatus::Failed
    };

    db.with(|conn| {
        conn.execute(
            "INSERT INTO evidence
                 (id, mission_id, criterion_id, session_id, kind, ok, summary,
                  code, code_args, detail, ts_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                record.id,
                record.mission_id,
                record.criterion_id,
                record.session_id,
                record.kind.as_str(),
                record.ok as i64,
                record.summary,
                record.code,
                record.code_args,
                record.detail,
                record.ts_ms
            ],
        )?;
        conn.execute(
            "UPDATE acceptance_criteria SET status = ?2 WHERE id = ?1",
            params![criterion.id, status.as_str()],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Whether a guardrail stops this criterion from being checked right now (§35).
///
/// Returns true when the check was held or refused, so the caller skips it.
/// Only a Command criterion can match: the others touch nothing outside the
/// project directory.
fn hold_for_guardrail(
    db: &Database,
    mission: &Mission,
    criterion: &AcceptanceCriterion,
) -> Result<bool> {
    let Verification::Command { command, .. } = &criterion.verification else {
        return Ok(false);
    };
    let matches = crate::guardrail::classify(command);
    if matches.is_empty() {
        return Ok(false);
    }

    // The strictest decision across everything the command does. Allowing the
    // whole command because one part of it was permitted would defeat the point.
    fn rank(decision: crate::guardrail::Decision) -> u8 {
        match decision {
            crate::guardrail::Decision::Allow => 0,
            crate::guardrail::Decision::Ask => 1,
            crate::guardrail::Decision::Deny => 2,
        }
    }

    let mut strictest: Option<(crate::guardrail::Match, crate::guardrail::Decision)> = None;
    for m in matches {
        let resolved = db.with(|conn| {
            Ok(crate::guardrail::policy::resolve(
                conn,
                Some(&mission.project_id),
                m.operation,
            )?)
        })?;
        let stricter = strictest
            .as_ref()
            .map(|(_, current)| rank(resolved.decision) > rank(*current))
            .unwrap_or(true);
        if stricter {
            strictest = Some((m, resolved.decision));
        }
    }

    let Some((matched, decision)) = strictest else {
        return Ok(false);
    };
    if decision == crate::guardrail::Decision::Allow {
        return Ok(false);
    }

    let denied = decision == crate::guardrail::Decision::Deny;
    crate::guardrail::record(
        db,
        crate::guardrail::NewEvent {
            project_id: Some(&mission.project_id),
            session_id: None,
            mission_id: Some(&mission.id),
            criterion_id: Some(&criterion.id),
            origin: crate::guardrail::Origin::Verification,
            operation: matched.operation,
            fragment: &matched.fragment,
            command,
            status: if denied {
                crate::guardrail::Status::Denied
            } else {
                // Unlike an agent tool call, a verification can genuinely be
                // paused and resumed: nothing is mid-flight waiting on an
                // answer, so the question can wait for the person to come back.
                crate::guardrail::Status::Pending
            },
            reason: if denied {
                crate::guardrail::policy::reason::POLICY_DENIES
            } else {
                crate::guardrail::policy::reason::ASKED_HUMAN
            },
        },
    )?;

    if denied {
        record_refusal(db, &criterion.id, matched.operation.as_str())?;
    }
    Ok(true)
}

/// Run one held criterion now that its approval has been given (§35).
pub fn verify_criterion(db: &Database, criterion_id: &str) -> Result<()> {
    let (mission_id, project_dir): (String, String) = db.with(|conn| {
        conn.query_row(
            "SELECT c.mission_id, p.path
               FROM acceptance_criteria c
               JOIN missions m ON m.id = c.mission_id
               JOIN projects p ON p.id = m.project_id
              WHERE c.id = ?1",
            [criterion_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
    })?;

    let criterion = detail(db, &mission_id)?
        .criteria
        .into_iter()
        .find(|c| c.id == criterion_id)
        .ok_or_else(|| MissionError::Unknown(criterion_id.to_string()))?;

    run_and_record(
        db,
        &mission_id,
        &criterion,
        std::path::Path::new(&project_dir),
    )
}

/// Record that a criterion could not be checked because a guardrail refused.
///
/// Deliberately evidence, and deliberately negative. §30 asks whether a
/// criterion *holds*, and "we were not allowed to find out" is not a yes — so
/// it must block completion exactly as a failure does, while saying plainly
/// that nothing was actually tested.
pub fn record_refusal(db: &Database, criterion_id: &str, operation: &str) -> Result<()> {
    let mission_id: String = db.with(|conn| {
        conn.query_row(
            "SELECT mission_id FROM acceptance_criteria WHERE id = ?1",
            [criterion_id],
            |row| row.get(0),
        )
    })?;

    let evidence = Evidence {
        id: uuid::Uuid::now_v7().to_string(),
        mission_id,
        criterion_id: Some(criterion_id.to_string()),
        session_id: None,
        kind: EvidenceKind::Manual,
        ok: false,
        summary: format!("Not checked: a guardrail refused {operation}"),
        // The same fact, in a form the interface can say in the reader's
        // language (§65). The English above stays as the fallback.
        code: Some("evidence.guardrailRefused".into()),
        code_args: serde_json::to_string(&serde_json::json!({ "operation": operation })).ok(),
        detail: Some(
            "The check was never run, so this criterion is unverified rather than failed."
                .into(),
        ),
        ts_ms: now_ms(),
    };

    db.with(|conn| {
        conn.execute(
            "INSERT INTO evidence
                 (id, mission_id, criterion_id, session_id, kind, ok, summary,
                  code, code_args, detail, ts_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                evidence.id,
                evidence.mission_id,
                evidence.criterion_id,
                evidence.session_id,
                evidence.kind.as_str(),
                evidence.ok as i64,
                evidence.summary,
                evidence.code,
                evidence.code_args,
                evidence.detail,
                evidence.ts_ms
            ],
        )?;
        // Pending, not failed: nothing was tested, and claiming a failure would
        // be as untrue as claiming a pass.
        conn.execute(
            "UPDATE acceptance_criteria SET status = ?2 WHERE id = ?1",
            params![criterion_id, CriterionStatus::Pending.as_str()],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Confirm a manual criterion, recording who vouched for it.
pub fn confirm_manual(db: &Database, criterion_id: &str, by: String) -> Result<()> {
    let id = criterion_id.to_string();
    db.with(move |conn| {
        let mission_id: String = conn.query_row(
            "SELECT mission_id FROM acceptance_criteria WHERE id = ?1",
            [&id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO evidence
                 (id, mission_id, criterion_id, kind, ok, summary, code, code_args, ts_ms)
             VALUES (?1, ?2, ?3, 'manual', 1, ?4, ?5, ?6, ?7)",
            params![
                uuid::Uuid::now_v7().to_string(),
                mission_id,
                id,
                format!("Confirmed by {by}"),
                "evidence.manual.confirmedBy",
                serde_json::to_string(&serde_json::json!({ "who": by })).ok(),
                now_ms()
            ],
        )?;
        conn.execute(
            "UPDATE acceptance_criteria SET status = 'verified' WHERE id = ?1",
            [&id],
        )?;
        Ok(())
    })?;
    Ok(())
}

// ---- Commands ---------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionSummary {
    #[serde(flatten)]
    pub mission: Mission,
    pub project_name: String,
    pub task_count: i64,
    pub tasks_done: i64,
    /// Required, active criteria that are not yet verified.
    pub open_criteria: i64,
}

/// Missions across every project, for Mission Control (§18).
pub fn summaries(db: &Database) -> Result<Vec<MissionSummary>> {
    Ok(db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT m.*,
                    p.name AS project_name,
                    (SELECT COUNT(*) FROM mission_tasks t WHERE t.mission_id = m.id)
                        AS task_count,
                    (SELECT COUNT(*) FROM mission_tasks t WHERE t.mission_id = m.id AND t.done = 1)
                        AS tasks_done,
                    (SELECT COUNT(*) FROM acceptance_criteria c
                      WHERE c.mission_id = m.id
                        AND c.removed_at IS NULL
                        AND c.required = 1
                        AND c.status != 'verified') AS open_criteria
               FROM missions m
               JOIN projects p ON p.id = m.project_id
              ORDER BY m.updated_at DESC",
        )?;
        // Collected before `stmt` is dropped; the iterator borrows it.
        let rows = stmt
            .query_map([], |row| {
                Ok(MissionSummary {
                    project_name: row.get("project_name")?,
                    task_count: row.get("task_count")?,
                    tasks_done: row.get("tasks_done")?,
                    open_criteria: row.get("open_criteria")?,
                    mission: row_to_mission(row)?,
                })
            })?
            .collect();
        rows
    })?)
}

#[tauri::command]
pub fn list_missions(state: State<'_, AppState>, project_id: Option<String>) -> Result<Vec<Mission>> {
    list(&state.db, project_id.as_deref())
}

#[tauri::command]
pub fn mission_summaries(state: State<'_, AppState>) -> Result<Vec<MissionSummary>> {
    summaries(&state.db)
}

#[tauri::command]
pub fn mission_detail(state: State<'_, AppState>, mission_id: String) -> Result<MissionDetail> {
    detail(&state.db, &mission_id)
}

#[tauri::command]
pub fn create_mission(state: State<'_, AppState>, mission: NewMission) -> Result<Mission> {
    create(&state.db, mission)
}

#[tauri::command]
pub fn set_mission_status(
    state: State<'_, AppState>,
    mission_id: String,
    status: MissionStatus,
    reason: Option<String>,
) -> Result<Mission> {
    set_status(&state.db, &mission_id, status, reason)
}

#[tauri::command]
pub fn verify_mission_now(
    state: State<'_, AppState>,
    mission_id: String,
) -> Result<MissionDetail> {
    verify_mission(&state.db, &mission_id)
}

#[tauri::command]
pub fn confirm_criterion(
    state: State<'_, AppState>,
    criterion_id: String,
    by: String,
) -> Result<()> {
    confirm_manual(&state.db, &criterion_id, by)
}

#[tauri::command]
pub fn withdraw_mission_criterion(
    state: State<'_, AppState>,
    criterion_id: String,
    reason: String,
    by: String,
) -> Result<()> {
    withdraw_criterion(&state.db, &criterion_id, reason, by)
}

#[tauri::command]
pub fn set_mission_autonomy(
    state: State<'_, AppState>,
    mission_id: String,
    autonomy: Option<Autonomy>,
) -> Result<Mission> {
    set_autonomy(&state.db, &mission_id, autonomy)
}

/// The whole §33 chain, in one answer.
///
/// A surface that offers to change a default has to be able to say what the
/// default currently *is* and what it would give way to — otherwise the word
/// "inherited" points at something nobody can see. Mission Detail has rendered
/// exactly that label since §33 shipped, against a project value and a global
/// value with no way to read or set either.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyChain {
    /// The global default. `None` means nothing has been chosen and `Guided`
    /// applies — which is a different statement from "Guided was chosen", and
    /// the surface says so.
    pub global: Option<Autonomy>,
    /// This project's own default, when a project was asked about.
    pub project: Option<Autonomy>,
    /// What a mission with no setting of its own would run at.
    pub effective: Autonomy,
}

/// Read the autonomy chain, optionally for one project.
pub fn chain(db: &Database, project_id: Option<&str>) -> Result<AutonomyChain> {
    let chain = db.with(|conn| {
        let global = global_autonomy(conn);
        let project = project_id.and_then(|id| project_autonomy(conn, id));
        Ok(AutonomyChain {
            global,
            project,
            // The same resolver missions use, not a second copy of the rule —
            // two spellings of an inheritance chain is how they drift apart.
            effective: resolve_autonomy(None, project, global),
        })
    })?;
    Ok(chain)
}

pub fn set_global(db: &Database, autonomy: Option<Autonomy>) -> Result<AutonomyChain> {
    db.with(|conn| {
        match autonomy {
            Some(level) => conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                params![GLOBAL_AUTONOMY_KEY, level.as_str()],
            )?,
            None => conn.execute("DELETE FROM settings WHERE key = ?1", [GLOBAL_AUTONOMY_KEY])?,
        };
        Ok(())
    })?;
    chain(db, None)
}

pub fn set_project(
    db: &Database,
    project_id: &str,
    autonomy: Option<Autonomy>,
) -> Result<AutonomyChain> {
    let id = project_id.to_string();
    let value = autonomy.map(|a| a.as_str().to_string());
    db.with(move |conn| {
        conn.execute("UPDATE projects SET autonomy = ?2 WHERE id = ?1", params![id, value])?;
        Ok(())
    })?;
    chain(db, Some(project_id))
}

#[tauri::command]
pub fn autonomy_chain(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<AutonomyChain> {
    chain(&state.db, project_id.as_deref())
}

/// Set — or clear — the global default.
///
/// `None` removes the row rather than storing a sentinel, so "no global
/// default has been chosen" has exactly one representation in the database and
/// `global_autonomy`'s existing `Option` keeps meaning what it already meant.
#[tauri::command]
pub fn set_global_autonomy(
    state: State<'_, AppState>,
    autonomy: Option<Autonomy>,
) -> Result<AutonomyChain> {
    set_global(&state.db, autonomy)
}

/// Set — or clear — one project's default.
///
/// Clearing is `NULL`, which is what `project_autonomy` already reads as "this
/// project has no opinion, ask the global default".
#[tauri::command]
pub fn set_project_autonomy(
    state: State<'_, AppState>,
    project_id: String,
    autonomy: Option<Autonomy>,
) -> Result<AutonomyChain> {
    set_project(&state.db, &project_id, autonomy)
}

#[tauri::command]
pub fn set_mission_task_done(
    state: State<'_, AppState>,
    task_id: String,
    done: bool,
) -> Result<()> {
    state.db.with(|conn| {
        conn.execute(
            "UPDATE mission_tasks SET done = ?2 WHERE id = ?1",
            params![task_id, done as i64],
        )?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        db: Database,
        project_id: String,
        dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let db = Database::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let project_id = "p1".to_string();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES (?1, 'Demo', ?2, 0, 0)",
                params![project_id, dir.path().to_string_lossy()],
            )?;
            Ok(())
        })
        .unwrap();
        Fixture { db, project_id, dir }
    }

    fn mission_with(f: &Fixture, criteria: Vec<NewCriterion>) -> Mission {
        create(
            &f.db,
            NewMission {
                project_id: f.project_id.clone(),
                title: "Ship the thing".into(),
                goal: Some("It works".into()),
                description: None,
                tasks: vec!["Do the work".into()],
                criteria,
                autonomy: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn a_mission_requires_a_title() {
        let f = fixture();
        let result = create(
            &f.db,
            NewMission {
                project_id: f.project_id.clone(),
                title: "   ".into(),
                goal: None,
                description: None,
                tasks: vec![],
                criteria: vec![],
                autonomy: None,
            },
        );
        assert!(matches!(result, Err(MissionError::MissingTitle)));
    }

    /// The heart of §30.
    #[test]
    fn a_mission_cannot_be_completed_while_a_required_criterion_is_unverified() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "The build passes".into(),
                required: true,
                verification: Verification::FileExists {
                    path: "dist/app.js".into(),
                },
            }],
        );

        let result = set_status(&f.db, &mission.id, MissionStatus::Completed, None);
        assert!(
            matches!(result, Err(MissionError::NotVerified { count: 1 })),
            "claiming completion must be refused without evidence"
        );

        // The mission did not move.
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().mission.status,
            MissionStatus::Ready
        );
    }

    #[test]
    fn verification_produces_evidence_and_then_completion_is_allowed() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "The artifact exists".into(),
                required: true,
                verification: Verification::FileExists {
                    path: "dist/app.js".into(),
                },
            }],
        );

        // Not there yet: verification records a failure, not a pass.
        let after_fail = verify_mission(&f.db, &mission.id).unwrap();
        assert_eq!(after_fail.criteria[0].status, CriterionStatus::Failed);
        assert_eq!(after_fail.evidence.len(), 1);
        assert!(!after_fail.evidence[0].ok);
        // `code`/`code_args` are set on the `Outcome` in `verify.rs`, but the
        // INSERT this row goes through is a separate place they can be
        // silently dropped again (as `run_and_record`'s once did) — round
        // trip through the real database, not just the in-memory struct.
        assert_eq!(
            after_fail.evidence[0].code.as_deref(),
            Some("evidence.file.missing")
        );
        assert!(set_status(&f.db, &mission.id, MissionStatus::Completed, None).is_err());

        // Do the work for real, then verify again.
        std::fs::create_dir_all(f.dir.path().join("dist")).unwrap();
        std::fs::write(f.dir.path().join("dist/app.js"), "console.log(1)").unwrap();

        let after_pass = verify_mission(&f.db, &mission.id).unwrap();
        assert_eq!(after_pass.criteria[0].status, CriterionStatus::Verified);
        assert!(after_pass.evidence.iter().any(|e| e.ok));
        assert!(after_pass
            .evidence
            .iter()
            .any(|e| e.code.as_deref() == Some("evidence.file.exists")));

        let completed = set_status(&f.db, &mission.id, MissionStatus::Completed, None).unwrap();
        assert_eq!(completed.status, MissionStatus::Completed);
        assert!(completed.completed_at.is_some());
    }

    #[test]
    fn a_command_criterion_runs_a_real_command() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "The check passes".into(),
                required: true,
                verification: Verification::Command {
                    command: "echo mission-check-ok".into(),
                    cwd: None,
                    expect_exit: 0,
                },
            }],
        );

        let verified = verify_mission(&f.db, &mission.id).unwrap();
        assert_eq!(verified.criteria[0].status, CriterionStatus::Verified);
        assert!(verified.evidence[0]
            .detail
            .as_ref()
            .unwrap()
            .contains("mission-check-ok"));
    }

    /// §31 — the requirement survives its own withdrawal.
    #[test]
    fn withdrawing_a_criterion_unblocks_completion_but_keeps_the_record() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "Impossible right now".into(),
                required: true,
                verification: Verification::Manual,
            }],
        );

        let before = detail(&f.db, &mission.id).unwrap();
        withdraw_criterion(
            &f.db,
            &before.criteria[0].id,
            "Descoped with the user".into(),
            "claude-code".into(),
        )
        .unwrap();

        let after = detail(&f.db, &mission.id).unwrap();
        assert!(!after.criteria[0].is_active());
        assert_eq!(after.criteria[0].removed_by.as_deref(), Some("claude-code"));
        assert!(after.criteria[0].removed_reason.is_some());

        // Still visible, still explained — never silently gone.
        assert_eq!(after.criteria.len(), 1);
        assert!(set_status(&f.db, &mission.id, MissionStatus::Completed, None).is_ok());
    }

    /// §34 — a mission that cannot proceed must be able to say so.
    #[test]
    fn a_mission_can_always_become_blocked_with_an_explanation() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "Deploy succeeds".into(),
                required: true,
                verification: Verification::Manual,
            }],
        );

        let blocked = set_status(
            &f.db,
            &mission.id,
            MissionStatus::Blocked,
            Some("Needs production credentials that are not available here".into()),
        )
        .unwrap();

        assert_eq!(blocked.status, MissionStatus::Blocked);
        assert!(blocked.blocked_reason.unwrap().contains("credentials"));
        assert!(blocked.completed_at.is_none());
    }

    #[test]
    fn manual_criteria_stay_unverified_until_a_person_confirms_them() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "The screen looks right".into(),
                required: true,
                verification: Verification::Manual,
            }],
        );

        // Running verification must not pass it.
        let after = verify_mission(&f.db, &mission.id).unwrap();
        assert_eq!(after.criteria[0].status, CriterionStatus::Pending);
        assert!(set_status(&f.db, &mission.id, MissionStatus::Completed, None).is_err());

        confirm_manual(&f.db, &after.criteria[0].id, "Alan".into()).unwrap();
        let confirmed = detail(&f.db, &mission.id).unwrap();
        assert_eq!(confirmed.criteria[0].status, CriterionStatus::Verified);
        let evidence = confirmed
            .evidence
            .iter()
            .find(|e| e.summary.contains("Alan"))
            .expect("confirmation evidence");
        // Pinned so the INSERT that writes this row cannot silently drop the
        // two columns again — the struct being right is not enough (§65).
        assert_eq!(evidence.code.as_deref(), Some("evidence.manual.confirmedBy"));
        assert_eq!(
            evidence.code_args.as_deref().map(|raw| serde_json::from_str::<serde_json::Value>(raw).unwrap()["who"].clone()),
            Some(serde_json::json!("Alan"))
        );
        assert!(set_status(&f.db, &mission.id, MissionStatus::Completed, None).is_ok());
    }

    /// A completed mission whose evidence later regresses must stop claiming
    /// to be complete. This is the same rule as §30, applied over time.
    #[test]
    fn completion_is_revoked_when_evidence_stops_holding() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "The artifact exists".into(),
                required: true,
                verification: Verification::FileExists {
                    path: "dist/app.js".into(),
                },
            }],
        );

        std::fs::create_dir_all(f.dir.path().join("dist")).unwrap();
        std::fs::write(f.dir.path().join("dist/app.js"), "built").unwrap();

        verify_mission(&f.db, &mission.id).unwrap();
        let completed = set_status(&f.db, &mission.id, MissionStatus::Completed, None).unwrap();
        assert_eq!(completed.status, MissionStatus::Completed);

        // The artifact disappears — a deleted build, a reverted commit.
        std::fs::remove_file(f.dir.path().join("dist/app.js")).unwrap();

        let after = verify_mission(&f.db, &mission.id).unwrap();
        assert_ne!(
            after.mission.status,
            MissionStatus::Completed,
            "a mission whose criteria no longer pass must not keep claiming completion"
        );
        assert_eq!(after.mission.status, MissionStatus::Failed);
        assert!(after
            .mission
            .blocked_reason
            .as_deref()
            .unwrap_or_default()
            .contains("no longer hold"));
        // And it is visible as something a human must look at.
        assert!(after.mission.status.needs_attention());
    }

    #[test]
    fn re_verifying_a_still_valid_completed_mission_leaves_it_completed() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "The artifact exists".into(),
                required: true,
                verification: Verification::FileExists {
                    path: "kept.txt".into(),
                },
            }],
        );
        std::fs::write(f.dir.path().join("kept.txt"), "x").unwrap();

        verify_mission(&f.db, &mission.id).unwrap();
        set_status(&f.db, &mission.id, MissionStatus::Completed, None).unwrap();

        let again = verify_mission(&f.db, &mission.id).unwrap();
        assert_eq!(again.mission.status, MissionStatus::Completed);
    }

    #[test]
    fn a_mission_with_no_criteria_can_complete() {
        // Not every piece of work needs formal criteria; the rule is only that
        // stated requirements must be met.
        let f = fixture();
        let mission = mission_with(&f, vec![]);
        assert!(set_status(&f.db, &mission.id, MissionStatus::Completed, None).is_ok());
    }

    /// Unattended is unreachable without this, so it is worth pinning (§32).
    #[test]
    fn a_missions_autonomy_can_be_set_and_cleared() {
        let f = fixture();
        let mission = mission_with(&f, vec![]);

        // The project says Guided; the mission overrides it.
        f.db.with(|conn| {
            conn.execute(
                "UPDATE projects SET autonomy = 'guided' WHERE id = ?1",
                [&f.project_id],
            )?;
            Ok(())
        })
        .unwrap();

        set_autonomy(&f.db, &mission.id, Some(Autonomy::Unattended)).unwrap();
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Unattended
        );

        // Cleared, it inherits again rather than being pinned to a default.
        set_autonomy(&f.db, &mission.id, None).unwrap();
        let after = detail(&f.db, &mission.id).unwrap();
        assert!(after.mission.autonomy.is_none());
        assert_eq!(after.effective_autonomy, Autonomy::Guided);
    }

    #[test]
    fn autonomy_falls_back_through_mission_project_and_global() {
        let f = fixture();
        let mission = mission_with(&f, vec![]);

        // Nothing set anywhere.
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Guided
        );

        f.db.with(|conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, 'unattended')",
                [GLOBAL_AUTONOMY_KEY],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Unattended
        );

        f.db.with(|conn| {
            conn.execute(
                "UPDATE projects SET autonomy = 'guided' WHERE id = ?1",
                [&f.project_id],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Guided,
            "the project setting must beat the global one"
        );

        f.db.with(|conn| {
            conn.execute(
                "UPDATE missions SET autonomy = 'autonomous' WHERE id = ?1",
                [&mission.id],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Autonomous,
            "the mission setting must beat both"
        );
    }

    /// The two levels of §33 that had no way in.
    ///
    /// `resolve_autonomy` has read the project and global levels since §33
    /// shipped, and nothing in the product could write either — so Mission
    /// Detail rendered "inherited" against a value that could not be seen or
    /// changed. This drives both from the outside and checks that a real
    /// mission's effective autonomy moves as a result.
    #[test]
    fn the_project_and_global_defaults_can_now_be_set_and_cleared() {
        let f = fixture();
        let mission = mission_with(&f, vec![]);

        // Nothing chosen anywhere. `Guided` applies, and the chain says so
        // *without* claiming Guided was chosen — that distinction is the whole
        // reason `global` is an `Option` rather than a level.
        let start = chain(&f.db, Some(&f.project_id)).unwrap();
        assert!(start.global.is_none());
        assert!(start.project.is_none());
        assert_eq!(start.effective, Autonomy::Guided);

        let after_global = set_global(&f.db, Some(Autonomy::Autonomous)).unwrap();
        assert_eq!(after_global.global, Some(Autonomy::Autonomous));
        assert_eq!(after_global.effective, Autonomy::Autonomous);
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Autonomous,
            "a real mission with no setting of its own has to follow the global default"
        );

        let after_project = set_project(&f.db, &f.project_id, Some(Autonomy::Guided)).unwrap();
        assert_eq!(after_project.project, Some(Autonomy::Guided));
        assert_eq!(
            after_project.global,
            Some(Autonomy::Autonomous),
            "setting a project default must not quietly overwrite the global one"
        );
        assert_eq!(after_project.effective, Autonomy::Guided);

        // Clearing is not the same as choosing Guided: it hands the decision
        // back up the chain, which is exactly what the surface has to be able
        // to express.
        let cleared = set_project(&f.db, &f.project_id, None).unwrap();
        assert!(cleared.project.is_none());
        assert_eq!(cleared.effective, Autonomy::Autonomous);

        let no_global = set_global(&f.db, None).unwrap();
        assert!(no_global.global.is_none(), "clearing removes the row rather than storing a sentinel");
        assert_eq!(no_global.effective, Autonomy::Guided);
        assert_eq!(
            detail(&f.db, &mission.id).unwrap().effective_autonomy,
            Autonomy::Guided
        );
    }

    /// Setting the same level twice must update, not fail on the primary key.
    #[test]
    fn choosing_a_global_default_twice_replaces_it() {
        let f = fixture();
        set_global(&f.db, Some(Autonomy::Guided)).unwrap();
        let second = set_global(&f.db, Some(Autonomy::Unattended)).unwrap();
        assert_eq!(second.global, Some(Autonomy::Unattended));

        let rows: i64 = f
            .db
            .with(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM settings WHERE key = ?1",
                    [GLOBAL_AUTONOMY_KEY],
                    |r| r.get(0),
                )
            })
            .unwrap();
        assert_eq!(rows, 1);
    }

    #[test]
    fn summaries_report_progress_across_projects() {
        let f = fixture();
        let mission = mission_with(
            &f,
            vec![NewCriterion {
                description: "c".into(),
                required: true,
                verification: Verification::Manual,
            }],
        );

        let all = summaries(&f.db).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].project_name, "Demo");
        assert_eq!(all[0].task_count, 1);
        assert_eq!(all[0].tasks_done, 0);
        assert_eq!(all[0].open_criteria, 1);
        assert_eq!(all[0].mission.id, mission.id);
    }
}
