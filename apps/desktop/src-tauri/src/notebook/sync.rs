//! Carrying the library between machines (M23).
//!
//! ## The shape, and why it is last-write-wins per row
//!
//! A prompt library belongs to one person and is sized in hundreds of rows, so
//! the obvious cheap design — push the whole thing, let the newest push win —
//! is tempting and wrong in exactly one case that will happen: a note written
//! on the laptop between two pushes from the desktop disappears, because the
//! desktop's copy of "the whole library" never contained it. Merging row by row
//! loses only the older of two edits to the *same* note, which is the smallest
//! thing a sync with no conflict screen can lose.
//!
//! Both sides compare `touched_at` and the larger wins. `>=` rather than `>` on
//! the server, so re-pushing an unchanged library is idempotent instead of
//! being rejected row by row.
//!
//! ## Deletion is a row, not an absence
//!
//! `sync_tombstones` exists because "not in the payload" and "deleted" are the
//! same thing on the wire and must not be. A tombstone beats a remote edit only
//! when it is *newer* than that edit: deleting a note here while editing it
//! there is a genuine conflict, and it is settled by the same rule as
//! everything else in this file rather than by a special case for delete.
//!
//! ## Two accounts, one machine
//!
//! `notebooks` and `notebook_notes` have no account column — like `settings`
//! they are machine-scoped, and migration 17 explains at length why adding one
//! is not the small change it looks like. So the merge remembers which account
//! the local library was last reconciled against. Pulling for a *different*
//! account replaces the library rather than merging it: two people's prompt
//! libraries stirred together is the one outcome here nobody can undo. A
//! library that has never synced is adopted by the first account to pull, the
//! same way `prefs::adopt_machine_settings` adopts this machine's preferences.
//!
//! The separation happens on the **first successful pull** and deliberately not
//! at sign-in. Clearing the library the moment somebody else signs in would be
//! the tidier rule and it would destroy data: a library that has never reached
//! the server exists in exactly one place, and an offline sign-in is precisely
//! the moment we cannot check whether it is safe anywhere else. So until a pull
//! lands, a second person signing in on a shared machine still sees the first
//! one's notes — which is not a regression, because before M23 the notebook was
//! machine-scoped and they always did. Closing that properly means an account
//! column on `notebooks`, which is the change migration 17 argued its way out
//! of for `settings`, and it should be argued again rather than smuggled in
//! here.

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::Result;
use crate::db::Database;

/// Which account the local library currently belongs to.
pub const OWNER_KEY: &str = "sync.notebookAccount";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncNotebook {
    pub id: String,
    pub name: String,
    pub position: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub touched_at: i64,
    /// `Some` is a tombstone. The other fields are then meaningless, and are
    /// carried only because one uniform row is cheaper than two wire shapes.
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncNote {
    pub id: String,
    #[serde(default)]
    pub notebook_id: Option<String>,
    pub title: String,
    pub body: String,
    pub pinned: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub touched_at: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncPayload {
    pub notebooks: Vec<SyncNotebook>,
    pub notes: Vec<SyncNote>,
}

/// Everything this machine has: live rows and tombstones together.
pub fn payload(db: &Database) -> Result<SyncPayload> {
    db.with(|conn| {
        let mut books = conn.prepare(
            "SELECT id, name, position, created_at, updated_at, touched_at FROM notebooks",
        )?;
        let mut notebooks: Vec<SyncNotebook> = books
            .query_map([], |row| {
                Ok(SyncNotebook {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    position: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    touched_at: row.get(5)?,
                    deleted_at: None,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut rows = conn.prepare(
            "SELECT id, notebook_id, title, body, pinned, created_at, updated_at, touched_at
               FROM notebook_notes",
        )?;
        let mut notes: Vec<SyncNote> = rows
            .query_map([], |row| {
                Ok(SyncNote {
                    id: row.get(0)?,
                    notebook_id: row.get(1)?,
                    title: row.get(2)?,
                    body: row.get(3)?,
                    pinned: row.get::<_, i64>(4)? != 0,
                    created_at: row.get(5)?,
                    updated_at: row.get(6)?,
                    touched_at: row.get(7)?,
                    deleted_at: None,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;

        let mut graves = conn.prepare("SELECT kind, id, deleted_at FROM sync_tombstones")?;
        let buried = graves.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for grave in buried {
            let (kind, id, at) = grave?;
            if kind == "notebook" {
                notebooks.push(SyncNotebook {
                    id,
                    created_at: at,
                    updated_at: at,
                    touched_at: at,
                    deleted_at: Some(at),
                    ..Default::default()
                });
            } else {
                notes.push(SyncNote {
                    id,
                    created_at: at,
                    updated_at: at,
                    touched_at: at,
                    deleted_at: Some(at),
                    ..Default::default()
                });
            }
        }

        Ok(SyncPayload { notebooks, notes })
    })
    .map_err(|e| e.to_string())
}

/// Record that a row was deleted here, so a later pull cannot restore it.
pub fn bury(conn: &rusqlite::Connection, kind: &str, id: &str, at: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO sync_tombstones (kind, id, deleted_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (kind, id) DO UPDATE SET deleted_at = excluded.deleted_at",
        params![kind, id, at],
    )?;
    Ok(())
}

/// Fold a remote library into the local one.
///
/// `account_id` is who the remote library belongs to. When that disagrees with
/// the owner recorded on this machine the local rows are cleared first — see
/// the module note. Returns `true` when anything actually changed, so the
/// caller can decide whether the surface needs telling.
pub fn merge(db: &Database, account_id: &str, remote: &SyncPayload) -> Result<bool> {
    let owner: Option<String> = crate::settings::get(db, OWNER_KEY);
    let replace = matches!(owner.as_deref(), Some(previous) if previous != account_id);

    let remote = remote.clone();
    let changed = db
        .with(move |conn| {
            let tx = conn.unchecked_transaction()?;
            if replace {
                tx.execute("DELETE FROM notebook_notes", [])?;
                tx.execute("DELETE FROM notebooks", [])?;
                tx.execute("DELETE FROM sync_tombstones", [])?;
            }
            let mut changed = replace;

            for book in &remote.notebooks {
                let grave: Option<i64> = tx
                    .query_row(
                        "SELECT deleted_at FROM sync_tombstones WHERE kind = 'notebook' AND id = ?1",
                        [&book.id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if grave.is_some_and(|at| at >= book.touched_at) {
                    continue;
                }
                let local: Option<i64> = tx
                    .query_row(
                        "SELECT touched_at FROM notebooks WHERE id = ?1",
                        [&book.id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if local.is_some_and(|at| at > book.touched_at) {
                    continue;
                }
                if let Some(at) = book.deleted_at {
                    tx.execute("DELETE FROM notebooks WHERE id = ?1", [&book.id])?;
                    bury(&tx, "notebook", &book.id, at)?;
                } else {
                    tx.execute(
                        "INSERT INTO notebooks (id, name, position, created_at, updated_at, touched_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT (id) DO UPDATE SET name = excluded.name,
                             position = excluded.position, updated_at = excluded.updated_at,
                             touched_at = excluded.touched_at",
                        params![
                            book.id,
                            book.name,
                            book.position,
                            book.created_at,
                            book.updated_at,
                            book.touched_at
                        ],
                    )?;
                    tx.execute(
                        "DELETE FROM sync_tombstones WHERE kind = 'notebook' AND id = ?1",
                        [&book.id],
                    )?;
                }
                changed = true;
            }

            for note in &remote.notes {
                let grave: Option<i64> = tx
                    .query_row(
                        "SELECT deleted_at FROM sync_tombstones WHERE kind = 'note' AND id = ?1",
                        [&note.id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if grave.is_some_and(|at| at >= note.touched_at) {
                    continue;
                }
                let local: Option<i64> = tx
                    .query_row(
                        "SELECT touched_at FROM notebook_notes WHERE id = ?1",
                        [&note.id],
                        |row| row.get(0),
                    )
                    .optional()?;
                if local.is_some_and(|at| at > note.touched_at) {
                    continue;
                }
                if let Some(at) = note.deleted_at {
                    tx.execute("DELETE FROM notebook_notes WHERE id = ?1", [&note.id])?;
                    bury(&tx, "note", &note.id, at)?;
                } else {
                    // A note whose folder has not arrived — or was deleted here
                    // — lands unfiled rather than failing the whole merge on a
                    // foreign key. Unfiled is a real, visible place; a note lost
                    // to a rolled-back transaction is not.
                    let filed: Option<String> = match &note.notebook_id {
                        Some(id) => tx
                            .query_row("SELECT id FROM notebooks WHERE id = ?1", [id], |row| {
                                row.get(0)
                            })
                            .optional()?,
                        None => None,
                    };
                    tx.execute(
                        "INSERT INTO notebook_notes
                             (id, notebook_id, title, body, pinned, created_at, updated_at, touched_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT (id) DO UPDATE SET notebook_id = excluded.notebook_id,
                             title = excluded.title, body = excluded.body, pinned = excluded.pinned,
                             updated_at = excluded.updated_at, touched_at = excluded.touched_at",
                        params![
                            note.id,
                            filed,
                            note.title,
                            note.body,
                            note.pinned as i64,
                            note.created_at,
                            note.updated_at,
                            note.touched_at
                        ],
                    )?;
                    tx.execute(
                        "DELETE FROM sync_tombstones WHERE kind = 'note' AND id = ?1",
                        [&note.id],
                    )?;
                }
                changed = true;
            }

            tx.commit()?;
            Ok(changed)
        })
        .map_err(|e| e.to_string())?;

    crate::settings::set(db, OWNER_KEY, &account_id.to_string())?;
    Ok(changed)
}
