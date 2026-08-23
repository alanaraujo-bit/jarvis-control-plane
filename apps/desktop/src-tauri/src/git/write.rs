//! Git operations that change something (§44).
//!
//! Everything here is a command the user's own `git` would run, spelled the way
//! they would spell it (D5). Nothing is emulated: staging is `git add`, not a
//! hand-written index update.
//!
//! ## The spellings, and why they are these ones
//!
//! Each was checked against real Git 2.55 in a scratch repository before it was
//! written down, because the obvious spelling is wrong in three of the five
//! cases and wrong **silently**:
//!
//! - **Discard** is `restore --source=HEAD --staged --worktree`. Plain
//!   `git restore <path>` restores the worktree *from the index*, so a file
//!   with staged content comes back as the staged version rather than as the
//!   committed one — the button would say "discard my changes" and keep half of
//!   them, with no error and nothing on screen to show it. The long form also
//!   happens to be the only spelling that handles a **staged deletion**: plain
//!   `git restore` fails there outright with `pathspec did not match any
//!   file(s) known to git`.
//! - **Unstage** is `restore --staged`, except in a repository with no commits,
//!   where it fails with `could not resolve 'HEAD'` — there is no HEAD to reset
//!   the index to. `git rm --cached` is what works there.
//! - **Discarding an untracked file** is not a restore at all. There is no
//!   committed version to return to, so it is `git clean -f -- <path>`.
//!
//! ## Renames need both names
//!
//! The same lesson `git::diff::against_head` learned: a rename is one change
//! with two paths, and a command naming only one of them does half the job.
//! Discarding a staged rename with only the new path leaves the old path
//! deleted. Verified: with both names, the tree comes back clean.
//!
//! ## Paths
//!
//! Every path reaching here is repository-relative and has already been through
//! `files::resolve`, which is the project boundary (§41). Every command puts
//! its paths after `--` so a file whose name begins with `-` is a path and
//! never an option.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::status::ChangeKind;
use super::{run, Result};

/// One file a Git operation should act on.
///
/// `from_path` matters for exactly one reason and it is not cosmetic — see the
/// module note on renames.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    /// Repository-relative, forward slashes, as Git spells it.
    pub path: String,
    pub from_path: Option<String>,
    pub kind: ChangeKind,
}

impl Target {
    /// Every path this target occupies in the repository.
    fn paths(&self) -> Vec<&str> {
        match &self.from_path {
            Some(from) => vec![self.path.as_str(), from.as_str()],
            None => vec![self.path.as_str()],
        }
    }
}

/// Build an argument list ending in `--` and then the paths.
fn with_paths<'a>(leading: &[&'a str], paths: &[&'a str]) -> Vec<&'a str> {
    let mut args: Vec<&str> = leading.to_vec();
    args.push("--");
    args.extend_from_slice(paths);
    args
}

/// Collect every repository path the targets occupy, de-duplicated.
fn all_paths(targets: &[Target]) -> Vec<&str> {
    let mut paths: Vec<&str> = targets.iter().flat_map(Target::paths).collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// Add the targets to the index.
///
/// `git add` covers every case on its own, including a deletion — verified,
/// because the intuition that a removed file needs `git rm` is a common one and
/// it is wrong on modern Git: `git add -- <deleted path>` stages the deletion.
pub fn stage(root: &Path, targets: &[Target]) -> Result<()> {
    let paths = all_paths(targets);
    if paths.is_empty() {
        return Ok(());
    }
    run(root, &with_paths(&["add"], &paths)).map(|_| ())
}

/// Take the targets back out of the index, leaving the files alone.
///
/// `has_commits` is passed in rather than probed here so a caller acting on
/// several files asks Git once. See the module note for what happens without
/// it.
pub fn unstage(root: &Path, targets: &[Target], has_commits: bool) -> Result<()> {
    let paths = all_paths(targets);
    if paths.is_empty() {
        return Ok(());
    }
    if has_commits {
        run(root, &with_paths(&["restore", "--staged"], &paths)).map(|_| ())
    } else {
        // No HEAD to reset the index entry to, so the entry is removed instead.
        // `--cached` is what keeps the file on disk; without it this deletes the
        // user's work, which is the opposite of unstaging.
        run(root, &with_paths(&["rm", "--cached", "-q"], &paths)).map(|_| ())
    }
}

/// Return the targets to their committed state, throwing away everything
/// uncommitted.
///
/// Three code paths, because Git genuinely has three cases here and merging
/// them would break two of them:
///
/// - A file Git has never seen has no committed state to return to, so it is
///   removed.
/// - With no commits at all there is no `HEAD`, so a tracked file can only be
///   un-tracked and then removed.
/// - Otherwise, back to `HEAD`, index and worktree together.
pub fn discard(root: &Path, targets: &[Target], has_commits: bool) -> Result<()> {
    let (untracked, tracked): (Vec<&Target>, Vec<&Target>) = targets
        .iter()
        .partition(|t| t.kind == ChangeKind::Untracked);

    if !untracked.is_empty() {
        let paths: Vec<&str> = untracked.iter().flat_map(|t| t.paths()).collect();
        // `-f` is required for clean to do anything at all; `-d` is deliberately
        // not passed, so an empty directory left behind stays behind rather than
        // widening this into a directory removal.
        run(root, &with_paths(&["clean", "-f", "-q"], &paths))?;
    }

    if !tracked.is_empty() {
        let paths: Vec<&str> = tracked.iter().flat_map(|t| t.paths()).collect();
        if has_commits {
            run(
                root,
                &with_paths(
                    &["restore", "--source=HEAD", "--staged", "--worktree"],
                    &paths,
                ),
            )?;
        } else {
            // Tracked but never committed: forget it, then remove it. In that
            // order — after `rm --cached` the file is untracked, which is what
            // `clean` is able to act on.
            run(root, &with_paths(&["rm", "--cached", "-q"], &paths))?;
            run(root, &with_paths(&["clean", "-f", "-q"], &paths))?;
        }
    }

    Ok(())
}

/// Commit what is in the index.
///
/// `--only` is not passed and neither is `-a`: what gets committed is exactly
/// what the user staged, which is the whole point of showing them a staged
/// section. The message goes through stdin-free `-m` because a commit message
/// is not a path and cannot be confused for one.
///
/// Hooks run. That is deliberate (D5): this is the user's repository and their
/// `pre-commit` is part of how their repository works. A hook that fails fails
/// the commit, and the error text is theirs to read.
pub fn commit(root: &Path, message: &str) -> Result<String> {
    run(root, &["commit", "-m", message])
}

/// Whether anything is staged, for deciding if a commit is possible.
pub fn has_staged_changes(root: &Path) -> bool {
    // `diff --cached --quiet` exits 1 when there *is* a difference, which makes
    // an error the positive answer here.
    run(root, &["diff", "--cached", "--quiet"]).is_err()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real repository, because everything this module claims is a claim
    /// about what the `git` binary does (§80).
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        run(path, &["init", "--initial-branch=main"]).unwrap();
        run(path, &["config", "user.email", "test@example.com"]).unwrap();
        run(path, &["config", "user.name", "Test"]).unwrap();
        // This machine has `core.autocrlf=true` globally, so a checkout rewrites
        // LF to CRLF and an assertion on file contents fails for a reason that
        // has nothing to do with the operation under test. Pinned here rather
        // than worked around in `discard`: the product deliberately does **not**
        // override the user's Git configuration (D5), so `git restore` writing
        // CRLF on a machine configured that way is correct behaviour and
        // exactly what the user's own `git restore` would have done.
        run(path, &["config", "core.autocrlf", "false"]).unwrap();
        dir
    }

    fn commit_all(path: &Path, message: &str) {
        run(path, &["add", "."]).unwrap();
        run(path, &["commit", "-m", message]).unwrap();
    }

    fn status(path: &Path) -> String {
        run(path, &["status", "--porcelain=v1"]).unwrap()
    }

    fn target(path: &str, kind: ChangeKind) -> Target {
        Target {
            path: path.to_string(),
            from_path: None,
            kind,
        }
    }

    #[test]
    fn staging_and_unstaging_a_modification_round_trips() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("a.txt"), "one\n").unwrap();
        commit_all(path, "init");

        std::fs::write(path.join("a.txt"), "two\n").unwrap();
        assert_eq!(status(path), " M a.txt");

        stage(path, &[target("a.txt", ChangeKind::Modified)]).unwrap();
        assert_eq!(status(path), "M  a.txt");

        unstage(path, &[target("a.txt", ChangeKind::Modified)], true).unwrap();
        assert_eq!(status(path), " M a.txt");
        // The file itself was never touched.
        assert_eq!(std::fs::read_to_string(path.join("a.txt")).unwrap(), "two\n");
    }

    /// The bug this module exists to avoid.
    ///
    /// Plain `git restore <path>` would leave the staged version in place and
    /// report success, so "Discard" would throw away half the change and say
    /// nothing. The assertion is on the file's **bytes**, not on Git's exit
    /// status, because the wrong command exits zero.
    #[test]
    fn discarding_a_file_with_staged_content_returns_it_to_head() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("a.txt"), "committed\n").unwrap();
        commit_all(path, "init");

        std::fs::write(path.join("a.txt"), "staged\n").unwrap();
        run(path, &["add", "a.txt"]).unwrap();
        std::fs::write(path.join("a.txt"), "working\n").unwrap();
        assert_eq!(status(path), "MM a.txt");

        discard(path, &[target("a.txt", ChangeKind::Modified)], true).unwrap();
        assert_eq!(
            std::fs::read_to_string(path.join("a.txt")).unwrap(),
            "committed\n",
            "discard must reach HEAD, not stop at the index"
        );
        assert_eq!(status(path), "");
    }

    #[test]
    fn discarding_an_untracked_file_removes_it() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("a.txt"), "one\n").unwrap();
        commit_all(path, "init");

        std::fs::write(path.join("new.txt"), "brand new\n").unwrap();
        discard(path, &[target("new.txt", ChangeKind::Untracked)], true).unwrap();
        assert!(!path.join("new.txt").exists());
        assert_eq!(status(path), "");
    }

    /// A deletion is the case where the same operation is a *recovery*.
    ///
    /// Both spellings of a deletion have to work: `git restore` on its own
    /// fails outright on the staged one.
    #[test]
    fn a_deleted_file_comes_back_whether_or_not_the_deletion_was_staged() {
        for stage_the_deletion in [false, true] {
            let dir = repo();
            let path = dir.path();
            std::fs::write(path.join("a.txt"), "committed\n").unwrap();
            commit_all(path, "init");

            std::fs::remove_file(path.join("a.txt")).unwrap();
            if stage_the_deletion {
                run(path, &["add", "a.txt"]).unwrap();
                assert_eq!(status(path), "D  a.txt");
            } else {
                assert_eq!(status(path), " D a.txt");
            }

            discard(path, &[target("a.txt", ChangeKind::Deleted)], true).unwrap();
            assert_eq!(
                std::fs::read_to_string(path.join("a.txt")).unwrap(),
                "committed\n"
            );
            assert_eq!(status(path), "");
        }
    }

    /// A rename is one change with two paths (see `git::diff::against_head`).
    #[test]
    fn discarding_a_rename_needs_both_of_its_names() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("old.txt"), "one\ntwo\nthree\n").unwrap();
        commit_all(path, "init");
        run(path, &["mv", "old.txt", "new.txt"]).unwrap();

        let renamed = Target {
            path: "new.txt".into(),
            from_path: Some("old.txt".into()),
            kind: ChangeKind::Renamed,
        };
        discard(path, &[renamed], true).unwrap();

        assert!(path.join("old.txt").exists(), "the original must come back");
        assert!(!path.join("new.txt").exists(), "the new name must go");
        assert_eq!(status(path), "");
    }

    /// Without a commit there is no `HEAD`, and the ordinary spellings fail.
    #[test]
    fn a_repository_with_no_commits_can_still_unstage_and_discard() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("a.txt"), "one\n").unwrap();
        std::fs::write(path.join("b.txt"), "two\n").unwrap();
        run(path, &["add", "."]).unwrap();
        assert_eq!(status(path), "A  a.txt\nA  b.txt");

        unstage(path, &[target("a.txt", ChangeKind::Added)], false).unwrap();
        assert!(path.join("a.txt").exists(), "unstaging must not delete");
        assert!(status(path).contains("?? a.txt"));

        discard(path, &[target("b.txt", ChangeKind::Added)], false).unwrap();
        assert!(!path.join("b.txt").exists());
    }

    #[test]
    fn a_conflicted_file_can_be_returned_to_the_committed_version() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("c.txt"), "base\n").unwrap();
        commit_all(path, "base");
        run(path, &["checkout", "-b", "other"]).unwrap();
        std::fs::write(path.join("c.txt"), "other\n").unwrap();
        commit_all(path, "other");
        run(path, &["checkout", "main"]).unwrap();
        std::fs::write(path.join("c.txt"), "main\n").unwrap();
        commit_all(path, "main");
        // The merge fails; that is the point.
        let _ = run(path, &["merge", "other"]);
        assert!(status(path).contains("UU c.txt"));

        discard(path, &[target("c.txt", ChangeKind::Conflicted)], true).unwrap();
        assert_eq!(std::fs::read_to_string(path.join("c.txt")).unwrap(), "main\n");
    }

    #[test]
    fn committing_takes_what_was_staged_and_nothing_else() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("a.txt"), "one\n").unwrap();
        commit_all(path, "init");

        std::fs::write(path.join("a.txt"), "changed\n").unwrap();
        std::fs::write(path.join("b.txt"), "unstaged\n").unwrap();
        assert!(!has_staged_changes(path));

        stage(path, &[target("a.txt", ChangeKind::Modified)]).unwrap();
        assert!(has_staged_changes(path));
        commit(path, "only a").unwrap();

        // b.txt is still sitting there untracked, exactly as it was.
        assert_eq!(status(path), "?? b.txt");
        assert!(!has_staged_changes(path));
        let log = run(path, &["log", "--oneline", "-1", "--format=%s"]).unwrap();
        assert_eq!(log, "only a");
    }

    /// A filename beginning with `-` must be a path, never an option.
    #[test]
    fn a_leading_dash_in_a_filename_is_still_a_filename() {
        let dir = repo();
        let path = dir.path();
        std::fs::write(path.join("a.txt"), "one\n").unwrap();
        commit_all(path, "init");

        std::fs::write(path.join("--weird.txt"), "tricky\n").unwrap();
        discard(path, &[target("--weird.txt", ChangeKind::Untracked)], true).unwrap();
        assert!(!path.join("--weird.txt").exists());
        assert_eq!(status(path), "");
    }
}
