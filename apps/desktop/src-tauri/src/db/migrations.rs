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
pub const SCHEMA_VERSION: u32 = 19;

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
    },
    Migration {
        version: 10,
        sql: r#"
    -- ---- Global Search backfill (§51, D30) ---------------------------------
    --
    -- Migration 9 gave conversation content somewhere to live, but only from
    -- that build onward: every session recorded before it has its structured
    -- frames on disk in the session log and nothing in `session_events`, so it
    -- is invisible to search in a way that looks identical to "no match".
    --
    -- This column is the backfill's bookmark, and it is *only* a column. The
    -- scan itself deliberately does not run here: reading every session log on
    -- the machine is unbounded work over on-disk data, and a migration is the
    -- one place where failing halfway leaves a database claiming a version it
    -- does not have. `search::backfill` does the walk afterwards, off the
    -- startup path, one session per transaction, resumable from this bookmark.
    --
    -- NULL means "not yet backfilled". A session created from this build on is
    -- stamped at insert time, because its live transcript tailer already
    -- mirrors it and re-reading its log would only duplicate rows.
    ALTER TABLE sessions ADD COLUMN events_backfilled_at INTEGER;
"#,
    },
    Migration {
        version: 11,
        sql: r#"
    -- ---- Accounts (§66) ----------------------------------------------------
    --
    -- One row per signed-in provider account. Several accounts on the same
    -- provider is the ordinary case this exists for: four Claude Pro
    -- subscriptions, each with its own five-hour allowance, and work that
    -- should move to the next one rather than stop.
    --
    -- `config_dir` is the whole mechanism. Each account owns a directory handed
    -- to the provider as its configuration root (CLAUDE_CONFIG_DIR / CODEX_HOME)
    -- when a session starts, so two accounts never share a credential file.
    -- Nothing here ever rewrites the machine's own credentials: the account
    -- already signed in on this machine is *adopted* — its row points at the
    -- real ~/.claude — and every account added afterwards gets a directory of
    -- ours. Swapping one global credential file instead would log the user out
    -- of the session they are sitting in front of, and could not let a running
    -- session finish on the old account while new work starts on the next one,
    -- which is the entire point of the feature.
    --
    -- No secret is ever stored in this table. `email`, `org_name` and `plan`
    -- are identity, read back from the provider's own status command, and exist
    -- so a person can tell four accounts apart (§60/§61).
    CREATE TABLE provider_accounts (
        id          TEXT PRIMARY KEY,
        provider    TEXT NOT NULL,
        label       TEXT NOT NULL,
        config_dir  TEXT NOT NULL,
        -- 1 for the machine's own configuration directory, which is adopted
        -- rather than created — and never deleted when the account is removed.
        adopted     INTEGER NOT NULL DEFAULT 0,
        email       TEXT,
        org_id      TEXT,
        org_name    TEXT,
        plan        TEXT,
        signed_in   INTEGER NOT NULL DEFAULT 0,
        checked_at  INTEGER,
        -- Exactly one account per provider is the one new sessions start on.
        active      INTEGER NOT NULL DEFAULT 0,
        -- Taken out of the rotation by the user. Distinct from exhausted: one
        -- is a decision, the other is a measurement.
        paused      INTEGER NOT NULL DEFAULT 0,
        position    INTEGER NOT NULL,
        created_at  INTEGER NOT NULL,
        last_used_at INTEGER
    );
    CREATE UNIQUE INDEX idx_accounts_dir ON provider_accounts (provider, config_dir);
    CREATE INDEX idx_accounts_order ON provider_accounts (provider, position);

    -- Everything a provider has *stated* about an account's allowance.
    --
    -- Append-only, and deliberately not one mutable "current quota" row: the
    -- reset time of a window is only knowable from an observation, a rejection
    -- has to stay on the record after it clears, and the observed forecast
    -- (§28) takes its calibration from past rejections. A row here is always
    -- something a provider said, never something we inferred — an estimate
    -- lives in the aggregate, not in this table.
    CREATE TABLE account_limit_events (
        id           INTEGER PRIMARY KEY AUTOINCREMENT,
        account_id   TEXT NOT NULL REFERENCES provider_accounts (id) ON DELETE CASCADE,
        session_id   TEXT,
        ts_ms        INTEGER NOT NULL,
        -- five_hour | weekly | opus_weekly | unknown — the provider's own name
        -- for the window, kept verbatim rather than mapped onto ours.
        window       TEXT NOT NULL,
        -- ok | warning | rejected
        status       TEXT NOT NULL,
        resets_at_ms INTEGER,
        percent      REAL,
        detail       TEXT
    );
    CREATE INDEX idx_limit_events_account ON account_limit_events (account_id, ts_ms DESC);
    CREATE INDEX idx_limit_events_window ON account_limit_events (account_id, window, ts_ms DESC);

    -- Which account a session ran on, and which account spent these tokens.
    --
    -- NULL means "recorded before accounts existed", which is a different thing
    -- from "the default account" and must not be folded into it: a quota window
    -- computed over rows that predate the feature would attribute somebody
    -- else's spend to whichever account happens to be first in the list.
    ALTER TABLE sessions ADD COLUMN account_id TEXT;
    ALTER TABLE usage_samples ADD COLUMN account_id TEXT;
    CREATE INDEX idx_usage_account ON usage_samples (account_id, ts_ms);
"#,
    },
    Migration {
        version: 12,
        sql: r#"
    -- ---- Notifications (§49) ------------------------------------------------
    --
    -- What needed a person, and whether they have seen it yet.
    --
    -- ## Why this is not the activity log
    --
    -- The two look alike and are not. `activity` is an immutable record of what
    -- happened, written at the bar "would a person want to know this later".
    -- A notification is written at the bar "does somebody need to look at this
    -- now", it carries read state, and it is deduplicated. Folding them
    -- together would mean either an activity log with a mutable column that
    -- means nothing for most of its rows, or a notification list that has to
    -- re-derive what needs attention out of prose every time it is opened.
    --
    -- The two bars genuinely disagree in both directions. A finished agent turn
    -- is worth a notification and is deliberately *not* worth an activity row —
    -- there are dozens per session, and §48 says a log that records everything
    -- is a log nobody reads. A quota threshold crossed at 3am is worth an
    -- activity row and is not worth waking anybody.
    --
    -- ## What is stored, and what is not
    --
    -- `kind` and `reason` are stable identifiers the UI localises (§65), never
    -- prose. `preview` is the exception and is deliberate: it is the agent's
    -- own words — the question it drew on its own screen — and translating an
    -- agent's question would be inventing one. It is short, and it is the only
    -- untranslated string the surface shows.
    --
    -- `confidence` travels with the row for the same reason it travels with a
    -- usage figure (§28): a question read off a terminal is Observed, a turn
    -- the provider declared finished is Official, and the surface must be able
    -- to tell them apart rather than presenting a reading as a statement.
    CREATE TABLE notifications (
        id          INTEGER PRIMARY KEY AUTOINCREMENT,
        ts_ms       INTEGER NOT NULL,
        -- needsApproval | finished | stopped
        kind        TEXT NOT NULL,
        -- Which flavour of that kind, e.g. providerPrompt, turnEnded.
        reason      TEXT NOT NULL,
        -- official | observed
        confidence  TEXT NOT NULL,
        project_id  TEXT REFERENCES projects (id) ON DELETE CASCADE,
        session_id  TEXT,
        mission_id  TEXT,
        provider    TEXT,
        preview     TEXT,
        -- When the person laid eyes on it. NULL is the whole point of the row.
        seen_at     INTEGER,
        -- When they went to it from here, as against merely seeing it.
        acted_at    INTEGER
    );
    CREATE INDEX idx_notifications_recent ON notifications (ts_ms DESC);
    CREATE INDEX idx_notifications_unseen ON notifications (seen_at, ts_ms DESC);
    CREATE INDEX idx_notifications_session ON notifications (session_id, ts_ms DESC);
"#,
    },
    Migration {
        version: 13,
        sql: r#"
    -- ---- Session History (§88, D36–D39) -------------------------------------
    --
    -- `sessions.title` has existed since migration 1 and nothing has ever
    -- written to it. This migration is what makes it mean something.
    --
    -- ## Why a title needs a source
    --
    -- Three different things can name a session, and they are not equally
    -- trustworthy: the person renamed it, the provider named it itself (Claude
    -- Code writes an `ai-title` line into its own transcript — 89 of the 124
    -- transcripts on this machine carry one), or we cut it out of the first
    -- thing that was typed. Showing all three identically would assert
    -- something the product does not know, which is the same mistake §28 exists
    -- to prevent for a token count.
    --
    -- Precedence is user > provider > derived, and it is enforced in
    -- `session::title`, not here: a rename that a later `ai-title` silently
    -- overwrote would be the product ignoring the one input it was given.
    --
    -- NULL means the session has no title yet, which is the honest state for a
    -- shell that has never been named and never will be.
    ALTER TABLE sessions ADD COLUMN title_source TEXT;

    -- The backfill's bookmark, and only a bookmark — same shape and same
    -- reasoning as migration 10's (D30, D38). The walk itself does not run
    -- here: a migration is the one place where failing halfway leaves a
    -- database claiming a version it does not have.
    --
    -- Unlike D30's, this backfill reads `session_events` rather than the logs
    -- on disk, because that table already holds every user message of every
    -- session — D30's own walk put them there.
    ALTER TABLE sessions ADD COLUMN title_backfilled_at INTEGER;

    -- History is a question about *this machine*, not about one project, and
    -- the only index on `sessions.created_at` is `(project_id, created_at DESC)`
    -- — a prefix that a cross-project ORDER BY cannot use, so the query would
    -- sort every session on every page.
    CREATE INDEX idx_sessions_recent ON sessions (created_at DESC);

    -- A history row states what a session cost, which is `SUM(...) WHERE
    -- session_id = ?` once per row. Every index on this table so far is by
    -- time, project or account — none of them has `session_id` as a prefix, so
    -- that sum would scan every usage sample ever recorded, once per row on
    -- the page.
    CREATE INDEX idx_usage_session ON usage_samples (session_id);
"#,
    },
    Migration {
        version: 14,
        sql: r#"
    -- ---- Continuing a past session (§88, D41) -------------------------------
    --
    -- Which session this one picked up from. A resumed session is a **new**
    -- session — a new process, a new log, a new row — that carries the previous
    -- conversation's context. Recording where it came from is what turns two
    -- rows into one thread, the same way `sessions.mission_id` does for §86.
    --
    -- NULL is the ordinary case: most sessions start from nothing.
    ALTER TABLE sessions ADD COLUMN resumed_from TEXT;

    -- ---- Belt and braces for migration 13's indexes -------------------------
    --
    -- Migration 13's SQL was edited after it was first written, and rule 1 at
    -- the top of this file exists because that is exactly how a database ends
    -- up missing a statement while a freshly built one looks perfect (item 9 in
    -- HANDOFF). The reasoning said every edit landed before any build ran, and
    -- `a_shipped_migration_is_never_edited` now pins 13's text — but reasoning
    -- is the thing item 9 warns against, and the cost of being wrong is every
    -- history page scanning `usage_samples` end to end, once per row.
    --
    -- `IF NOT EXISTS` makes this a no-op on a database that already has them
    -- and a repair on one that does not. It is deliberately a **new migration**
    -- rather than an edit to 13, which is the only correct way to add a
    -- statement to a version that has already run somewhere.
    CREATE INDEX IF NOT EXISTS idx_sessions_recent ON sessions (created_at DESC);
    CREATE INDEX IF NOT EXISTS idx_usage_session ON usage_samples (session_id);
"#,
    },
    Migration {
        version: 15,
        sql: r#"
    -- ---- The newest live quota reading per account (§66, M16) ---------------
    --
    -- Both CLIs answer, on demand, how full an account's windows are right now
    -- (`docs/M16-QUOTA.md` §1). Asking costs a CLI startup — a second or two —
    -- which is fine for a refresh and far too slow for opening a panel. This
    -- table is what the panel draws instantly while a fresh probe runs behind
    -- it, so the surface is never a spinner over an empty card.
    --
    -- **One row per account, deliberately.** `account_limit_events` is already
    -- the append-only history and every live reading is folded into it, so a
    -- second log here would be a second record of the same facts that could
    -- disagree with the first. This is a cache, and it says so by holding
    -- exactly one row.
    --
    -- `payload` is the serialised reading rather than a column per field. The
    -- provider's own description of the request ends "Experimental — the
    -- response shape may change", so pinning its shape into a schema would
    -- guarantee a migration every time it moves. A payload this build cannot
    -- parse is discarded and re-probed, which is the correct behaviour for a
    -- cache and would be the wrong behaviour for a record.
    CREATE TABLE account_live_readings (
        account_id TEXT PRIMARY KEY
                   REFERENCES provider_accounts (id) ON DELETE CASCADE,
        read_at_ms INTEGER NOT NULL,
        payload    TEXT NOT NULL
    );
"#,
    },
    Migration {
        version: 16,
        sql: r#"
    -- ---- The Notebook (M19) -------------------------------------------------
    --
    -- The person's own library: ideas, and the prompts they have been keeping in
    -- WhatsApp messages to themselves. Global, never briefed to an agent, and
    -- hoarded for months.
    --
    -- **This is not `project_notes` (§40) and must not become it.** That table
    -- is working memory *about one project*: it lives in that project's Brain,
    -- it can be promoted into knowledge an agent is briefed with, and
    -- `brain::delete_note` says in as many words that a note "is a scratchpad
    -- entry whose whole purpose is to be temporary". Every one of those is the
    -- opposite of what this table holds. Making `project_notes.project_id`
    -- nullable to serve both would have forced `promote_note` -- which has to
    -- know *which* project's knowledge to write into -- to handle a note that
    -- belongs to no project. Two names for two things beats one name for two
    -- behaviours (§23 is about not keeping the same fact twice, and these are
    -- not the same fact).

    -- Folders, one level deep.
    --
    -- A nullable self-referencing `parent_id` would buy recursive rendering,
    -- cycle prevention on every move, and a decision about what a cascade does
    -- -- in service of a nesting depth most people never build. This is the
    -- same call `MAX_SLOTS` made for split panes, and it is a choice rather
    -- than an oversight: a future session wanting nesting should add it
    -- deliberately, not assume it was forgotten.
    CREATE TABLE notebooks (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        position   INTEGER NOT NULL,
        created_at INTEGER NOT NULL,
        updated_at INTEGER NOT NULL
    );

    -- `notebook_id` is nullable, and NULL means **unfiled** rather than
    -- orphaned. That is what makes deleting a folder safe: ON DELETE SET NULL
    -- drops its notes into Unfiled instead of taking them with it. Somebody who
    -- has kept forty prompts for a year must not lose them to one click, and a
    -- confirmation dialog is a weaker guarantee than a schema that cannot do
    -- the damage in the first place.
    CREATE TABLE notebook_notes (
        id          TEXT PRIMARY KEY,
        notebook_id TEXT REFERENCES notebooks (id) ON DELETE SET NULL,
        -- Optional. The surface falls back to the body's first line, so there
        -- is never a nameless row in the list and never a required field
        -- between having an idea and writing it down.
        title       TEXT NOT NULL DEFAULT '',
        body        TEXT NOT NULL DEFAULT '',
        pinned      INTEGER NOT NULL DEFAULT 0,
        created_at  INTEGER NOT NULL,
        updated_at  INTEGER NOT NULL
    );

    -- The list order, exactly: pinned first, then most recently edited.
    CREATE INDEX idx_notebook_notes
        ON notebook_notes (notebook_id, pinned DESC, updated_at DESC);
"#,
    },
    Migration {
        version: 17,
        sql: r#"
    -- ---- Identity (M20) ------------------------------------------------------
    --
    -- A *person's* account. Not to be confused with `accounts` (M13/M16), which
    -- is a provider subscription and whose whole identity is a configuration
    -- directory on disk. Two different things, two different names -- see
    -- docs/M20-IDENTITY.md section 3.
    --
    -- Nothing in the core reads this to decide whether work may happen. The
    -- product is local-first, and half of it runs with nobody present:
    -- unattended runs, the search backfill, the notification feed. An account is
    -- additive -- it names the person, holds their preferences, and is the seat
    -- a future cloud sync would attach to.
    CREATE TABLE identity_accounts (
        id            TEXT PRIMARY KEY,
        -- Stored trimmed and lower-cased, so UNIQUE is the case-insensitive
        -- uniqueness people actually expect from an e-mail address. Normalising
        -- at the write beats a COLLATE NOCASE index, because then every reader
        -- sees the same string rather than whichever casing was typed first.
        email         TEXT NOT NULL UNIQUE,
        display_name  TEXT NOT NULL,
        -- An Argon2id PHC string: algorithm, parameters and salt travel with the
        -- hash, so changing the cost later verifies old passwords instead of
        -- locking everyone out. NULL means no local password -- reserved for an
        -- account linked to an external provider. Deliberately not naming a
        -- blocker number here: renumbering one would mean editing this
        -- migration, and a migration is the one file that must never change.
        password_hash TEXT,
        -- 'local' today; 'google' when Google sign-in is available.
        auth_provider TEXT NOT NULL DEFAULT 'local',
        -- Guessing at the keyboard is the realistic threat to a local account:
        -- somebody holding the database file does not need to guess at all. So
        -- these two are about the person standing there, and the lockout is
        -- deliberately short rather than punitive.
        failed_attempts INTEGER NOT NULL DEFAULT 0,
        locked_until  INTEGER,
        created_at    INTEGER NOT NULL,
        updated_at    INTEGER NOT NULL,
        last_signed_in_at INTEGER
    );

    -- Preferences that belong to a person rather than to this machine.
    --
    -- `settings` deliberately did **not** grow an account column. Its contract is
    -- "unset has one spelling: no row", and mission::store, onboarding and
    -- settings::get/set all read it unscoped -- a scope column would have
    -- silently changed what every existing reader sees. `settings` stays
    -- machine-scoped; this is the person's copy, and identity::prefs mirrors
    -- between them on sign-in. Which preference is which is decided per key, in
    -- identity::prefs::CARRIED.
    CREATE TABLE identity_settings (
        account_id TEXT NOT NULL REFERENCES identity_accounts (id) ON DELETE CASCADE,
        key        TEXT NOT NULL,
        value      TEXT NOT NULL,
        PRIMARY KEY (account_id, key)
    ) WITHOUT ROWID;
"#,
    },
    Migration {
        version: 18,
        sql: r#"
    -- ---- Telling two provider accounts apart (M21) ---------------------------
    --
    -- An account in this product is a configuration directory, and *nothing
    -- stops two directories being signed into the same subscription*. It is not
    -- hypothetical: `claude auth login` in an empty directory reuses whatever
    -- claude.ai session the browser already holds, so the ordinary way to add a
    -- second account signs it into the first one, silently, in about a second.
    -- Both cards then draw the same dial off one allowance, which reads as a
    -- broken meter rather than as the truth.
    --
    -- M13 already had the guard (`accounts::same_subscription`) and it could
    -- not fire, because it keyed on an e-mail that was eleven hours stale. The
    -- three columns here are what make that guard trustworthy.

    -- The provider's own identifier for the subscription, from `oauthAccount`
    -- in the provider's config. Authoritative where e-mail is merely a label:
    -- it survives an alias, a rename and a change of casing, and it is the same
    -- string in every directory signed into one account. Kept *additive* -- an
    -- account with no uuid still compares by e-mail, because Codex publishes no
    -- equivalent and an absent value must never read as "matches".
    ALTER TABLE provider_accounts ADD COLUMN account_uuid TEXT;

    -- When the identity was last *attempted*, as opposed to `checked_at`, which
    -- from now on means the last time one was successfully read.
    --
    -- The two were one column, and a failed read returned early without
    -- stamping anything -- so "the CLI could not answer" and "nothing has
    -- changed since the last answer" were the same state, indistinguishable
    -- forever. A card cannot say "identity not verified since 12:06" while the
    -- only timestamp it has says the opposite.
    ALTER TABLE provider_accounts ADD COLUMN identity_attempted_at INTEGER;

    -- When this row's *current* subscription was first seen on it.
    --
    -- A directory can be signed out and signed back in as somebody else -- which
    -- is exactly what happened to the machine account on this machine. When it
    -- does, every `account_limit_events` row and every `usage_samples` row
    -- before that moment belongs to the previous subscription, and
    -- `quota::calibration` and `implied_allowance` learn an allowance from
    -- them. Reading them as this account's history attributes one person's
    -- spend to another: the exact "it merged the two accounts" failure this
    -- migration exists to end.
    --
    -- Zero means "always" -- the honest default for rows that predate this
    -- column, whose history has no known boundary in it.
    ALTER TABLE provider_accounts ADD COLUMN subscription_since INTEGER NOT NULL DEFAULT 0;

    CREATE INDEX idx_accounts_subscription ON provider_accounts (provider, account_uuid);
"#,
    },
    Migration {
        version: 19,
        sql: r#"
    -- ---- Recovering the history that was always on disk (M22) ----------------
    --
    -- Analytics could only ever see what J.A.R.V.I.S. itself watched happen.
    -- On this machine that is two days and 889 samples, while the provider's
    -- own transcripts hold **twenty days and 45,487 turns** of the same work,
    -- with real gaps in it. A calendar and a streak over two days are
    -- decorations; over twenty days of measured work they are the screen.
    --
    -- Same shape as D30's search backfill and for the same reasons: a
    -- migration adds *columns*, and the walk over unbounded on-disk data
    -- happens afterwards, off the startup path, resumable.

    -- The provider's own identifier for one turn.
    --
    -- Every usage-bearing line in a Claude Code transcript carries `uuid`
    -- (verified across the real corpus). Recording it makes the backfill
    -- idempotent for free: `INSERT OR IGNORE` against the unique index below
    -- means re-running after a crash, or shipping a build that walks the same
    -- files again, cannot double a single token.
    --
    -- NULL for every row written live by the tailer, which is why the index is
    -- partial -- those rows are deduplicated by not being walked at all (the
    -- backfill skips any transcript whose session J.A.R.V.I.S. already ran).
    ALTER TABLE usage_samples ADD COLUMN origin_uuid TEXT;
    CREATE UNIQUE INDEX idx_usage_origin ON usage_samples (origin_uuid)
        WHERE origin_uuid IS NOT NULL;

    -- What to call the project a historical turn belongs to.
    --
    -- `project_id` references `projects`, and the transcripts cover 35
    -- directories against 3 registered projects. Inventing project rows for
    -- folders the person never opened here would put strangers in their
    -- project list; leaving the attribution NULL would collapse twenty days of
    -- history into one unlabelled heap. So the name travels with the row, read
    -- from the `cwd` the provider recorded, and the surface reads
    -- `COALESCE(projects.name, project_label)`.
    ALTER TABLE usage_samples ADD COLUMN project_label TEXT;

    CREATE INDEX idx_usage_day ON usage_samples (ts_ms);

    -- One row per transcript already walked: the bookmark, and only a bookmark.
    --
    -- Keyed on the file path, with its size and modification time, so a
    -- transcript that has since grown -- the session was resumed, or is the one
    -- running right now -- is walked again for its new turns, while the unique
    -- index above keeps the turns it already had from being counted twice.
    CREATE TABLE usage_backfill_files (
        path      TEXT PRIMARY KEY,
        size      INTEGER NOT NULL,
        mtime_ms  INTEGER NOT NULL,
        turns     INTEGER NOT NULL,
        done_at   INTEGER NOT NULL
    ) WITHOUT ROWID;
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
            (10, 0xee5e_755e_d603_e551),
            (11, 0xd1c1_2851_e515_228c),
            (12, 0x4ea3_d5cb_46cb_d4a8),
            (13, 0xc7ca_954d_e678_f1bd),
            (14, 0x1dd0_9220_2d82_afda),
            (15, 0x2668_ef6f_85b5_38f8),
            (16, 0x709a_d823_8bfb_79ff),
            // Re-recorded once, deliberately, while M20 was still unreleased.
            // The two edits were inside SQL comments — the applied schema is
            // byte-identical either way, and the only database that had run
            // this migration was this development machine. That is the *only*
            // circumstance in which a number here may be changed rather than a
            // new migration added; the comments now name no blocker number, so
            // a future renumbering cannot make this happen again.
            (17, 0x3905_0f33_a709_3a14),
            (18, 0x3ff2_979b_f471_0e66),
            (19, 0x1085_a050_9b08_3287),
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
                "notifications",
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
