//! Project files (§41).
//!
//! Everything here is scoped to one project folder. The webview asks for paths
//! **relative to the project root** and never gets to name an absolute one: a
//! renderer that can read `C:\Users\...\.ssh\id_rsa` by asking nicely is not a
//! local-first product (§3), it is a file server with a nice icon.
//!
//! `resolve` is the whole security boundary and is tested directly, including
//! the cases that make a naive prefix check wrong on Windows — verbatim
//! (`\\?\`) canonical paths, case-insensitive comparison, and symlinks that
//! escape the root.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::git;
use crate::AppState;

/// Largest file the editor will open.
///
/// Above this we say so rather than handing the webview 80 MB and watching it
/// stop responding. §81: a limit that is explained is a feature; a hang is not.
pub const MAX_EDITABLE_BYTES: u64 = 2 * 1024 * 1024;

/// How much of a file is inspected for NUL bytes to decide it is binary.
/// The same window Git uses, so our answer and `git diff`'s agree.
const BINARY_SNIFF_BYTES: usize = 8000;

#[derive(Debug, thiserror::Error)]
pub enum FileError {
    #[error("that path is outside the project")]
    Outside,
    #[error("no such project")]
    NoProject,
    #[error("that path does not exist: {0}")]
    NotFound(String),
    #[error("{0}")]
    Io(String),
    #[error("database error: {0}")]
    Db(#[from] crate::db::DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

impl serde::Serialize for FileError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, FileError>;

/// Drop Windows' extended-length prefix so two canonical paths can be compared
/// and so nothing `\\?\`-shaped ever reaches the interface.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(stripped) => PathBuf::from(stripped),
        None => path.to_path_buf(),
    }
}

/// True when `child` is `root` itself or lies beneath it.
///
/// Comparison is component-wise, never on raw strings: `C:\proj-evil` starts
/// with the string `C:\proj` but is a different directory. On Windows the
/// comparison is also case-insensitive, because `C:\Proj\a` and `c:\proj\A`
/// are the same file — a case-sensitive check would reject a legitimate path,
/// and, read the other way round, would let one escape by changing case.
fn contains(root: &Path, child: &Path) -> bool {
    let root = strip_verbatim(root);
    let child = strip_verbatim(child);

    #[cfg(windows)]
    {
        let root = PathBuf::from(root.to_string_lossy().to_lowercase());
        let child = PathBuf::from(child.to_string_lossy().to_lowercase());
        child.starts_with(&root)
    }
    #[cfg(not(windows))]
    {
        child.starts_with(&root)
    }
}

/// Turn a project-relative path from the webview into an absolute path that is
/// provably inside `root`.
///
/// Two independent checks, because either alone can be defeated:
///
/// 1. The **requested** path must be relative and contain no `..` and no root
///    or drive component. This rejects `..\..\secrets` and `C:\Windows` before
///    they touch the filesystem, and it is the only check available for a path
///    that does not exist yet — a new file being saved.
/// 2. The **resolved** path must still be inside the root once the filesystem
///    has had its say. Only this catches a symlink or junction inside the
///    project pointing somewhere else entirely. Because the target may not
///    exist, the deepest ancestor that *does* exist is the one canonicalised;
///    the components below it were already proven free of `..` by check 1, so
///    they cannot climb back out.
pub fn resolve(root: &Path, relative: &str) -> Result<PathBuf> {
    let requested = Path::new(relative);

    for component in requested.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            // ParentDir, RootDir, and Windows' Prefix (drive letters, UNC).
            _ => return Err(FileError::Outside),
        }
    }

    let joined = root.join(requested);

    // Canonicalise the deepest ancestor that exists, the target itself included
    // when it does.
    let mut existing = joined.as_path();
    let anchor = loop {
        if let Ok(canonical) = existing.canonicalize() {
            break canonical;
        }
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return Err(FileError::Outside),
        }
    };

    let canonical_root = root
        .canonicalize()
        .map_err(|e| FileError::Io(e.to_string()))?;
    if !contains(&canonical_root, &anchor) {
        return Err(FileError::Outside);
    }

    Ok(joined)
}

// ---- Listing ----------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// Path relative to the project root, always with forward slashes so the
    /// same string is a stable key in the UI on every platform.
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    /// `None` for directories — the number would be the directory entry's own
    /// size on disk, which means nothing to anyone.
    pub size: Option<u64>,
    /// Git ignores it. Shown, but quietly: hiding a file the user can see in
    /// Explorer is more confusing than dimming it.
    pub ignored: bool,
}

/// Turn a path into the forward-slash, root-relative form the UI uses.
fn relative_key(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// List one directory.
///
/// Not recursive: the tree asks for children as they are opened, so a
/// repository with 200k files under `node_modules` costs nothing until somebody
/// actually opens `node_modules`.
pub fn list_dir(root: &Path, relative: &str) -> Result<Vec<FileEntry>> {
    let dir = resolve(root, relative)?;
    if !dir.is_dir() {
        return Err(FileError::NotFound(relative.to_string()));
    }

    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&dir).map_err(|e| FileError::Io(e.to_string()))? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // `.git` is machinery, not the user's code, and browsing into it is a
        // way to corrupt a repository by accident.
        if name == ".git" {
            continue;
        }

        // `DirEntry::file_type` does not follow symlinks; a link to a directory
        // should still open like one, so ask the metadata that does.
        let meta = std::fs::metadata(&path).ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);

        entries.push(FileEntry {
            path: relative_key(root, &path),
            name,
            is_dir,
            size: meta.as_ref().filter(|m| m.is_file()).map(|m| m.len()),
            ignored: false,
        });
    }

    // One `git check-ignore` for the whole directory rather than one per entry.
    let names: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let ignored = git::status::ignored(root, &names);
    for entry in &mut entries {
        entry.ignored = ignored.contains(&entry.path);
    }

    // Directories first, then case-insensitive by name — the ordering every
    // file tree uses, so nobody has to learn ours.
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    Ok(entries)
}

// ---- Reading and writing ----------------------------------------------------

/// Why a file has no text to show. Absent means it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Unreadable {
    Binary,
    TooLarge,
}

/// Milliseconds since the epoch for a file's modified time.
///
/// The unit the rest of the product uses for time, and the only fact we need
/// about it: whether it is the same as it was when the file was read.
fn modified_ms(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContents {
    pub path: String,
    pub size: u64,
    /// When the file was last written, as the editor found it. Handed back on
    /// save so a write can tell whether anything happened in between.
    pub modified_ms: Option<i64>,
    /// Absent when `unreadable` is set. The product says why rather than
    /// rendering mojibake and calling it a file (§81).
    pub text: Option<String>,
    pub unreadable: Option<Unreadable>,
    /// The file ends with a newline. Preserved on save, because rewriting a
    /// whole repository's trailing newlines is a diff nobody asked for.
    pub trailing_newline: bool,
    /// The file uses CRLF. Also preserved on save.
    pub crlf: bool,
}

/// Does this look like a binary file?
///
/// Git's heuristic: a NUL byte near the start. Cheap, and it agrees with what
/// `git diff` will say about the same file — which matters, because Review
/// shows both and they must not contradict each other.
fn looks_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF_BYTES)].contains(&0)
}

pub fn read(root: &Path, relative: &str) -> Result<FileContents> {
    let path = resolve(root, relative)?;
    let meta = std::fs::metadata(&path).map_err(|e| FileError::Io(e.to_string()))?;
    let size = meta.len();

    let mut contents = FileContents {
        path: relative.to_string(),
        size,
        modified_ms: modified_ms(&meta),
        text: None,
        unreadable: None,
        trailing_newline: false,
        crlf: false,
    };

    if size > MAX_EDITABLE_BYTES {
        contents.unreadable = Some(Unreadable::TooLarge);
        return Ok(contents);
    }

    let bytes = std::fs::read(&path).map_err(|e| FileError::Io(e.to_string()))?;
    if looks_binary(&bytes) {
        contents.unreadable = Some(Unreadable::Binary);
        return Ok(contents);
    }

    let text = String::from_utf8_lossy(&bytes).to_string();
    contents.trailing_newline = text.ends_with('\n');
    contents.crlf = text.contains("\r\n");
    // The editor works in `\n` throughout; the original ending is restored on
    // save, so opening a CRLF file and saving it unchanged is a no-op diff.
    contents.text = Some(text.replace("\r\n", "\n"));
    Ok(contents)
}

/// What happened to a save.
///
/// A refusal is not an error: it is an answer the interface has to render in
/// the user's own language (§65), and a serialised error string cannot be
/// translated. So the outcome is data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum WriteOutcome {
    Written { modified_ms: Option<i64> },
    /// The file on disk is not the one that was opened. Nothing was written.
    Stale { modified_ms: Option<i64> },
}

/// Write a file back, restoring the line endings and trailing newline it had.
///
/// `expected_modified_ms` is the modified time the editor saw when it read the
/// file. When it is supplied and no longer matches, **nothing is written** and
/// the caller is told the file is stale.
///
/// This matters more here than in an ordinary editor. The whole product exists
/// so that an agent can be working in one tab while a person reads in another,
/// which makes "both of us edited this file" the normal case rather than the
/// exotic one. Overwriting blind would delete an agent's work with no trace
/// that it ever existed, and the person doing it would have no way to know.
///
/// Passing `None` writes regardless. That is how the interface offers to save
/// anyway, once it has said what happened.
pub fn write(
    root: &Path,
    relative: &str,
    text: &str,
    crlf: bool,
    trailing_newline: bool,
    expected_modified_ms: Option<i64>,
) -> Result<WriteOutcome> {
    let path = resolve(root, relative)?;

    if let Some(expected) = expected_modified_ms {
        // A file that has since been deleted is not stale — writing it back is
        // exactly what the user is asking for.
        if let Ok(meta) = std::fs::metadata(&path) {
            let current = modified_ms(&meta);
            if current != Some(expected) {
                return Ok(WriteOutcome::Stale {
                    modified_ms: current,
                });
            }
        }
    }

    let mut out = text.replace("\r\n", "\n");
    if trailing_newline {
        if !out.ends_with('\n') {
            out.push('\n');
        }
    } else {
        while out.ends_with('\n') {
            out.pop();
        }
    }
    if crlf {
        out = out.replace('\n', "\r\n");
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| FileError::Io(e.to_string()))?;
    }
    std::fs::write(&path, out).map_err(|e| FileError::Io(e.to_string()))?;

    Ok(WriteOutcome::Written {
        modified_ms: std::fs::metadata(&path).ok().as_ref().and_then(modified_ms),
    })
}

// ---- Commands ---------------------------------------------------------------

/// Look up a project's folder, failing rather than defaulting.
///
/// Every command below routes through this: the root comes from the database,
/// never from the webview, so a caller cannot nominate its own root and make
/// the confinement check tautological.
pub fn project_root(state: &AppState, project_id: &str) -> Result<PathBuf> {
    use rusqlite::OptionalExtension;

    let path: Option<String> = state.db.with(|conn| {
        conn.query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .optional()
    })?;

    let path = path.ok_or(FileError::NoProject)?;
    let root = PathBuf::from(&path);
    if !root.is_dir() {
        return Err(FileError::NotFound(path));
    }
    Ok(root)
}

#[tauri::command]
pub fn list_files(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
) -> Result<Vec<FileEntry>> {
    let root = project_root(&state, &project_id)?;
    list_dir(&root, &path)
}

#[tauri::command]
pub fn read_file(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
) -> Result<FileContents> {
    let root = project_root(&state, &project_id)?;
    read(&root, &path)
}

#[tauri::command]
pub fn write_file(
    state: State<'_, AppState>,
    project_id: String,
    path: String,
    text: String,
    crlf: bool,
    trailing_newline: bool,
    expected_modified_ms: Option<i64>,
) -> Result<WriteOutcome> {
    let root = project_root(&state, &project_id)?;
    write(
        &root,
        &path,
        &text,
        crlf,
        trailing_newline,
        expected_modified_ms,
    )
}

#[cfg(test)]
mod tests;
