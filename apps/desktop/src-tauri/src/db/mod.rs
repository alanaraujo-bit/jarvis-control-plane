//! Local persistence.
//!
//! SQLite in WAL mode holds everything searchable and relational: projects,
//! sessions, missions, notes, decisions, activity. Bulk terminal output does
//! **not** live here — it belongs in the session log (see `session::log`).
//!
//! Migrations are forward-only and versioned. An update must never destroy a
//! user's projects or history (§62), so every migration is additive and the
//! schema version is checked on open.

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::Connection;

pub mod migrations;

pub use migrations::SCHEMA_VERSION;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not prepare the data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "this database was written by a newer version of J.A.R.V.I.S. \
         (schema {found}, this build understands {supported})"
    )]
    FromTheFuture { found: u32, supported: u32 },
}

pub type Result<T> = std::result::Result<T, DbError>;

/// The application database.
///
/// A single connection behind a mutex. SQLite writes serialise anyway, and the
/// access pattern here is many small reads plus occasional writes from command
/// handlers — a pool would add contention bookkeeping for no gain at this size.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        Self::configure(&conn)?;
        migrations::run(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    /// In-memory database, for tests.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn)?;
        migrations::run(&conn)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn configure(conn: &Connection) -> Result<()> {
        // WAL keeps readers from blocking the writer, which matters because
        // agent sessions write continuously while the UI reads.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // NORMAL is the right durability trade for WAL: safe against process
        // crashes, and only at risk from a power loss mid-checkpoint.
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(())
    }

    /// Run a closure with the connection held.
    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock();
        Ok(f(&conn)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates_to_the_current_schema() {
        let db = Database::open_in_memory().unwrap();
        let version: u32 = db
            .with(|c| c.query_row("SELECT MAX(version) FROM schema_migrations", [], |r| r.get(0)))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn migrations_are_idempotent_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jarvis.db");
        for _ in 0..3 {
            let db = Database::open(&path).unwrap();
            let applied: u32 = db
                .with(|c| c.query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0)))
                .unwrap();
            assert_eq!(applied, SCHEMA_VERSION);
        }
    }

    #[test]
    fn refuses_a_database_from_a_newer_build() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("jarvis.db");
        Database::open(&path).unwrap();

        {
            let conn = Connection::open(&path).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, 0)",
                [SCHEMA_VERSION + 5],
            )
            .unwrap();
        }

        // Refusing is the only safe response: silently running would let an
        // older build write rows a newer schema cannot interpret (§91).
        assert!(matches!(
            Database::open(&path),
            Err(DbError::FromTheFuture { .. })
        ));
    }

    #[test]
    fn foreign_keys_cascade_sessions_with_their_project() {
        let db = Database::open_in_memory().unwrap();
        db.with(|c| {
            c.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'Demo', 'C:\\demo', 0, 0)",
                [],
            )?;
            c.execute(
                "INSERT INTO sessions (id, project_id, provider, cwd, state, created_at, log_dir)
                 VALUES ('s1', 'p1', 'shell', 'C:\\demo', 'idle', 0, 'logs/s1')",
                [],
            )?;
            c.execute("DELETE FROM projects WHERE id = 'p1'", [])?;
            let remaining: u32 =
                c.query_row("SELECT COUNT(*) FROM sessions", [], |r| r.get(0))?;
            assert_eq!(remaining, 0);
            Ok(())
        })
        .unwrap();
    }
}
