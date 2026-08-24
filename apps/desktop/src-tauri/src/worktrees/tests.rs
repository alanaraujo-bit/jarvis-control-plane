//! Worktree tests, against real repositories and a real database (§80).
//!
//! `git::worktree` already pins the Git behaviour. What is tested here is the
//! part this module adds: that a worktree becomes a project, that the two
//! spellings of a path are recognised as one folder, and that a removal which
//! would take uncommitted work with it stops and asks.

use super::*;
use std::sync::Arc;

use crate::db::Database;
use crate::session::manager::SessionManager;

/// A real `AppState` over an in-memory database and a temporary data directory.
fn state(data_dir: &std::path::Path) -> AppState {
    AppState {
        db: Arc::new(Database::open_in_memory().unwrap()),
        sessions: SessionManager::default(),
        autopilots: crate::autopilot::driver::Autopilots::default(),
        voice: crate::voice::VoiceState::default(),
        attention: Arc::new(crate::notify::Attention::default()),
        data_dir: data_dir.to_path_buf(),
    }
}

/// A repository with one commit, opened as a project.
fn project(state: &AppState, root: &std::path::Path) -> crate::project::Project {
    git::run(root, &["init", "--initial-branch=main"]).unwrap();
    git::run(root, &["config", "user.email", "test@example.com"]).unwrap();
    git::run(root, &["config", "user.name", "Test"]).unwrap();
    git::run(root, &["config", "core.autocrlf", "false"]).unwrap();
    std::fs::write(root.join("README.md"), "# demo\n").unwrap();
    git::run(root, &["add", "."]).unwrap();
    git::run(root, &["commit", "-m", "initial"]).unwrap();
    crate::project::open(&state.db, root).unwrap()
}

/// A repository inside its own directory, so worktree siblings land in the
/// temporary tree rather than beside it.
fn workspace() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("app");
    std::fs::create_dir_all(&root).unwrap();
    (dir, root)
}

#[test]
fn a_created_worktree_becomes_a_project_that_knows_where_it_came_from() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);

    let child = create(&state, &parent.id, "feature", BranchMode::Create).unwrap();

    // A project in its own right: this is what makes Files, the editor and
    // Review work inside it without any of them knowing worktrees exist.
    assert!(child.is_git);
    assert_eq!(child.git_branch.as_deref(), Some("feature"));
    assert_ne!(child.id, parent.id);
    assert!(std::path::Path::new(&child.path).is_dir());

    let of: Option<String> = state
        .db
        .with(|conn| {
            Ok(conn.query_row(
                "SELECT worktree_of FROM projects WHERE id = ?1",
                [&child.id],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(of.as_deref(), Some(parent.id.as_str()));
}

/// The join that decides whether the list can recognise anything at all.
#[test]
fn a_worktree_is_matched_to_its_project_across_both_path_spellings() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);
    create(&state, &parent.id, "feature", BranchMode::Create).unwrap();

    let report = report(&state, &parent.id).unwrap();
    assert!(report.is_repo);
    assert_eq!(report.trees.len(), 2);

    let main = &report.trees[0];
    assert!(main.tree.is_main);
    assert!(main.is_current, "the open project is the main tree here");
    assert_eq!(main.project_id.as_deref(), Some(parent.id.as_str()));

    // Git reports forward slashes and the projects table stores what Windows
    // gave us. Comparing the raw strings matches nothing, and every worktree —
    // including the one being looked at — renders as one J.A.R.V.I.S. has never
    // seen.
    let child = &report.trees[1];
    assert_eq!(child.tree.branch.as_deref(), Some("feature"));
    assert!(
        child.project_id.is_some(),
        "a worktree we created must be recognised as a project"
    );
    assert!(!child.is_current);
}

/// A worktree made in a terminal is a real state, and it is shown rather than
/// hidden.
#[test]
fn a_worktree_git_knows_about_but_we_have_never_opened_has_no_project() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);

    // Created behind our back, the way a person would at a prompt.
    let outside = root.parent().unwrap().join("made-by-hand");
    git::run(
        &root,
        &["worktree", "add", "-b", "manual", &outside.to_string_lossy()],
    )
    .unwrap();

    let report = report(&state, &parent.id).unwrap();
    assert_eq!(report.trees.len(), 2);
    let unknown = report.trees.iter().find(|t| !t.tree.is_main).unwrap();
    assert_eq!(unknown.tree.branch.as_deref(), Some("manual"));
    assert_eq!(unknown.project_id, None);
}

#[test]
fn a_clean_worktree_is_removed_and_its_project_archived() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);
    let child = create(&state, &parent.id, "feature", BranchMode::Create).unwrap();

    let outcome = remove(&state, &parent.id, &child.path, false, None).unwrap();
    assert!(matches!(outcome, RemoveOutcome::Done));
    assert!(!std::path::Path::new(&child.path).is_dir());

    // Archived, not deleted: the sessions and activity recorded against it are
    // history.
    let archived: i64 = state
        .db
        .with(|conn| {
            Ok(conn.query_row(
                "SELECT archived FROM projects WHERE id = ?1",
                [&child.id],
                |row| row.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(archived, 1);
    assert!(!crate::project::list(&state.db)
        .unwrap()
        .iter()
        .any(|p| p.id == child.id));
}

/// The behaviour this module exists to get right.
///
/// Git refuses to remove a worktree with work in it. Passing `--force`
/// speculatively would make that protection vanish silently — the same mistake
/// as reaching for plain `git restore` in §44.
#[test]
fn a_worktree_with_work_in_it_stops_and_asks_rather_than_being_forced() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);
    let child = create(&state, &parent.id, "feature", BranchMode::Create).unwrap();
    std::fs::write(
        std::path::Path::new(&child.path).join("unsaved.txt"),
        "an hour of thinking\n",
    )
    .unwrap();

    // First attempt: Git says no, and we relay the question.
    let outcome = remove(&state, &parent.id, &child.path, false, None).unwrap();
    assert!(
        matches!(outcome, RemoveOutcome::HasWork { .. }),
        "expected the work to be noticed, got {outcome:?}"
    );
    assert!(std::path::Path::new(&child.path).is_dir(), "nothing removed");

    // Forcing is guarded, and nothing has been answered yet.
    let outcome = remove(&state, &parent.id, &child.path, true, None).unwrap();
    assert!(
        matches!(outcome, RemoveOutcome::NeedsApproval { .. }),
        "forcing must go through the guardrail, got {outcome:?}"
    );
    assert!(std::path::Path::new(&child.path).is_dir(), "still nothing removed");

    // Answered.
    let outcome = remove(
        &state,
        &parent.id,
        &child.path,
        true,
        Some(Choice::AllowOnce),
    )
    .unwrap();
    assert!(matches!(outcome, RemoveOutcome::Done));
    assert!(!std::path::Path::new(&child.path).is_dir());
}

/// A refusal is a refusal, whatever the webview sends afterwards.
#[test]
fn refusing_the_forced_removal_leaves_the_worktree_alone() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);
    let child = create(&state, &parent.id, "feature", BranchMode::Create).unwrap();
    std::fs::write(std::path::Path::new(&child.path).join("unsaved.txt"), "work\n").unwrap();

    let outcome = remove(
        &state,
        &parent.id,
        &child.path,
        true,
        Some(Choice::NeverAllow),
    )
    .unwrap();
    assert!(matches!(outcome, RemoveOutcome::Refused { .. }));
    assert!(std::path::Path::new(&child.path).is_dir());

    // And now the rule denies it, so claiming approval changes nothing.
    let outcome = remove(
        &state,
        &parent.id,
        &child.path,
        true,
        Some(Choice::AllowOnce),
    )
    .unwrap();
    assert!(
        matches!(outcome, RemoveOutcome::Refused { .. }),
        "a Deny is not overridable from the webview"
    );
    assert!(std::path::Path::new(&child.path).is_dir());
}

/// The repository is not a worktree you can remove, and the surface should
/// never offer it (§81).
#[test]
fn the_main_working_tree_is_not_removable() {
    let (guard, root) = workspace();
    let state = state(guard.path());
    let parent = project(&state, &root);

    let outcome = remove(&state, &parent.id, &parent.path, false, None).unwrap();
    assert!(matches!(outcome, RemoveOutcome::Failed { .. }));
    assert!(std::path::Path::new(&parent.path).is_dir());
}

#[test]
fn the_report_says_plainly_when_a_project_is_not_a_repository() {
    let guard = tempfile::tempdir().unwrap();
    let state = state(guard.path());
    let plain = guard.path().join("plain");
    std::fs::create_dir_all(&plain).unwrap();
    let project = crate::project::open(&state.db, &plain).unwrap();

    let report = report(&state, &project.id).unwrap();
    assert!(!report.is_repo);
    assert!(report.trees.is_empty());
}

#[test]
fn two_spellings_of_one_folder_are_one_folder() {
    assert!(same_folder(r"C:\Users\Alan\app", "C:/Users/Alan/app"));
    assert!(same_folder("C:/Users/Alan/app/", "C:/Users/Alan/app"));
    assert!(!same_folder("C:/Users/Alan/app", "C:/Users/Alan/app-feature"));
    #[cfg(windows)]
    assert!(same_folder(r"c:\users\alan\app", "C:/Users/Alan/app"));
}
