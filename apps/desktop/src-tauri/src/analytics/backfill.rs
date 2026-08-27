//! Recovering the usage history that was always on disk (M22).
//!
//! Analytics could only ever count what J.A.R.V.I.S. itself watched happen.
//! Measured on this machine the day this was written: **889 samples across two
//! days** in the database, against **45,487 usage-bearing turns across twenty
//! days** sitting in the provider's own transcripts — the same work, by the
//! same person, most of it done before this product existed to watch it.
//!
//! That gap is not a rounding error, it decides whether the screen is worth
//! opening. A calendar of activity and a "days worked" streak over two days
//! are decorations; over twenty days with real gaps in them — 6, 20 and 21
//! August are empty in the real corpus — they are the screen.
//!
//! ## Why this is not a migration
//!
//! Same answer as `search::backfill`, and for the same reason: a migration runs
//! in one transaction on the startup path before the window exists, and walking
//! every transcript on the machine is unbounded work over on-disk data of
//! unknown size. Migration 19 adds columns and a bookmark table; the walk
//! happens here, after startup, on its own thread, resumable.
//!
//! ## What makes it safe to run twice
//!
//! Two independent guards, because this one writes *numbers people read*:
//!
//! 1. **Every turn carries the provider's own `uuid`**, stored in
//!    `usage_samples.origin_uuid` under a unique index. `INSERT OR IGNORE`
//!    therefore makes a second walk a no-op rather than a doubling.
//! 2. **A transcript whose session J.A.R.V.I.S. ran is never walked at all.**
//!    Those turns are already in the table from the live tailer, with a NULL
//!    `origin_uuid` that guard 1 could not match. Measured: 7 of 207
//!    transcripts overlap. The file name *is* the session id — Claude Code is
//!    launched with `--session-id` (M3) — so the check is exact and costs one
//!    set lookup per file.
//!
//! ## What it does not do
//!
//! It does not invent `sessions` or `projects` rows. The corpus spans 35
//! directories against 3 registered projects, and putting folders the person
//! never opened here into their project list would be worse than the gap it
//! fixes. The project *name* travels on the row instead (`project_label`),
//! read from the `cwd` the provider recorded.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::params;
use serde_json::Value;

use crate::db::Database;
use crate::providers::conversation::ConversationItem;
use crate::session::log::now_ms;

/// Rows written per transaction.
///
/// The database is one connection behind one mutex, so a long transaction here
/// stalls everything else. Same reasoning and same figure as `search::backfill`.
const CHUNK: usize = 500;

/// How long the thread waits before starting.
///
/// Longer than the search backfill's five seconds, deliberately: that one
/// competes for the same disk, and analytics being complete a moment later
/// costs nobody anything.
const STARTUP_GRACE: Duration = Duration::from_secs(12);

/// How long to rest between transcripts.
const BETWEEN_FILES: Duration = Duration::from_millis(40);

/// One turn's worth of numbers, ready to write.
struct Row {
    origin_uuid: String,
    ts_ms: i64,
    provider: &'static str,
    model: Option<String>,
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    confidence: String,
    project_label: Option<String>,
}

pub struct Report {
    pub files: usize,
    pub skipped: usize,
    pub rows: usize,
    pub elapsed_ms: u128,
}

/// Start the backfill on its own thread. Returns immediately.
pub fn spawn(db: Arc<Database>, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("usage-backfill".into())
        .spawn(move || {
            std::thread::sleep(STARTUP_GRACE);
            if stop.load(Ordering::Relaxed) {
                return;
            }
            // Logged even when there is nothing to do — a background task that
            // is silent in its steady state cannot be told apart from one that
            // never started (HANDOFF items 23 and 33).
            match run(&db, &stop) {
                Ok(report) => tracing::info!(
                    files = report.files,
                    skipped = report.skipped,
                    rows = report.rows,
                    ms = report.elapsed_ms,
                    "usage backfill finished"
                ),
                Err(error) => tracing::warn!(%error, "usage backfill stopped early"),
            }
        })
        .expect("spawn usage backfill");
}

/// Every directory a provider may keep transcripts in, across every account.
///
/// Account directories are included because an account added in M13 keeps its
/// own `projects/` tree (see `accounts::transcript_root`) — history recorded
/// there is no less real for having been made under a second configuration.
fn transcript_roots(db: &Database) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(machine) = crate::accounts::machine_config_dir("claude-code") {
        if let Some(root) = crate::accounts::transcript_root("claude-code", &machine) {
            roots.push(root);
        }
    }
    if let Ok(accounts) = crate::accounts::list(db) {
        for account in accounts.iter().filter(|a| a.provider == "claude-code" && !a.adopted) {
            if let Some(root) =
                crate::accounts::transcript_root(&account.provider, Path::new(&account.config_dir))
            {
                roots.push(root);
            }
        }
    }
    roots
}

/// Every `.jsonl` under a root, depth-first, ignoring what cannot be read.
fn transcripts(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            transcripts(&path, out);
        } else if path.extension().is_some_and(|e| e == "jsonl") {
            out.push(path);
        }
    }
}

/// Session ids this product ran itself, whose turns the tailer already stored.
fn already_recorded(db: &Database) -> HashSet<String> {
    db.with(|conn| {
        let mut stmt =
            conn.prepare("SELECT id, provider_session_id FROM sessions")?;
        let mut ids = HashSet::new();
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        for row in rows.flatten() {
            ids.insert(row.0);
            if let Some(provider_id) = row.1 {
                ids.insert(provider_id);
            }
        }
        Ok(ids)
    })
    .unwrap_or_default()
}

/// A readable project name from the working directory a transcript recorded.
///
/// The last path segment, which is what the person calls the folder. Falls back
/// to the whole string rather than to nothing: an unusual path is still a
/// better label than an empty one.
fn label_from_cwd(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim().trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return None;
    }
    let name = trimmed
        .rsplit(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or(trimmed);
    Some(name.to_string())
}

/// Read one transcript line into a row, or nothing.
///
/// **The numbers come from `providers::claude::parse_line`**, the same parser
/// Conversation View and the live tailer use, rather than from a second reader
/// written here. That is not tidiness: M13 §2.4 records a Codex field that
/// changed under a duplicated parser and went wrong silently for weeks. Only
/// the two bookkeeping fields this backfill needs and that parser does not
/// carry — the turn's `uuid` and the session's `cwd` — are read locally.
fn row_from_line(line: &str) -> Option<Row> {
    if !line.contains("\"usage\"") {
        return None;
    }
    let value: Value = serde_json::from_str(line).ok()?;
    let origin_uuid = value.get("uuid").and_then(Value::as_str)?.to_string();
    let project_label = value
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(label_from_cwd);

    let usage = crate::providers::claude::parse_line(line)
        .into_iter()
        .find_map(|item| match item {
            ConversationItem::Message {
                usage: Some(usage),
                ts_ms,
                ..
            } => Some((usage, ts_ms)),
            _ => None,
        });
    let (usage, ts_ms) = usage?;
    if usage.is_empty() || ts_ms <= 0 {
        return None;
    }

    Some(Row {
        origin_uuid,
        ts_ms,
        provider: "claude-code",
        model: usage.model.clone(),
        input: usage.input.unwrap_or(0) as i64,
        output: usage.output.unwrap_or(0) as i64,
        cache_read: usage.cache_read.unwrap_or(0) as i64,
        cache_write: usage.cache_write.unwrap_or(0) as i64,
        // Whatever the adapter stamped. Recovering a turn from disk does not
        // make its numbers any less (or more) official than they were (§28).
        confidence: format!("{:?}", usage.confidence).to_lowercase(),
        project_label,
    })
}

/// Whether this transcript has already been walked in its current state.
///
/// Size and modification time together, rather than the path alone: a session
/// that was resumed — or the one running right now — grows, and its new turns
/// are worth having. Re-walking it is cheap and cannot double anything, because
/// the turns it already had are rejected by the unique index.
fn unchanged(db: &Database, path: &Path, size: i64, mtime_ms: i64) -> bool {
    db.with(|conn| {
        let found: Option<(i64, i64)> = conn
            .query_row(
                "SELECT size, mtime_ms FROM usage_backfill_files WHERE path = ?1",
                [path.to_string_lossy().to_string()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        Ok(found == Some((size, mtime_ms)))
    })
    .unwrap_or(false)
}

fn write(db: &Database, rows: &[Row]) -> crate::db::Result<usize> {
    db.with(|conn| {
        let tx = conn.unchecked_transaction()?;
        let mut written = 0usize;
        {
            let mut stmt = tx.prepare(
                "INSERT OR IGNORE INTO usage_samples
                     (session_id, project_id, provider, model, ts_ms,
                      input_tokens, output_tokens, cache_read_tokens, cache_write_tokens,
                      confidence, origin_uuid, project_label)
                 VALUES (NULL, NULL, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for row in rows {
                written += stmt.execute(params![
                    row.provider,
                    row.model,
                    row.ts_ms,
                    row.input,
                    row.output,
                    row.cache_read,
                    row.cache_write,
                    row.confidence,
                    row.origin_uuid,
                    row.project_label,
                ])?;
            }
        }
        tx.commit()?;
        Ok(written)
    })
}

/// Walk every transcript once. Resumable, idempotent, and interruptible.
pub fn run(db: &Database, stop: &AtomicBool) -> crate::db::Result<Report> {
    let started = Instant::now();
    let known = already_recorded(db);

    let mut files = Vec::new();
    for root in transcript_roots(db) {
        transcripts(&root, &mut files);
    }

    let mut report = Report {
        files: 0,
        skipped: 0,
        rows: 0,
        elapsed_ms: 0,
    };

    for path in files {
        if stop.load(Ordering::Relaxed) {
            break;
        }

        // A transcript J.A.R.V.I.S. produced is already in the table, turn by
        // turn, from the live tailer — and those rows have no `origin_uuid` for
        // the unique index to catch. Skipping the file is the only thing
        // standing between this backfill and doubled totals for today.
        let stem = path.file_stem().map(|s| s.to_string_lossy().to_string());
        if stem.as_deref().is_some_and(|id| known.contains(id)) {
            report.skipped += 1;
            continue;
        }

        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        let size = meta.len() as i64;
        let mtime_ms = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if unchanged(db, &path, size, mtime_ms) {
            report.skipped += 1;
            continue;
        }

        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut batch: Vec<Row> = Vec::with_capacity(CHUNK);
        let mut turns = 0usize;
        for line in text.lines() {
            if let Some(row) = row_from_line(line) {
                batch.push(row);
                turns += 1;
            }
            if batch.len() >= CHUNK {
                report.rows += write(db, &batch)?;
                batch.clear();
            }
        }
        if !batch.is_empty() {
            report.rows += write(db, &batch)?;
        }

        // Stamped last, so an interrupted file is walked again next launch.
        let _ = db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_backfill_files (path, size, mtime_ms, turns, done_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (path) DO UPDATE SET
                     size = excluded.size, mtime_ms = excluded.mtime_ms,
                     turns = excluded.turns, done_at = excluded.done_at",
                params![
                    path.to_string_lossy().to_string(),
                    size,
                    mtime_ms,
                    turns as i64,
                    now_ms()
                ],
            )?;
            Ok(())
        });

        report.files += 1;
        std::thread::sleep(BETWEEN_FILES);
    }

    report.elapsed_ms = started.elapsed().as_millis();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TURN: &str = r#"{"cwd":"C:\\Users\\a\\Projetos\\estoca","uuid":"u-1","type":"assistant","timestamp":"2026-08-20T18:00:00.000Z","message":{"model":"claude-opus-5","role":"assistant","type":"message","content":[{"type":"text","text":"hi"}],"usage":{"input_tokens":5,"output_tokens":7,"cache_creation_input_tokens":11,"cache_read_input_tokens":13}}}"#;

    #[test]
    fn a_turn_becomes_a_row_with_all_four_token_columns() {
        let row = row_from_line(TURN).expect("a usage-bearing turn");
        assert_eq!(row.origin_uuid, "u-1");
        assert_eq!(row.input, 5);
        assert_eq!(row.output, 7);
        // All four are stored even though the quota window ignores cache reads:
        // throwing a column away at write time is unrecoverable, and the
        // surface should be free to choose later.
        assert_eq!(row.cache_write, 11);
        assert_eq!(row.cache_read, 13);
        assert_eq!(row.model.as_deref(), Some("claude-opus-5"));
        assert_eq!(row.project_label.as_deref(), Some("estoca"));
        assert_eq!(row.confidence, "official");
    }

    #[test]
    fn lines_without_usage_are_not_rows() {
        assert!(row_from_line(r#"{"type":"user","uuid":"u-2"}"#).is_none());
        assert!(row_from_line("not json at all").is_none());
        // A usage block with nothing in it is not a measurement.
        assert!(row_from_line(
            r#"{"uuid":"u-3","type":"assistant","timestamp":"2026-08-20T18:00:00.000Z","message":{"role":"assistant","type":"message","content":[],"usage":{}}}"#
        )
        .is_none());
    }

    #[test]
    fn a_project_label_is_the_folder_the_person_would_name() {
        assert_eq!(label_from_cwd("C:\\Users\\a\\Projetos\\estoca").as_deref(), Some("estoca"));
        assert_eq!(label_from_cwd("/home/a/work/api/").as_deref(), Some("api"));
        assert_eq!(label_from_cwd("   ").as_deref(), None);
    }

    /// The property the whole design hangs on: walking twice changes nothing.
    #[test]
    fn walking_the_same_turns_twice_does_not_double_a_single_token() {
        let db = Database::open_in_memory().unwrap();
        let rows: Vec<Row> = vec![row_from_line(TURN).unwrap()];

        let first = write(&db, &rows).unwrap();
        let second = write(&db, &rows).unwrap();
        assert_eq!(first, 1);
        assert_eq!(second, 0, "the unique index on origin_uuid must reject it");

        let total: i64 = db
            .with(|conn| {
                conn.query_row(
                    "SELECT COALESCE(SUM(input_tokens + output_tokens), 0) FROM usage_samples",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(total, 12, "a second walk must leave the totals identical");
    }

    /// Backfilled rows must stay out of the Accounts screen's arithmetic.
    ///
    /// `accounts::quota` filters every figure by `account_id`, and history
    /// recovered from disk has none — it was not spent under any account this
    /// product knows about. If that ever stopped being true, twenty days of
    /// other work would land inside somebody's five-hour window.
    #[test]
    fn recovered_history_carries_no_account_and_no_session() {
        let db = Database::open_in_memory().unwrap();
        write(&db, &[row_from_line(TURN).unwrap()]).unwrap();

        let (account, session, project): (Option<String>, Option<String>, Option<String>) = db
            .with(|conn| {
                conn.query_row(
                    "SELECT account_id, session_id, project_id FROM usage_samples",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(account, None, "history belongs to no account window");
        assert_eq!(session, None);
        assert_eq!(project, None, "no project row is invented for it");
    }
}

/// Run the real backfill against a copy of this machine's database.
///
/// `#[ignore]`d: it needs a database that only exists on a machine that has run
/// the app, and it walks every transcript on disk. Kept because it is the only
/// check that exercises the whole thing against the real corpus — and because
/// the number it prints is the one that decides whether the Analytics screen
/// has anything to show.
///
/// **Works on a copy.** The installed app holds the original open.
///
/// `cargo test real_machine_usage_backfill -- --ignored --nocapture`
#[cfg(test)]
#[test]
#[ignore]
fn real_machine_usage_backfill() {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return;
    };
    let live = std::path::Path::new(&appdata)
        .join("dev.jarvis.desktop")
        .join("jarvis.db");
    if !live.exists() {
        return;
    }
    let copy = std::env::temp_dir().join(format!("jarvis-usage-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&copy);
    std::fs::copy(&live, &copy).unwrap();
    let db = Database::open(&copy).unwrap();

    let before: i64 = db
        .with(|c| c.query_row("SELECT COUNT(*) FROM usage_samples", [], |r| r.get(0)))
        .unwrap();
    let stop = AtomicBool::new(false);
    let first = run(&db, &stop).unwrap();
    let after: i64 = db
        .with(|c| c.query_row("SELECT COUNT(*) FROM usage_samples", [], |r| r.get(0)))
        .unwrap();

    println!(
        "files={} skipped={} rows={} in {}ms   samples {} -> {}",
        first.files, first.skipped, first.rows, first.elapsed_ms, before, after
    );

    // Days, in local time, the way a person reads a calendar.
    let days: Vec<(String, i64, i64)> = db
        .with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT date(ts_ms/1000,'unixepoch','localtime') d,
                        COUNT(*), SUM(input_tokens+output_tokens+cache_write_tokens)
                   FROM usage_samples GROUP BY d ORDER BY d",
            )?;
            let rows: rusqlite::Result<Vec<_>> = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect();
            Ok(rows?)
        })
        .unwrap();
    println!("distinct days = {}", days.len());
    for (day, turns, tokens) in &days {
        println!("  {day}  turns={turns:<6} tokens={tokens}");
    }

    // The property everything else rests on.
    let second = run(&db, &stop).unwrap();
    let twice: i64 = db
        .with(|c| c.query_row("SELECT COUNT(*) FROM usage_samples", [], |r| r.get(0)))
        .unwrap();
    println!("second walk: files={} rows={}", second.files, second.rows);
    assert_eq!(
        after, twice,
        "a second walk added rows — the backfill is not idempotent, and every \
         number on the Analytics screen would grow each time the app starts"
    );

    let _ = std::fs::remove_file(&copy);
}
