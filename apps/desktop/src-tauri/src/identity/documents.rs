//! Everything else a person sets up, carried by their account (M27).
//!
//! M23 carries nine preferences and the notebook. This carries the rest as
//! last-write-wins documents (`docs/M27-ACCOUNT-SYNC.md`): a few more
//! settings, projects, project notes and knowledge, guardrail policies and the
//! provider-account registry. The kinds and setting keys are closed lists,
//! mirrored by `apps/social-api/src/documents.ts`.
//!
//! ## A ledger instead of a clock on every table
//!
//! None of those tables records when a row last changed in a way two machines
//! can compare. `sync_documents` does it for them: each push hashes the
//! current documents, a changed hash takes `touched_at = now`, and a document
//! that disappeared becomes a tombstone. A remote document is applied only
//! when it is newer than the ledger's entry, and the ledger then takes its
//! hash and time — so applying is never mistaken for a local change.
//!
//! A remote document this machine cannot represent yet (a project whose folder
//! is not here, a note for such a project) is **held**: kept in the ledger,
//! retried on every pull, and never tombstoned.
//!
//! Provider logins never cross this boundary: the registry travels, and an
//! account arrives signed out with an empty directory of its own.

use std::collections::HashMap;
use std::path::Path;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::Result;
use crate::db::Database;

/// Settings carried as documents: the core-owned ones M23 never carried.
pub const SETTING_KEYS: &[&str] = &[
    crate::accounts::rotation::policy::POLICY_KEY,
    "autonomy.global",
    crate::settings::START_MINIMIZED_KEY,
    "localai.runtime",
    "workspace.snapshot.v1",
];

/// The order a pull applies kinds in: a note needs its project first.
const APPLY_ORDER: &[&str] = &["setting", "providerAccount", "project", "projectNote", "projectKnowledge", "guardrailPolicy"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub kind: String,
    pub id: String,
    pub body: Value,
    pub touched_at: i64,
    #[serde(default)]
    pub deleted_at: Option<i64>,
}

fn hash(body: &Value) -> String {
    // `serde_json::Map` is ordered by key here, so the text is canonical and a
    // body that came back from Postgres hashes like the one that went up.
    let digest = Sha256::digest(body.to_string().as_bytes());
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A folder spelled one way per folder, for matching projects by path.
fn path_key(path: &str) -> String {
    let text = path.replace('\\', "/");
    let text = text.trim_end_matches('/');
    if cfg!(windows) {
        text.to_lowercase()
    } else {
        text.to_string()
    }
}

/// What this machine has now, as (kind, id) -> body.
fn current(db: &Database) -> Result<HashMap<(String, String), Value>> {
    db.with(|conn| {
        let mut out = HashMap::new();
        for key in SETTING_KEYS {
            let raw: Option<String> = conn
                .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
                .optional()?;
            if let Some(value) = raw.and_then(|raw| serde_json::from_str::<Value>(&raw).ok()) {
                out.insert(("setting".to_string(), key.to_string()), value);
            }
        }
        let mut stmt = conn.prepare(
            "SELECT id, name, path, autonomy, archived FROM projects WHERE worktree_of IS NULL",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, json!({
                "name": r.get::<_, String>(1)?,
                "path": r.get::<_, String>(2)?,
                "autonomy": r.get::<_, Option<String>>(3)?,
                "archived": r.get::<_, i64>(4)? != 0,
            })))
        })? {
            let (id, body) = row?;
            out.insert(("project".to_string(), id), body);
        }
        let mut stmt = conn.prepare(
            "SELECT n.id, p.path, n.body, n.pinned, n.created_at, n.updated_at
               FROM project_notes n JOIN projects p ON p.id = n.project_id",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, json!({
                "projectPath": r.get::<_, String>(1)?,
                "body": r.get::<_, String>(2)?,
                "pinned": r.get::<_, i64>(3)? != 0,
                "createdAt": r.get::<_, i64>(4)?,
                "updatedAt": r.get::<_, i64>(5)?,
            })))
        })? {
            let (id, body) = row?;
            out.insert(("projectNote".to_string(), id), body);
        }
        let mut stmt = conn.prepare(
            "SELECT k.id, p.path, k.kind, k.body, k.source, k.archived, k.created_at, k.updated_at
               FROM project_knowledge k JOIN projects p ON p.id = k.project_id",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, json!({
                "projectPath": r.get::<_, String>(1)?,
                "kind": r.get::<_, String>(2)?,
                "body": r.get::<_, String>(3)?,
                "source": r.get::<_, String>(4)?,
                "archived": r.get::<_, i64>(5)? != 0,
                "createdAt": r.get::<_, i64>(6)?,
                "updatedAt": r.get::<_, i64>(7)?,
            })))
        })? {
            let (id, body) = row?;
            out.insert(("projectKnowledge".to_string(), id), body);
        }
        let mut stmt = conn.prepare(
            "SELECT g.id, p.path, g.operation, g.decision, g.created_at, g.updated_at
               FROM guardrail_policies g LEFT JOIN projects p ON p.id = g.project_id",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, json!({
                "projectPath": r.get::<_, Option<String>>(1)?,
                "operation": r.get::<_, String>(2)?,
                "decision": r.get::<_, String>(3)?,
                "createdAt": r.get::<_, i64>(4)?,
                "updatedAt": r.get::<_, i64>(5)?,
            })))
        })? {
            let (id, body) = row?;
            out.insert(("guardrailPolicy".to_string(), id), body);
        }
        // A copied login is a copy of this machine's; it is not the person's to carry.
        let mut stmt = conn.prepare(
            "SELECT id, provider, label, email, position, paused FROM provider_accounts
              WHERE credential_origin = 'own'",
        )?;
        for row in stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, json!({
                "provider": r.get::<_, String>(1)?,
                "label": r.get::<_, String>(2)?,
                "email": r.get::<_, Option<String>>(3)?,
                "position": r.get::<_, i64>(4)?,
                "paused": r.get::<_, i64>(5)? != 0,
            })))
        })? {
            let (id, body) = row?;
            out.insert(("providerAccount".to_string(), id), body);
        }
        Ok(out)
    })
    .map_err(|e| e.to_string())
}

struct Entry {
    hash: String,
    body: Option<String>,
    touched_at: i64,
    deleted_at: Option<i64>,
    held: bool,
}

fn ledger(db: &Database) -> Result<HashMap<(String, String), Entry>> {
    db.with(|conn| {
        let mut stmt = conn.prepare("SELECT kind, id, hash, body, touched_at, deleted_at, held FROM sync_documents")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                (r.get::<_, String>(0)?, r.get::<_, String>(1)?),
                Entry {
                    hash: r.get(2)?,
                    body: r.get(3)?,
                    touched_at: r.get(4)?,
                    deleted_at: r.get(5)?,
                    held: r.get::<_, i64>(6)? != 0,
                },
            ))
        })?;
        rows.collect()
    })
    .map_err(|e| e.to_string())
}

fn record(db: &Database, kind: &str, id: &str, body: &Value, touched_at: i64, deleted_at: Option<i64>, held: bool) -> Result<()> {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO sync_documents (kind, id, hash, body, touched_at, deleted_at, held)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (kind, id) DO UPDATE SET hash = excluded.hash, body = excluded.body,
               touched_at = excluded.touched_at, deleted_at = excluded.deleted_at, held = excluded.held",
            params![kind, id, hash(body), body.to_string(), touched_at, deleted_at, held as i64],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Bring the ledger up to date with this machine and return what to push:
/// every document this machine represents, plus tombstones. Held documents
/// came from the server and are not sent back.
pub fn outgoing(db: &Database, now_ms: i64) -> Result<Vec<Document>> {
    let now = current(db)?;
    let known = ledger(db)?;
    for ((kind, id), body) in &now {
        let entry = known.get(&(kind.clone(), id.clone()));
        let changed = entry.is_none_or(|e| e.hash != hash(body) || e.deleted_at.is_some() || e.held);
        if changed {
            // A held document that is now here unchanged keeps the clock it
            // arrived with; anything else changed on this machine just now.
            let touched = match entry {
                Some(e) if e.held && e.deleted_at.is_none() && e.hash == hash(body) => e.touched_at,
                _ => now_ms,
            };
            record(db, kind, id, body, touched, None, false)?;
        }
    }
    for ((kind, id), entry) in &known {
        if !entry.held && entry.deleted_at.is_none() && !now.contains_key(&(kind.clone(), id.clone())) {
            record(db, kind, id, &Value::Null, now_ms, Some(now_ms), false)?;
        }
    }
    Ok(ledger(db)?
        .into_iter()
        .filter(|(_, e)| !e.held)
        .map(|((kind, id), e)| Document {
            body: if e.deleted_at.is_some() {
                Value::Null
            } else {
                now.get(&(kind.clone(), id.clone())).cloned().unwrap_or(Value::Null)
            },
            kind,
            id,
            touched_at: e.touched_at,
            deleted_at: e.deleted_at,
        })
        .collect())
}

enum Applied {
    Done,
    /// Not representable here yet; keep it and try again on the next pull.
    Held,
}

fn str_of<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key).and_then(Value::as_str)
}

fn project_id_for(conn: &rusqlite::Connection, path: &str) -> rusqlite::Result<Option<String>> {
    let wanted = path_key(path);
    let mut stmt = conn.prepare("SELECT id, path FROM projects")?;
    let rows: Vec<(String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(rows.into_iter().find(|(_, p)| path_key(p) == wanted).map(|(id, _)| id))
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn apply_one(db: &Database, data_dir: &Path, doc: &Document) -> Result<Applied> {
    let body = &doc.body;
    let deleted = doc.deleted_at.is_some();
    match doc.kind.as_str() {
        "setting" if SETTING_KEYS.contains(&doc.id.as_str()) => {
            if deleted {
                crate::settings::clear(db, &doc.id).map_err(|e| e.to_string())?;
            } else {
                db.with(|c| {
                    c.execute(
                        "INSERT INTO settings (key, value) VALUES (?1, ?2)
                         ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                        params![doc.id, body.to_string()],
                    )?;
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            }
            Ok(Applied::Done)
        }
        "project" => {
            if deleted {
                // Archived, never unregistered: the folder and its history stay.
                db.with(|c| c.execute("UPDATE projects SET archived = 1 WHERE id = ?1", [&doc.id]))
                    .map_err(|e| e.to_string())?;
                return Ok(Applied::Done);
            }
            let Some(path) = str_of(body, "path") else { return Ok(Applied::Done) };
            let name = str_of(body, "name").unwrap_or(path).to_string();
            let autonomy = str_of(body, "autonomy").map(str::to_string);
            let archived = body.get("archived").and_then(Value::as_bool).unwrap_or(false) as i64;
            db.with(|c| {
                let by_id: Option<String> =
                    c.query_row("SELECT id FROM projects WHERE id = ?1", [&doc.id], |r| r.get(0)).optional()?;
                if let Some(id) = by_id.or(project_id_for(c, path)?) {
                    c.execute(
                        "UPDATE projects SET name = ?2, autonomy = ?3, archived = ?4 WHERE id = ?1",
                        params![id, name, autonomy, archived],
                    )?;
                    return Ok(Applied::Done);
                }
                if !Path::new(path).is_dir() {
                    return Ok(Applied::Held);
                }
                let ts = now_ms();
                c.execute(
                    "INSERT INTO projects (id, name, path, is_git, created_at, last_opened_at, archived, autonomy)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
                    params![doc.id, name, path, Path::new(path).join(".git").exists() as i64, ts, archived, autonomy],
                )?;
                Ok(Applied::Done)
            })
            .map_err(|e| e.to_string())
        }
        "projectNote" | "projectKnowledge" => db
            .with(|c| {
                let table = if doc.kind == "projectNote" { "project_notes" } else { "project_knowledge" };
                if deleted {
                    c.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [&doc.id])?;
                    return Ok(Applied::Done);
                }
                let Some(project) = str_of(body, "projectPath").map(|p| project_id_for(c, p)).transpose()?.flatten() else {
                    return Ok(Applied::Held);
                };
                let created = body.get("createdAt").and_then(Value::as_i64).unwrap_or(doc.touched_at);
                let updated = body.get("updatedAt").and_then(Value::as_i64).unwrap_or(doc.touched_at);
                let text = str_of(body, "body").unwrap_or_default();
                if doc.kind == "projectNote" {
                    c.execute(
                        "INSERT INTO project_notes (id, project_id, body, pinned, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                         ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id, body = excluded.body,
                           pinned = excluded.pinned, updated_at = excluded.updated_at",
                        params![doc.id, project, text, body.get("pinned").and_then(Value::as_bool).unwrap_or(false) as i64, created, updated],
                    )?;
                } else {
                    c.execute(
                        "INSERT INTO project_knowledge (id, project_id, kind, body, source, archived, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                         ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id, kind = excluded.kind,
                           body = excluded.body, source = excluded.source, archived = excluded.archived,
                           updated_at = excluded.updated_at",
                        params![
                            doc.id,
                            project,
                            str_of(body, "kind").unwrap_or("what"),
                            text,
                            str_of(body, "source").unwrap_or("human"),
                            body.get("archived").and_then(Value::as_bool).unwrap_or(false) as i64,
                            created,
                            updated
                        ],
                    )?;
                }
                Ok(Applied::Done)
            })
            .map_err(|e| e.to_string()),
        "guardrailPolicy" => db
            .with(|c| {
                if deleted {
                    c.execute("DELETE FROM guardrail_policies WHERE id = ?1", [&doc.id])?;
                    return Ok(Applied::Done);
                }
                let project = match str_of(body, "projectPath") {
                    None => None,
                    Some(path) => match project_id_for(c, path)? {
                        Some(id) => Some(id),
                        None => return Ok(Applied::Held),
                    },
                };
                let (Some(operation), Some(decision)) = (str_of(body, "operation"), str_of(body, "decision")) else {
                    return Ok(Applied::Done);
                };
                let created = body.get("createdAt").and_then(Value::as_i64).unwrap_or(doc.touched_at);
                let updated = body.get("updatedAt").and_then(Value::as_i64).unwrap_or(doc.touched_at);
                c.execute(
                    "INSERT INTO guardrail_policies (id, project_id, operation, decision, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (id) DO UPDATE SET project_id = excluded.project_id, operation = excluded.operation,
                       decision = excluded.decision, updated_at = excluded.updated_at",
                    params![doc.id, project, operation, decision, created, updated],
                )?;
                Ok(Applied::Done)
            })
            .map_err(|e| e.to_string()),
        "providerAccount" => {
            // Never deleted from here: the account's directory holds a login.
            if deleted {
                return Ok(Applied::Done);
            }
            let (Some(provider), label) = (str_of(body, "provider"), str_of(body, "label").unwrap_or_default()) else {
                return Ok(Applied::Done);
            };
            let position = body.get("position").and_then(Value::as_i64).unwrap_or(0);
            let paused = body.get("paused").and_then(Value::as_bool).unwrap_or(false);
            if crate::accounts::get(db, &doc.id)?.is_some() {
                db.with(|c| {
                    c.execute(
                        "UPDATE provider_accounts SET label = ?2, position = ?3, paused = ?4 WHERE id = ?1",
                        params![doc.id, label, position, paused as i64],
                    )
                })
                .map_err(|e| e.to_string())?;
                return Ok(Applied::Done);
            }
            let email = str_of(body, "email");
            let same = crate::accounts::list(db)?.into_iter().any(|a| {
                a.provider == provider && email.is_some() && a.email.as_deref().map(str::to_lowercase) == email.map(str::to_lowercase)
            });
            if same {
                // This machine already has that subscription under its own id.
                return Ok(Applied::Held);
            }
            crate::accounts::restore(db, data_dir, &doc.id, provider, label, email, position, paused)?;
            Ok(Applied::Done)
        }
        _ => Ok(Applied::Done),
    }
}

/// Fold the account's documents into this machine. Returns whether anything
/// changed here. Held documents from earlier pulls are retried too.
pub fn apply(db: &Database, data_dir: &Path, remote: &[Document]) -> Result<bool> {
    let known = ledger(db)?;
    let mut todo: Vec<Document> = remote
        .iter()
        .filter(|doc| known.get(&(doc.kind.clone(), doc.id.clone())).is_none_or(|e| doc.touched_at > e.touched_at))
        .cloned()
        .collect();
    for ((kind, id), entry) in &known {
        let fresher = remote.iter().any(|d| &d.kind == kind && &d.id == id && d.touched_at > entry.touched_at);
        if entry.held && !fresher {
            let body = entry.body.as_deref().and_then(|b| serde_json::from_str(b).ok()).unwrap_or(Value::Null);
            todo.push(Document { kind: kind.clone(), id: id.clone(), body, touched_at: entry.touched_at, deleted_at: entry.deleted_at });
        }
    }
    todo.sort_by_key(|doc| APPLY_ORDER.iter().position(|k| *k == doc.kind).unwrap_or(usize::MAX));
    let mut changed = false;
    for doc in &todo {
        let held = matches!(apply_one(db, data_dir, doc)?, Applied::Held);
        changed |= !held;
        record(db, &doc.kind, &doc.id, &doc.body, doc.touched_at, doc.deleted_at, held)?;
    }
    Ok(changed)
}

/// A different person signed in on this machine: their account has never seen
/// this ledger's clocks, so it starts over (the data itself is left alone).
pub fn reset_ledger(db: &Database) -> Result<()> {
    db.with(|c| c.execute("DELETE FROM sync_documents", []).map(|_| ()))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "documents_tests.rs"]
mod tests;
