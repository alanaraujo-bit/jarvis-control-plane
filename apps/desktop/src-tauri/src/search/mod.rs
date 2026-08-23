//! Global Search (§51).
//!
//! The one question the rest of the memory layer (§36–§40), missions (§29) and
//! activity (§48) could not yet answer: **where did I see that?**
//!
//! Nothing here is a second store. Knowledge, notes, missions and activity are
//! searched where they already live, with a plain `LIKE` — these tables are
//! small even for a heavy user, so an index would be machinery this product
//! does not need yet (the same restraint D22 applies to derived facts).
//!
//! Conversation content is the one source large enough, across enough
//! sessions, to want a real index: `session_events` (see `session::transcript`
//! for the write side) mirrors what an agent said, thought, ran and got back,
//! and `session_events_fts` is an FTS5 index over it.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;
use crate::AppState;

pub type Result<T> = std::result::Result<T, String>;

/// A query shorter than this is not a search, it is a browse — and a
/// single-character `LIKE '%a%'` would return most of the database.
const MIN_QUERY_CHARS: usize = 2;

/// How many rows each source contributes at most, before the combined list is
/// sorted and shown. A search box is not a report; a handful of good matches
/// per kind beats an exhaustive list nobody scrolls to the end of.
const PER_SOURCE_LIMIT: i64 = 8;

/// Characters either side of a match kept in a snippet.
const SNIPPET_RADIUS: usize = 60;

/// Where a result came from.
///
/// A closed set, serialised through `as_str` rather than a `rename_all` rule —
/// the same choice `brain::Kind` makes, for the same reason (D13): one
/// identity, one spelling, shared by storage, the wire format and the i18n
/// keys the surface uses to label each group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Knowledge,
    Note,
    Mission,
    Activity,
    Conversation,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Knowledge => "knowledge",
            Self::Note => "note",
            Self::Mission => "mission",
            Self::Activity => "activity",
            Self::Conversation => "conversation",
        }
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
        match text.as_str() {
            "knowledge" => Ok(Self::Knowledge),
            "note" => Ok(Self::Note),
            "mission" => Ok(Self::Mission),
            "activity" => Ok(Self::Activity),
            "conversation" => Ok(Self::Conversation),
            other => Err(serde::de::Error::custom(format!("unknown search kind: {other}"))),
        }
    }
}

/// One match, wherever it came from.
///
/// Deliberately carries raw identifiers rather than assembled sentences —
/// `subKind`, `label` and `kind` are codes the surface localises (§65); only
/// `heading` and `snippet` are prose, and both are the *content itself*
/// (somebody's note, an agent's own words), never text this crate composed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub kind: Kind,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    /// The id of the underlying row: a knowledge id, a note id, a mission id,
    /// an activity id, or a session id for a conversation match.
    pub entity_id: String,
    /// Set for a conversation match: which session it happened in, and which
    /// provider ran it, so the surface can open (or re-open) that session
    /// rather than only naming the project it happened in.
    pub session_id: Option<String>,
    pub session_provider: Option<String>,
    pub mission_id: Option<String>,
    pub ts_ms: i64,
    /// A code, not free text: `brain.kind.*` for knowledge, a mission status,
    /// an activity kind, or a `ConversationItem` tag (message | thinking |
    /// toolCall | toolResult | error).
    pub sub_kind: Option<String>,
    /// Who or what, when a kind needs to say and `subKind` does not carry it:
    /// the speaking role, a tool name, ok/error, pinned.
    pub label: Option<String>,
    /// Free text the entity already carries, e.g. a mission's own title. Never
    /// assembled here (§65) — empty when the entity has none of its own.
    pub heading: String,
    /// An excerpt of the matched content, centred on the match where one was
    /// found.
    pub snippet: String,
}

fn like_pattern(query: &str) -> String {
    // `%` and `_` are LIKE wildcards; a search for either character must match
    // it literally; `\` is the escape character chosen below and must itself
    // be escaped first, or a literal backslash in the query would escape the
    // wrong thing.
    let escaped = query.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// An excerpt of `haystack` centred on the first case-insensitive occurrence
/// of `needle`, or the start of `haystack` if the two disagree (a LIKE match
/// and a Rust find can differ on Unicode case-folding at the edges).
///
/// Byte offsets from `str::find` are mapped back to `char` boundaries before
/// slicing — this project's text is read in Portuguese and English both, and
/// slicing on a byte offset that lands mid-character panics.
fn snippet_around(haystack: &str, needle: &str) -> String {
    let haystack = haystack.trim();
    let chars: Vec<char> = haystack.chars().collect();

    let match_char_index = if needle.is_empty() {
        None
    } else {
        let lower_hay = haystack.to_lowercase();
        let lower_needle = needle.to_lowercase();
        lower_hay.find(&lower_needle).map(|byte_pos| {
            haystack
                .char_indices()
                .position(|(b, _)| b >= byte_pos)
                .unwrap_or(0)
        })
    };

    let (start, end) = match match_char_index {
        Some(index) => (
            index.saturating_sub(SNIPPET_RADIUS),
            (index + needle.chars().count() + SNIPPET_RADIUS).min(chars.len()),
        ),
        None => (0, (SNIPPET_RADIUS * 2).min(chars.len())),
    };

    let slice: String = chars[start..end].iter().collect();
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < chars.len() { "…" } else { "" };
    format!("{prefix}{slice}{suffix}")
}

/// Turn a query into an FTS5 `MATCH` expression that cannot be read as
/// anything but a plain-text search.
///
/// Every token is wrapped as a quoted phrase with a trailing prefix `*`, so a
/// query carrying a bare `"`, `:`, `-` or a boolean keyword can never be
/// parsed as FTS5 query syntax (a column filter, `NEAR`, `OR`, …) — it is
/// always just text to find.
fn fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|token| format!("\"{}\"*", token.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn knowledge_matches(conn: &Connection, like: &str, out: &mut Vec<SearchResult>) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT k.id, k.project_id, p.name, k.kind, k.body, k.updated_at
           FROM project_knowledge k
           JOIN projects p ON p.id = k.project_id
          WHERE k.archived = 0 AND k.body LIKE ?1 ESCAPE '\\'
          ORDER BY k.updated_at DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![like, PER_SOURCE_LIMIT], |row| {
        let body: String = row.get("body")?;
        Ok(SearchResult {
            kind: Kind::Knowledge,
            project_id: row.get("project_id")?,
            project_name: row.get(2)?,
            entity_id: row.get("id")?,
            session_id: None,
            session_provider: None,
            mission_id: None,
            ts_ms: row.get("updated_at")?,
            sub_kind: Some(row.get("kind")?),
            label: None,
            heading: String::new(),
            snippet: body,
        })
    })?;
    for row in rows {
        out.push(row?);
    }
    Ok(())
}

fn note_matches(conn: &Connection, like: &str, out: &mut Vec<SearchResult>) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT n.id, n.project_id, p.name, n.body, n.pinned, n.updated_at
           FROM project_notes n
           JOIN projects p ON p.id = n.project_id
          WHERE n.body LIKE ?1 ESCAPE '\\'
          ORDER BY n.updated_at DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![like, PER_SOURCE_LIMIT], |row| {
        let body: String = row.get("body")?;
        let pinned: i64 = row.get("pinned")?;
        Ok(SearchResult {
            kind: Kind::Note,
            project_id: row.get("project_id")?,
            project_name: row.get(2)?,
            entity_id: row.get("id")?,
            session_id: None,
            session_provider: None,
            mission_id: None,
            ts_ms: row.get("updated_at")?,
            sub_kind: None,
            label: if pinned != 0 { Some("pinned".into()) } else { None },
            heading: String::new(),
            snippet: body,
        })
    })?;
    for row in rows {
        out.push(row?);
    }
    Ok(())
}

fn mission_matches(
    conn: &Connection,
    like: &str,
    query: &str,
    out: &mut Vec<SearchResult>,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT m.id, m.project_id, p.name, m.title, m.goal, m.description,
                m.status, m.updated_at
           FROM missions m
           JOIN projects p ON p.id = m.project_id
          WHERE m.title LIKE ?1 ESCAPE '\\'
             OR m.goal LIKE ?1 ESCAPE '\\'
             OR m.description LIKE ?1 ESCAPE '\\'
          ORDER BY m.updated_at DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![like, PER_SOURCE_LIMIT], |row| {
        let title: String = row.get("title")?;
        let goal: Option<String> = row.get("goal")?;
        let description: Option<String> = row.get("description")?;
        // Show the field that actually matched, not always the title — a hit
        // buried in a long description is exactly what a title-only result
        // would hide.
        let snippet_source = [goal.as_deref(), description.as_deref()]
            .into_iter()
            .flatten()
            .find(|text| text.to_lowercase().contains(&query.to_lowercase()))
            .unwrap_or(&title);
        let snippet = snippet_around(snippet_source, query);
        Ok(SearchResult {
            kind: Kind::Mission,
            project_id: row.get("project_id")?,
            project_name: row.get(2)?,
            entity_id: row.get("id")?,
            session_id: None,
            session_provider: None,
            mission_id: Some(row.get("id")?),
            ts_ms: row.get("updated_at")?,
            sub_kind: Some(row.get("status")?),
            label: None,
            heading: title,
            snippet,
        })
    })?;
    for row in rows {
        out.push(row?);
    }
    Ok(())
}

fn activity_matches(
    conn: &Connection,
    like: &str,
    query: &str,
    out: &mut Vec<SearchResult>,
) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.project_id, p.name, a.mission_id, a.kind, a.severity,
                a.title, a.detail, a.ts_ms
           FROM activity a
           LEFT JOIN projects p ON p.id = a.project_id
          WHERE a.title LIKE ?1 ESCAPE '\\' OR a.detail LIKE ?1 ESCAPE '\\'
          ORDER BY a.ts_ms DESC
          LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![like, PER_SOURCE_LIMIT], |row| {
        let title: String = row.get("title")?;
        let detail: Option<String> = row.get("detail")?;
        let snippet_source = detail.as_deref().unwrap_or(&title);
        let snippet = snippet_around(snippet_source, query);
        Ok(SearchResult {
            kind: Kind::Activity,
            project_id: row.get("project_id")?,
            project_name: row.get(2)?,
            entity_id: row.get::<_, i64>("id")?.to_string(),
            session_id: None,
            session_provider: None,
            mission_id: row.get("mission_id")?,
            ts_ms: row.get("ts_ms")?,
            sub_kind: Some(row.get("kind")?),
            label: Some(row.get("severity")?),
            heading: title,
            snippet,
        })
    })?;
    for row in rows {
        out.push(row?);
    }
    Ok(())
}

/// Conversation content, via the FTS5 index `session::transcript` maintains.
///
/// Failures here are logged and swallowed rather than propagated: a query the
/// tokeniser dislikes must not take down the other four sources, which are
/// plain parameterised SQL and cannot fail on the text itself.
fn conversation_matches(conn: &Connection, query: &str, out: &mut Vec<SearchResult>) {
    let match_expr = fts_query(query);
    if match_expr.is_empty() {
        return;
    }

    let result = (|| -> rusqlite::Result<Vec<SearchResult>> {
        let mut stmt = conn.prepare(
            "SELECT f.session_id, f.ts_ms, f.project_id, f.kind, f.label,
                    snippet(session_events_fts, 5, '', '', '…', 12) AS excerpt,
                    p.name AS project_name, s.provider, s.title
               FROM session_events_fts f
               LEFT JOIN projects p ON p.id = f.project_id
               LEFT JOIN sessions s ON s.id = f.session_id
              WHERE session_events_fts MATCH ?1
              ORDER BY bm25(session_events_fts)
              LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![match_expr, PER_SOURCE_LIMIT], |row| {
            Ok(SearchResult {
                kind: Kind::Conversation,
                project_id: row.get("project_id")?,
                project_name: row.get("project_name")?,
                entity_id: row.get("session_id")?,
                session_id: row.get("session_id")?,
                session_provider: row.get("provider")?,
                mission_id: None,
                ts_ms: row.get("ts_ms")?,
                sub_kind: Some(row.get("kind")?),
                label: row.get("label")?,
                heading: row.get::<_, Option<String>>("title")?.unwrap_or_default(),
                snippet: row.get("excerpt")?,
            })
        })?;
        rows.collect()
    })();

    match result {
        Ok(rows) => out.extend(rows),
        Err(e) => tracing::warn!(error = %e, query, "conversation search failed"),
    }
}

/// Everything that matches `query`, newest first, across every project.
///
/// "Global" means every project, deliberately: the question this answers is
/// "where did I see that", and scoping to whichever project happens to be open
/// would silently miss the answer whenever it lives somewhere else.
pub fn search(db: &Database, query: &str) -> Result<Vec<SearchResult>> {
    let query = query.trim();
    if query.chars().filter(|c| !c.is_whitespace()).count() < MIN_QUERY_CHARS {
        return Ok(Vec::new());
    }
    let like = like_pattern(query);

    let mut out = Vec::new();
    db.with(|conn| {
        knowledge_matches(conn, &like, &mut out)?;
        note_matches(conn, &like, &mut out)?;
        mission_matches(conn, &like, query, &mut out)?;
        activity_matches(conn, &like, query, &mut out)?;
        conversation_matches(conn, query, &mut out);
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    out.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms));
    Ok(out)
}

// ---- Commands ---------------------------------------------------------------

#[tauri::command]
pub fn global_search(state: State<'_, AppState>, query: String) -> Result<Vec<SearchResult>> {
    search(&state.db, &query)
}

#[cfg(test)]
mod tests;
