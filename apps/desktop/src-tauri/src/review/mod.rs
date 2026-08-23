//! Diff / Review (§43).
//!
//! The question this surface answers is **"what did this agent change?"**, so
//! two sources are joined:
//!
//! - **Git**, for what is actually different. Nothing here decides that for
//!   itself; the diff is the one the user's own `git diff` would print (D5).
//! - **The session log**, for who did it. `file_changes` already records the
//!   files each session touched (mirrored out of the same append-only log
//!   everything else reads, D2), so attribution is a join rather than an
//!   invention.
//!
//! Read-only, deliberately. Staging, discarding or restoring a file is the
//! product running a destructive Git operation on the user's behalf, which by
//! D11 has to go through the guardrail rather than sit behind a plain button.
//! That belongs with Git proper (§44), not with reading a diff.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::files::{self, FileError};
use crate::git::{
    self,
    diff::FileDiff,
    status::ChangeKind,
};
use crate::AppState;

pub type Result<T> = std::result::Result<T, FileError>;

/// A session that touched a file, and what it was working on.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attribution {
    pub session_id: String,
    pub provider: String,
    pub title: Option<String>,
    pub mission_id: Option<String>,
    pub mission_title: Option<String>,
    /// When this session last touched the file.
    pub last_ts_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewFile {
    /// Project-relative, forward slashes — the same key the file tree uses.
    pub path: String,
    pub from_path: Option<String>,
    pub kind: ChangeKind,
    // Whether a change is staged is parsed and tested (`ChangedFile`), but not
    // sent: Review is read-only, so nothing here can act on it. §44 adds it
    // back when there is a stage button to attach it to.
    pub insertions: u32,
    pub deletions: u32,
    pub binary: bool,
    /// We declined to read this new file to count its lines. Distinct from
    /// binary, and very distinct from "nothing changed".
    pub too_large: bool,
    /// Sessions that touched this file, most recent first. Empty means nobody
    /// we were watching did — a change made outside J.A.R.V.I.S., which is a
    /// fact worth showing rather than hiding.
    pub sessions: Vec<Attribution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewReport {
    pub is_repo: bool,
    /// A repository with no commits yet cannot be diffed against `HEAD`;
    /// the surface says so instead of reporting no changes.
    pub has_commits: bool,
    pub branch: Option<String>,
    pub files: Vec<ReviewFile>,
}

/// Parse `git diff --numstat -z`.
///
/// The `-z` record for an ordinary file is `ins TAB del TAB path`. For a rename
/// the path field is **empty** and the old and new paths follow as two separate
/// NUL fields, old first — the opposite order to the one `git status -z` uses
/// for the same rename. Both orders are pinned by tests against a real
/// repository, because getting this backwards silently attributes a rename's
/// line counts to the wrong file.
///
/// A binary file reports `-` for both counts.
fn parse_numstat(out: &str) -> HashMap<String, (u32, u32, bool)> {
    let mut counts = HashMap::new();
    let mut fields = out.split('\0').filter(|s| !s.is_empty());

    while let Some(record) = fields.next() {
        let mut parts = record.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) =
            (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };

        let binary = added == "-" || removed == "-";
        let insertions = added.parse().unwrap_or(0);
        let deletions = removed.parse().unwrap_or(0);

        let path = if path.is_empty() {
            // A rename: skip the old path, key on the new one, which is what
            // `git status` reports and therefore what we are joining against.
            let _old = fields.next();
            match fields.next() {
                Some(new) => new.to_string(),
                None => continue,
            }
        } else {
            path.to_string()
        };

        counts.insert(path.replace('\\', "/"), (insertions, deletions, binary));
    }

    counts
}

/// Which sessions touched which files, keyed by project-relative path.
///
/// `file_changes.path` is **relative to the session's working directory**, not
/// to the project — Claude Code's `trackingPath` is spelled that way, with
/// backslashes on Windows. Verified against real transcripts on this machine:
/// a session started in `C:\Users\Alan Araujo` records
/// `jogo-da-velha-lan\src\game\types.ts`. Joining those strings directly
/// against Git's forward-slash paths matches nothing at all and looks exactly
/// like "no agent touched this file", so the session's `cwd` is folded back in
/// here.
fn attributions(
    state: &AppState,
    project_id: &str,
    root: &Path,
    wanted: &std::collections::HashSet<String>,
) -> Result<HashMap<String, Vec<Attribution>>> {
    // Nothing changed, so there is nobody to attribute it to. Worth an early
    // return rather than a query: this table grows with every file an agent
    // touches, forever, and the common case on a clean tree is to need none of
    // it.
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }

    let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>, String, String, i64)> =
        state.db.with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT fc.session_id, s.provider, s.title, s.mission_id, m.title,
                        s.cwd, fc.path, MAX(fc.ts_ms)
                   FROM file_changes fc
                   JOIN sessions s ON s.id = fc.session_id
              LEFT JOIN missions m ON m.id = s.mission_id
                  WHERE fc.project_id = ?1
               GROUP BY fc.session_id, fc.path
                  ORDER BY MAX(fc.ts_ms) DESC",
            )?;
            let rows = stmt.query_map([project_id], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;

    let mut map: HashMap<String, Vec<Attribution>> = HashMap::new();

    for (session_id, provider, title, mission_id, mission_title, cwd, path, ts_ms) in rows {
        let Some(key) = project_relative(root, Path::new(&cwd), &path) else {
            continue;
        };
        // Only files that actually differ are ever displayed, so the map never
        // grows past the size of the change set no matter how long the project
        // has been worked on.
        if !wanted.contains(&key) {
            continue;
        }
        let entry = map.entry(key).or_default();
        // One row per session per file; the query already grouped them.
        if entry.iter().any(|a| a.session_id == session_id) {
            continue;
        }
        entry.push(Attribution {
            session_id,
            provider,
            title,
            mission_id,
            mission_title,
            last_ts_ms: ts_ms,
        });
    }

    Ok(map)
}

/// Fold a session-relative recorded path into a project-relative one.
///
/// Returns `None` when the file lies outside the project — an agent started in
/// a parent folder can legitimately touch files this surface has no business
/// showing.
fn project_relative(root: &Path, cwd: &Path, recorded: &str) -> Option<String> {
    let absolute = cwd.join(recorded.replace('\\', "/"));
    // Neither path is canonicalised: the file may well have been deleted, and
    // the recorded path is already anchored to a `cwd` we wrote ourselves.
    let relative = absolute.strip_prefix(root).ok()?;
    Some(relative.to_string_lossy().replace('\\', "/"))
}

/// Line count of a file Git has never seen, for the summary row.
///
/// Both "no count" cases have to be told apart, not merged. A new file we
/// declined to read is not a file with no changes in it: reporting zero for a
/// two-megabyte addition, with nothing on the row to say why, is exactly the
/// kind of silent blank §81 exists to forbid.
struct UntrackedCounts {
    lines: u32,
    binary: bool,
    too_large: bool,
}

fn untracked_counts(root: &Path, project_path: &str) -> UntrackedCounts {
    let none = |binary, too_large| UntrackedCounts {
        lines: 0,
        binary,
        too_large,
    };

    match files::read(root, project_path) {
        Ok(contents) => match contents.text {
            Some(text) => {
                let body = text.strip_suffix('\n').unwrap_or(&text);
                UntrackedCounts {
                    lines: if body.is_empty() {
                        0
                    } else {
                        body.split('\n').count() as u32
                    },
                    binary: false,
                    too_large: false,
                }
            }
            None => none(
                contents.unreadable == Some(files::Unreadable::Binary),
                contents.unreadable == Some(files::Unreadable::TooLarge),
            ),
        },
        Err(_) => none(false, false),
    }
}

/// Everything that differs from `HEAD` in one project.
pub fn report(state: &AppState, project_id: &str) -> Result<ReviewReport> {
    let root = files::project_root(state, project_id)?;

    let Some(location) = git::locate(&root) else {
        return Ok(ReviewReport {
            is_repo: false,
            has_commits: false,
            branch: None,
            files: Vec::new(),
        });
    };

    let info = git::inspect(&root);
    let has_commits = git::status::has_commits(&location.root);

    let numstat = if has_commits {
        git::run(
            &location.root,
            &["diff", "--numstat", "-z", "--no-ext-diff", "-M", "HEAD"],
        )
        .map(|out| parse_numstat(&out))
        .unwrap_or_default()
    } else {
        HashMap::new()
    };

    // The change set first: it decides which attributions are worth loading.
    let changes = git::status::changed_files(&location.root);
    let wanted: std::collections::HashSet<String> = changes
        .iter()
        .filter_map(|c| location.to_project(&c.path))
        .collect();

    let touched = attributions(state, project_id, &root, &wanted)?;
    let mut files_out = Vec::new();

    for change in changes {
        // Git speaks in repository-relative paths; everything else here is
        // project-relative, and a project can be a subdirectory of its
        // repository.
        let Some(path) = location.to_project(&change.path) else {
            continue;
        };
        let from_path = change
            .from_path
            .as_deref()
            .and_then(|p| location.to_project(p));

        let (insertions, deletions, binary, too_large) = match change.kind {
            ChangeKind::Untracked => {
                let counts = untracked_counts(&root, &path);
                (counts.lines, 0, counts.binary, counts.too_large)
            }
            _ => {
                let (added, removed, binary) =
                    numstat.get(&change.path).copied().unwrap_or((0, 0, false));
                (added, removed, binary, false)
            }
        };

        files_out.push(ReviewFile {
            sessions: touched.get(&path).cloned().unwrap_or_default(),
            path,
            from_path,
            kind: change.kind,
            insertions,
            deletions,
            binary,
            too_large,
        });
    }

    // Files an agent touched come first, most recently touched at the top: this
    // surface exists to review agent work, so that is what should be under the
    // cursor. Everything else keeps Git's own ordering underneath.
    files_out.sort_by(|a, b| {
        let a_ts = a.sessions.first().map(|s| s.last_ts_ms);
        let b_ts = b.sessions.first().map(|s| s.last_ts_ms);
        b_ts.cmp(&a_ts).then_with(|| a.path.cmp(&b.path))
    });

    Ok(ReviewReport {
        is_repo: true,
        has_commits,
        branch: info.branch,
        files: files_out,
    })
}

/// One file's diff.
pub fn file_diff(
    state: &AppState,
    project_id: &str,
    path: &str,
    kind: ChangeKind,
    from_path: Option<&str>,
) -> Result<FileDiff> {
    let root = files::project_root(state, project_id)?;
    let Some(location) = git::locate(&root) else {
        return Err(FileError::NotFound(path.to_string()));
    };

    // The path is checked against the project boundary even though Git is the
    // one that will read it: `files::resolve` is the only thing standing
    // between the webview and `../../.ssh/id_rsa`, and Review must not be a
    // way around it.
    let _ = files::resolve(&root, path)?;

    if kind == ChangeKind::Untracked {
        let contents = files::read(&root, path)?;
        return Ok(match contents.text {
            Some(text) => git::diff::added_file(path, &text),
            // No text to show, and the two reasons are not the same thing.
            // Reporting "no line changed" for a two-megabyte new file would be
            // a plain untruth.
            None => FileDiff {
                path: path.to_string(),
                from_path: None,
                kind: ChangeKind::Untracked,
                binary: contents.unreadable == Some(files::Unreadable::Binary),
                too_large: contents.unreadable == Some(files::Unreadable::TooLarge),
                insertions: 0,
                deletions: 0,
                hunks: Vec::new(),
                truncated: false,
            },
        });
    }

    let repo_path = location.to_repo(path);
    // A renamed file needs both of its names on the command line, or Git cannot
    // pair them and reports the move as a brand-new file (see `against_head`).
    let repo_from = from_path.map(|from| location.to_repo(from));
    let patch = git::diff::against_head(&location.root, &repo_path, repo_from.as_deref())
        .unwrap_or_default();
    let (hunks, binary, insertions, deletions, truncated) = git::diff::parse_unified(&patch);

    Ok(FileDiff {
        path: path.to_string(),
        from_path: from_path.map(str::to_string),
        kind,
        binary,
        too_large: false,
        insertions,
        deletions,
        hunks,
        truncated,
    })
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
pub fn review_report(state: State<'_, AppState>, project_id: String) -> Result<ReviewReport> {
    report(&state, &project_id)
}

#[tauri::command]
pub fn review_file_diff(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    kind: ChangeKind,
    from_path: Option<String>,
) -> Result<FileDiff> {
    file_diff(&state, &project_id, &path, kind, from_path.as_deref())
}

#[cfg(test)]
mod tests;
