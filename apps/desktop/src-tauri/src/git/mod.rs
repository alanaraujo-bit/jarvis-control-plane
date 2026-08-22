//! Git integration.
//!
//! Implemented over the `git` executable rather than libgit2. The reasons are
//! deliberate (see docs/DECISIONS.md):
//!
//! - Worktrees (§45) are only fully supported by the CLI.
//! - Output matches exactly what the user's own `git` does, including their
//!   config, hooks, credential helpers and safe.directory rules. A library
//!   would quietly diverge.
//! - Git is already a required tool (§14), so this adds no new dependency.
//!
//! Every call is non-interactive: a Git subprocess that opens a credential
//! prompt or a pager would hang the caller with no UI to resolve it.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git could not be launched: {0}")]
    Spawn(#[from] std::io::Error),
    #[error("git exited with status {status}: {stderr}")]
    Failed { status: i32, stderr: String },
}

pub type Result<T> = std::result::Result<T, GitError>;

/// A snapshot of a repository, cheap enough to refresh whenever a project opens.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub is_repo: bool,
    pub branch: Option<String>,
    pub remote: Option<String>,
    /// True when HEAD is detached or the repository has no commits yet.
    pub detached: bool,
}

/// Run a git command in `cwd` and capture stdout.
pub fn run(cwd: &Path, args: &[&str]) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        // Never block on a credential prompt or a pager — there is no terminal
        // attached to answer one.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args);
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let out = cmd.output()?;
    if !out.status.success() {
        return Err(GitError::Failed {
            status: out.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Describe the repository containing `path`, if there is one.
pub fn inspect(path: &Path) -> RepoInfo {
    // `--is-inside-work-tree` is the reliable test: it accounts for worktrees,
    // submodules and repositories discovered from a subdirectory, which a bare
    // `.git` directory check does not.
    let inside = run(path, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s == "true")
        .unwrap_or(false);

    if !inside {
        return RepoInfo::default();
    }

    // Empty on a detached HEAD, and it fails outright before the first commit.
    let branch = run(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .filter(|s| !s.is_empty());

    let remote = run(path, &["remote", "get-url", "origin"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(|url| sanitize_remote(&url));

    RepoInfo {
        is_repo: true,
        detached: branch.is_none(),
        branch,
        remote,
    }
}

/// Strip credentials that people sometimes embed in remote URLs.
///
/// A remote is displayed in the UI and written to the local database; a token
/// pasted into a clone URL must not travel with it (§60).
fn sanitize_remote(url: &str) -> String {
    let Some(scheme_end) = url.find("://") else {
        return url.to_string(); // scp-style (git@host:path) carries no secret
    };
    let (scheme, rest) = url.split_at(scheme_end + 3);
    match rest.split_once('@') {
        Some((_credentials, host)) => format!("{scheme}{host}"),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_credentials_from_remote_urls() {
        assert_eq!(
            sanitize_remote("https://user:ghp_secrettoken@github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
        assert_eq!(
            sanitize_remote("https://ghp_secrettoken@github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
    }

    #[test]
    fn leaves_ordinary_remotes_untouched() {
        assert_eq!(
            sanitize_remote("https://github.com/acme/app.git"),
            "https://github.com/acme/app.git"
        );
        assert_eq!(
            sanitize_remote("git@github.com:acme/app.git"),
            "git@github.com:acme/app.git"
        );
    }

    #[test]
    fn reports_a_non_repository_honestly() {
        let dir = tempfile::tempdir().unwrap();
        let info = inspect(dir.path());
        assert!(!info.is_repo);
        assert!(info.branch.is_none());
    }

    #[test]
    fn inspects_a_real_repository() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        // A real repository, not a mock: this asserts the actual parsing works
        // against the git binary the product will use (§80).
        run(path, &["init", "--initial-branch=main"]).unwrap();
        run(path, &["config", "user.email", "test@example.com"]).unwrap();
        run(path, &["config", "user.name", "Test"]).unwrap();
        std::fs::write(path.join("README.md"), "# demo\n").unwrap();
        run(path, &["add", "."]).unwrap();
        run(path, &["commit", "-m", "initial"]).unwrap();

        let info = inspect(path);
        assert!(info.is_repo);
        assert_eq!(info.branch.as_deref(), Some("main"));
        assert!(!info.detached);
        assert!(info.remote.is_none());
    }
}
