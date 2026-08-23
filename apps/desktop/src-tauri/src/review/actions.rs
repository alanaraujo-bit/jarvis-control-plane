//! Git operations from the Review surface (§44).
//!
//! Review used to read and nothing else, and the reason was never that writing
//! is hard — it is that discarding a change is the product destroying a
//! person's work on their behalf, and D11 says that goes through the guardrail
//! rather than sitting behind a plain button.
//!
//! ## What is guarded and what is not
//!
//! | Action | Guarded | Why |
//! |---|---|---|
//! | Stage | no | The file is untouched; the index is a scratchpad. |
//! | Unstage | no | Same, in reverse. |
//! | Discard | **yes** | `git.discard-changes`. Nothing anywhere can undo it. |
//! | Commit | no | Adds to history rather than removing from it. |
//!
//! Making stage ask would be worse than useless: a guardrail that interrupts
//! harmless work is one the user switches off, and then the discard is
//! unguarded too. The classifier's test table is weighted towards the negative
//! cases for the same reason.
//!
//! ## Discard and restore are one operation with two honest names
//!
//! Returning a **deleted** file to its committed state recovers work rather
//! than destroying it, and it is tempting to leave that ungated. It is not
//! carved out, for a reason found by looking at real statuses: a file can be
//! deleted in the working tree while carrying staged modifications (`AD`, `MD`),
//! and returning *that* to `HEAD` throws the staged work away. The carve-out
//! would be right in the common case and quietly wrong in the one that costs
//! something. So the operation is gated uniformly and the **surface** changes
//! the word — see `review.discard` and `review.restore` in the catalogue.

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::files::{self, FileError};
use crate::git::{self, status::ChangeKind, write::Target};
use crate::guardrail::{
    classify::Operation,
    commands::Choice,
    surface::{self, Gate, Verdict},
};
use crate::AppState;

pub type Result<T> = std::result::Result<T, FileError>;

/// What the surface is asking for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GitAction {
    Stage,
    Unstage,
    /// Return the file to its committed state, whatever that costs.
    Discard,
}

impl GitAction {
    /// The operation class this action performs, if any.
    ///
    /// `None` is not "unclassified" — it is the positive statement that this
    /// action changes nothing that cannot be changed back.
    fn operation(self) -> Option<Operation> {
        match self {
            Self::Stage | Self::Unstage => None,
            Self::Discard => Some(Operation::GitDiscardChanges),
        }
    }
}

/// One file the action should apply to, as the webview names it.
///
/// Project-relative, exactly the key the file tree and `ReviewFile` use.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTarget {
    pub path: String,
    pub from_path: Option<String>,
    pub kind: ChangeKind,
}

/// What became of the request.
///
/// Internally tagged so the webview reads one `status` field.
///
/// **`rename_all_fields` is not decoration.** `rename_all = "camelCase"` on an
/// enum renames the *variants* and leaves the fields of its struct variants in
/// snake_case — which is precisely how `WriteOutcome` shipped `modified_ms` to
/// a webview reading `modifiedMs`, breaking the editor's conflict check with
/// every test still passing. `an_outcome_serialises_with_the_field_names_the_webview_reads`
/// pins this one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum ActionOutcome {
    /// It happened.
    Done,
    /// A rule says ask. **Nothing has been done**, and nothing was recorded —
    /// the surface should put the §35 choices to the person and call back.
    NeedsApproval {
        operation: Operation,
        /// The exact command that will run if they say yes. Shown verbatim:
        /// approving a paraphrase is not approving anything.
        command: String,
    },
    /// A rule refuses. The refusal is in the guardrail history.
    Refused {
        operation: Operation,
        /// A stable code the UI localises (§65), never prose.
        reason: String,
    },
}

/// Turn the webview's targets into repository-relative ones.
///
/// Every path goes through `files::resolve` first. Review is not allowed to be
/// the way around the project boundary (§41) just because Git is the thing that
/// will open the file — and a discard reaching outside the project would delete
/// something the user never showed us.
///
/// A deleted file no longer exists, so its *own* resolution cannot be checked
/// against the filesystem; `resolve` still rejects `..`, absolute and
/// drive-qualified paths, which is what stops a crafted request.
fn to_repo_targets(
    root: &std::path::Path,
    location: &git::RepoLocation,
    targets: &[ActionTarget],
) -> Result<Vec<Target>> {
    targets
        .iter()
        .map(|target| {
            check_confined(root, &target.path)?;
            if let Some(from) = &target.from_path {
                check_confined(root, from)?;
            }
            Ok(Target {
                path: location.to_repo(&target.path),
                from_path: target.from_path.as_deref().map(|f| location.to_repo(f)),
                kind: target.kind,
            })
        })
        .collect()
}

/// The path is inside the project, whether or not it exists right now.
fn check_confined(root: &std::path::Path, relative: &str) -> Result<()> {
    match files::resolve(root, relative) {
        Ok(_) => Ok(()),
        // A file that has been deleted is exactly what a restore is for, so
        // "not found" is not a boundary failure. `Outside` still is.
        Err(FileError::NotFound(_)) => Ok(()),
        Err(other) => Err(other),
    }
}

/// The command that will be run, for the record and for the confirmation.
///
/// Built from the same pieces `git::write` uses so that what the person
/// approves is what executes. It is a description, not the thing that runs —
/// `git::write` passes arguments to the process directly and never through a
/// shell, so no quoting here can change what happens.
fn describe(action: GitAction, targets: &[Target], has_commits: bool) -> String {
    let paths: Vec<&str> = targets
        .iter()
        .flat_map(|t| match &t.from_path {
            Some(from) => vec![t.path.as_str(), from.as_str()],
            None => vec![t.path.as_str()],
        })
        .collect();
    let joined = paths.join(" ");

    match action {
        GitAction::Stage => format!("git add -- {joined}"),
        GitAction::Unstage if has_commits => format!("git restore --staged -- {joined}"),
        GitAction::Unstage => format!("git rm --cached -- {joined}"),
        GitAction::Discard => {
            let untracked: Vec<&str> = targets
                .iter()
                .filter(|t| t.kind == ChangeKind::Untracked)
                .map(|t| t.path.as_str())
                .collect();
            // An untracked file has no committed version, so the honest
            // description of discarding it is a removal, not a restore.
            if untracked.len() == targets.len() && !untracked.is_empty() {
                format!("git clean -f -- {}", untracked.join(" "))
            } else if has_commits {
                format!("git restore --source=HEAD --staged --worktree -- {joined}")
            } else {
                format!("git rm --cached -- {joined} && git clean -f -- {joined}")
            }
        }
    }
}

/// Perform a Git action on some files, asking the guardrail first.
pub fn act(
    state: &AppState,
    project_id: &str,
    action: GitAction,
    targets: &[ActionTarget],
    choice: Option<Choice>,
) -> Result<ActionOutcome> {
    let root = files::project_root(state, project_id)?;
    let Some(location) = git::locate(&root) else {
        return Err(FileError::NoProject);
    };
    if targets.is_empty() {
        return Ok(ActionOutcome::Done);
    }

    let repo_targets = to_repo_targets(&root, &location, targets)?;
    let has_commits = git::status::has_commits(&location.root);
    let command = describe(action, &repo_targets, has_commits);

    if let Some(operation) = action.operation() {
        match surface::gate(
            &state.db,
            project_id,
            None,
            operation,
            &command,
            choice,
        )? {
            Verdict::Proceed => {}
            Verdict::NeedsApproval(Gate { operation, .. }) => {
                return Ok(ActionOutcome::NeedsApproval { operation, command })
            }
            Verdict::Refused { gate, reason } => {
                return Ok(ActionOutcome::Refused {
                    operation: gate.operation,
                    reason: reason.to_string(),
                })
            }
        }
    }

    match action {
        GitAction::Stage => git::write::stage(&location.root, &repo_targets),
        GitAction::Unstage => git::write::unstage(&location.root, &repo_targets, has_commits),
        GitAction::Discard => git::write::discard(&location.root, &repo_targets, has_commits),
    }
    .map_err(|e| FileError::Io(e.to_string()))?;

    // Recorded whether or not a guardrail spoke. The guardrail log is the
    // record of a *rule* having applied; the activity log is the record of
    // something having happened, and a discard is a moment worth knowing about
    // however freely it was permitted (§48).
    crate::activity::record(
        &state.db,
        match action {
            GitAction::Stage => "git.staged",
            GitAction::Unstage => "git.unstaged",
            GitAction::Discard => "git.discarded",
        },
        match action {
            // A discard is not routine, and the log should not read as if it is.
            GitAction::Discard => crate::activity::Severity::Warning,
            _ => crate::activity::Severity::Info,
        },
        &summarise(targets),
        Some(command),
        Some(project_id),
        None,
        None,
    );

    Ok(ActionOutcome::Done)
}

/// A short subject line for the activity entry.
fn summarise(targets: &[ActionTarget]) -> String {
    match targets {
        [only] => only.path.clone(),
        many => format!("{} files", many.len()),
    }
}

/// Commit what is staged.
///
/// Not guarded: a commit adds to history rather than taking anything out of it,
/// and it is the one operation here that makes everything else recoverable.
pub fn commit(state: &AppState, project_id: &str, message: &str) -> Result<()> {
    let root = files::project_root(state, project_id)?;
    let Some(location) = git::locate(&root) else {
        return Err(FileError::NoProject);
    };

    let message = message.trim();
    if message.is_empty() {
        return Err(FileError::Io("a commit needs a message".into()));
    }

    git::write::commit(&location.root, message).map_err(|e| FileError::Io(e.to_string()))?;

    crate::activity::record(
        &state.db,
        "git.committed",
        crate::activity::Severity::Info,
        message,
        None,
        Some(project_id),
        None,
        None,
    );
    Ok(())
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
pub fn review_git_action(
    state: State<'_, AppState>,
    project_id: String,
    action: GitAction,
    targets: Vec<ActionTarget>,
    choice: Option<Choice>,
) -> Result<ActionOutcome> {
    act(&state, &project_id, action, &targets, choice)
}

#[tauri::command]
pub fn review_commit(
    state: State<'_, AppState>,
    project_id: String,
    message: String,
) -> Result<()> {
    commit(&state, &project_id, &message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(path: &str, kind: ChangeKind) -> Target {
        Target {
            path: path.to_string(),
            from_path: None,
            kind,
        }
    }

    /// Check the **bytes**, not the Rust type (D13, one layer down).
    ///
    /// `#[serde(rename_all = "camelCase")]` on an enum renames the variants and
    /// leaves the fields of its struct variants untouched. That exact mistake
    /// shipped `{"status":"written","modified_ms":…}` to a webview reading
    /// `modifiedMs`, which silently disabled the editor's overwrite protection
    /// after the first save while every test passed. Nothing about the Rust
    /// side looks wrong when it happens, so the assertion has to be on the JSON.
    #[test]
    fn an_outcome_serialises_with_the_field_names_the_webview_reads() {
        let json = serde_json::to_string(&ActionOutcome::NeedsApproval {
            operation: Operation::GitDiscardChanges,
            command: "git restore -- a.txt".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"status":"needsApproval","operation":"git.discard-changes","command":"git restore -- a.txt"}"#
        );

        let json = serde_json::to_string(&ActionOutcome::Refused {
            operation: Operation::GitDiscardChanges,
            reason: "policyDenies".into(),
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"status":"refused","operation":"git.discard-changes","reason":"policyDenies"}"#
        );

        assert_eq!(
            serde_json::to_string(&ActionOutcome::Done).unwrap(),
            r#"{"status":"done"}"#
        );
    }

    #[test]
    fn an_action_serialises_as_the_name_the_webview_sends() {
        for (action, expected) in [
            (GitAction::Stage, "\"stage\""),
            (GitAction::Unstage, "\"unstage\""),
            (GitAction::Discard, "\"discard\""),
        ] {
            assert_eq!(serde_json::to_string(&action).unwrap(), expected);
            assert_eq!(
                serde_json::from_str::<GitAction>(expected).unwrap(),
                action
            );
        }
    }

    /// Only the destructive one is gated, and it is gated by name.
    #[test]
    fn staging_states_that_it_is_reversible_rather_than_failing_to_match() {
        assert_eq!(GitAction::Stage.operation(), None);
        assert_eq!(GitAction::Unstage.operation(), None);
        assert_eq!(
            GitAction::Discard.operation(),
            Some(Operation::GitDiscardChanges)
        );
    }

    /// What the person approves has to be what runs.
    #[test]
    fn the_description_matches_the_command_that_will_be_run() {
        let modified = [target("src/app.ts", ChangeKind::Modified)];
        assert_eq!(
            describe(GitAction::Discard, &modified, true),
            "git restore --source=HEAD --staged --worktree -- src/app.ts"
        );
        assert_eq!(
            describe(GitAction::Unstage, &modified, true),
            "git restore --staged -- src/app.ts"
        );
        assert_eq!(
            describe(GitAction::Stage, &modified, false),
            "git add -- src/app.ts"
        );

        // An untracked file has nothing committed to return to, so describing
        // the discard as a "restore" would be a plain untruth about what is
        // going to happen to it.
        let untracked = [target("notes.md", ChangeKind::Untracked)];
        assert_eq!(
            describe(GitAction::Discard, &untracked, true),
            "git clean -f -- notes.md"
        );

        // With no commits there is no HEAD, and the description says so.
        assert_eq!(
            describe(GitAction::Unstage, &modified, false),
            "git rm --cached -- src/app.ts"
        );
    }

    /// A rename is one change with two names, and the confirmation must show
    /// both — the old path is the one that comes back.
    #[test]
    fn a_renamed_file_is_described_with_both_of_its_names() {
        let renamed = [Target {
            path: "new.txt".into(),
            from_path: Some("old.txt".into()),
            kind: ChangeKind::Renamed,
        }];
        assert_eq!(
            describe(GitAction::Discard, &renamed, true),
            "git restore --source=HEAD --staged --worktree -- new.txt old.txt"
        );
    }

    /// The project boundary is not optional just because Git is the thing that
    /// will open the file (§41).
    #[test]
    fn a_path_outside_the_project_is_refused_before_anything_runs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let location = git::RepoLocation {
            root: root.to_path_buf(),
            prefix: String::new(),
        };

        for escape in ["../secrets.txt", "..\\secrets.txt", "C:/Windows/system32/x"] {
            let targets = vec![ActionTarget {
                path: escape.to_string(),
                from_path: None,
                kind: ChangeKind::Modified,
            }];
            assert!(
                matches!(
                    to_repo_targets(root, &location, &targets),
                    Err(FileError::Outside)
                ),
                "{escape} must not be reachable"
            );
        }
    }

    /// A deleted file cannot be resolved against the filesystem, and restoring
    /// one is the whole point of the button. "Missing" must not read as
    /// "outside".
    #[test]
    fn a_deleted_file_is_still_an_allowed_target() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let location = git::RepoLocation {
            root: root.to_path_buf(),
            prefix: String::new(),
        };
        let targets = vec![ActionTarget {
            path: "gone.txt".into(),
            from_path: None,
            kind: ChangeKind::Deleted,
        }];
        let repo = to_repo_targets(root, &location, &targets).unwrap();
        assert_eq!(repo[0].path, "gone.txt");
    }

    /// A project that is a subdirectory of its repository has to have its
    /// prefix folded in, or the action would name a path Git does not have.
    #[test]
    fn a_project_below_the_repository_root_is_translated_into_repository_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let location = git::RepoLocation {
            root: root.join("repo"),
            prefix: "apps/desktop/".into(),
        };
        let targets = vec![ActionTarget {
            path: "src/app.ts".into(),
            from_path: None,
            kind: ChangeKind::Modified,
        }];
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/app.ts"), "x").unwrap();

        let repo = to_repo_targets(root, &location, &targets).unwrap();
        assert_eq!(repo[0].path, "apps/desktop/src/app.ts");
    }
}
