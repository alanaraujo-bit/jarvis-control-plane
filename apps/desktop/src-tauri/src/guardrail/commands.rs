//! Guardrail commands exposed to the UI (§35).

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;
use crate::session::log::now_ms;
use crate::AppState;

/// Errors cross the IPC boundary as text, as everywhere else in this crate:
/// the webview renders a message, and a structured error type would have to be
/// mirrored in TypeScript to say the same thing.
type IpcResult<T> = std::result::Result<T, String>;

use super::classify::{self, Operation};
use super::policy::{self, Decision, Resolved, Scope};
use super::{GuardrailEvent, Status};

/// One operation as the settings surface shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolicyView {
    pub operation: Operation,
    /// What applies here, after the chain.
    pub decision: Decision,
    /// Which scope decided it, so the UI can say *why* (§28's instinct applied
    /// to settings: never show a resolved value as if it were set here).
    pub scope: Scope,
    /// What the wider scope would say if the project rule were cleared. `None`
    /// when this view is already the global one.
    pub inherited: Option<Decision>,
}

/// Every operation and what policy currently says about it.
///
/// `project_id` is `None` for the global list in Settings.
#[tauri::command]
pub fn guardrail_policies(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> IpcResult<Vec<PolicyView>> {
    policies(&state.db, project_id.as_deref()).map_err(|e| e.to_string())
}

fn policies(db: &Database, project_id: Option<&str>) -> crate::db::Result<Vec<PolicyView>> {
    db.with(|conn| {
        classify::ALL
            .iter()
            .map(|op| {
                let Resolved { decision, scope } = policy::resolve(conn, project_id, *op)?;
                let inherited = match project_id {
                    None => None,
                    Some(_) => Some(policy::resolve(conn, None, *op)?.decision),
                };
                Ok(PolicyView {
                    operation: *op,
                    decision,
                    scope,
                    inherited,
                })
            })
            .collect()
    })
}

/// Set or clear a rule.
///
/// `decision: None` clears the rule at this scope so the wider one applies
/// again — which is not the same as setting it to Ask.
#[tauri::command]
pub fn set_guardrail_policy(
    state: State<'_, AppState>,
    project_id: Option<String>,
    operation: Operation,
    decision: Option<Decision>,
) -> IpcResult<Vec<PolicyView>> {
    state
        .db
        .with(|conn| policy::set(conn, project_id.as_deref(), operation, decision, now_ms()))
        .map_err(|e| e.to_string())?;
    refresh_live_sessions(&state.db);
    policies(&state.db, project_id.as_deref()).map_err(|e| e.to_string())
}

/// Push changed policy out to every session that is already running.
///
/// Without this a rule would apply to the *next* agent, which is not what
/// anyone means when they change a safety setting.
fn refresh_live_sessions(db: &Database) {
    let dirs: Vec<String> = db
        .with(|conn| {
            let mut stmt =
                conn.prepare("SELECT log_dir FROM sessions WHERE ended_at IS NULL")?;
            let rows: rusqlite::Result<Vec<String>> =
                stmt.query_map([], |row| row.get(0))?.collect();
            rows
        })
        .unwrap_or_default();

    for dir in dirs {
        super::sessions::refresh(db, std::path::Path::new(&dir));
    }
}

#[tauri::command]
pub fn guardrail_events(
    state: State<'_, AppState>,
    project_id: Option<String>,
    mission_id: Option<String>,
    limit: Option<u32>,
) -> IpcResult<Vec<GuardrailEvent>> {
    super::list(
        &state.db,
        project_id.as_deref(),
        mission_id.as_deref(),
        limit.unwrap_or(100),
    )
    .map_err(|e| e.to_string())
}

/// Approvals waiting for a person.
#[tauri::command]
pub fn guardrail_pending(
    state: State<'_, AppState>,
    mission_id: Option<String>,
) -> IpcResult<Vec<GuardrailEvent>> {
    super::pending(&state.db, mission_id.as_deref()).map_err(|e| e.to_string())
}

/// The four answers §35 offers when a guardrail asks.
///
/// `AllowOnce` deliberately stores nothing: approving one command is not the
/// same as changing a policy, and conflating them is how safety settings drift
/// open one dialog at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Choice {
    AllowOnce,
    AllowForProject,
    AlwaysAllow,
    NeverAllow,
}

impl Choice {
    /// The reason code stored on the settled event, for the UI to localise.
    fn reason(self) -> &'static str {
        match self {
            Self::AllowOnce => "allowedOnce",
            Self::AllowForProject => "allowedForProject",
            Self::AlwaysAllow => "allowedAlways",
            Self::NeverAllow => "neverAllowed",
        }
    }
}

/// Answer a pending approval.
///
/// The check that was held is run **here**, as part of answering — not left for
/// the next verification pass. Someone who approves an operation means it to
/// happen now, and a one-time allowance that has to be remembered until the
/// next run would need bookkeeping whose only purpose is to be got wrong.
#[tauri::command]
pub fn decide_guardrail(
    state: State<'_, AppState>,
    event_id: String,
    choice: Choice,
    by: Option<String>,
) -> IpcResult<Option<crate::mission::model::MissionDetail>> {
    decide(&state, event_id, choice, by).map_err(|e| e.to_string())
}

fn decide(
    state: &State<'_, AppState>,
    event_id: String,
    choice: Choice,
    by: Option<String>,
) -> crate::mission::Result<Option<crate::mission::model::MissionDetail>> {
    let Some(event) = super::get(&state.db, &event_id)? else {
        return Ok(None);
    };
    if event.status != Status::Pending {
        // Already answered. Answering twice would run the command twice.
        return Ok(None);
    }
    let by = by.unwrap_or_else(|| "user".to_string());

    // Remember the answer, where the answer was meant to be remembered.
    let stored = match choice {
        Choice::AllowOnce => None,
        Choice::AllowForProject => Some((event.project_id.clone(), Decision::Allow)),
        Choice::AlwaysAllow => Some((None, Decision::Allow)),
        // Refusals are recorded against the project rather than everywhere: the
        // narrower reading of "never" is the one that can be widened later,
        // and the wider one is available in Settings.
        Choice::NeverAllow => Some((event.project_id.clone(), Decision::Deny)),
    };
    if let Some((scope, decision)) = stored {
        state.db.with(|conn| {
            policy::set(conn, scope.as_deref(), event.operation, Some(decision), now_ms())
        })?;
        refresh_live_sessions(&state.db);
    }

    let allowed = choice != Choice::NeverAllow;
    super::settle(
        &state.db,
        &event_id,
        if allowed { Status::Allowed } else { Status::Denied },
        &by,
        choice.reason(),
    )?;

    crate::activity::record(
        &state.db,
        if allowed { "guardrail.allowed" } else { "guardrail.denied" },
        crate::activity::Severity::Info,
        event.operation.as_str(),
        Some(event.command.clone()),
        event.project_id.as_deref(),
        event.session_id.as_deref(),
        event.mission_id.as_deref(),
    );

    // Only a held verification has something to run now.
    let Some(criterion_id) = event.criterion_id.as_deref() else {
        return Ok(None);
    };
    if allowed {
        crate::mission::store::verify_criterion(&state.db, criterion_id)?;
    } else {
        crate::mission::store::record_refusal(&state.db, criterion_id, event.operation.as_str())?;
    }

    let Some(mission_id) = event.mission_id.as_deref() else {
        return Ok(None);
    };
    release_if_nothing_is_waiting(&state.db, mission_id)?;
    crate::mission::store::detail(&state.db, mission_id).map(Some)
}

/// Return a mission to Ready once no approval is holding it.
///
/// Waiting means "needs a human decision" (§34). When the last decision has
/// been made, continuing to display Waiting would be the mission failing to
/// explain itself just as badly as saying nothing at all.
fn release_if_nothing_is_waiting(
    db: &Database,
    mission_id: &str,
) -> crate::mission::Result<()> {
    if !super::pending(db, Some(mission_id))?.is_empty() {
        return Ok(());
    }
    let current = crate::mission::store::detail(db, mission_id)?;
    if current.mission.status == crate::mission::model::MissionStatus::Waiting {
        crate::mission::store::set_status(
            db,
            mission_id,
            crate::mission::model::MissionStatus::Ready,
            None,
        )?;
    }
    Ok(())
}

/// What a command would trigger, without running anything.
///
/// Used by the UI to explain a rule with a real example rather than prose, and
/// worth having on its own: a guardrail nobody can interrogate is a guardrail
/// nobody trusts.
#[tauri::command]
pub fn guardrail_classify(command: String) -> Vec<classify::Match> {
    classify::classify(&command)
}
