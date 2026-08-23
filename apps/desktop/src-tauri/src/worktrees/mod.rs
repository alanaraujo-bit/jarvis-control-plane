//! Worktrees as projects (§45).
//!
//! ## The decision this module is
//!
//! A worktree is registered as **its own project**. That is the whole design,
//! and it is what makes §45 small.
//!
//! A project in this product is a folder on the machine with a checkout in it
//! (§16). A worktree is a folder on the machine with a checkout in it. They are
//! the same thing, and the alternative — a worktree as a *view* inside one
//! project — would mean teaching `files::project_root` which tree it is looking
//! at, splitting `file_changes` attribution across trees, and reworking the
//! path confinement that §41 calls the security boundary. All to describe
//! something the filesystem already describes.
//!
//! This only works because of a fact that was checked rather than assumed:
//! `rev-parse --show-toplevel` inside a worktree returns the **worktree's own**
//! path. So `git::locate` answers about the worktree, and Files, the editor,
//! Review, attribution, sessions and guardrails all work inside one with no
//! changes at all. Had it returned the main repository instead, opening a
//! worktree would have shown the wrong tree's files and the wrong tree's diff —
//! which is why there is a test pinning it in `git::worktree`.
//!
//! `projects.worktree_of` records where it came from, so the list can say what
//! a folder is instead of showing an unexplained sibling.
//!
//! ## What is guarded
//!
//! Creating a worktree destroys nothing. Removing a **clean** one destroys
//! nothing either — everything in it is committed, which is exactly why Git
//! removes it without argument.
//!
//! Removing a **dirty** one needs `--force` and takes uncommitted work with it,
//! so that goes through the guardrail as `fs.recursive-delete`: it removes a
//! directory tree, which is what that rule already says it governs. A third
//! operation class for it would be a rule nobody could tell apart from the one
//! above it.
//!
//! And the refusal is Git's, not ours. The removal is attempted without
//! `--force` first; only when Git says the tree has work in it does the product
//! offer to force past that. Passing `--force` speculatively would make Git's
//! protection disappear silently, which is the same mistake as reaching for
//! plain `git restore` in §44.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::files::{self, FileError};
use crate::git::{
    self,
    worktree::{BranchMode, Worktree},
};
use crate::guardrail::{
    classify::Operation,
    commands::Choice,
    surface::{self, Verdict},
};
use crate::AppState;

pub type Result<T> = std::result::Result<T, FileError>;

/// One worktree, plus what J.A.R.V.I.S. knows about it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeView {
    #[serde(flatten)]
    pub tree: Worktree,
    /// The project row for this checkout, when there is one. `None` means Git
    /// knows about the worktree and J.A.R.V.I.S. has never opened it — a real
    /// state, and one worth showing rather than hiding: it is what a worktree
    /// created in a terminal looks like.
    pub project_id: Option<String>,
    /// This is the project currently open.
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeReport {
    pub is_repo: bool,
    pub trees: Vec<WorktreeView>,
}

/// Every worktree of the project's repository.
pub fn report(state: &AppState, project_id: &str) -> Result<WorktreeReport> {
    let root = files::project_root(state, project_id)?;
    let Some(location) = git::locate(&root) else {
        return Ok(WorktreeReport {
            is_repo: false,
            trees: Vec::new(),
        });
    };

    let trees = git::worktree::list(&location.root).unwrap_or_default();

    // One query rather than one per tree: the paths are already in hand.
    let known: Vec<(String, String)> = state.db.with(|conn| {
        let mut stmt = conn.prepare("SELECT id, path FROM projects")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let views = trees
        .into_iter()
        .map(|tree| {
            let matched = known
                .iter()
                .find(|(_, path)| same_folder(path, &tree.path))
                .map(|(id, _)| id.clone());
            WorktreeView {
                is_current: matched.as_deref() == Some(project_id),
                project_id: matched,
                tree,
            }
        })
        .collect();

    Ok(WorktreeReport {
        is_repo: true,
        trees: views,
    })
}

/// Whether two spellings name the same folder.
///
/// Git reports forward slashes; the projects table stores what the platform
/// gave us, which on Windows is backslashes. Comparing the two raw strings
/// matches nothing, which would render every worktree as one J.A.R.V.I.S. has
/// never seen — including the project the user is looking at. Case is folded
/// for the same reason `files::contains` folds it: on Windows these are one
/// folder.
fn same_folder(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        #[cfg(windows)]
        {
            s.to_lowercase()
        }
        #[cfg(not(windows))]
        {
            s
        }
    };
    norm(a) == norm(b)
}

/// Create a worktree and register it as a project.
///
/// The **branch** is what the caller names. The directory is derived from it in
/// `git::worktree::location_for` — this is the one operation in the product
/// that deliberately writes outside a project root, and a renderer that could
/// choose that location would have the arbitrary directory creation §41 exists
/// to prevent.
pub fn create(
    state: &AppState,
    project_id: &str,
    branch: &str,
    mode: BranchMode,
) -> Result<crate::project::Project> {
    let root = files::project_root(state, project_id)?;
    let Some(location) = git::locate(&root) else {
        return Err(FileError::NoProject);
    };

    let branch = branch.trim();
    if branch.is_empty() {
        return Err(FileError::Io("a worktree needs a branch name".into()));
    }

    let created = git::worktree::add(&location.root, branch, mode)
        .map_err(|e| FileError::Io(e.to_string()))?;

    // Registering it is what makes every other surface work in it.
    let project = crate::project::open(&state.db, &created)
        .map_err(|e| FileError::Io(e.to_string()))?;

    let (child, parent) = (project.id.clone(), project_id.to_string());
    state.db.with(move |conn| {
        conn.execute(
            "UPDATE projects SET worktree_of = ?2 WHERE id = ?1",
            rusqlite::params![child, parent],
        )?;
        Ok(())
    })?;

    crate::activity::record(
        &state.db,
        "git.worktreeAdded",
        crate::activity::Severity::Info,
        branch,
        Some(created.to_string_lossy().to_string()),
        Some(project_id),
        None,
        None,
    );

    Ok(project)
}

/// What happened when a removal was attempted.
///
/// Internally tagged, with `rename_all_fields` — see the note on
/// `review::actions::ActionOutcome` for why that second attribute is not
/// optional.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum RemoveOutcome {
    Done,
    /// Git refused because the worktree has uncommitted work in it. Nothing has
    /// been removed. Forcing past this is a guarded operation, so the surface
    /// asks before offering it.
    HasWork {
        command: String,
    },
    /// A rule says ask before removing a worktree with work in it.
    NeedsApproval {
        operation: Operation,
        command: String,
    },
    Refused {
        operation: Operation,
        reason: String,
    },
    /// Git refused for some other reason — locked, missing, not a worktree.
    /// Reported verbatim rather than forced past.
    Failed {
        message: String,
    },
}

/// Remove a worktree.
///
/// `force` is only ever set by the surface after the person has been told the
/// tree has work in it. Without it this attempts the plain removal, which Git
/// performs only when there is nothing to lose.
pub fn remove(
    state: &AppState,
    project_id: &str,
    path: &str,
    force: bool,
    choice: Option<Choice>,
) -> Result<RemoveOutcome> {
    let root = files::project_root(state, project_id)?;
    let Some(location) = git::locate(&root) else {
        return Err(FileError::NoProject);
    };

    // The main working tree is the repository. Git refuses, but saying so here
    // means the surface never offers it in the first place (§81).
    let trees = git::worktree::list(&location.root).unwrap_or_default();
    let target = trees.iter().find(|t| same_folder(&t.path, path));
    if target.map(|t| t.is_main).unwrap_or(false) {
        return Ok(RemoveOutcome::Failed {
            message: "the main working tree is the repository itself".into(),
        });
    }

    let command = format!(
        "git worktree remove {}{path}",
        if force { "--force " } else { "" }
    );

    if force {
        // Removing a tree with work in it takes that work with it. Same class
        // as any other directory removal, and the same gate §44 uses.
        match surface::gate(
            &state.db,
            project_id,
            None,
            Operation::RecursiveDelete,
            &command,
            choice,
        )? {
            Verdict::Proceed => {}
            Verdict::NeedsApproval(gate) => {
                return Ok(RemoveOutcome::NeedsApproval {
                    operation: gate.operation,
                    command,
                })
            }
            Verdict::Refused { gate, reason } => {
                return Ok(RemoveOutcome::Refused {
                    operation: gate.operation,
                    reason: reason.to_string(),
                })
            }
        }
    }

    match git::worktree::remove(&location.root, path, force) {
        Ok(()) => {
            archive_the_project_for(state, path)?;
            crate::activity::record(
                &state.db,
                "git.worktreeRemoved",
                crate::activity::Severity::Warning,
                target.and_then(|t| t.branch.clone()).unwrap_or_else(|| path.to_string()).as_str(),
                Some(command),
                Some(project_id),
                None,
                None,
            );
            Ok(RemoveOutcome::Done)
        }
        // Not an error to report — a question to ask. Git is telling us there
        // is work in there, which is the one thing worth stopping for.
        Err(e) if git::worktree::is_dirty_refusal(&e) => Ok(RemoveOutcome::HasWork {
            command: format!("git worktree remove --force {path}"),
        }),
        Err(e) => Ok(RemoveOutcome::Failed {
            message: e.to_string(),
        }),
    }
}

/// The folder is gone, so its project row should not go on offering to open it.
///
/// Archived rather than deleted, like every other project (§35) — the sessions,
/// missions and activity recorded against it are history, and history is not
/// removed because a directory was.
fn archive_the_project_for(state: &AppState, path: &str) -> Result<()> {
    let rows: Vec<(String, String)> = state.db.with(|conn| {
        let mut stmt = conn.prepare("SELECT id, path FROM projects WHERE archived = 0")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    let Some((id, _)) = rows.into_iter().find(|(_, p)| same_folder(p, path)) else {
        return Ok(());
    };
    state.db.with(move |conn| {
        conn.execute("UPDATE projects SET archived = 1 WHERE id = ?1", [&id])?;
        Ok(())
    })?;
    Ok(())
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
pub fn worktree_report(state: State<'_, AppState>, project_id: String) -> Result<WorktreeReport> {
    report(&state, &project_id)
}

#[tauri::command]
pub fn worktree_create(
    state: State<'_, AppState>,
    project_id: String,
    branch: String,
    mode: BranchMode,
) -> Result<crate::project::Project> {
    create(&state, &project_id, &branch, mode)
}

#[tauri::command]
pub fn worktree_remove(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    force: Option<bool>,
    choice: Option<Choice>,
) -> Result<RemoveOutcome> {
    remove(&state, &project_id, &path, force.unwrap_or(false), choice)
}

#[cfg(test)]
mod tests;
