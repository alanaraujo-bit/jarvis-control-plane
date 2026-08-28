//! Durable presentation state for the multi-project workbench.
//!
//! Processes and their logs already live in the session core. This snapshot is
//! deliberately only the user's arrangement: which projects are docked and
//! which session/layout should be shown when a project is revisited.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppState;

const KEY: &str = "workspace.snapshot.v1";
const MAX_PROJECTS: usize = 32;
const MAX_SESSIONS_PER_PROJECT: usize = 64;
const MAX_PANES: usize = 4;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceSnapshot {
    pub open_project_ids: Vec<String>,
    pub active_project_id: Option<String>,
    pub projects: HashMap<String, ProjectWorkspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProjectWorkspace {
    pub area: String,
    pub view: String,
    pub active_session_id: Option<String>,
    pub session_order: Vec<String>,
    pub pane_session_ids: Vec<String>,
    pub split_direction: String,
}

impl Default for ProjectWorkspace {
    fn default() -> Self {
        Self {
            area: "sessions".into(),
            view: "terminal".into(),
            active_session_id: None,
            session_order: Vec::new(),
            pane_session_ids: Vec::new(),
            split_direction: "columns".into(),
        }
    }
}

impl WorkspaceSnapshot {
    fn normalise(mut self) -> Self {
        unique_bounded(&mut self.open_project_ids, MAX_PROJECTS);
        let open: HashSet<_> = self.open_project_ids.iter().cloned().collect();
        self.projects.retain(|id, _| open.contains(id));

        if self
            .active_project_id
            .as_ref()
            .is_some_and(|id| !open.contains(id))
        {
            self.active_project_id = None;
        }

        for project in self.projects.values_mut() {
            if !matches!(
                project.area.as_str(),
                "sessions" | "files" | "review" | "preview" | "worktrees" | "brain" | "settings"
            ) {
                project.area = "sessions".into();
            }
            if !matches!(project.view.as_str(), "terminal" | "conversation") {
                project.view = "terminal".into();
            }
            if !matches!(project.split_direction.as_str(), "columns" | "rows" | "grid") {
                project.split_direction = "columns".into();
            }
            unique_bounded(&mut project.session_order, MAX_SESSIONS_PER_PROJECT);
            unique_bounded(&mut project.pane_session_ids, MAX_PANES);
            project
                .pane_session_ids
                .retain(|id| project.session_order.contains(id));
            if project
                .active_session_id
                .as_ref()
                .is_some_and(|id| !project.session_order.contains(id))
            {
                project.active_session_id = project.session_order.first().cloned();
            }
            if project.pane_session_ids.len() == 1 {
                project.pane_session_ids.clear();
            }
        }
        self
    }
}

fn unique_bounded(values: &mut Vec<String>, maximum: usize) {
    let mut seen = HashSet::new();
    values.retain(|value| !value.is_empty() && seen.insert(value.clone()));
    values.truncate(maximum);
}

#[tauri::command]
pub fn workspace_snapshot(state: State<'_, AppState>) -> WorkspaceSnapshot {
    crate::settings::get_or(&state.db, KEY, WorkspaceSnapshot::default()).normalise()
}

#[tauri::command]
pub fn workspace_save(
    state: State<'_, AppState>,
    snapshot: WorkspaceSnapshot,
) -> Result<WorkspaceSnapshot, String> {
    let snapshot = snapshot.normalise();
    crate::settings::set(&state.db, KEY, &snapshot)?;
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_keeps_focus_inside_the_workspace() {
        let mut projects = HashMap::new();
        projects.insert(
            "a".into(),
            ProjectWorkspace {
                active_session_id: Some("missing".into()),
                session_order: vec!["s1".into(), "s1".into(), "s2".into()],
                pane_session_ids: vec!["missing".into(), "s2".into()],
                ..ProjectWorkspace::default()
            },
        );
        let snapshot = WorkspaceSnapshot {
            open_project_ids: vec!["a".into(), "a".into()],
            active_project_id: Some("gone".into()),
            projects,
        }
        .normalise();

        assert_eq!(snapshot.open_project_ids, vec!["a"]);
        assert_eq!(snapshot.active_project_id, None);
        let project = &snapshot.projects["a"];
        assert_eq!(project.session_order, vec!["s1", "s2"]);
        assert_eq!(project.active_session_id.as_deref(), Some("s1"));
        assert!(project.pane_session_ids.is_empty());
    }

    #[test]
    fn a_workspace_snapshot_survives_the_local_database_round_trip() {
        let db = crate::db::Database::open_in_memory().unwrap();
        let snapshot = WorkspaceSnapshot {
            open_project_ids: vec!["project-a".into(), "project-b".into()],
            active_project_id: Some("project-b".into()),
            projects: HashMap::from([(
                "project-b".into(),
                ProjectWorkspace {
                    active_session_id: Some("session-2".into()),
                    session_order: vec!["session-1".into(), "session-2".into()],
                    pane_session_ids: vec!["session-1".into(), "session-2".into()],
                    split_direction: "grid".into(),
                    ..ProjectWorkspace::default()
                },
            )]),
        };

        crate::settings::set(&db, KEY, &snapshot).unwrap();
        let restored: WorkspaceSnapshot = crate::settings::get(&db, KEY).unwrap();
        assert_eq!(restored, snapshot);
    }
}
