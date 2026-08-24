//! What a session is called (§88, D36/D38).
//!
//! `sessions.title` existed from migration 1 and nothing ever wrote to it, so
//! every session on every installation of this product is untitled. This module
//! is the writer, and the whole of it is one rule:
//!
//! > **user > provider > derived.**
//!
//! A person who renames a session and watches an `ai-title` overwrite it ten
//! seconds later has been told the product does not respect the one input it
//! was given. So precedence is enforced in SQL, in the `WHERE` clause of the
//! update itself, rather than by reading the current value and deciding in
//! Rust: the tailer thread and a rename from the UI can arrive in either order,
//! and a check-then-write would lose that race exactly as often as it is close.

use rusqlite::params;

use crate::db::Database;
use crate::providers::conversation::truncate;

pub type Result<T> = std::result::Result<T, String>;

/// The longest a stored title may be, in characters.
///
/// Shared by every source so a renamed session and a provider-named one cannot
/// disagree about how much a row can hold.
pub const MAX_CHARS: usize = 72;

/// Where a title came from.
///
/// Serialised through `as_str` rather than a `rename_all` rule — the same
/// choice `search::Kind` and `brain::Kind` make (D13): one identity, one
/// spelling, shared by the column, the wire format and the i18n key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Somebody named it here. Outranks everything and is never overwritten.
    User,
    /// The provider named it itself — Claude Code's `ai-title`.
    Provider,
    /// Cut from the first thing that was typed in the session.
    Derived,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Provider => "provider",
            Self::Derived => "derived",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "user" => Self::User,
            "provider" => Self::Provider,
            "derived" => Self::Derived,
            _ => return None,
        })
    }

    /// Higher wins. Only ever compared, never shown.
    fn rank(self) -> i64 {
        match self {
            Self::Derived => 1,
            Self::Provider => 2,
            Self::User => 3,
        }
    }

    /// The highest existing rank this source is allowed to replace.
    ///
    /// Equal-rank replacement is right for the two *stated* sources and wrong
    /// for the derived one:
    ///
    /// * a person renaming a session twice means the second name;
    /// * Claude Code writes `ai-title` more than once as a conversation grows,
    ///   and the later one is the better summary;
    /// * a derived title, though, is the **first** thing that was typed, and
    ///   every message after it would otherwise re-derive and rename the
    ///   session. A session called by its latest sentence is not titled, it
    ///   flickers.
    fn overwrites_up_to(self) -> i64 {
        match self {
            Self::Derived => 0,
            other => other.rank(),
        }
    }
}

impl serde::Serialize for Source {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for Source {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("unknown title source: {text}")))
    }
}

/// Tidy a title from any source into what a row can hold.
///
/// Collapses whitespace and clips to `MAX_CHARS` **by character**, never by
/// byte — `truncate` is the shared helper for the reason HANDOFF item 36
/// records: a byte offset can land inside an accented letter, and "configuração"
/// cut in half is not a shorter title, it is mojibake.
pub fn clean(text: &str) -> String {
    truncate(text.trim(), MAX_CHARS)
}

/// Record a title, if this source outranks whatever is already there.
///
/// Returns whether the row actually changed. The precedence test is part of the
/// statement, so two writers racing settle it in SQLite rather than between two
/// round trips.
///
/// A `NULL` `title_source` on a row that has a title cannot happen from this
/// build, but a row written by an older one could have anything: `COALESCE` to
/// rank 0 means such a row is treated as weaker than every real source, which
/// is the safe direction — a title gets replaced rather than a real one being
/// permanently frozen out by a value nobody can explain.
pub fn set(db: &Database, session_id: &str, source: Source, text: &str) -> Result<bool> {
    let cleaned = clean(text);
    if cleaned.is_empty() {
        return Ok(false);
    }

    db.with(|conn| {
        let changed = conn.execute(
            "UPDATE sessions
                SET title = ?2, title_source = ?3
              WHERE id = ?1
                AND COALESCE(
                      CASE title_source
                          WHEN 'user'     THEN 3
                          WHEN 'provider' THEN 2
                          WHEN 'derived'  THEN 1
                      END, 0) <= ?4
                AND (title IS NOT ?2 OR title_source IS NOT ?3)",
            params![session_id, cleaned, source.as_str(), source.overwrites_up_to()],
        )?;
        Ok(changed > 0)
    })
    .map_err(|e| e.to_string())
}

/// Derive a title for one session from the first thing typed in it.
///
/// Reads `session_events`, not the log on disk (D38): that table already holds
/// every user message of every session here, put there by D30's own backfill,
/// so this costs one indexed lookup rather than a file walk.
///
/// Returns `Ok(false)` when there is nothing to derive from — a shell nobody
/// typed a whole line into, or a session whose events have not been mirrored.
/// That is the ordinary state for a lot of rows, not a failure.
pub fn derive(db: &Database, session_id: &str) -> Result<bool> {
    let first: Option<String> = db
        .with(|conn| {
            conn.query_row(
                "SELECT text FROM session_events
                  WHERE session_id = ?1 AND kind = 'message' AND label = 'user'
                  ORDER BY ts_ms ASC, seq ASC
                  LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(other),
            })
        })
        .map_err(|e| e.to_string())?;

    match first {
        Some(text) if !text.trim().is_empty() => set(db, session_id, Source::Derived, &text),
        _ => Ok(false),
    }
}

/// How many sessions one pass of the backfill will look at.
///
/// The database has a single mutex-guarded connection, so a pass that held it
/// for the whole history would stall the app. Chunked for the same reason
/// `search::backfill` is, and with the same bookmark shape.
const CHUNK: usize = 100;

/// Give every session recorded before this feature existed a derived title.
///
/// Idempotent and resumable: `title_backfilled_at` is stamped per session
/// whether or not a title was found, so a session with nothing to derive from
/// is looked at once and never again. A crash halfway leaves the rest to be
/// picked up on the next run.
///
/// Deliberately **not** a migration (D30's reasoning, unchanged): a migration is
/// the one place where failing halfway leaves a database claiming a version it
/// does not have.
///
/// Returns how many sessions were given a title.
pub fn backfill(db: &Database) -> Result<usize> {
    let mut titled = 0usize;

    loop {
        let batch: Vec<String> = db
            .with(|conn| {
                // `events_backfilled_at IS NOT NULL` is not a nicety, it is the
                // ordering dependency made safe. This derivation reads
                // `session_events`, which for an old session is filled in by
                // `search::backfill` — running in another thread, at the same
                // launch. Without this clause a session looked at first would
                // find no events, be stamped, and stay untitled for the rest of
                // the installation's life. With it, such a session is simply
                // not a candidate yet and is picked up on a later launch.
                //
                // A session started from this build stamps that column at
                // birth (migration 10), so it is a candidate immediately —
                // and its live tailer has usually titled it already.
                let mut stmt = conn.prepare(
                    "SELECT id FROM sessions
                      WHERE title_backfilled_at IS NULL
                        AND events_backfilled_at IS NOT NULL
                      ORDER BY created_at DESC
                      LIMIT ?1",
                )?;
                let rows = stmt.query_map(params![CHUNK as i64], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<Vec<String>>>()
            })
            .map_err(|e| e.to_string())?;

        if batch.is_empty() {
            return Ok(titled);
        }

        let now = crate::session::log::now_ms();
        for id in &batch {
            if derive(db, id).unwrap_or(false) {
                titled += 1;
            }
            // Stamped after the attempt, and outside it: a session with no user
            // message at all must not be reconsidered on every launch for the
            // rest of the installation's life.
            let stamped = db
                .with(|conn| {
                    conn.execute(
                        "UPDATE sessions SET title_backfilled_at = ?2 WHERE id = ?1",
                        params![id, now],
                    )?;
                    Ok(())
                })
                .map_err(|e| e.to_string());
            if let Err(e) = stamped {
                // Without the bookmark this session would be retried forever.
                // Stop rather than spin.
                tracing::warn!(session = %id, error = %e, "could not stamp a title backfill");
                return Ok(titled);
            }
        }
    }
}

/// How long after launch the backfill starts.
///
/// Longer than `search::backfill`'s own grace, deliberately: that one has to
/// have made progress before this one has anything to read (see the query's
/// own note). Neither correctness nor completeness depends on the gap — the
/// `events_backfilled_at` clause is what makes it safe — but starting after it
/// means most machines are fully titled on the first launch rather than the
/// second.
const STARTUP_GRACE: std::time::Duration = std::time::Duration::from_secs(20);

/// Run the backfill in the background, once, off the startup path.
pub fn spawn(db: std::sync::Arc<Database>) {
    std::thread::Builder::new()
        .name("title-backfill".into())
        .spawn(move || {
            std::thread::sleep(STARTUP_GRACE);
            // Logged even when there is nothing to do, for the reason
            // `search::backfill::spawn` gives at length: a background task
            // that is silent in its steady state cannot be told apart from
            // one that never started.
            match backfill(&db) {
                Ok(titled) => tracing::info!(titled, "session title backfill finished"),
                Err(e) => tracing::warn!(error = %e, "session title backfill stopped early"),
            }
        })
        .expect("spawn title backfill");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'demo', 'C:/demo', 0, 0)",
                [],
            )?;
            conn.execute(
                // `events_backfilled_at` set, because a session whose events
                // are not in the index yet is deliberately not a backfill
                // candidate — see the query in `backfill`.
                "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir,
                                       created_at, events_backfilled_at)
                 VALUES ('s1', 'p1', 'claude-code', 'C:/demo', 'idle', 'C:/logs', 0, 1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn stored(db: &Database) -> (Option<String>, Option<String>) {
        db.with(|conn| {
            conn.query_row(
                "SELECT title, title_source FROM sessions WHERE id = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .unwrap()
    }

    /// The whole point of D36. A person named it; a machine may not rename it.
    #[test]
    fn a_provider_title_never_overwrites_a_rename() {
        let db = db();

        assert!(set(&db, "s1", Source::User, "Login form").unwrap());
        // Exactly the sequence that happens for real: a rename during the first
        // minute of a session, and Claude Code's own `ai-title` arriving after.
        assert!(!set(&db, "s1", Source::Provider, "hello.txt file creation").unwrap());

        assert_eq!(
            stored(&db),
            (Some("Login form".into()), Some("user".into())),
            "a title somebody typed must survive whatever the provider decides"
        );
    }

    #[test]
    fn a_provider_title_replaces_a_derived_one() {
        let db = db();

        assert!(set(&db, "s1", Source::Derived, "add a login form please").unwrap());
        assert!(set(&db, "s1", Source::Provider, "Login form").unwrap());

        assert_eq!(stored(&db), (Some("Login form".into()), Some("provider".into())));
    }

    /// A second `ai-title` with the same text must not report a change — the
    /// tailer sees every line the provider writes, and a "changed" that is
    /// always true would make any listener on it useless.
    #[test]
    fn writing_the_same_title_twice_changes_nothing() {
        let db = db();
        assert!(set(&db, "s1", Source::Provider, "Login form").unwrap());
        assert!(!set(&db, "s1", Source::Provider, "Login form").unwrap());
    }

    #[test]
    fn a_title_is_clipped_by_character_not_by_byte() {
        // 80 characters of an accented word: a byte-offset clip would cut one
        // in half and store replacement garbage (HANDOFF item 36).
        let long = "configuração ".repeat(8);
        let db = db();
        set(&db, "s1", Source::User, &long).unwrap();

        let (title, _) = stored(&db);
        let title = title.unwrap();
        assert!(title.chars().count() <= MAX_CHARS + 1, "clipped to the bound");
        assert!(
            !title.contains('\u{fffd}'),
            "no character was split: {title}"
        );
        assert!(title.contains("configuração"));
    }

    #[test]
    fn an_empty_title_is_not_a_title() {
        let db = db();
        assert!(!set(&db, "s1", Source::User, "   ").unwrap());
        assert_eq!(stored(&db), (None, None));
    }

    /// The derivation reads the search index, not the log (D38).
    #[test]
    fn a_title_is_derived_from_the_first_thing_typed() {
        let db = db();
        db.with(|conn| {
            // Out of order on purpose: the *first* message is the title, and
            // "first" means by timestamp, not by insertion.
            conn.execute(
                "INSERT INTO session_events (session_id, seq, ts_ms, kind, payload, label, text)
                 VALUES ('s1', 2, 2000, 'message', '{}', 'user', 'and now the tests')",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_events (session_id, seq, ts_ms, kind, payload, label, text)
                 VALUES ('s1', 1, 1000, 'message', '{}', 'user', 'add a login form')",
                [],
            )?;
            // An assistant reply must never be mistaken for what was asked.
            conn.execute(
                "INSERT INTO session_events (session_id, seq, ts_ms, kind, payload, label, text)
                 VALUES ('s1', 0, 500, 'message', '{}', 'assistant', 'certainly')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        assert!(derive(&db, "s1").unwrap());
        assert_eq!(
            stored(&db),
            (Some("add a login form".into()), Some("derived".into()))
        );
    }

    #[test]
    fn a_session_with_nothing_typed_in_it_is_left_untitled() {
        let db = db();
        assert!(!derive(&db, "s1").unwrap());
        assert_eq!(stored(&db), (None, None));
    }

    /// A session with no user message must be looked at once, not on every
    /// launch forever.
    #[test]
    fn the_backfill_stamps_even_a_session_it_could_not_title() {
        let db = db();

        assert_eq!(backfill(&db).unwrap(), 0);

        let stamp: Option<i64> = db
            .with(|conn| {
                conn.query_row(
                    "SELECT title_backfilled_at FROM sessions WHERE id = 's1'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert!(stamp.is_some(), "the bookmark is what stops it being retried");

        // And a second pass has nothing left to do.
        assert_eq!(backfill(&db).unwrap(), 0);
    }

    #[test]
    fn the_backfill_titles_what_it_can_and_is_idempotent() {
        let db = db();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO session_events (session_id, seq, ts_ms, kind, payload, label, text)
                 VALUES ('s1', 1, 1000, 'message', '{}', 'user', 'add a login form')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        assert_eq!(backfill(&db).unwrap(), 1);
        assert_eq!(backfill(&db).unwrap(), 0, "a second pass finds nothing left");
        assert_eq!(stored(&db).1, Some("derived".into()));
    }

    /// The ordering trap the `events_backfilled_at` clause exists for: a
    /// session whose conversation has not been indexed yet must be left alone,
    /// not stamped as "looked at" while there was nothing to look at.
    #[test]
    fn a_session_whose_events_are_not_indexed_yet_is_not_a_candidate() {
        let db = db();
        db.with(|conn| {
            conn.execute(
                "UPDATE sessions SET events_backfilled_at = NULL WHERE id = 's1'",
                [],
            )?;
            conn.execute(
                "INSERT INTO session_events (session_id, seq, ts_ms, kind, payload, label, text)
                 VALUES ('s1', 1, 1000, 'message', '{}', 'user', 'add a login form')",
                [],
            )?;
            Ok(())
        })
        .unwrap();

        assert_eq!(backfill(&db).unwrap(), 0);
        let stamp: Option<i64> = db
            .with(|conn| {
                conn.query_row(
                    "SELECT title_backfilled_at FROM sessions WHERE id = 's1'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert!(
            stamp.is_none(),
            "stamping it here would make it untitled forever"
        );

        // Once the search backfill has been past, it is titled on the next run.
        db.with(|conn| {
            conn.execute(
                "UPDATE sessions SET events_backfilled_at = 1 WHERE id = 's1'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        assert_eq!(backfill(&db).unwrap(), 1);
    }

    #[test]
    fn sources_round_trip_through_storage() {
        for source in [Source::User, Source::Provider, Source::Derived] {
            assert_eq!(Source::parse(source.as_str()), Some(source));
        }
        assert_eq!(Source::parse("nonsense"), None);
    }
}
