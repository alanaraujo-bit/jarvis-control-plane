//! Project Brain (§36–§39) and Notes (§40) — the memory layer.
//!
//! ## The one distinction this module is built on
//!
//! **Knowledge is briefed to an agent. A note is not.**
//!
//! Knowledge is what stays true about a project: what it is, how it is built,
//! what will bite you, what a word means here. A note is working memory — a
//! reminder, a link, a thing to come back to.
//!
//! That single question decides which one something is, which is why there is
//! no per-item "include this" switch for somebody to forget to set. Handing an
//! agent a person's todo list as context would be worse than handing it
//! nothing: it is the kind of noise that makes a model confidently wrong.
//!
//! A note can be **promoted** into knowledge when it turns out to be durable,
//! which is the normal path for something learned in the middle of the work.
//!
//! ## Stated and derived are never mixed
//!
//! The Brain shows two kinds of thing and marks which is which, for the same
//! reason §28 stamps confidence on every number: *"nobody has committed to
//! `main` in three weeks"* and *"deploys go through the staging branch"* are
//! not the same kind of claim, and a surface that renders them identically is
//! inviting the reader to trust the weaker one as much as the stronger.
//!
//! - **Stated** knowledge is written by a person or an agent, and carries
//!   `source` so the reader can weigh it. An agent's claim about a project is
//!   not the owner's.
//! - **Derived** facts are computed from the append-only log every time they
//!   are asked for and are **never stored** (D2) — the same choice Review made.
//!   A stored derived fact is a fact that can go stale without anyone noticing.

pub mod brief;
pub mod derived;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;
use crate::session::log::now_ms;
use crate::AppState;

pub type Result<T> = std::result::Result<T, String>;

/// What a piece of knowledge is about.
///
/// A closed set, deliberately. An open tag field turns into forty synonyms for
/// the same idea and nothing can be grouped or briefed coherently. Serialised
/// through `as_str`, never a `rename_all` rule — one identity, one spelling
/// (D13).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// What this project *is*. The first thing anyone needs.
    What,
    /// How work is done here: conventions, house style, the way things are laid
    /// out.
    Convention,
    /// What will bite you. The most valuable kind, and the one that is usually
    /// only in somebody's head.
    Gotcha,
    /// What a word means *in this project*, where it differs from the usual.
    Glossary,
}

/// Every kind, in the order the Brain shows them.
///
/// The order is the order a newcomer needs them in — what it is, then how it is
/// done, then what will hurt, then what the words mean.
pub const KINDS: &[Kind] = &[Kind::What, Kind::Convention, Kind::Gotcha, Kind::Glossary];

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::What => "what",
            Self::Convention => "convention",
            Self::Gotcha => "gotcha",
            Self::Glossary => "glossary",
        }
    }
    pub fn parse(text: &str) -> Option<Self> {
        KINDS.iter().copied().find(|k| k.as_str() == text)
    }
}

impl Serialize for Kind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Kind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("unknown kind: {text}")))
    }
}

/// Who said it.
///
/// Kept because it changes how a reader should weigh the claim (§28). An agent
/// writing "the seed depends on ordering" is reporting something it inferred;
/// the owner writing it is stating something they know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Human,
    Agent,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Agent => "agent",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "agent" => Self::Agent,
            _ => Self::Human,
        }
    }
}

impl Serialize for Source {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        Ok(Self::parse(&String::deserialize(d)?))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Knowledge {
    pub id: String,
    pub project_id: String,
    pub kind: Kind,
    pub body: String,
    pub source: Source,
    /// The session that taught this, when an agent wrote it.
    pub session_id: Option<String>,
    pub mission_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Note {
    pub id: String,
    pub project_id: String,
    pub body: String,
    pub pinned: bool,
    pub mission_id: Option<String>,
    pub session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn row_to_knowledge(row: &rusqlite::Row<'_>) -> rusqlite::Result<Knowledge> {
    Ok(Knowledge {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        // A row written by a newer build may name a kind this one does not
        // know. Falling back keeps the rest readable rather than failing the
        // whole query — the same choice `guardrail::row_to_event` makes.
        kind: Kind::parse(&row.get::<_, String>("kind")?).unwrap_or(Kind::What),
        body: row.get("body")?,
        source: Source::parse(&row.get::<_, String>("source")?),
        session_id: row.get("session_id")?,
        mission_id: row.get("mission_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn row_to_note(row: &rusqlite::Row<'_>) -> rusqlite::Result<Note> {
    Ok(Note {
        id: row.get("id")?,
        project_id: row.get("project_id")?,
        body: row.get("body")?,
        pinned: row.get::<_, i64>("pinned")? != 0,
        mission_id: row.get("mission_id")?,
        session_id: row.get("session_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Everything stated about a project, oldest first within each kind.
///
/// Ordered by creation rather than by edit, so the Brain reads in the order it
/// was built up. Re-wording an entry should not move it to the bottom of its
/// section and quietly reshuffle a document somebody is reading.
pub fn knowledge(db: &Database, project_id: &str) -> Result<Vec<Knowledge>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT * FROM project_knowledge
              WHERE project_id = ?1 AND archived = 0
              ORDER BY created_at ASC",
        )?;
        let rows: rusqlite::Result<Vec<_>> =
            stmt.query_map([project_id], row_to_knowledge)?.collect();
        rows
    })
    .map_err(|e| e.to_string())
}

/// Notes for a project, pinned first and newest first within that.
pub fn notes(db: &Database, project_id: &str) -> Result<Vec<Note>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT * FROM project_notes
              WHERE project_id = ?1
              ORDER BY pinned DESC, updated_at DESC",
        )?;
        let rows: rusqlite::Result<Vec<_>> = stmt.query_map([project_id], row_to_note)?.collect();
        rows
    })
    .map_err(|e| e.to_string())
}

/// Record something known about a project.
pub fn add_knowledge(
    db: &Database,
    project_id: &str,
    kind: Kind,
    body: &str,
    source: Source,
    session_id: Option<&str>,
    mission_id: Option<&str>,
) -> Result<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err("brain.emptyBody".into());
    }
    let id = uuid::Uuid::now_v7().to_string();
    let stored = id.clone();
    let (project_id, body) = (project_id.to_string(), body.to_string());
    let (session_id, mission_id) = (
        session_id.map(str::to_string),
        mission_id.map(str::to_string),
    );

    db.with(move |conn| {
        let now = now_ms();
        conn.execute(
            "INSERT INTO project_knowledge
                 (id, project_id, kind, body, source, session_id, mission_id,
                  created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
            params![
                stored,
                project_id,
                kind.as_str(),
                body,
                source.as_str(),
                session_id,
                mission_id,
                now
            ],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(id)
}

/// Re-word an entry, or move it to another kind.
pub fn update_knowledge(db: &Database, id: &str, kind: Kind, body: &str) -> Result<()> {
    let body = body.trim();
    if body.is_empty() {
        return Err("brain.emptyBody".into());
    }
    let (id, body) = (id.to_string(), body.to_string());
    db.with(move |conn| {
        conn.execute(
            "UPDATE project_knowledge SET kind = ?2, body = ?3, updated_at = ?4 WHERE id = ?1",
            params![id, kind.as_str(), body, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Retire an entry that has stopped being true.
///
/// Archived rather than deleted. Knowledge that was true and stopped being true
/// is still a fact about the project, and this table is small — the cost of
/// keeping it is nothing next to losing the record of what people used to
/// believe about their own system.
pub fn archive_knowledge(db: &Database, id: &str) -> Result<()> {
    let id = id.to_string();
    db.with(move |conn| {
        conn.execute(
            "UPDATE project_knowledge SET archived = 1, updated_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub fn add_note(
    db: &Database,
    project_id: &str,
    body: &str,
    mission_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<String> {
    let body = body.trim();
    if body.is_empty() {
        return Err("brain.emptyBody".into());
    }
    let id = uuid::Uuid::now_v7().to_string();
    let stored = id.clone();
    let (project_id, body) = (project_id.to_string(), body.to_string());
    let (mission_id, session_id) = (
        mission_id.map(str::to_string),
        session_id.map(str::to_string),
    );

    db.with(move |conn| {
        let now = now_ms();
        conn.execute(
            "INSERT INTO project_notes
                 (id, project_id, body, mission_id, session_id, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![stored, project_id, body, mission_id, session_id, now],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    Ok(id)
}

pub fn update_note(db: &Database, id: &str, body: &str) -> Result<()> {
    let body = body.trim();
    if body.is_empty() {
        return Err("brain.emptyBody".into());
    }
    let (id, body) = (id.to_string(), body.to_string());
    db.with(move |conn| {
        conn.execute(
            "UPDATE project_notes SET body = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, body, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub fn set_note_pinned(db: &Database, id: &str, pinned: bool) -> Result<()> {
    let id = id.to_string();
    db.with(move |conn| {
        conn.execute(
            "UPDATE project_notes SET pinned = ?2 WHERE id = ?1",
            params![id, pinned as i64],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// A note is genuinely deleted, unlike knowledge.
///
/// The asymmetry is deliberate. A note is a scratchpad entry whose whole
/// purpose is to be disposable; keeping every crossed-off reminder forever
/// would make the surface useless for the thing it is for.
pub fn delete_note(db: &Database, id: &str) -> Result<()> {
    let id = id.to_string();
    db.with(move |conn| {
        conn.execute("DELETE FROM project_notes WHERE id = ?1", [&id])?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Turn a note into knowledge.
///
/// The normal path for something learned in the middle of the work: it is
/// jotted down where jotting is cheap, and moved once it turns out to be
/// durable. The note is removed, because leaving both would put the same
/// sentence in two places and let them drift apart.
pub fn promote_note(db: &Database, id: &str, kind: Kind) -> Result<String> {
    let note = db
        .with(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM project_notes WHERE id = ?1")?;
            let mut rows = stmt.query_map([id], row_to_note)?;
            rows.next().transpose()
        })
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "brain.noSuchNote".to_string())?;

    let new_id = add_knowledge(
        db,
        &note.project_id,
        kind,
        &note.body,
        // A note is something the person wrote, so the knowledge it becomes is
        // theirs. An agent's own note would have arrived through `add_note`
        // with a session, and that provenance travels below.
        Source::Human,
        note.session_id.as_deref(),
        note.mission_id.as_deref(),
    )?;
    delete_note(db, id)?;
    Ok(new_id)
}

/// Everything the Brain shows for one project.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrainReport {
    pub knowledge: Vec<Knowledge>,
    pub notes: Vec<Note>,
    /// Computed now, never stored (D2/§28).
    pub facts: Vec<derived::Fact>,
    /// What has happened in this project, newest first (§39).
    pub timeline: Vec<derived::Moment>,
    /// How many characters an agent would be briefed with, so the cost of the
    /// brain is visible rather than a surprise in a token bill.
    pub brief_size: usize,
}

pub fn report(state: &AppState, project_id: &str) -> Result<BrainReport> {
    let knowledge = knowledge(&state.db, project_id)?;
    let brief_size = brief::compose(&knowledge).map(|b| b.len()).unwrap_or(0);
    Ok(BrainReport {
        notes: notes(&state.db, project_id)?,
        facts: derived::facts(&state.db, project_id)?,
        timeline: derived::timeline(&state.db, project_id)?,
        brief_size,
        knowledge,
    })
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
pub fn brain_report(state: State<'_, AppState>, project_id: String) -> Result<BrainReport> {
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_add_knowledge(
    state: State<'_, AppState>,
    project_id: String,
    kind: Kind,
    body: String,
) -> Result<BrainReport> {
    add_knowledge(&state.db, &project_id, kind, &body, Source::Human, None, None)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_update_knowledge(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
    kind: Kind,
    body: String,
) -> Result<BrainReport> {
    update_knowledge(&state.db, &id, kind, &body)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_archive_knowledge(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
) -> Result<BrainReport> {
    archive_knowledge(&state.db, &id)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_add_note(
    state: State<'_, AppState>,
    project_id: String,
    body: String,
) -> Result<BrainReport> {
    add_note(&state.db, &project_id, &body, None, None)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_update_note(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
    body: String,
) -> Result<BrainReport> {
    update_note(&state.db, &id, &body)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_set_note_pinned(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
    pinned: bool,
) -> Result<BrainReport> {
    set_note_pinned(&state.db, &id, pinned)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_delete_note(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
) -> Result<BrainReport> {
    delete_note(&state.db, &id)?;
    report(&state, &project_id)
}

#[tauri::command]
pub fn brain_promote_note(
    state: State<'_, AppState>,
    project_id: String,
    id: String,
    kind: Kind,
) -> Result<BrainReport> {
    promote_note(&state.db, &id, kind)?;
    report(&state, &project_id)
}

/// The exact text an agent would be given, for the person to read first.
///
/// A briefing nobody can inspect is a briefing nobody can trust — the same
/// reason the guardrail shows the command it is about to run verbatim (§35).
#[tauri::command]
pub fn brain_preview_brief(state: State<'_, AppState>, project_id: String) -> Result<Option<String>> {
    Ok(brief::compose(&knowledge(&state.db, &project_id)?))
}

#[cfg(test)]
mod tests;
