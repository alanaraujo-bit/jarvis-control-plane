//! The Notebook (M19) — the person's own library of ideas and prompts.
//!
//! ## What this is for
//!
//! Alan kept his prompts in WhatsApp messages to himself. This is where they
//! live instead: folders, notes, searchable, and — the part that makes it
//! belong in *this* product rather than being a note-taking app — a note can be
//! handed straight to the agent currently on screen.
//!
//! ## Why this is not `project_notes` (§40)
//!
//! Notes already exist, and they stay. They are working memory **about one
//! project**: they live in that project's Brain tab, they can be promoted into
//! knowledge an agent gets briefed with, and `brain::delete_note` says outright
//! that a note "is a scratchpad entry whose whole purpose is to be temporary".
//!
//! Every one of those is the opposite of what is stored here. This library
//! belongs to no project (a prompt scoped to one project is useless — the point
//! is to reach for it everywhere), it is never briefed, and it is kept for
//! months. Making `project_notes.project_id` nullable to serve both would have
//! forced every existing reader to handle a note belonging to no project,
//! starting with `promote_note`, which has to know *which* project's knowledge
//! to write into. §23 forbids keeping the same fact twice; these are not the
//! same fact.
//!
//! ## The whole library is returned at once, deliberately
//!
//! `report` hands back every notebook and every note, bodies included, and the
//! surface searches and filters it in the webview. That is what makes the
//! overlay open with no spinner and filter with no round trip per keystroke,
//! which is most of what "premium" means for a thing you open twenty times a
//! day. It is affordable because a personal prompt library is sized in
//! hundreds of rows, not millions. The day that stops being true, the report
//! grows a preview column and the editor fetches one body on demand — a change
//! to this file and nothing else.

pub mod commands;

#[cfg(test)]
mod tests;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::session::log::now_ms;

pub type Result<T> = std::result::Result<T, String>;

/// A folder. One level — see the note in migration 16 for why that is a choice.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Notebook {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

/// One note. A prompt is a note you send — there is no `kind` column, because a
/// per-item switch is a switch somebody forgets to set (D21's lesson, one
/// module over).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    /// `None` is **unfiled**, never orphaned. Deleting a folder drops its notes
    /// here rather than taking them with it.
    pub notebook_id: Option<String>,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NotebookReport {
    pub notebooks: Vec<Notebook>,
    pub notes: Vec<Note>,
}

fn row_to_notebook(row: &rusqlite::Row<'_>) -> rusqlite::Result<Notebook> {
    Ok(Notebook {
        id: row.get("id")?,
        name: row.get("name")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get("id")?,
        notebook_id: row.get("notebook_id")?,
        title: row.get("title")?,
        body: row.get("body")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Everything, in the order the surface draws it.
pub fn report(db: &Database) -> Result<NotebookReport> {
    db.with(|conn| {
        // `id` here for the same reason as the notes below: two folders can be
        // created in one millisecond, and a map that reorders itself is worse
        // than a map in the wrong order.
        let mut books =
            conn.prepare("SELECT * FROM notebooks ORDER BY position, created_at, id")?;
        let notebooks: rusqlite::Result<Vec<_>> =
            books.query_map([], row_to_notebook)?.collect();

        // Pinned first, then most recently edited — the same order the index is
        // built for, so the list never needs a sort in the webview.
        //
        // **`id` is the final tiebreak, and it is not decoration.** Timestamps
        // are milliseconds, and two notes created or edited inside the same
        // millisecond is not hypothetical — duplicating a note does exactly
        // that, and so does a test. With `updated_at` and `created_at` both
        // equal the order was whatever SQLite felt like, which means a list
        // that can quietly reshuffle between two reads of unchanged data.
        // Found by a flaky test of this very ordering. `id` is a UUIDv7, so
        // descending is newest-created-first: stable *and* meaningful.
        let mut rows = conn.prepare(
            "SELECT * FROM notebook_notes
              ORDER BY pinned DESC, updated_at DESC, created_at DESC, id DESC",
        )?;
        let notes: rusqlite::Result<Vec<_>> = rows.query_map([], row_to_note)?.collect();

        Ok(NotebookReport {
            notebooks: notebooks?,
            notes: notes?,
        })
    })
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Folders
// ---------------------------------------------------------------------------

pub fn create_notebook(db: &Database, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("notebook.emptyName".into());
    }
    let id = uuid::Uuid::now_v7().to_string();
    let (stored, name) = (id.clone(), name.to_string());

    db.with(move |conn| {
        let now = now_ms();
        // Appended, not inserted at the top: a new folder should not push the
        // one somebody is looking at down the list.
        let next: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(position), -1) + 1 FROM notebooks",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        conn.execute(
            "INSERT INTO notebooks (id, name, position, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)",
            params![stored, name, next, now],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    Ok(id)
}

pub fn rename_notebook(db: &Database, id: &str, name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        return Err("notebook.emptyName".into());
    }
    let (id, name) = (id.to_string(), name.to_string());
    db.with(move |conn| {
        conn.execute(
            "UPDATE notebooks SET name = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, name, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Delete a folder. **Its notes survive**, unfiled.
///
/// The schema does this rather than this function: `ON DELETE SET NULL` on
/// `notebook_notes.notebook_id`, with foreign keys enforced (`db::open` sets
/// the pragma). A confirmation dialog would be a weaker guarantee than a shape
/// that cannot do the damage — somebody who has kept forty prompts for a year
/// should not be one mis-click from losing them, and the surface says where
/// they went rather than asking whether to destroy them.
pub fn delete_notebook(db: &Database, id: &str) -> Result<()> {
    let id = id.to_string();
    db.with(move |conn| {
        conn.execute("DELETE FROM notebooks WHERE id = ?1", [&id])?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Notes
// ---------------------------------------------------------------------------

/// A new, empty note, ready to be typed into.
///
/// Created empty rather than from a filled-in form: the gesture is "I have an
/// idea", and putting a dialog between that and a cursor is how a scratchpad
/// stops being used. An empty note is a real row — it is saved the moment
/// anything is typed, and deleting it is one click.
pub fn create_note(db: &Database, notebook_id: Option<&str>) -> Result<String> {
    let id = uuid::Uuid::now_v7().to_string();
    let (stored, notebook_id) = (id.clone(), notebook_id.map(str::to_string));

    db.with(move |conn| {
        let now = now_ms();
        conn.execute(
            "INSERT INTO notebook_notes
                 (id, notebook_id, title, body, pinned, created_at, updated_at)
             VALUES (?1, ?2, '', '', 0, ?3, ?3)",
            params![stored, notebook_id, now],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    Ok(id)
}

/// Save a note's title and body.
///
/// Both at once, because the surface autosaves and the two always travel
/// together — two commands would mean two `updated_at` values for one edit and
/// a list that reorders itself twice per keystroke burst.
pub fn update_note(db: &Database, id: &str, title: &str, body: &str) -> Result<()> {
    let (id, title, body) = (id.to_string(), title.to_string(), body.to_string());
    db.with(move |conn| {
        conn.execute(
            "UPDATE notebook_notes SET title = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, title, body, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Pin or unpin. **Does not touch `updated_at`** — pinning is a decision about
/// where something sits, not an edit to it, and bumping the timestamp would
/// silently reorder the list under the person who just pinned something.
pub fn set_note_pinned(db: &Database, id: &str, pinned: bool) -> Result<()> {
    let id = id.to_string();
    db.with(move |conn| {
        conn.execute(
            "UPDATE notebook_notes SET pinned = ?2 WHERE id = ?1",
            params![id, pinned as i64],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Move a note to another folder, or to `None` for unfiled.
///
/// Same reasoning as pinning: filing is not editing, so `updated_at` stands.
pub fn move_note(db: &Database, id: &str, notebook_id: Option<&str>) -> Result<()> {
    let (id, notebook_id) = (id.to_string(), notebook_id.map(str::to_string));
    db.with(move |conn| {
        conn.execute(
            "UPDATE notebook_notes SET notebook_id = ?2 WHERE id = ?1",
            params![id, notebook_id],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Copy a note, in place.
///
/// A prompt library is edited by variation — the same prompt with one clause
/// changed — and the alternative is select-all, copy, new note, paste, which is
/// four gestures for one intent. The copy lands beside the original, unpinned,
/// so it cannot be mistaken for the one that was already trusted.
pub fn duplicate_note(db: &Database, id: &str) -> Result<String> {
    let new_id = uuid::Uuid::now_v7().to_string();
    let (source, stored) = (id.to_string(), new_id.clone());

    db.with(move |conn| {
        let now = now_ms();
        conn.execute(
            "INSERT INTO notebook_notes
                 (id, notebook_id, title, body, pinned, created_at, updated_at)
             SELECT ?1, notebook_id, title, body, 0, ?2, ?2
               FROM notebook_notes WHERE id = ?3",
            params![stored, now, source],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    Ok(new_id)
}

/// Delete one note. Genuinely, and only ever one at a time.
pub fn delete_note(db: &Database, id: &str) -> Result<()> {
    let id = id.to_string();
    db.with(move |conn| {
        conn.execute("DELETE FROM notebook_notes WHERE id = ?1", [&id])?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}
