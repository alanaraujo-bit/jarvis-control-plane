//! Session History against a real database and a real directory on disk.
//!
//! The two things worth testing here are the two that were wrong somewhere
//! else first: a search that finds a session by what was *said* in it (D26 —
//! every substantive reply was silently missing from the index because of one
//! `match` arm), and a delete that actually takes the FTS rows with it (D39 —
//! the index is standalone, so nothing cascades).

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
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p2', 'other', 'C:/other', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    db
}

fn add_session(db: &Database, id: &str, project: &str, provider: &str, created: i64, log_dir: &str) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at)
             VALUES (?1, ?2, ?3, 'C:/demo', 'idle', ?4, ?5)",
            rusqlite::params![id, project, provider, log_dir, created],
        )?;
        Ok(())
    })
    .unwrap();
}

fn say(db: &Database, session: &str, seq: i64, ts: i64, role: &str, text: &str) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO session_events (session_id, seq, ts_ms, kind, payload, project_id, label, text)
             VALUES (?1, ?2, ?3, 'message', '{}', 'p1', ?4, ?5)",
            rusqlite::params![session, seq, ts, role, text],
        )?;
        conn.execute(
            "INSERT INTO session_events_fts (session_id, ts_ms, project_id, kind, label, text)
             VALUES (?1, ?2, 'p1', 'message', ?3, ?4)",
            rusqlite::params![session, ts, role, text],
        )?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn history_returns_closed_sessions_which_is_the_whole_point() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/logs/s1");
    db.with(|conn| {
        conn.execute("UPDATE sessions SET ended_at = 2000 WHERE id = 's1'", [])?;
        Ok(())
    })
    .unwrap();

    let page = page(&db, &[], &Query::default()).unwrap();
    assert_eq!(page.entries.len(), 1, "session_list would return none of these");
    assert_eq!(page.entries[0].ended_at, Some(2000));
    assert!(!page.entries[0].live);
}

#[test]
fn newest_first_and_a_cursor_walks_backwards_without_repeating_or_skipping() {
    let db = db();
    for i in 0..(PAGE * 2 + 5) {
        add_session(
            &db,
            &format!("s{i:03}"),
            "p1",
            "claude-code",
            1000 + i as i64,
            "C:/nothing",
        );
    }
    let total = PAGE * 2 + 5;

    let mut seen: Vec<String> = Vec::new();
    let mut query = Query::default();
    loop {
        let page = page(&db, &[], &query).unwrap();
        assert!(page.entries.len() <= PAGE);
        for entry in &page.entries {
            seen.push(entry.id.clone());
        }
        if !page.has_more {
            break;
        }
        let last = page.entries.last().unwrap();
        query.before_ts = Some(last.created_at);
        query.before_id = Some(last.id.clone());
    }

    assert_eq!(seen.len(), total, "every session was returned exactly once");
    let mut unique = seen.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), total, "no session was returned twice");

    // Newest first, throughout — not merely within a page.
    let mut expected: Vec<String> = (0..total).map(|i| format!("s{i:03}")).collect();
    expected.reverse();
    assert_eq!(seen, expected);
}

/// Two sessions started in the same millisecond is not a curiosity — it is what
/// happens when several are opened at once, and it is exactly where a cursor on
/// the timestamp alone loses a row at a page boundary.
#[test]
fn a_cursor_does_not_lose_a_session_started_in_the_same_millisecond() {
    let db = db();
    add_session(&db, "a", "p1", "claude-code", 5000, "C:/nothing");
    add_session(&db, "b", "p1", "claude-code", 5000, "C:/nothing");

    let first = page(
        &db,
        &[],
        &Query {
            before_ts: Some(5000),
            before_id: Some("b".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(
        first.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
        vec!["a"],
        "the tie-break on id is what keeps the second one reachable"
    );
}

#[test]
fn a_session_is_found_by_what_was_said_inside_it() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    say(&db, "s1", 1, 1100, "user", "please add a login form");
    say(&db, "s1", 2, 1200, "assistant", "the tree is clean now");

    let found = page(
        &db,
        &[],
        &Query {
            text: Some("clean".into()),
            ..Default::default()
        },
    )
    .unwrap();

    assert_eq!(found.entries.len(), 1);
    assert!(found.searched);
    assert!(
        found.entries[0]
            .snippet
            .as_deref()
            .unwrap_or_default()
            .contains("clean"),
        "the row has to show the line that matched, not just the session"
    );
}

#[test]
fn a_session_is_found_by_the_name_it_was_given() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    rename(&db, "s1", "Login form").unwrap();

    let found = page(
        &db,
        &[],
        &Query {
            text: Some("login".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(found.entries.len(), 1);
    assert_eq!(found.entries[0].title.as_deref(), Some("Login form"));
    assert_eq!(found.entries[0].title_source, Some(title::Source::User));
}

/// A query carrying FTS5 syntax has to be text, not an expression — the same
/// guarantee `search::fts_query` gives Global Search.
#[test]
fn a_query_full_of_operators_is_a_search_and_not_an_error() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    say(&db, "s1", 1, 1100, "user", "run the tests");

    for query in ["OR", "NEAR(", "col:value", "\"unclosed", "-x", "a AND b"] {
        let outcome = page(
            &db,
            &[],
            &Query {
                text: Some(query.into()),
                ..Default::default()
            },
        );
        assert!(outcome.is_ok(), "`{query}` must be searched, not parsed");
    }
}

#[test]
fn filters_narrow_by_project_and_provider() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    add_session(&db, "s2", "p2", "claude-code", 1100, "C:/nothing");
    add_session(&db, "s3", "p1", "codex", 1200, "C:/nothing");

    let by_project = page(
        &db,
        &[],
        &Query {
            project_id: Some("p1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_project.entries.len(), 2);

    let by_provider = page(
        &db,
        &[],
        &Query {
            provider: Some("codex".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(by_provider.entries.len(), 1);
    assert_eq!(by_provider.entries[0].id, "s3");

    let both = page(
        &db,
        &[],
        &Query {
            project_id: Some("p2".into()),
            provider: Some("codex".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(both.entries.is_empty());
}

#[test]
fn a_row_counts_the_turns_a_person_actually_took() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    say(&db, "s1", 1, 1100, "user", "one");
    say(&db, "s1", 2, 1200, "assistant", "a long reply");
    say(&db, "s1", 3, 1300, "user", "two");

    let entry = &page(&db, &[], &Query::default()).unwrap().entries[0];
    assert_eq!(entry.turns, 2, "two things were asked, not three");
    assert_eq!(entry.events, 3);
}

#[test]
fn tokens_are_absent_rather_than_zero_when_nothing_reported_any() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");

    let entry = &page(&db, &[], &Query::default()).unwrap().entries[0];
    assert_eq!(
        entry.tokens, None,
        "a session nobody measured must not be drawn as one that cost nothing"
    );

    db.with(|conn| {
        conn.execute(
            "INSERT INTO usage_samples
                 (session_id, project_id, provider, ts_ms, input_tokens, output_tokens, confidence)
             VALUES ('s1', 'p1', 'claude-code', 1100, 120, 30, 'official')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let entry = &page(&db, &[], &Query::default()).unwrap().entries[0];
    assert_eq!(entry.tokens, Some(150));
}

/// D39, and the reason it is written down: nothing cascades into a standalone
/// FTS5 table, so a delete that forgets it leaves Global Search answering for
/// a conversation that is gone.
#[test]
fn deleting_a_session_takes_its_search_index_rows_with_it() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    say(&db, "s1", 1, 1100, "user", "something memorable");

    let hits = |db: &Database| -> i64 {
        db.with(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM session_events_fts WHERE session_events_fts MATCH 'memorable'",
                [],
                |row| row.get(0),
            )
        })
        .unwrap()
    };
    assert_eq!(hits(&db), 1);

    delete(&db, &[], "s1").unwrap();

    assert_eq!(hits(&db), 0, "the index kept a row for a session that is gone");
    let left: i64 = db
        .with(|conn| conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0)))
        .unwrap();
    assert_eq!(left, 0);
}

#[test]
fn deleting_a_session_removes_its_log_directory_and_says_what_it_freed() {
    let dir = std::env::temp_dir().join(format!("jarvis-history-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(dir.join("attachments")).unwrap();
    std::fs::write(dir.join("stream.log"), vec![7u8; 4096]).unwrap();
    std::fs::write(dir.join("attachments").join("a.png"), vec![1u8; 512]).unwrap();

    let db = db();
    add_session(
        &db,
        "s1",
        "p1",
        "claude-code",
        1000,
        &dir.to_string_lossy(),
    );

    // The size is reported on the row before anything is deleted, too.
    let entry = &page(&db, &[], &Query::default()).unwrap().entries[0];
    assert_eq!(entry.bytes, 4096 + 512, "nested files count");

    let outcome = delete(&db, &[], "s1").unwrap();
    assert_eq!(outcome.bytes_freed, 4096 + 512);
    assert!(outcome.log_removed);
    assert!(!dir.exists(), "the log is the disk this was meant to free");
}

/// A running agent is writing to that log. Removing it is a crash, not a
/// delete, and the core refuses rather than the UI merely hiding the button.
#[test]
fn a_live_session_cannot_be_deleted() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");

    let refused = delete(&db, &["s1".to_string()], "s1");
    assert_eq!(refused.unwrap_err(), "history.deleteLive");

    let left: i64 = db
        .with(|conn| conn.query_row("SELECT COUNT(*) FROM sessions", [], |row| row.get(0)))
        .unwrap();
    assert_eq!(left, 1, "nothing was removed");
}

#[test]
fn a_missing_log_directory_is_zero_bytes_and_not_a_failure() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/definitely/not/here");

    let entry = &page(&db, &[], &Query::default()).unwrap().entries[0];
    assert_eq!(entry.bytes, 0);

    // HANDOFF item 37: a row can outlive its log. Deleting it must still work.
    let outcome = delete(&db, &[], "s1").unwrap();
    assert_eq!(outcome.bytes_freed, 0);
    assert!(outcome.log_removed);
}

#[test]
fn renaming_refuses_an_empty_name_rather_than_clearing_one() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    rename(&db, "s1", "Login form").unwrap();

    assert_eq!(rename(&db, "s1", "   ").unwrap_err(), "history.emptyTitle");
    assert_eq!(
page(&db, &[], &Query::default()).unwrap().entries[0].title.as_deref(),
        Some("Login form")
    );
}

#[test]
fn a_live_session_is_reported_as_live() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");

    let page = page(&db, &["s1".to_string()], &Query::default()).unwrap();
    assert!(page.entries[0].live);
}

#[test]
fn storage_adds_up_every_session_log() {
    let root = std::env::temp_dir().join(format!("jarvis-storage-{}", uuid::Uuid::now_v7()));
    let a = root.join("a");
    let b = root.join("b");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(a.join("stream.log"), vec![0u8; 1000]).unwrap();
    std::fs::write(b.join("stream.log"), vec![0u8; 2000]).unwrap();

    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1, &a.to_string_lossy());
    add_session(&db, "s2", "p1", "claude-code", 2, &b.to_string_lossy());

    let storage = storage(&db).unwrap();
    assert_eq!(storage.sessions, 2);
    assert_eq!(storage.bytes, 3000);

    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn only_providers_that_have_run_here_are_offered_as_filters() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    add_session(&db, "s2", "p1", "shell", 1100, "C:/nothing");
    add_session(&db, "s3", "p1", "claude-code", 1200, "C:/nothing");

    assert_eq!(
        providers_seen(&db).unwrap(),
        vec!["claude-code".to_string(), "shell".to_string()],
        "codex has never run here and must not be offered"
    );
}

/// A one-character query is a browse, not a search — the same bound Global
/// Search applies, and for the same reason.
#[test]
fn a_query_too_short_to_be_one_browses_instead() {
    let db = db();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");

    let page = page(
        &db,
        &[],
        &Query {
            text: Some("a".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!page.searched);
    assert_eq!(page.entries.len(), 1);
}

#[test]
fn the_mission_a_session_worked_on_travels_with_the_row() {
    let db = db();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO missions (id, project_id, title, created_at, updated_at)
             VALUES ('m1', 'p1', 'Ship the login form', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    add_session(&db, "s1", "p1", "claude-code", 1000, "C:/nothing");
    db.with(|conn| {
        conn.execute("UPDATE sessions SET mission_id = 'm1' WHERE id = 's1'", [])?;
        Ok(())
    })
    .unwrap();

    let entry = &page(&db, &[], &Query::default()).unwrap().entries[0];
    assert_eq!(entry.mission_id.as_deref(), Some("m1"));
    assert_eq!(entry.mission_title.as_deref(), Some("Ship the login form"));
}
