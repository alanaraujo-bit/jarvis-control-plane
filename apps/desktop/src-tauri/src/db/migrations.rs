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
pub const SCHEMA_VERSION: u32 = 9;

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
    },
    Migration {
        version: 5,
        sql: r#"
    -- ---- Guardrails (§35) --------------------------------------------------
    -- A rule about one class of sensitive operation, at one scope.
    --
    -- A NULL project_id is the global rule. The absence of a row is meaningful
    -- and is not the same as a row saying 'ask': "follow whatever the wider
    -- scope decides" is a different intention from "ask here regardless".
    CREATE TABLE guardrail_policies (
        id         TEXT PRIMARY KEY,
        project_id TEXT REFERENCES projects (id) ON DELETE CASCADE,
        operation  TEXT NOT NULL,
        -- ask | allow | deny
        decision   TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );
    -- IFNULL folds the global scope into the same index, so a project rule and
    -- the global rule for one operation cannot collide.
    CREATE UNIQUE INDEX idx_guardrail_scope
        ON guardrail_policies (operation, IFNULL(project_id, ''));

    -- Every time a guardrail had something to say, and what came of it.
    --
    -- One table for both origins on purpose. A user asking "what has this
    -- stopped?" does not care whether the command came from an agent's tool
    -- call or from a mission's own verification — and two tables would make
    -- that one question into two queries that could disagree.
    CREATE TABLE guardrail_events (
        id           TEXT PRIMARY KEY,
        ts_ms        INTEGER NOT NULL,
        project_id   TEXT REFERENCES projects (id) ON DELETE CASCADE,
        session_id   TEXT REFERENCES sessions (id) ON DELETE SET NULL,
        mission_id   TEXT,
        criterion_id TEXT,
        -- agent | verification
        origin       TEXT NOT NULL,
        operation    TEXT NOT NULL,
        -- The text that matched, kept verbatim: a guardrail that will not say
        -- what made it fire cannot be reviewed, and an unreviewable guardrail
        -- gets switched off.
        fragment     TEXT NOT NULL,
        command      TEXT NOT NULL,
        -- pending | allowed | denied | asked
        status       TEXT NOT NULL,
        -- A stable code the UI localises, never prose (§65).
        reason       TEXT NOT NULL,
        decided_at   INTEGER,
        decided_by   TEXT
    );
    CREATE INDEX idx_guardrail_events_time ON guardrail_events (ts_ms DESC);
    CREATE INDEX idx_guardrail_events_pending ON guardrail_events (status, ts_ms DESC);
    CREATE INDEX idx_guardrail_events_mission ON guardrail_events (mission_id, ts_ms DESC);
"#,
    },
    Migration {
        version: 6,
        sql: r#"
    -- A stable code the UI localises, so evidence can say what happened in the
    -- reader's own language (§65).
    --
    -- Evidence summaries are generated in Rust and have been English-only; that
    -- is a known correctness gap in a shipped feature. These columns are the fix
    -- the roadmap describes, starting with the summaries guardrails add rather
    -- than leaving one more untranslatable string behind. NULL means the summary
    -- is all there is, which stays true of evidence written by earlier builds.
    --
    -- Its own migration rather than an edit to 5: a database that has already
    -- applied 5 will never re-run it, so appending to a migration that has been
    -- applied anywhere leaves those columns missing while the code expects them.
    -- That is rule 1 at the top of this file, and it was learned here the way
    -- the rest of this codebase learns things — the surface came up blank on a
    -- machine that had the earlier schema.
    ALTER TABLE evidence ADD COLUMN code TEXT;
    -- Arguments for the message, as JSON. Keeps one code reusable instead of
    -- needing a separate code per distinct sentence.
    ALTER TABLE evidence ADD COLUMN code_args TEXT;
"#,
    },
    Migration {
        version: 7,
        sql: r#"
    -- ---- Worktrees (§45) ---------------------------------------------------
    --
    -- A worktree is a second checkout of one repository, in its own directory,
    -- on its own branch. In this product that is a **project**: a project is a
    -- folder on this machine with a checkout in it (§16), and that is exactly
    -- what a worktree is. Verified rather than assumed --
    -- `rev-parse --show-toplevel` inside a worktree returns the worktree's own
    -- path, so Files, the editor and Review all answer about the right tree
    -- with no changes, and path confinement (§41) keeps the meaning it has
    -- everywhere else.
    --
    -- What is missing without this column is only the relationship. A worktree
    -- registered as a bare project row is a folder that appeared in the list
    -- with no explanation of where it came from, and removing the worktree
    -- would leave that row pointing at nothing.
    ALTER TABLE projects ADD COLUMN worktree_of TEXT REFERENCES projects (id);
"#,
    },
    Migration {
        version: 8,
        sql: r#"
    -- ---- Project Brain (§36–§38) and Notes (§40) ---------------------------
    --
    -- Two tables, because these are two different things and the difference is
    -- load-bearing rather than cosmetic: **knowledge is briefed to an agent and
    -- a note is not**. That single question decides which one something is, so
    -- it does not need a per-item switch that somebody has to remember to set.
    --
    -- Knowledge is what stays true about a project — what it is, how it is
    -- built, what will bite you. A note is working memory: a reminder, a link,
    -- a thing to come back to. Handing an agent somebody's todo list as
    -- context would be worse than handing it nothing.

    CREATE TABLE project_knowledge (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
        -- what | convention | gotcha | glossary. Stored as the same id the i18n
        -- keys use, never prose (D13).
        kind        TEXT NOT NULL,
        body        TEXT NOT NULL,
        -- human | agent. Who said it, so the reader can weigh it (§28). An
        -- agent's claim about a project is not the same kind of fact as the
        -- owner's, and flattening them would hide that.
        source      TEXT NOT NULL,
        -- Where it came from, when an agent wrote it. Kept so an entry can be
        -- traced back to the session that learned it.
        session_id  TEXT REFERENCES sessions (id) ON DELETE SET NULL,
        mission_id  TEXT REFERENCES missions (id) ON DELETE SET NULL,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL,
        -- Archived rather than deleted, like everything else here: knowledge
        -- that stopped being true is still a fact about the project's history.
        archived    INTEGER NOT NULL DEFAULT 0
    );
    CREATE INDEX idx_knowledge_project ON project_knowledge (project_id, archived, kind);

    CREATE TABLE project_notes (
        id          TEXT PRIMARY KEY,
        project_id  TEXT NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
        body        TEXT NOT NULL,
        pinned      INTEGER NOT NULL DEFAULT 0,
        mission_id  TEXT REFERENCES missions (id) ON DELETE SET NULL,
        session_id  TEXT REFERENCES sessions (id) ON DELETE SET NULL,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );
    CREATE INDEX idx_notes_project ON project_notes (project_id, pinned DESC, updated_at DESC);
"#,
    },
    Migration {
        version: 9,
        sql: r#"
    -- ---- Global Search (§51) ------------------------------------------------
    --
    -- `session_events` has had no writer since migration 1 -- a repo-wide check
    -- of every `INSERT` site confirms it. It exists, with zero rows, on every
    -- installation. The columns below are additive, per rule 2 at the top of
    -- this file, and give conversation content -- what an agent said, thought,
    -- ran, and got back -- somewhere to live so it can be found later (§39).
    --
    -- `kind` is repurposed here: nothing ever wrote the shape the original
    -- comment described, so nothing depends on that meaning. It now carries
    -- `ConversationItem`'s own tag (message | thinking | toolCall | toolResult |
    -- error) rather than the coarser session-log `EventKind`, which collapses
    -- message, thinking, turnEnded and error together -- exactly the distinction
    -- search needs to tell what the agent said from what it was only thinking.
    ALTER TABLE session_events ADD COLUMN project_id TEXT REFERENCES projects (id) ON DELETE CASCADE;
    -- Who or what, when a kind needs to say: the speaking role for a message,
    -- the tool name for a call, ok/error for a result. NULL where there is only
    -- one thing it could be.
    ALTER TABLE session_events ADD COLUMN label TEXT;
    -- The plain text search actually matches. `payload` stays the full JSON
    -- item for anything that needs the structured shape back.
    ALTER TABLE session_events ADD COLUMN text TEXT;
    CREATE INDEX idx_events_project_time ON session_events (project_id, ts_ms DESC);

    -- A standalone FTS5 index, not `content=session_events`: that mode keys
    -- against an integer rowid, and session_events is `WITHOUT ROWID` with a
    -- composite (session_id, seq) key. Keeping its own copy of the handful of
    -- columns a result needs costs nothing at this table's size and keeps the
    -- index decoupled from session_events' key shape.
    CREATE VIRTUAL TABLE session_events_fts USING fts5(
        session_id UNINDEXED,
        ts_ms UNINDEXED,
        project_id UNINDEXED,
        kind UNINDEXED,
        label UNINDEXED,
        text
    );
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

    /// Fingerprint of a migration's SQL.
    ///
    /// FNV-1a: not cryptography, and it does not need to be. It only has to
    /// change when the text changes.
    fn fingerprint(sql: &str) -> u64 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in sql.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        hash
    }

    /// Rule 1 at the top of this file, made executable.
    ///
    /// A shipped migration's text must never change. Appending an `ALTER TABLE`
    /// to a migration that has already run leaves those columns missing on
    /// exactly the machines that matter — the ones carrying a user's history —
    /// while a database built from scratch looks perfect. That happened here:
    /// the mission surface went blank on the installed copy and every test
    /// stayed green, because every test built its database from nothing.
    ///
    /// It cannot be caught by replaying migrations, because the test only ever
    /// has the *current* text to replay — the first attempt at this test made
    /// that mistake and passed while the bug was reintroduced. A recorded
    /// fingerprint is what actually notices.
    ///
    /// **When this fails:** if you edited a migration, undo it and add a new
    /// one. Only update a number here when adding a genuinely new migration.
    #[test]
    fn a_shipped_migration_is_never_edited() {
        const SHIPPED: &[(u32, u64)] = &[
            (1, 0x8c60_1cfb_a4c0_8781),
            (2, 0xa812_ab92_91c6_aae9),
            (3, 0x8988_cb64_23bd_8b74),
            (4, 0xe44a_5219_db9e_f30d),
            (5, 0x6ee9_96ed_2d3e_b954),
            (6, 0x1851_a45c_4cbe_3881),
            (7, 0x119d_80b1_fc88_9edc),
            (8, 0x93ed_a072_1ac5_8f63),
            (9, 0x5843_2197_f138_27bd),
        ];

        for migration in MIGRATIONS {
            let Some((_, expected)) = SHIPPED.iter().find(|(v, _)| *v == migration.version) else {
                panic!(
                    "migration {} has no recorded fingerprint. If it is new, add \
                     (version, 0x{:016x}) to SHIPPED.",
                    migration.version,
                    fingerprint(migration.sql)
                );
            };
            assert_eq!(
                fingerprint(migration.sql),
                *expected,
                "migration {} was edited after shipping. Databases that already \
                 applied it will never see the change — undo the edit and add a \
                 new migration instead. (Its fingerprint is now 0x{:016x}.)",
                migration.version,
                fingerprint(migration.sql)
            );
        }
    }

    /// Upgrading step by step must land on the same schema as building fresh.
    ///
    /// This catches a migration that is not additive — one that assumes a state
    /// only a fresh database has. It does **not** catch a migration whose text
    /// was edited; `a_shipped_migration_is_never_edited` is what does that.
    #[test]
    fn upgrading_an_older_database_produces_the_same_schema_as_a_new_one() {
        let columns = |conn: &Connection, table: &str| -> Vec<String> {
            let mut stmt = conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}') ORDER BY name"))
                .unwrap();
            let rows: rusqlite::Result<Vec<String>> =
                stmt.query_map([], |row| row.get(0)).unwrap().collect();
            rows.unwrap()
        };

        // A database that stopped at each earlier version, then upgraded.
        for stop_at in 1..SCHEMA_VERSION {
            let stepped = Connection::open_in_memory().unwrap();
            stepped
                .execute(
                    "CREATE TABLE IF NOT EXISTS schema_migrations (
                         version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL)",
                    [],
                )
                .unwrap();
            for migration in MIGRATIONS.iter().filter(|m| m.version <= stop_at) {
                stepped
                    .execute_batch(&format!("BEGIN; {} COMMIT;", migration.sql))
                    .unwrap();
                stepped
                    .execute(
                        "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, 0)",
                        [migration.version],
                    )
                    .unwrap();
            }
            // Now upgrade it the way a real installation would.
            run(&stepped).unwrap();

            let fresh = Connection::open_in_memory().unwrap();
            run(&fresh).unwrap();

            for table in [
                "evidence",
                "guardrail_events",
                "missions",
                "sessions",
                "projects",
                "session_events",
            ] {
                assert_eq!(
                    columns(&stepped, table),
                    columns(&fresh, table),
                    "a database upgraded from v{stop_at} has a different `{table}` \
                     than a new one — a migration was edited after it shipped"
                );
            }
        }
    }
}
