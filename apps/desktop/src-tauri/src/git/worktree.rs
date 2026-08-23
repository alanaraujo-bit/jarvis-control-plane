//! Git worktrees (§45).
//!
//! A worktree is a second checkout of the same repository in its own directory,
//! on its own branch. In this product that is what lets an agent work on
//! something without touching the tree the person is reading — the reason §45
//! exists at all.
//!
//! ## Why this is barely any code
//!
//! Because of a decision made much earlier. D5 chose the `git` executable over
//! libgit2 *specifically* so worktrees would be fully supported, and that bet
//! pays off here: `worktree add`, `list` and `remove` do the work, and this
//! module parses.
//!
//! And because of one fact worth checking rather than assuming:
//! `rev-parse --show-toplevel` run inside a worktree returns the **worktree's
//! own** path, not the main repository's. Verified against Git 2.55. So
//! `git::locate` already answers correctly inside a worktree, which means
//! Files, the editor and Review work there with no changes at all — a worktree
//! is simply a folder with a checkout in it, and that is exactly what a project
//! is in this product (§16).
//!
//! ## Parsing
//!
//! `worktree list --porcelain -z` — `-z` is supported and is the only safe
//! choice, because the common path on Windows contains a space
//! (`C:\Users\Alan Araujo\…`). Records are NUL-terminated *lines* with an
//! **extra NUL** between records, so a record boundary is an empty field.
//!
//! A record is `worktree <path>`, then some of `HEAD <sha>`, `branch <ref>`,
//! `detached`, `bare`, `locked [<reason>]`, `prunable [<reason>]`. The bare
//! flag lines carry no value and a parser that assumes `key value` throughout
//! misreads them.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{run, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Worktree {
    /// Absolute path of the checkout.
    pub path: String,
    /// Short branch name, absent when HEAD is detached.
    pub branch: Option<String>,
    pub head: Option<String>,
    pub detached: bool,
    /// The main working tree, which cannot be removed.
    pub is_main: bool,
    /// Locked against removal, with Git's reason when it gave one.
    pub locked: bool,
    pub lock_reason: Option<String>,
    /// Git believes the directory is gone and the record can be pruned.
    pub prunable: bool,
}

/// Parse `git worktree list --porcelain -z`.
///
/// Separated from the process call so the flag lines and the record separator
/// can be tested against captured bytes as well as a real repository.
///
/// The **first** record is always the main working tree — Git documents the
/// order and it is what `is_main` is derived from. Deriving it by comparing
/// paths against `--git-common-dir` would be the same answer through more
/// string handling, and would get it wrong the moment a path is spelled
/// differently.
pub fn parse_list(out: &str) -> Vec<Worktree> {
    let mut trees: Vec<Worktree> = Vec::new();

    for field in out.split('\0') {
        // An empty field is the blank line between records. Nothing to do:
        // a new record starts when its `worktree` line arrives.
        if field.is_empty() {
            continue;
        }

        let (key, value) = match field.split_once(' ') {
            Some((k, v)) => (k, Some(v)),
            // `detached`, `bare` and a reasonless `locked` are bare words.
            None => (field, None),
        };

        if key == "worktree" {
            trees.push(Worktree {
                path: value.unwrap_or_default().replace('\\', "/"),
                branch: None,
                head: None,
                detached: false,
                is_main: trees.is_empty(),
                locked: false,
                lock_reason: None,
                prunable: false,
            });
            continue;
        }

        let Some(current) = trees.last_mut() else {
            continue; // a field before any `worktree` line
        };

        match key {
            "HEAD" => current.head = value.map(str::to_string),
            // Stored short: `refs/heads/feature/x` is `feature/x` to a person,
            // and the same spelling `git::inspect` reports for a branch.
            "branch" => {
                current.branch = value.map(|v| {
                    v.strip_prefix("refs/heads/").unwrap_or(v).to_string()
                })
            }
            "detached" => current.detached = true,
            "locked" => {
                current.locked = true;
                current.lock_reason = value.map(str::to_string).filter(|r| !r.is_empty());
            }
            "prunable" => current.prunable = true,
            // `bare` and anything a newer Git adds. Ignored rather than
            // guessed at.
            _ => {}
        }
    }

    trees
}

/// Every worktree of the repository containing `cwd`.
pub fn list(cwd: &Path) -> Result<Vec<Worktree>> {
    let out = run(cwd, &["worktree", "list", "--porcelain", "-z"])?;
    Ok(parse_list(&out))
}

/// Turn a branch name into a directory name.
///
/// Branch names may contain `/`, which is a path separator and would put the
/// worktree somewhere nobody asked for — `feature/login` would nest a `login`
/// directory inside a new `feature` one. Everything Windows forbids in a name
/// goes the same way.
pub fn directory_name(repo_name: &str, branch: &str) -> String {
    let safe: String = branch
        .chars()
        .map(|c| match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' => c,
            _ => '-',
        })
        .collect();
    // Collapse runs and trim, so `feature//x` does not become `feature--x`.
    let collapsed = safe
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    // Dots are kept so `release/v1.2` reads as itself, but a run of them is
    // not: `..` is a directory name with a meaning, and while this result is a
    // single path segment and cannot traverse anywhere, a folder called
    // `app-a-..-..-b` is a thing nobody should have to explain. A trailing dot
    // is separately illegal on Windows.
    let mut trimmed = String::with_capacity(collapsed.len());
    let mut last_was_dot = false;
    for c in collapsed.chars() {
        if c == '.' {
            if last_was_dot {
                continue;
            }
            last_was_dot = true;
        } else {
            last_was_dot = false;
        }
        trimmed.push(c);
    }
    let trimmed = trimmed.trim_matches(['.', '-']);

    format!("{repo_name}-{}", if trimmed.is_empty() { "work" } else { trimmed })
}

/// Where a new worktree for `branch` will go.
///
/// **Derived here rather than accepted from the caller.** The webview names a
/// branch, never a directory: this is the one operation in the product that
/// deliberately writes *outside* the project root, and letting a renderer
/// choose that location would hand it the arbitrary directory creation that
/// §41's path confinement exists to prevent.
///
/// A sibling of the repository, which is where people put worktrees by hand and
/// keeps the repository's own tree free of directories Git would then have to
/// be told to ignore.
pub fn location_for(repo_root: &Path, branch: &str) -> Option<PathBuf> {
    let parent = repo_root.parent()?;
    let repo_name = repo_root.file_name()?.to_string_lossy().to_string();
    Some(parent.join(directory_name(&repo_name, branch)))
}

/// What `add` should do about the branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BranchMode {
    /// Create the branch as part of creating the worktree.
    Create,
    /// Check out a branch that already exists.
    Existing,
}

/// Create a worktree for `branch` beside the repository.
///
/// Returns where it was created.
///
/// Git refuses to check the same branch out twice, and that refusal is worth
/// passing straight through: it is the thing that stops two agents editing the
/// same branch from two directories and is exactly why worktrees are safe to
/// hand to an agent in the first place.
pub fn add(repo_root: &Path, branch: &str, mode: BranchMode) -> Result<PathBuf> {
    let path = location_for(repo_root, branch).ok_or_else(|| super::GitError::Failed {
        status: -1,
        stderr: "the repository has no parent directory to put a worktree in".into(),
    })?;
    let path_str = path.to_string_lossy().to_string();

    let args: Vec<&str> = match mode {
        BranchMode::Create => vec!["worktree", "add", "-b", branch, &path_str],
        // The branch goes after the path: `worktree add <path> <commit-ish>`.
        BranchMode::Existing => vec!["worktree", "add", &path_str, branch],
    };
    run(repo_root, &args)?;
    Ok(path)
}

/// Remove a worktree.
///
/// `force` is not decoration and is not passed speculatively. Git refuses to
/// remove a worktree containing modified or untracked files without it —
/// verified: exit 128, `contains modified or untracked files, use --force`.
/// That refusal is Git protecting uncommitted work, so the product's job is to
/// pass it through and let the guardrail handle the forced case, not to add
/// `--force` and make the refusal disappear.
pub fn remove(repo_root: &Path, path: &str, force: bool) -> Result<()> {
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(path);
    run(repo_root, &args).map(|_| ())
}

/// Whether a removal failed only because the worktree has work in it.
///
/// Matched on Git's own message rather than on the exit status, because 128 is
/// also what a missing path or a locked worktree produces and offering to force
/// past those would be offering something that cannot work.
pub fn is_dirty_refusal(error: &super::GitError) -> bool {
    match error {
        super::GitError::Failed { stderr, .. } => {
            stderr.contains("contains modified or untracked files")
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        run(path, &["init", "--initial-branch=main"]).unwrap();
        run(path, &["config", "user.email", "test@example.com"]).unwrap();
        run(path, &["config", "user.name", "Test"]).unwrap();
        run(path, &["config", "core.autocrlf", "false"]).unwrap();
        std::fs::write(path.join("README.md"), "# demo\n").unwrap();
        run(path, &["add", "."]).unwrap();
        run(path, &["commit", "-m", "initial"]).unwrap();
        dir
    }

    /// Captured from real `git worktree list --porcelain -z` output.
    ///
    /// The two things a parser gets wrong: the record separator is an **empty
    /// field** (records are NUL-terminated, so a blank line becomes two NULs in
    /// a row), and `detached`/`locked` carry no value.
    #[test]
    fn parses_the_porcelain_record_format_including_its_flag_lines() {
        let out = concat!(
            "worktree C:/repos/app\0HEAD abc123\0branch refs/heads/main\0\0",
            "worktree C:/repos/app-spike\0HEAD def456\0detached\0locked\0\0",
            "worktree C:/repos/app-feature\0HEAD 111222\0",
            "branch refs/heads/feature/login\0prunable gitdir file points to non-existent location\0\0",
        );
        let trees = parse_list(out);
        assert_eq!(trees.len(), 3);

        assert_eq!(trees[0].path, "C:/repos/app");
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert!(trees[0].is_main, "the first record is the main working tree");
        assert!(!trees[0].detached);

        assert!(trees[1].detached);
        assert!(trees[1].locked);
        assert_eq!(trees[1].lock_reason, None, "a bare `locked` has no reason");
        assert_eq!(trees[1].branch, None);
        assert!(!trees[1].is_main);

        // The short spelling, and a branch name containing a slash survives it.
        assert_eq!(trees[2].branch.as_deref(), Some("feature/login"));
        assert!(trees[2].prunable);
    }

    /// The common path on this platform contains a space.
    #[test]
    fn a_path_with_spaces_is_one_path() {
        let out = "worktree C:/Users/Alan Araujo/Projetos/app\0HEAD abc\0branch refs/heads/main\0\0";
        let trees = parse_list(out);
        assert_eq!(trees.len(), 1);
        assert_eq!(trees[0].path, "C:/Users/Alan Araujo/Projetos/app");
    }

    #[test]
    fn a_branch_name_never_becomes_a_path_separator() {
        // The bug this prevents: `feature/login` creating `app-feature/login`,
        // which is a `login` directory inside a new `feature` one.
        assert_eq!(directory_name("app", "feature/login"), "app-feature-login");
        assert_eq!(directory_name("app", "fix-123"), "app-fix-123");
        assert_eq!(directory_name("app", "release/v1.2"), "app-release-v1.2");
        assert!(!directory_name("app", "a/../../b").contains(".."));
        // Runs collapse rather than producing a wall of dashes.
        assert_eq!(directory_name("app", "a//b"), "app-a-b");
        // Something has to be left to name the directory.
        assert_eq!(directory_name("app", "///"), "app-work");
    }

    #[test]
    fn a_worktree_is_created_beside_the_repository_and_listed() {
        let dir = repo();
        let root = dir.path();

        let created = add(root, "feature", BranchMode::Create).unwrap();
        assert!(created.exists());
        assert_eq!(
            created.parent().unwrap(),
            root.parent().unwrap(),
            "worktrees are siblings of the repository, not children"
        );

        let trees = list(root).unwrap();
        assert_eq!(trees.len(), 2);
        assert!(trees[0].is_main);
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert_eq!(trees[1].branch.as_deref(), Some("feature"));
        assert!(!trees[1].is_main);
    }

    /// The fact everything else in M6 relies on: inside a worktree, `locate`
    /// answers about the worktree.
    ///
    /// If `rev-parse --show-toplevel` returned the *main* repository instead,
    /// opening a worktree as a project would silently show the wrong tree's
    /// files and the wrong tree's diff.
    #[test]
    fn a_worktree_locates_as_its_own_repository_root() {
        let dir = repo();
        let root = dir.path();
        let created = add(root, "feature", BranchMode::Create).unwrap();

        let location = super::super::locate(&created).unwrap();
        assert_eq!(
            location.root.canonicalize().unwrap(),
            created.canonicalize().unwrap()
        );
        assert_eq!(location.prefix, "", "a worktree root has no prefix");

        // And it reports its own branch, not the main tree's.
        let info = super::super::inspect(&created);
        assert!(info.is_repo);
        assert_eq!(info.branch.as_deref(), Some("feature"));
    }

    /// Git protects uncommitted work in a worktree; we pass that through.
    #[test]
    fn a_worktree_with_work_in_it_is_not_removed_without_force() {
        let dir = repo();
        let root = dir.path();
        let created = add(root, "feature", BranchMode::Create).unwrap();
        std::fs::write(created.join("scratch.txt"), "unsaved thinking\n").unwrap();

        let path = created.to_string_lossy().to_string();
        let refusal = remove(root, &path, false).unwrap_err();
        assert!(
            is_dirty_refusal(&refusal),
            "expected the dirty-tree refusal, got: {refusal}"
        );
        assert!(created.exists(), "nothing may be removed by a refused call");

        // With force, and only with force.
        remove(root, &path, true).unwrap();
        assert!(!created.exists());
        assert_eq!(list(root).unwrap().len(), 1);
    }

    /// A clean worktree holds nothing that is not already committed, which is
    /// why Git removes it without argument and why the product does not ask.
    #[test]
    fn a_clean_worktree_is_removed_without_force() {
        let dir = repo();
        let root = dir.path();
        let created = add(root, "feature", BranchMode::Create).unwrap();

        remove(root, &created.to_string_lossy(), false).unwrap();
        assert!(!created.exists());
    }

    /// Two checkouts of one branch is the thing worktrees exist to prevent.
    #[test]
    fn the_same_branch_cannot_be_checked_out_twice() {
        let dir = repo();
        let root = dir.path();
        add(root, "feature", BranchMode::Create).unwrap();

        let again = add(root, "feature", BranchMode::Create);
        assert!(again.is_err(), "git must refuse a duplicate branch");
    }

    #[test]
    fn an_existing_branch_can_be_checked_out_into_a_worktree() {
        let dir = repo();
        let root = dir.path();
        run(root, &["branch", "already-here"]).unwrap();

        let created = add(root, "already-here", BranchMode::Existing).unwrap();
        assert!(created.exists());
        assert_eq!(
            super::super::inspect(&created).branch.as_deref(),
            Some("already-here")
        );
    }
}
