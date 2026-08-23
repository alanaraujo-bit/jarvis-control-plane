//! Making Global Search (§51) find what was said *before* it existed.
//!
//! `session::transcript::mirror` indexes conversation content as it arrives,
//! which means search has been forward-only since the day it shipped (D25). A
//! session recorded before that build has every word of it on disk — the log
//! is the record, and it always was — and nothing in `session_events`. The
//! failure mode is the bad kind: searching for something you know you said
//! returns an empty list that looks exactly like "no match" rather than
//! "recorded before this could be indexed".
//!
//! This closes that gap once, per installation.
//!
//! ## Why this is not a migration
//!
//! It is the obvious place for it and it is the wrong place for it. A
//! migration runs inside one transaction with its own version record, on the
//! startup path, before the window exists. Walking every session log on the
//! machine is unbounded work over on-disk data of unknown size, and a failure
//! partway through a migration is precisely the situation rule 9 in
//! `docs/HANDOFF.md` was written about. So migration 10 adds a *column* — the
//! bookmark — and nothing else, and the scan happens here: after startup, on
//! its own thread, one session per transaction, resumable from that bookmark,
//! and re-runnable without duplicating a single row.
//!
//! ## What makes it safe to run twice
//!
//! A session is backfilled inside a delete-then-insert, so re-running after a
//! crash halfway through cannot leave doubled rows: whatever the interrupted
//! attempt wrote is cleared before the retry writes anything. The bookmark is
//! stamped last, so a session is only ever "done" once its rows are committed.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::params;

use crate::db::Database;
use crate::providers::conversation::ConversationItem;
use crate::session::log::{now_ms, SessionLogReader};
use crate::session::transcript::search_event;

/// How many rows are written per transaction.
///
/// The database is one connection behind one mutex (see `db::Database`), so a
/// long transaction here is a stall everywhere else. Chunking keeps the lock
/// held for a few milliseconds at a time instead of for a whole session.
const CHUNK: usize = 500;

/// How long the thread waits before starting.
///
/// Nothing about this is urgent, and the first seconds after launch belong to
/// the window, the project list and any session being restored. Search is
/// simply more complete a moment later than it was.
const STARTUP_GRACE: Duration = Duration::from_secs(5);

/// How long to rest between sessions.
///
/// Deliberate, not accidental: a backfill that saturates the disk to finish
/// four seconds sooner is a worse product than one nobody notices running.
const BETWEEN_SESSIONS: Duration = Duration::from_millis(120);

/// One session's worth of rows waiting to be written.
struct Row {
    seq: i64,
    ts_ms: i64,
    kind: &'static str,
    label: Option<String>,
    text: String,
    payload: String,
}

/// Start the backfill on its own thread. Returns immediately.
///
/// `stop` is checked between rows so a caller — today, a test; tomorrow, a
/// shutdown hook — can end a walk part-way. Quitting the app does not go
/// through it: the process simply exits, which is safe here rather than merely
/// tolerable, because every write is inside a transaction and the bookmark is
/// stamped last. An interrupted session keeps its NULL bookmark and is cleared
/// and redone on the next launch.
pub fn spawn(db: Arc<Database>, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("search-backfill".into())
        .spawn(move || {
            std::thread::sleep(STARTUP_GRACE);
            if stop.load(Ordering::Relaxed) {
                return;
            }
            // Logged even when there is nothing to do, and that is the point.
            // A background task that is silent in its steady state is a task
            // nobody can tell apart from one that never started — which is
            // exactly the failure items 23 and 33 in `docs/HANDOFF.md` record,
            // and exactly what made this thread's own wiring hard to confirm
            // the first time. One line per launch is a price worth paying to
            // be able to answer "did it run?" from the log alone.
            match run(&db, &stop) {
                Ok(report) => tracing::info!(
                    sessions = report.sessions,
                    rows = report.rows,
                    ms = report.elapsed_ms,
                    "search backfill finished"
                ),
                Err(e) => tracing::warn!(error = %e, "search backfill stopped early"),
            }
        })
        .expect("spawn search backfill");
}

pub struct Report {
    pub sessions: usize,
    pub rows: usize,
    pub elapsed_ms: u128,
}

/// Backfill every session that has not been backfilled yet.
///
/// Exposed (rather than folded into `spawn`) so a test can run it
/// synchronously against a real log on disk and assert on what became findable.
pub fn run(db: &Database, stop: &AtomicBool) -> crate::db::Result<Report> {
    let started = Instant::now();
    let pending = pending_sessions(db)?;
    let mut report = Report { sessions: 0, rows: 0, elapsed_ms: 0 };

    for (session_id, project_id, log_dir) in pending {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match backfill_one(db, &session_id, project_id.as_deref(), &log_dir, stop) {
            Ok(rows) => {
                report.sessions += 1;
                report.rows += rows;
            }
            // One unreadable session must not stop the other forty-one. It
            // keeps its NULL bookmark and is retried next launch, which is the
            // right behaviour for a log that is merely locked right now.
            Err(e) => {
                tracing::warn!(session = %session_id, error = %e, "could not backfill a session");
            }
        }
        std::thread::sleep(BETWEEN_SESSIONS);
    }

    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

/// Sessions still carrying a NULL bookmark, newest first.
///
/// Newest first on purpose: the sessions someone is most likely to search for
/// are the recent ones, so the gap closes where it is felt before it closes
/// everywhere.
fn pending_sessions(db: &Database) -> crate::db::Result<Vec<(String, Option<String>, String)>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, log_dir
               FROM sessions
              WHERE events_backfilled_at IS NULL
              ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect()
    })
}

/// Read one session's log and index everything in it that has words.
///
/// Returns how many rows were written.
fn backfill_one(
    db: &Database,
    session_id: &str,
    project_id: Option<&str>,
    log_dir: &str,
    stop: &AtomicBool,
) -> std::result::Result<usize, String> {
    let Ok(reader) = SessionLogReader::open(log_dir) else {
        // The directory is gone — a session from a previous install, or one
        // whose logs were cleaned up. There is nothing to index and there
        // never will be, so stamp it rather than reopening the question on
        // every launch for the rest of the product's life.
        stamp(db, session_id).map_err(|e| e.to_string())?;
        return Ok(0);
    };

    // Clear first, so a retry after an interrupted attempt cannot double up.
    clear(db, session_id).map_err(|e| e.to_string())?;

    let mut pending: Vec<Row> = Vec::new();
    let mut written = 0usize;
    let mut failed: Option<String> = None;

    let walk = reader.for_each_structured(|event| {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        // Not every structured frame is a conversation item — lifecycle,
        // attachments and approvals share the log and have their own shapes.
        // Asking serde is both the check and the parse.
        let Ok(item) = serde_json::from_slice::<ConversationItem>(&event.payload) else {
            return true;
        };
        let Some((kind, label, text)) = search_event(&item) else {
            return true;
        };
        if text.trim().is_empty() {
            return true;
        }

        pending.push(Row {
            // The log's own seq, so the row lands on the composite primary key
            // exactly where the live writer would have put it.
            seq: event.seq as i64,
            ts_ms: item.ts_ms(),
            kind,
            label,
            text,
            payload: serde_json::to_string(&item).unwrap_or_default(),
        });

        if pending.len() >= CHUNK {
            match flush(db, session_id, project_id, &pending) {
                Ok(()) => {
                    written += pending.len();
                    pending.clear();
                }
                Err(e) => {
                    failed = Some(e.to_string());
                    return false;
                }
            }
        }
        true
    });

    if let Err(e) = walk {
        return Err(format!("reading {log_dir}: {e}"));
    }
    if let Some(e) = failed {
        return Err(e);
    }

    if !pending.is_empty() {
        flush(db, session_id, project_id, &pending).map_err(|e| e.to_string())?;
        written += pending.len();
    }

    // Last, and only on success: from here the session is never read again.
    stamp(db, session_id).map_err(|e| e.to_string())?;
    Ok(written)
}

/// Remove anything a previous attempt wrote for this session.
fn clear(db: &Database, session_id: &str) -> crate::db::Result<()> {
    db.with(|conn| {
        conn.execute("DELETE FROM session_events WHERE session_id = ?1", [session_id])?;
        conn.execute("DELETE FROM session_events_fts WHERE session_id = ?1", [session_id])?;
        Ok(())
    })
}

/// Write one chunk, both to the table and to the index that search reads.
///
/// Both statements are plain `INSERT`, and the table's was briefly
/// `INSERT OR REPLACE` — which is the trap this codebase keeps meeting (item
/// 17, D26): the two writes have to agree, and only one of them *can* absorb a
/// conflict. `session_events` has a primary key and `session_events_fts` has
/// none, so `OR REPLACE` on the first would overwrite the row while the second
/// happily appended a duplicate, and search would return the same line twice
/// with nothing anywhere reporting a problem. `clear()` is what actually makes
/// a retry safe; `OR REPLACE` would only have hidden the case where it did
/// not. Removed and the idempotence test still passes, which is the evidence
/// that it was masking rather than guarding.
fn flush(
    db: &Database,
    session_id: &str,
    project_id: Option<&str>,
    rows: &[Row],
) -> crate::db::Result<()> {
    db.with(|conn| {
        let tx = conn.unchecked_transaction()?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO session_events
                     (session_id, seq, ts_ms, project_id, kind, label, text, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            let mut index = tx.prepare_cached(
                "INSERT INTO session_events_fts (session_id, ts_ms, project_id, kind, label, text)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for row in rows {
                insert.execute(params![
                    session_id,
                    row.seq,
                    row.ts_ms,
                    project_id,
                    row.kind,
                    row.label,
                    row.text,
                    row.payload,
                ])?;
                index.execute(params![
                    session_id,
                    row.ts_ms,
                    project_id,
                    row.kind,
                    row.label,
                    row.text,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

/// Mark a session as backfilled.
fn stamp(db: &Database, session_id: &str) -> crate::db::Result<()> {
    db.with(|conn| {
        conn.execute(
            "UPDATE sessions SET events_backfilled_at = ?2 WHERE id = ?1",
            params![session_id, now_ms()],
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::conversation::{Role, TokenUsage};
    use crate::session::event::EventKind;
    use crate::session::log::SessionLog;

    /// A session log on disk with the exact mixture a real one has: terminal
    /// bytes, a question, a reply carrying usage alongside its text, a tool
    /// call, and a lifecycle frame that is structured but is not a
    /// conversation item at all.
    fn a_real_log(dir: &std::path::Path) {
        let mut log = SessionLog::open(dir).unwrap();

        log.append(EventKind::PtyOutput, b"\x1b[2J$ claude\r\n").unwrap();
        log.append(
            EventKind::Message,
            &serde_json::to_vec(&ConversationItem::Message {
                role: Role::User,
                text: "why does the installer skip NSIS".into(),
                ts_ms: 1_000,
                usage: None,
            })
            .unwrap(),
        )
        .unwrap();
        log.append(EventKind::PtyOutput, b"thinking...\r\n").unwrap();
        log.append(
            EventKind::Message,
            &serde_json::to_vec(&ConversationItem::Message {
                role: Role::Assistant,
                // The ordinary shape of a reply: text *and* usage (D26).
                text: "because makensis never starts on this machine".into(),
                ts_ms: 2_000,
                usage: Some(TokenUsage { input: Some(90), ..TokenUsage::default() }),
            })
            .unwrap(),
        )
        .unwrap();
        log.append(
            EventKind::ToolCall,
            &serde_json::to_vec(&ConversationItem::ToolCall {
                name: "Bash".into(),
                summary: "cargo build --release".into(),
                ts_ms: 3_000,
                id: "t1".into(),
            })
            .unwrap(),
        )
        .unwrap();
        // Structured, and not a `ConversationItem`. The walk must step over it
        // rather than treat a failed parse as a reason to stop.
        log.append(EventKind::Lifecycle, br#"{"state":"exited"}"#).unwrap();
    }

    fn seeded(log_dir: &std::path::Path) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'demo', 'C:/demo', 0, 0)",
                [],
            )?;
            // No `events_backfilled_at`: NULL is exactly what a session
            // recorded before Global Search existed looks like.
            conn.execute(
                "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at)
                 VALUES ('old', 'p1', 'claude-code', 'C:/demo', 'exited', ?1, 0)",
                [log_dir.to_string_lossy()],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn rows(db: &Database) -> i64 {
        db.with(|conn| {
            conn.query_row("SELECT COUNT(*) FROM session_events WHERE session_id = 'old'", [], |r| {
                r.get(0)
            })
        })
        .unwrap()
    }

    /// The whole point, stated as the user's own experience: something said in
    /// a session recorded before search existed can be searched for.
    #[test]
    fn a_session_recorded_before_search_existed_becomes_findable() {
        let dir = tempfile::tempdir().unwrap();
        a_real_log(dir.path());
        let db = seeded(dir.path());

        // Before: the words are on disk and search cannot see them. This
        // assertion is the bug, not a formality.
        assert!(
            super::super::search(&db, "makensis").unwrap().is_empty(),
            "the fixture must start in the broken state, or the test proves nothing"
        );

        let report = run(&db, &AtomicBool::new(false)).unwrap();
        assert_eq!(report.sessions, 1);
        assert_eq!(report.rows, 3, "two messages and a tool call; lifecycle is not one");

        let hits = super::super::search(&db, "makensis").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].kind, crate::search::Kind::Conversation);
        assert_eq!(hits[0].session_id.as_deref(), Some("old"));
        assert!(
            hits[0].snippet.contains("makensis"),
            "a result has to show the words it matched: {:?}",
            hits[0].snippet
        );

        // The user's own side of the exchange, and the tool by its name.
        assert_eq!(super::super::search(&db, "installer").unwrap().len(), 1);
        assert_eq!(super::super::search(&db, "cargo").unwrap().len(), 1);
    }

    /// A session is read once and never again — otherwise every launch would
    /// re-walk every log on the machine forever.
    #[test]
    fn a_backfilled_session_is_not_read_again() {
        let dir = tempfile::tempdir().unwrap();
        a_real_log(dir.path());
        let db = seeded(dir.path());

        assert_eq!(run(&db, &AtomicBool::new(false)).unwrap().sessions, 1);
        assert_eq!(run(&db, &AtomicBool::new(false)).unwrap().sessions, 0);
        assert_eq!(rows(&db), 3);
    }

    /// The failure this design exists to survive: the process dies part-way
    /// through a session, so its bookmark is still NULL and its rows are
    /// half-written. Re-running must produce the same three rows, not six —
    /// and search must return one hit, not two identical ones.
    #[test]
    fn an_interrupted_backfill_is_redone_without_doubling_anything() {
        let dir = tempfile::tempdir().unwrap();
        a_real_log(dir.path());
        let db = seeded(dir.path());

        run(&db, &AtomicBool::new(false)).unwrap();
        // Exactly what a crash between the last flush and the stamp leaves.
        db.with(|conn| {
            conn.execute("UPDATE sessions SET events_backfilled_at = NULL WHERE id = 'old'", [])?;
            Ok(())
        })
        .unwrap();

        run(&db, &AtomicBool::new(false)).unwrap();
        assert_eq!(rows(&db), 3, "delete-then-insert is what keeps a retry idempotent");
        assert_eq!(
            super::super::search(&db, "makensis").unwrap().len(),
            1,
            "the FTS index has to be cleared with the table, or a retry doubles every result"
        );
    }

    /// A session whose log directory no longer exists is settled, not retried
    /// on every launch for the rest of the product's life.
    #[test]
    fn a_session_whose_log_is_gone_is_settled_rather_than_retried() {
        let db = seeded(std::path::Path::new("C:/nowhere/at/all"));

        assert_eq!(run(&db, &AtomicBool::new(false)).unwrap().rows, 0);
        assert_eq!(
            run(&db, &AtomicBool::new(false)).unwrap().sessions,
            0,
            "it must not come back as pending"
        );
    }

    /// Stopping is honoured, and stopping leaves nothing claiming to be done.
    #[test]
    fn stopping_leaves_the_session_pending_rather_than_half_indexed() {
        let dir = tempfile::tempdir().unwrap();
        a_real_log(dir.path());
        let db = seeded(dir.path());

        let report = run(&db, &AtomicBool::new(true)).unwrap();
        assert_eq!(report.sessions, 0);

        let bookmark: Option<i64> = db
            .with(|conn| {
                conn.query_row("SELECT events_backfilled_at FROM sessions WHERE id = 'old'", [], |r| {
                    r.get(0)
                })
            })
            .unwrap();
        assert!(bookmark.is_none(), "an unfinished session must still look unfinished");
    }

    /// The real thing: the logs real agent sessions actually wrote on this
    /// machine, read by the real walker, indexed and then searched for.
    ///
    /// A fixture proves the logic and cannot prove the shape. This reads
    /// `%APPDATA%\dev.jarvis.desktop\sessions` — ten directories holding 82
    /// conversation items from genuine Claude Code runs — points session rows
    /// at them, and asks Global Search for a word taken out of what it just
    /// indexed. Nothing here writes anywhere but a temporary database, and the
    /// logs are only ever opened for reading.
    ///
    /// Why it builds its own rows instead of copying the live database: those
    /// sessions have **no rows left**. Simulating a fresh install for
    /// Onboarding (§13) meant deleting `jarvis.db`, which took every session
    /// row with it and left the directories behind — see the note in
    /// `docs/HANDOFF.md` about orphaned session directories. The logs are the
    /// real artefact either way, and they are what this exercises.
    ///
    /// `#[ignore]`d because it depends on this machine's own history.
    #[test]
    #[ignore = "needs this machine's own recorded sessions"]
    fn real_recorded_sessions_become_searchable() {
        let appdata = std::env::var_os("APPDATA").expect("APPDATA");
        let root = std::path::Path::new(&appdata).join("dev.jarvis.desktop").join("sessions");
        assert!(root.is_dir(), "no recorded sessions at {root:?}");

        // Only the directories that actually hold a conversation. A plain
        // shell session records lifecycle frames and terminal bytes, and has
        // nothing for search to find — which is correct, not a shortfall.
        let mut with_conversation = Vec::new();
        for entry in std::fs::read_dir(&root).unwrap() {
            let dir = entry.unwrap().path();
            let Ok(reader) = SessionLogReader::open(&dir) else { continue };
            let mut items = 0;
            let _ = reader.for_each_structured(|e| {
                if serde_json::from_slice::<ConversationItem>(&e.payload).is_ok() {
                    items += 1;
                }
                true
            });
            if items > 0 {
                with_conversation.push(dir);
            }
        }
        assert!(
            !with_conversation.is_empty(),
            "no recorded session on this machine holds a conversation"
        );

        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'recorded', 'C:/recorded', 0, 0)",
                [],
            )?;
            for (i, dir) in with_conversation.iter().enumerate() {
                conn.execute(
                    "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at)
                     VALUES (?1, 'p1', 'claude-code', 'C:/recorded', 'exited', ?2, ?3)",
                    params![format!("s{i}"), dir.to_string_lossy(), i as i64],
                )?;
            }
            Ok(())
        })
        .unwrap();

        let report = run(&db, &AtomicBool::new(false)).unwrap();
        eprintln!(
            "indexed {} rows from {} real session logs in {}ms",
            report.rows, report.sessions, report.elapsed_ms
        );
        assert_eq!(report.sessions, with_conversation.len());
        assert!(report.rows > 0, "real logs full of conversation produced nothing indexable");

        // Take a word out of what was just indexed and ask search for it,
        // exactly as the surface would. Choosing the word from the data rather
        // than hard-coding one keeps this a test of real history instead of a
        // test of a phrase someone happened to remember.
        let (session_id, text): (String, String) = db
            .with(|conn| {
                conn.query_row(
                    "SELECT session_id, text FROM session_events
                      WHERE kind = 'message' AND LENGTH(text) > 40
                      ORDER BY LENGTH(text) DESC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
            })
            .unwrap();
        let word = text
            .split_whitespace()
            .find(|w| w.chars().count() > 6 && w.chars().all(char::is_alphanumeric))
            .unwrap_or_else(|| panic!("no searchable word in {text:?}"));
        eprintln!("asking Global Search for {word:?}, said in {session_id}");

        let hits = super::super::search(&db, word).unwrap();
        let conversation: Vec<_> =
            hits.iter().filter(|h| h.kind == crate::search::Kind::Conversation).collect();
        assert!(
            !conversation.is_empty(),
            "a word this build just indexed out of a real session log is not findable"
        );
        assert!(
            conversation.iter().any(|h| h.session_id.as_deref() == Some(&session_id)),
            "found matches, but not in the session the word actually came from"
        );
        assert!(
            conversation.iter().any(|h| h.snippet.to_lowercase().contains(&word.to_lowercase())),
            "a result must show the words it matched"
        );
    }
}
