//! Autopilot commands exposed to the UI (§32).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::mission::model::{Autonomy, MissionStatus};
use crate::AppState;

type IpcResult<T> = std::result::Result<T, String>;

/// What the UI shows about a driven run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunStatus {
    pub session_id: String,
    pub mission_id: String,
    pub state: super::RunState,
    pub turns: u32,
    pub budget: u32,
}

/// Whether a mission is being driven right now.
#[tauri::command]
pub fn autopilot_status(
    state: State<'_, AppState>,
    mission_id: String,
) -> IpcResult<Option<RunStatus>> {
    Ok(state.autopilots.for_mission(&mission_id).map(|run| RunStatus {
        session_id: run.session_id.clone(),
        mission_id: run.mission_id.clone(),
        state: run.state(),
        turns: run.turns(),
        budget: super::plan::DEFAULT_TURN_BUDGET,
    }))
}

/// Start an agent and drive it towards a mission until it is done (§32).
///
/// Refuses unless the mission's effective autonomy is Unattended. Autonomy is
/// the user's statement about how much they want to be involved (§33), and
/// starting an unsupervised run on a mission set to Guided would override that
/// statement rather than honour it.
#[tauri::command]
pub fn autopilot_start(
    state: State<'_, AppState>,
    mission_id: String,
) -> IpcResult<RunStatus> {
    let detail = crate::mission::store::detail(&state.db, &mission_id)
        .map_err(|e| e.to_string())?;

    if detail.effective_autonomy != Autonomy::Unattended {
        return Err("autopilot.requiresUnattended".into());
    }
    if detail.mission.status.is_finished() {
        return Err("autopilot.alreadyFinished".into());
    }
    if state.autopilots.for_mission(&mission_id).is_some() {
        return Err("autopilot.alreadyRunning".into());
    }

    // A run starts from a clean statement of what is actually true, so the
    // first instruction is not based on a stale verification (§30).
    let detail = crate::mission::store::verify_mission(&state.db, &mission_id)
        .map_err(|e| e.to_string())?;

    // Guardrails may already be holding something. Starting an agent that
    // cannot act would be the resource-burning §34 forbids.
    let held = crate::guardrail::pending(&state.db, Some(&mission_id))
        .map_err(|e| e.to_string())?;
    if !held.is_empty() {
        return Err("autopilot.awaitingApproval".into());
    }

    let project_id = detail.mission.project_id.clone();

    // Claude Code asks "is this a project you trust?" the first time it opens a
    // folder, and waits. Under Guided or Autonomous someone answers it. Here
    // nobody is watching, so the run would sit on that dialog until its budget
    // ran out — the indefinite consumption §34 forbids, in the same shape as
    // D12's permission prompt but arriving before the agent has said a word.
    //
    // Not hypothetical: a worktree is a brand-new folder (§45), so "run this
    // mission unattended in a fresh worktree" is exactly the case that hangs.
    //
    // Refusing here rather than accepting trust on the user's behalf: that is a
    // security decision, and it is theirs to make in Claude Code's own
    // interface. An *unknown* answer proceeds — see `folder_is_trusted`.
    if let Ok(root) = crate::files::project_root(&state, &project_id) {
        if crate::providers::claude::folder_is_trusted(&root) == Some(false) {
            return Err("autopilot.folderNotTrusted".into());
        }
    }

    let started = crate::session::commands::start_agent_session(
        &state,
        &project_id,
        crate::session::commands::SessionKind::ClaudeCode,
        Some(mission_id.clone()),
        // Driven, so guardrails must treat this session as having nobody to
        // ask — see `AgentLaunch::driven`.
        true,
    )
    .map_err(|e| e.to_string())?;

    let _ = crate::mission::store::set_status(
        &state.db,
        &mission_id,
        MissionStatus::Running,
        None,
    );

    let run = super::start(
        Arc::clone(&started.session),
        Arc::clone(&state.db),
        state.session_dir(&started.id),
        mission_id.clone(),
        project_id,
    );
    state.autopilots.insert(Arc::clone(&run));

    Ok(RunStatus {
        session_id: run.session_id.clone(),
        mission_id,
        state: run.state(),
        turns: run.turns(),
        budget: super::plan::DEFAULT_TURN_BUDGET,
    })
}

/// Stop driving, leaving the session alive.
///
/// Taking over is the point: someone who stops an autopilot usually wants to
/// type the next thing themselves, and killing the agent would throw away the
/// context it has built up.
#[tauri::command]
pub fn autopilot_stop(state: State<'_, AppState>, mission_id: String) -> IpcResult<()> {
    if let Some(run) = state.autopilots.for_mission(&mission_id) {
        run.stop();
        // The seat is free, so a guardrail can put a question to whoever is
        // now watching (§35).
        crate::guardrail::sessions::set_driven(&state.session_dir(&run.session_id), false);
    }
    Ok(())
}
