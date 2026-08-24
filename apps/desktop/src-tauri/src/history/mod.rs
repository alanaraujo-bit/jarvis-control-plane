//! Session History (§88).
//!
//! Every session this product has ever run, in one place: titled, searchable,
//! grouped by when it happened, openable, renameable and deletable.
//!
//! ## Why this is not `session_list`
//!
//! `session::commands::session_list` filters `ended_at IS NULL` and is scoped
//! to one project. That is exactly right for what it is — the query the project
//! workspace adopts its live terminal tabs from — and it is why a closed
//! session was, until this module existed, unreachable from anywhere in the
//! product. Loosening it would resurrect finished sessions as live terminal
//! tabs. So history is a separate read, and `session_list` is left alone.
//!
//! ## Nothing here is a second store
//!
//! The rows come from `sessions`, the counts from `session_events` and
//! `usage_samples`, the search from `session_events_fts` — every one of them
//! already written by the transcript tailer (§51, D26). The only thing this
//! module reads off the filesystem is how large a session's log actually is,
//! because that is a fact about the disk and nowhere else knows it.
//!
//! ## Ordering, stated rather than assumed
//!
//! Rows are ordered and grouped by when a session **started**, not by its last
//! activity. Last activity would need `MAX(ts_ms)` over every session's events
//! to sort a page, which is not something an index can serve, and "started" is
//! a fact each row already carries. The consequence is honest and small: a
//! session opened yesterday and still running is filed under yesterday, and the
//! row says how long it ran.

pub mod commands;

use rusqlite::{params_from_iter, types::Value as SqlValue, Connection};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::session::title;

pub type Result<T> = std::result::Result<T, String>;

/// How many rows one page holds.
///
/// A page is what the surface renders at once; more arrive as it is scrolled.
/// Paged by keyset (`created_at`, `id`) rather than `OFFSET`, because `OFFSET`
/// re-walks everything it skips — the cost of page twelve grows with twelve,
/// and a history that gets slower the further back you look is the specific
/// failure this feature has to avoid.
pub const PAGE: usize = 40;

/// The most rows a search will return.
///
/// A search box is not a report (`search::PER_SOURCE_LIMIT` makes the same
/// judgement). Beyond this the answer is "narrow the query", not "scroll".
const SEARCH_LIMIT: usize = 60;

/// One session, as history sees it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub project_id: String,
    pub project_name: String,
    pub provider: String,
    /// `None` for a session nothing ever named — a shell, usually.
    pub title: Option<String>,
    /// Which of the three named it (§88, D36). Never inferred from the title.
    pub title_source: Option<title::Source>,
    pub state: String,
    pub created_at: i64,
    pub ended_at: Option<i64>,
    /// The mission this session was working on (§86), if any.
    pub mission_id: Option<String>,
    pub mission_title: Option<String>,
    /// How many times a person said something. The honest measure of how much
    /// of a conversation this was — counting every frame would rank a session
    /// that ran one long build above one with twenty exchanges in it.
    pub turns: i64,
    /// Every structured item recorded: what was said, thought, run, returned.
    pub events: i64,
    /// Input + output tokens across the session. `None` when the provider
    /// reported none — which is not the same as zero, and is not drawn as it.
    pub tokens: Option<i64>,
    /// Bytes the session's own log directory occupies.
    pub bytes: i64,
    /// Whether a process is still attached. Read from the manager, never from
    /// the stored state — a row outlives its process after a crash.
    pub live: bool,
    /// Set only on a search hit: the line that matched, in context.
    pub snippet: Option<String>,
}

/// What the caller is asking for.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Query {
    /// Free text. Matches a session's title **and** what was said inside it —
    /// the second is the part VS Code's own list cannot do, and it runs on the
    /// FTS5 index §51 already built.
    pub text: Option<String>,
    pub project_id: Option<String>,
    pub provider: Option<String>,
    /// Only sessions that started at or after this instant.
    pub since_ms: Option<i64>,
    /// Keyset cursor: the last row of the previous page.
    pub before_ts: Option<i64>,
    pub before_id: Option<String>,
}

/// A page of history, and whether there is more behind it.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub entries: Vec<Entry>,
    /// False once the caller has reached the end, so the surface can stop
    /// asking rather than discovering it by getting nothing back.
    pub has_more: bool,
    /// True when this page answers a text query, so the surface knows a
    /// missing snippet means "matched on the title" rather than "not a search".
    pub searched: bool,
}

/// What this machine is holding on to (§88, D39).
///
/// Shown because nothing else in the product ever says it. Session logs are
/// never pruned by anything — the log **is** the record (§23) — and a person
/// cannot make a decision about disk they cannot see. This is the visibility
/// half of that; the deciding half is theirs.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub sessions: i64,
    pub bytes: i64,
}

/// One page of history.
pub fn page(db: &Database, live: &[String], query: &Query) -> Result<Page> {
    let text = query
        .text
        .as_deref()
        .map(str::trim)
        .filter(|t| t.chars().count() >= crate::search::MIN_QUERY_CHARS);

    let mut entries = db
        .with(|conn| match text {
            Some(text) => search(conn, query, text),
            None => browse(conn, query),
        })
        .map_err(|e| e.to_string())?;

    // One page holds `PAGE + 1` so "is there more" is answered by the query
    // rather than by a second `COUNT(*)` over everything behind it.
    let has_more = text.is_none() && entries.len() > PAGE;
    if has_more {
        entries.truncate(PAGE);
    }

    for entry in &mut entries {
        entry.live = live.iter().any(|id| id == &entry.id);
    }

    Ok(Page {
        entries,
        has_more,
        searched: text.is_some(),
    })
}

/// The columns and joins every history row needs, wherever the ids came from.
///
/// Written once because the browse and the search read the same row and must
/// never drift into reporting different numbers for the same session.
const SELECT: &str = "
    SELECT s.id, s.project_id, p.name, s.provider, s.title, s.title_source,
           s.state, s.created_at, s.ended_at, s.mission_id, m.title,
           s.log_dir,
           (SELECT COUNT(*) FROM session_events e
             WHERE e.session_id = s.id AND e.kind = 'message' AND e.label = 'user'),
           (SELECT COUNT(*) FROM session_events e WHERE e.session_id = s.id),
           (SELECT SUM(COALESCE(u.input_tokens, 0) + COALESCE(u.output_tokens, 0))
              FROM usage_samples u WHERE u.session_id = s.id)
      FROM sessions s
      JOIN projects p ON p.id = s.project_id
      LEFT JOIN missions m ON m.id = s.mission_id
";

fn read_entry(row: &rusqlite::Row<'_>, snippet: Option<String>) -> rusqlite::Result<Entry> {
    let log_dir: String = row.get(11)?;
    Ok(Entry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        project_name: row.get(2)?,
        provider: row.get(3)?,
        title: row.get(4)?,
        title_source: row
            .get::<_, Option<String>>(5)?
            .as_deref()
            .and_then(title::Source::parse),
        state: row.get(6)?,
        created_at: row.get(7)?,
        ended_at: row.get(8)?,
        mission_id: row.get(9)?,
        mission_title: row.get(10)?,
        turns: row.get(12)?,
        events: row.get(13)?,
        tokens: row.get(14)?,
        bytes: directory_bytes(std::path::Path::new(&log_dir)),
        live: false, // the manager fills this in; a row cannot know it
        snippet,
    })
}

/// Filters shared by browsing and searching, as SQL plus its bound values.
fn filters(query: &Query, with_cursor: bool) -> (String, Vec<SqlValue>) {
    let mut sql = String::new();
    let mut values: Vec<SqlValue> = Vec::new();

    if let Some(project) = &query.project_id {
        values.push(SqlValue::Text(project.clone()));
        sql.push_str(&format!(" AND s.project_id = ?{}", values.len()));
    }
    if let Some(provider) = &query.provider {
        values.push(SqlValue::Text(provider.clone()));
        sql.push_str(&format!(" AND s.provider = ?{}", values.len()));
    }
    if let Some(since) = query.since_ms {
        values.push(SqlValue::Integer(since));
        sql.push_str(&format!(" AND s.created_at >= ?{}", values.len()));
    }

    // The keyset cursor. Spelled out rather than as a row-value comparison so
    // it reads the same way the index does, and so `id` breaks the tie for two
    // sessions started in the same millisecond — without it a page boundary
    // landing between them would silently skip one.
    if with_cursor {
        if let (Some(ts), Some(id)) = (query.before_ts, query.before_id.as_ref()) {
            values.push(SqlValue::Integer(ts));
            let ts_slot = values.len();
            values.push(SqlValue::Text(id.clone()));
            let id_slot = values.len();
            sql.push_str(&format!(
                " AND (s.created_at < ?{ts_slot} OR (s.created_at = ?{ts_slot} AND s.id < ?{id_slot}))"
            ));
        }
    }

    (sql, values)
}

fn browse(conn: &Connection, query: &Query) -> rusqlite::Result<Vec<Entry>> {
    let (where_sql, mut values) = filters(query, true);
    values.push(SqlValue::Integer(PAGE as i64 + 1));
    let limit_slot = values.len();

    let sql = format!(
        "{SELECT} WHERE 1 = 1{where_sql}
         ORDER BY s.created_at DESC, s.id DESC
         LIMIT ?{limit_slot}"
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), |row| read_entry(row, None))?;
    rows.collect()
}

/// Sessions matching free text, by title or by anything said inside them.
///
/// Two passes rather than one query: the title match is a `LIKE` over a column,
/// the content match is an FTS5 `MATCH` over a standalone index, and SQLite
/// cannot use both in one plan without a scan. Merged here, title matches
/// first, because somebody searching for a name they gave a session means that
/// session and not the twelve that happen to say the word.
fn search(conn: &Connection, query: &Query, text: &str) -> rusqlite::Result<Vec<Entry>> {
    let (where_sql, base_values) = filters(query, false);

    let mut ordered: Vec<(String, Option<String>)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Pass one — the session's own name.
    {
        let mut values = base_values.clone();
        values.push(SqlValue::Text(crate::search::like_pattern(text)));
        let like_slot = values.len();
        values.push(SqlValue::Integer(SEARCH_LIMIT as i64));
        let limit_slot = values.len();

        let sql = format!(
            "SELECT s.id FROM sessions s
              WHERE s.title LIKE ?{like_slot} ESCAPE '\\'{where_sql}
              ORDER BY s.created_at DESC
              LIMIT ?{limit_slot}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), |row| {
            row.get::<_, String>(0)
        })?;
        for id in rows {
            let id = id?;
            if seen.insert(id.clone()) {
                ordered.push((id, None));
            }
        }
    }

    // Pass two — what was actually said. `fts_query` is what keeps a query
    // carrying a colon, a quote or the word `OR` from being read as FTS5
    // syntax; it is shared with Global Search so both behave identically.
    {
        let mut values = base_values.clone();
        values.push(SqlValue::Text(crate::search::fts_query(text)));
        let match_slot = values.len();
        values.push(SqlValue::Integer(SEARCH_LIMIT as i64));
        let limit_slot = values.len();

        // Grouped by session: a history row is a session, not a line. `MIN` on
        // the timestamp is only there to pick one deterministic representative
        // line per session for the snippet.
        let sql = format!(
            "SELECT f.session_id, f.text
               FROM session_events_fts f
               JOIN sessions s ON s.id = f.session_id
              WHERE session_events_fts MATCH ?{match_slot}{where_sql}
              GROUP BY f.session_id
              ORDER BY MAX(f.ts_ms) DESC
              LIMIT ?{limit_slot}"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(values.iter()), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, matched) = row?;
            if seen.insert(id.clone()) {
                ordered.push((id, Some(crate::search::snippet_around(&matched, text))));
            }
        }
    }

    if ordered.is_empty() {
        return Ok(Vec::new());
    }
    ordered.truncate(SEARCH_LIMIT);

    // Load the full rows for the ids found, then put them back in the order the
    // two passes decided — `IN (...)` returns whatever order the plan likes,
    // and letting that through would make a title match sort below a body one.
    let placeholders = (1..=ordered.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("{SELECT} WHERE s.id IN ({placeholders})");
    let ids: Vec<SqlValue> = ordered
        .iter()
        .map(|(id, _)| SqlValue::Text(id.clone()))
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let loaded = stmt
        .query_map(params_from_iter(ids.iter()), |row| read_entry(row, None))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(ordered
        .into_iter()
        .filter_map(|(id, snippet)| {
            loaded.iter().find(|e| e.id == id).cloned().map(|mut e| {
                e.snippet = snippet;
                e
            })
        })
        .collect())
}

/// Rename a session (§88, D36).
///
/// `Source::User` outranks everything, so nothing the provider says afterwards
/// can take the name back. An empty string is a refusal rather than a clear:
/// a nameless row in a history list is a worse outcome than the name somebody
/// is trying to replace, and there is no gesture in the surface that means
/// "make this untitled again".
pub fn rename(db: &Database, session_id: &str, name: &str) -> Result<String> {
    let cleaned = title::clean(name);
    if cleaned.is_empty() {
        return Err("history.emptyTitle".into());
    }
    title::set(db, session_id, title::Source::User, &cleaned)?;
    Ok(cleaned)
}

/// What a delete actually removed.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Deleted {
    pub bytes_freed: i64,
    /// True when the log directory was removed as well as the rows. False means
    /// the rows are gone and the directory could not be — reported rather than
    /// swallowed, because the difference is disk the person expected back.
    pub log_removed: bool,
}

/// Delete a session: its rows, its search index entries, and its log (D39).
///
/// The FTS index is deleted **explicitly**. `session_events_fts` is a
/// standalone FTS5 table rather than a `content=`-linked one — migration 9 says
/// why, and the consequence is that there is no trigger and no cascade. Leaving
/// it would have Global Search go on returning hits, with snippets, for a
/// conversation that no longer exists anywhere.
///
/// A live session is refused. Taking an agent's log out from under it while it
/// is writing is not a delete, it is a crash.
pub fn delete(db: &Database, live: &[String], session_id: &str) -> Result<Deleted> {
    if live.iter().any(|id| id == session_id) {
        return Err("history.deleteLive".into());
    }

    let log_dir: Option<String> = db
        .with(|conn| {
            conn.query_row(
                "SELECT log_dir FROM sessions WHERE id = ?1",
                [session_id],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
        })
        .map_err(|e| e.to_string())?;

    let Some(log_dir) = log_dir else {
        return Err("history.notFound".into());
    };
    let path = std::path::PathBuf::from(&log_dir);
    let bytes = directory_bytes(&path);

    // One transaction: a session whose rows survived but whose search entries
    // did not, or the reverse, is worse than either outcome on its own.
    db.with(|conn| {
        conn.execute_batch("BEGIN")?;
        let result = (|| -> rusqlite::Result<()> {
            conn.execute(
                "DELETE FROM session_events_fts WHERE session_id = ?1",
                [session_id],
            )?;
            conn.execute("DELETE FROM session_events WHERE session_id = ?1", [session_id])?;
            // `notifications.session_id` carries no foreign key (migration 12
            // declares it as a plain column), so nothing cleans it up for us —
            // and a notification whose session is gone is a row the centre can
            // draw and nothing can open.
            conn.execute("DELETE FROM notifications WHERE session_id = ?1", [session_id])?;
            // `usage_samples`, `file_changes` and `activity` all reference
            // `sessions` and are handled by their own ON DELETE clauses.
            conn.execute("DELETE FROM sessions WHERE id = ?1", [session_id])?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                conn.execute_batch("COMMIT")?;
                Ok(())
            }
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    })
    .map_err(|e| e.to_string())?;

    // The directory last, and only after the rows are gone: a log with no row
    // pointing at it is invisible and harmless (HANDOFF item 37 is exactly
    // that state, arrived at by accident), while a row pointing at a directory
    // that is not there renders as a session that will not open.
    let log_removed = if path.exists() {
        std::fs::remove_dir_all(&path).is_ok()
    } else {
        true
    };

    Ok(Deleted {
        bytes_freed: bytes,
        log_removed,
    })
}

/// How much disk every session on this machine is holding.
pub fn storage(db: &Database) -> Result<Storage> {
    let dirs: Vec<String> = db
        .with(|conn| {
            let mut stmt = conn.prepare("SELECT log_dir FROM sessions")?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()
        })
        .map_err(|e| e.to_string())?;

    let bytes = dirs
        .iter()
        .map(|dir| directory_bytes(std::path::Path::new(dir)))
        .sum();

    Ok(Storage {
        sessions: dirs.len() as i64,
        bytes,
    })
}

/// Every provider that has actually run here, for the filter row.
///
/// Read from the data rather than from `providers::all()`: a filter offering
/// Codex on a machine that has never run it is a control that can only ever
/// empty the list, and one that omits a provider this build no longer ships
/// would hide sessions that are really there.
pub fn providers_seen(db: &Database) -> Result<Vec<String>> {
    db.with(|conn| {
        let mut stmt =
            conn.prepare("SELECT DISTINCT provider FROM sessions ORDER BY provider")?;
        let rows = stmt.query_map([], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()
    })
    .map_err(|e| e.to_string())
}

/// Bytes under a directory, one level deep and then some.
///
/// Session logs are a flat handful of files plus an `attachments/` directory,
/// so this walks rather than assuming either shape. A directory that is not
/// there is zero, not an error: HANDOFF item 37 records how a row can outlive
/// its log, and a history list that fails to draw because of it would be a
/// worse bug than the one it is reporting.
fn directory_bytes(path: &std::path::Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0i64;
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => total += directory_bytes(&entry.path()),
            Ok(_) => total += entry.metadata().map(|m| m.len() as i64).unwrap_or(0),
            Err(_) => {}
        }
    }
    total
}

#[cfg(test)]
mod tests;
