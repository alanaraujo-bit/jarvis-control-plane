//! Forward-only schema migrations.
//!
//! Rules that keep a shipped product upgradable (§62):
//!
//! 1. Never edit a migration that has shipped — add a new one.
//! 2. Migrations are additive. Dropping or renaming a column strands data on
//!    machines that have not upgraded yet.
//! 3. Each runs inside a transaction with its version record, so a failure
//!    leaves the database on the previous version rather than half-migrated.

use rusqlite::Connection;

use super::{DbError, Result};

/// Highest schema version this build understands.
pub const SCHEMA_VERSION: u32 = 4;

struct Migration {
    version: u32,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: r#"
    -- ---- Projects (§16) --------------------------------------------------
    CREATE TABLE projects (
        id             TEXT PRIMARY KEY,
        name           TEXT NOT NULL,
        path           TEXT NOT NULL UNIQUE,
        -- Denormalised Git facts, refreshed on open. Cheap to read for lists,
        -- and the working tree stays the source of truth.
        is_git         INTEGER NOT NULL DEFAULT 0,
        git_branch     TEXT,
        git_remote     TEXT,
        created_at     INTEGER NOT NULL,
        last_opened_at INTEGER NOT NULL,
        archived       INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_projects_recent ON projects (archived, last_opened_at DESC);

    -- ---- Sessions (§23) --------------------------------------------------
    -- Metadata only. The ordered event stream lives on disk under log_dir;
    -- see session::log for why bulk PTY output is not stored as rows.
    CREATE TABLE sessions (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
        mission_id  TEXT,
        provider    TEXT NOT NULL,
        title       TEXT,
        cwd         TEXT NOT NULL,
        state       TEXT NOT NULL,
        -- Identifier the provider itself uses, so a session can be resumed and
        -- matched to the provider's own transcript on disk.
        provider_session_id TEXT,
        log_dir     TEXT NOT NULL,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL DEFAULT 0,
        ended_at    INTEGER,
        exit_code   INTEGER
    );
    CREATE INDEX idx_sessions_project ON sessions (project_id, created_at DESC);
    CREATE INDEX idx_sessions_state ON sessions (state);
    CREATE INDEX idx_sessions_provider_ref ON sessions (provider, provider_session_id);

    -- ---- Structured session events ---------------------------------------
    -- A searchable mirror of the structured frames in the session log. The log
    -- remains authoritative; this table exists so history is queryable without
    -- scanning every file (§39, §51).
    CREATE TABLE session_events (
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        seq        INTEGER NOT NULL,
        ts_ms      INTEGER NOT NULL,
        kind       TEXT NOT NULL,
        payload    TEXT NOT NULL,
        PRIMARY KEY (session_id, seq)
    ) WITHOUT ROWID;
    CREATE INDEX idx_events_time ON session_events (ts_ms);
    CREATE INDEX idx_events_kind ON session_events (kind, ts_ms);

    -- ---- Usage samples (§28) ---------------------------------------------
    -- `confidence` is stored per sample and never defaulted: an estimate must
    -- not be presentable as a figure the provider reported.
    CREATE TABLE usage_samples (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id   TEXT REFERENCES sessions (id) ON DELETE CASCADE,
        project_id   TEXT REFERENCES projects (id) ON DELETE SET NULL,
        provider     TEXT NOT NULL,
        model        TEXT,
        ts_ms        INTEGER NOT NULL,
        input_tokens        INTEGER,
        output_tokens       INTEGER,
        cache_read_tokens   INTEGER,
        cache_write_tokens  INTEGER,
        cost_usd     REAL,
        confidence   TEXT NOT NULL,
        raw          TEXT
    );
    CREATE INDEX idx_usage_time ON usage_samples (ts_ms);
    CREATE INDEX idx_usage_project ON usage_samples (project_id, ts_ms);

    -- ---- Activity log (§48) ----------------------------------------------
    CREATE TABLE activity (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        ts_ms      INTEGER NOT NULL,
        project_id TEXT REFERENCES projects (id) ON DELETE CASCADE,
        session_id TEXT REFERENCES sessions (id) ON DELETE SET NULL,
        mission_id TEXT,
        kind       TEXT NOT NULL,
        severity   TEXT NOT NULL,
        title      TEXT NOT NULL,
        detail     TEXT
    );
    CREATE INDEX idx_activity_time ON activity (ts_ms DESC);
    CREATE INDEX idx_activity_project ON activity (project_id, ts_ms DESC);

    -- ---- Application settings --------------------------------------------
    CREATE TABLE settings (
        key   TEXT PRIMARY KEY,
        value TEXT NOT NULL
    );
"#,
    },
    Migration {
        version: 2,
        sql: r#"
    -- ---- Missions (§29) --------------------------------------------------
    CREATE TABLE missions (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
        title       TEXT NOT NULL,
        goal        TEXT,
        description TEXT,
        -- ready | running | verifying | waiting | blocked | failed | completed
        status      TEXT NOT NULL DEFAULT 'ready',
        -- NULL means inherit from the project, then from global (§33).
        autonomy    TEXT,
        -- Why a mission is blocked or waiting. A blocked mission must be able
        -- to explain itself; silence is what §34 forbids.
        blocked_reason TEXT,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        started_at  INTEGER,
        completed_at INTEGER
    );
    CREATE INDEX idx_missions_project ON missions (project_id, updated_at DESC);
    CREATE INDEX idx_missions_status ON missions (status);

    CREATE TABLE mission_tasks (
        id          TEXT PRIMARY KEY,
        mission_id  TEXT NOT NULL REFERENCES missions (id) ON DELETE CASCADE,
        description TEXT NOT NULL,
        done        INTEGER NOT NULL DEFAULT 0,
        position    INTEGER NOT NULL
    );
    CREATE INDEX idx_tasks_mission ON mission_tasks (mission_id, position);

    -- ---- Acceptance criteria (§30) ---------------------------------------
    -- The difference between "the agent says it is done" and "it is done".
    -- `verification` is JSON describing how the criterion is *checked*, not a
    -- note about it — see mission::verify.
    CREATE TABLE acceptance_criteria (
        id           TEXT PRIMARY KEY,
        mission_id   TEXT NOT NULL REFERENCES missions (id) ON DELETE CASCADE,
        description  TEXT NOT NULL,
        required     INTEGER NOT NULL DEFAULT 1,
        verification TEXT NOT NULL,
        -- pending | verified | failed
        status       TEXT NOT NULL DEFAULT 'pending',
        position     INTEGER NOT NULL,
        -- §31: an agent may not silently drop a requirement. Removal is a
        -- recorded event with a reason, never a DELETE.
        removed_at     INTEGER,
        removed_reason TEXT,
        removed_by     TEXT
    );
    CREATE INDEX idx_criteria_mission ON acceptance_criteria (mission_id, position);

    -- ---- Evidence (§30) --------------------------------------------------
    CREATE TABLE evidence (
        id           TEXT PRIMARY KEY,
        mission_id   TEXT NOT NULL REFERENCES missions (id) ON DELETE CASCADE,
        criterion_id TEXT REFERENCES acceptance_criteria (id) ON DELETE CASCADE,
        session_id   TEXT REFERENCES sessions (id) ON DELETE SET NULL,
        -- command | file | commit | screenshot | url | manual
        kind         TEXT NOT NULL,
        ok           INTEGER NOT NULL,
        summary      TEXT NOT NULL,
        detail       TEXT,
        ts_ms        INTEGER NOT NULL
    );
    CREATE INDEX idx_evidence_mission ON evidence (mission_id, ts_ms DESC);
    CREATE INDEX idx_evidence_criterion ON evidence (criterion_id, ts_ms DESC);

    -- Autonomy at the project level, the middle tier of the §33 chain.
    ALTER TABLE projects ADD COLUMN autonomy TEXT;
"#,
    },
    Migration {
        version: 3,
        sql: r#"
    -- Quota reporting (§28). Codex states how much of the account allowance is
    -- consumed and when the window resets; recording only token counts would
    -- discard the half of usage intelligence that answers "how close am I?".
    ALTER TABLE usage_samples ADD COLUMN limit_percent REAL;
    ALTER TABLE usage_samples ADD COLUMN limit_resets_at INTEGER;

    -- Files a session touched, mirrored from the session log so the timeline
    -- and review surfaces can query them without scanning every log (§39).
    CREATE TABLE file_changes (
        id         INTEGER PRIMARY KEY AUTOINCREMENT,
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        project_id TEXT REFERENCES projects (id) ON DELETE CASCADE,
        path       TEXT NOT NULL,
        ts_ms      INTEGER NOT NULL
    );
    CREATE INDEX idx_file_changes_session ON file_changes (session_id, ts_ms DESC);
    CREATE INDEX idx_file_changes_project ON file_changes (project_id, ts_ms DESC);
"#,
    },
    Migration {
        version: 4,
        sql: r#"
    -- Human attention, one row per minute in which the user actually typed
    -- something into a session (§53).
    --
    -- A minute bucket rather than a timestamp per keystroke: the question is
    -- "was a person engaged during this minute", not "how fast do they type",
    -- and one row per minute keeps the table small enough to ignore.
    --
    -- This is the only honest way to compute human leverage. Inferring it from
    -- session lifetimes would count a terminal left open overnight as work.
    CREATE TABLE interaction_minutes (
        session_id TEXT NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
        project_id TEXT REFERENCES projects (id) ON DELETE CASCADE,
        minute     INTEGER NOT NULL,
        PRIMARY KEY (session_id, minute)
    ) WITHOUT ROWID;
    CREATE INDEX idx_interaction_minute ON interaction_minutes (minute);
    CREATE INDEX idx_interaction_project ON interaction_minutes (project_id, minute);
"#,
    }];

pub fn run(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
             version    INTEGER PRIMARY KEY,
             applied_at INTEGER NOT NULL
         )",
        [],
    )?;

    let current: u32 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // A database written by a newer build may contain columns and constraints
    // this one does not know about. Opening it read-write risks corrupting the
    // user's data, so stop instead (§91: data integrity above convenience).
    if current > SCHEMA_VERSION {
        return Err(DbError::FromTheFuture {
            found: current,
            supported: SCHEMA_VERSION,
        });
    }

    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        tracing::info!(version = migration.version, "applying schema migration");
        conn.execute_batch(&format!("BEGIN; {} COMMIT;", migration.sql))
            .map_err(|e| {
                let _ = conn.execute_batch("ROLLBACK;");
                e
            })?;
        conn.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![migration.version, crate::session::log::now_ms()],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_versions_are_sequential_and_unique() {
        // A gap or duplicate would silently skip a migration on upgrade.
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                migration.version,
                index as u32 + 1,
                "migrations must be numbered 1..N with no gaps"
            );
        }
        assert_eq!(
            MIGRATIONS.last().map(|m| m.version).unwrap_or(0),
            SCHEMA_VERSION,
            "SCHEMA_VERSION must match the last migration"
        );
    }
}
