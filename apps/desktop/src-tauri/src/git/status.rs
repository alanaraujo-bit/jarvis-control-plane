//! Working-tree status, read from `git status --porcelain=v1 -z`.
//!
//! The porcelain format is the one Git promises not to change, and `-z` is the
//! only variant safe to parse: paths with spaces, quotes or non-ASCII bytes are
//! emitted verbatim between NUL separators instead of being C-quoted.
//!
//! One quirk that a reader of the non-`-z` format would get backwards, and
//! which is verified against real Git 2.55 in the tests: **in `-z` mode a
//! rename lists the new path first and the original second** — the opposite of
//! the `orig -> new` arrow in the human format, and the opposite of what
//! `git diff --numstat -z` does for the same rename.

use std::collections::HashSet;
use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What happened to a file, as Git sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    /// Git does not know about this file at all.
    Untracked,
    /// Both sides changed it. Shown as itself, never quietly folded into
    /// "modified" — a conflict is a different thing to a person.
    Conflicted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedFile {
    /// Repository-relative, forward slashes, exactly as Git spells it.
    pub path: String,
    /// Where a renamed file came from.
    pub from_path: Option<String>,
    pub kind: ChangeKind,
    /// The change is present in the index.
    pub staged: bool,
    /// The change is present in the working tree but not the index.
    pub unstaged: bool,
}

/// Run git and capture stdout **whatever the exit status**.
///
/// `git::run` treats a non-zero status as an error, which is right for commands
/// that either work or do not. It is wrong for the two used here: `check-ignore`
/// exits 1 to mean "nothing matched" and `status` can exit non-zero while still
/// having said something useful. Verified against Git 2.55: `check-ignore`
/// returns 0 when some paths are ignored, 1 when none are, and 128 only on a
/// real error such as running outside a repository.
fn capture(cwd: &Path, args: &[&str], stdin: Option<&str>) -> Option<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_PAGER", "cat")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd.spawn().ok()?;
    if let Some(text) = stdin {
        child.stdin.take()?.write_all(text.as_bytes()).ok()?;
    }
    let out = child.wait_with_output().ok()?;
    // 128 is git's "I could not do this at all" — no repository, bad object.
    // Anything else has produced output worth reading.
    if out.status.code() == Some(128) {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Which of `paths` Git ignores. Repository-relative, forward slashes.
///
/// One process for the whole directory rather than one per entry: a file tree
/// expands a folder at a time, and 400 git invocations to grey out
/// `node_modules` would be felt.
pub fn ignored(root: &Path, paths: &[String]) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    // `-z` on input as well as output, so a path containing a newline cannot
    // be read as two paths.
    let stdin: String = paths.iter().map(|p| format!("{p}\0")).collect();
    let Some(out) = capture(root, &["check-ignore", "-z", "--stdin"], Some(&stdin)) else {
        return HashSet::new();
    };
    out.split('\0')
        .filter(|s| !s.is_empty())
        .map(|s| s.replace('\\', "/"))
        .collect()
}

/// Parse `git status --porcelain=v1 -z` output.
///
/// Separated from the process call so it can be tested against captured bytes
/// as well as against a real repository.
pub fn parse_status(out: &str) -> Vec<ChangedFile> {
    let mut fields = out.split('\0').filter(|s| !s.is_empty());
    let mut changes = Vec::new();

    while let Some(record) = fields.next() {
        // "XY path" — two status letters, a space, then the path. Anything
        // shorter is not a record we understand, and guessing at it would be
        // worse than skipping it.
        if record.len() < 4 {
            continue;
        }
        let bytes = record.as_bytes();
        let index = bytes[0] as char;
        let worktree = bytes[1] as char;
        let path = record[3..].replace('\\', "/");

        // A rename consumes the following field: the original path.
        let from_path = if index == 'R' || worktree == 'R' || index == 'C' || worktree == 'C' {
            fields.next().map(|s| s.replace('\\', "/"))
        } else {
            None
        };

        let kind = if index == 'U' || worktree == 'U' || (index == 'A' && worktree == 'A') {
            ChangeKind::Conflicted
        } else if index == '?' {
            ChangeKind::Untracked
        } else if index == 'R' || worktree == 'R' {
            ChangeKind::Renamed
        } else if index == 'D' || worktree == 'D' {
            ChangeKind::Deleted
        } else if index == 'A' || index == 'C' {
            ChangeKind::Added
        } else {
            ChangeKind::Modified
        };

        changes.push(ChangedFile {
            path,
            from_path,
            kind,
            staged: index != ' ' && index != '?',
            unstaged: worktree != ' ' && worktree != '?',
        });
    }

    changes
}

/// Everything the working tree has that `HEAD` does not.
///
/// `--untracked-files=all`, not the default `normal`. `normal` collapses a
/// wholly untracked directory into a single entry ending in `/` — a folder with
/// two new files in it arrives as one record, `assets/`, which has no filename
/// to show, no line count, and nothing to diff. Found by adding a new folder to
/// a real project and seeing Review render a row with an empty name.
///
/// `all` lists the files themselves, which is also simply the better answer:
/// this surface exists to review files, and "a directory appeared" is not a
/// change anyone can read. Ignored files stay out either way.
pub fn changed_files(root: &Path) -> Vec<ChangedFile> {
    capture(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        None,
    )
    .map(|out| parse_status(&out))
    .unwrap_or_default()
}

/// True when the repository has at least one commit.
///
/// Worth asking before every `HEAD` comparison: in a repository created a
/// minute ago there is no `HEAD` to diff against, and `git diff HEAD` fails
/// outright rather than reporting that everything is new.
pub fn has_commits(root: &Path) -> bool {
    capture(root, &["rev-parse", "--verify", "--quiet", "HEAD"], None)
        .map(|out| !out.trim().is_empty())
        .unwrap_or(false)
}
