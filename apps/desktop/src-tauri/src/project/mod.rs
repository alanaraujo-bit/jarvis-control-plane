//! Projects (§16).
//!
//! A project is a folder on this machine. J.A.R.V.I.S. records where it is and
//! what it looks like, and never copies its contents anywhere (§3 local-first).

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;
use crate::git;
use crate::session::log::now_ms;
use crate::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_git: bool,
    pub git_branch: Option<String>,
    pub git_remote: Option<String>,
    pub created_at: i64,
    pub last_opened_at: i64,
    pub archived: bool,
    /// False when the folder has since been moved or deleted. Surfaced in the
    /// UI so a stale entry is explained rather than silently failing to open.
    pub exists: bool,
    /// The project this is a worktree of (§45).
    ///
    /// Without it a worktree is a folder that appeared in the list with no
    /// explanation of where it came from — which is what it looked like when
    /// this was stored and not shown.
    pub worktree_of: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("that folder does not exist: {0}")]
    NotFound(String),
    #[error("that path is a file, not a folder: {0}")]
    NotADirectory(String),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

// Tauri needs a serialisable error to send across the IPC boundary.
impl serde::Serialize for ProjectError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, ProjectError>;

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    let path: String = row.get("path")?;
    Ok(Project {
        exists: Path::new(&path).is_dir(),
        id: row.get("id")?,
        name: row.get("name")?,
        is_git: row.get::<_, i64>("is_git")? != 0,
        git_branch: row.get("git_branch")?,
        git_remote: row.get("git_remote")?,
        created_at: row.get("created_at")?,
        last_opened_at: row.get("last_opened_at")?,
        archived: row.get::<_, i64>("archived")? != 0,
        worktree_of: row.get("worktree_of")?,
        path,
    })
}

/// Derive a display name from a folder path.
///
/// Falls back through the path rather than showing an empty label for a drive
/// root such as `C:\`.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| path.to_string_lossy().trim_end_matches(['\\', '/']).to_string())
        .trim_matches(['\\', '/', ':'])
        .to_string()
}

/// Normalise a path so the same folder is never registered twice.
///
/// `canonicalize` on Windows yields a `\\?\` extended-length prefix, which is
/// correct but unreadable in the UI, so it is stripped for storage.
fn normalise(path: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let text = canonical.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(stripped) => PathBuf::from(stripped),
        None => canonical,
    }
}

fn upsert(conn: &Connection, path: &Path) -> rusqlite::Result<Project> {
    let path_text = path.to_string_lossy().to_string();
    let info = git::inspect(path);
    let now = now_ms();

    let existing: Option<String> = conn
        .query_row("SELECT id FROM projects WHERE path = ?1", [&path_text], |r| {
            r.get(0)
        })
        .optional()?;

    let id = match existing {
        // Re-opening refreshes the Git snapshot and un-archives the project;
        // the user asking for it back is an explicit signal.
        Some(id) => {
            conn.execute(
                "UPDATE projects
                    SET last_opened_at = ?2, is_git = ?3, git_branch = ?4,
                        git_remote = ?5, archived = 0
                  WHERE id = ?1",
                params![
                    id,
                    now,
                    info.is_repo as i64,
                    info.branch,
                    info.remote
                ],
            )?;
            id
        }
        None => {
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO projects
                     (id, name, path, is_git, git_branch, git_remote, created_at, last_opened_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    id,
                    display_name(path),
                    path_text,
                    info.is_repo as i64,
                    info.branch,
                    info.remote,
                    now
                ],
            )?;
            id
        }
    };

    conn.query_row("SELECT * FROM projects WHERE id = ?1", [&id], row_to_project)
}

pub fn open(db: &Database, path: &Path) -> Result<Project> {
    if !path.exists() {
        return Err(ProjectError::NotFound(path.to_string_lossy().into()));
    }
    if !path.is_dir() {
        return Err(ProjectError::NotADirectory(path.to_string_lossy().into()));
    }
    let path = normalise(path);
    Ok(db.with(|conn| upsert(conn, &path))?)
}

pub fn list(db: &Database) -> Result<Vec<Project>> {
    Ok(db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT * FROM projects
              WHERE archived = 0
              ORDER BY last_opened_at DESC",
        )?;
        let rows = stmt.query_map([], row_to_project)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?)
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
pub fn list_projects(state: State<'_, AppState>) -> Result<Vec<Project>> {
    list(&state.db)
}

#[tauri::command]
pub fn open_project(state: State<'_, AppState>, path: String) -> Result<Project> {
    open(&state.db, Path::new(&path))
}

/// Remove a project from J.A.R.V.I.S.
///
/// Archives rather than deletes, and never touches the folder on disk — the
/// user's code is not ours to remove (§35).
#[tauri::command]
pub fn archive_project(state: State<'_, AppState>, id: String) -> Result<()> {
    state.db.with(|conn| {
        conn.execute("UPDATE projects SET archived = 1 WHERE id = ?1", [&id])?;
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::open_in_memory().unwrap()
    }

    #[test]
    fn opening_a_folder_records_it() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        let project = open(&db, dir.path()).unwrap();

        assert_eq!(project.name, display_name(&normalise(dir.path())));
        assert!(!project.is_git);
        assert!(project.exists);
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn reopening_the_same_folder_does_not_duplicate_it() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();

        let first = open(&db, dir.path()).unwrap();
        let second = open(&db, dir.path()).unwrap();

        assert_eq!(first.id, second.id, "the same folder must be one project");
        assert_eq!(list(&db).unwrap().len(), 1);
    }

    #[test]
    fn detects_git_metadata_on_open() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        git::run(path, &["init", "--initial-branch=main"]).unwrap();
        git::run(path, &["config", "user.email", "t@example.com"]).unwrap();
        git::run(path, &["config", "user.name", "T"]).unwrap();
        std::fs::write(path.join("a.txt"), "x").unwrap();
        git::run(path, &["add", "."]).unwrap();
        git::run(path, &["commit", "-m", "init"]).unwrap();

        let project = open(&db, path).unwrap();
        assert!(project.is_git);
        assert_eq!(project.git_branch.as_deref(), Some("main"));
    }

    #[test]
    fn rejects_a_missing_folder() {
        let db = db();
        let missing = std::env::temp_dir().join("jarvis-does-not-exist-9f2a");
        assert!(matches!(
            open(&db, &missing),
            Err(ProjectError::NotFound(_))
        ));
    }

    #[test]
    fn rejects_a_file() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-folder.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(matches!(
            open(&db, &file),
            Err(ProjectError::NotADirectory(_))
        ));
    }

    #[test]
    fn archiving_hides_a_project_without_deleting_the_folder() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        let project = open(&db, dir.path()).unwrap();

        db.with(|c| {
            c.execute("UPDATE projects SET archived = 1 WHERE id = ?1", [&project.id])?;
            Ok(())
        })
        .unwrap();

        assert!(list(&db).unwrap().is_empty());
        assert!(dir.path().is_dir(), "archiving must never touch the folder");
    }

    #[test]
    fn reopening_an_archived_project_restores_it() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        let project = open(&db, dir.path()).unwrap();
        db.with(|c| {
            c.execute("UPDATE projects SET archived = 1 WHERE id = ?1", [&project.id])?;
            Ok(())
        })
        .unwrap();

        open(&db, dir.path()).unwrap();
        assert_eq!(list(&db).unwrap().len(), 1);
    }
}
