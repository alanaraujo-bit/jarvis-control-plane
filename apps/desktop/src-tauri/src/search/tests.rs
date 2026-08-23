use super::*;
use crate::activity;
use rusqlite::params;

fn db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p1', 'Aionix', 'C:\\aionix', 0, 0)",
            [],
        )?;
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p2', 'Casco', 'C:\\casco', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    db
}

fn insert_session(db: &Database, id: &str, project_id: &str, provider: &str) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at)
             VALUES (?1, ?2, ?3, 'C:\\demo', 'idle', 'C:\\logs', 0)",
            params![id, project_id, provider],
        )?;
        Ok(())
    })
    .unwrap();
}

/// Mirrors what `session::transcript::mirror` writes for one conversation
/// item, without pulling in the whole tailer machinery to test the query side.
fn insert_event(db: &Database, session_id: &str, project_id: &str, ts_ms: i64, kind: &str, label: Option<&str>, text: &str) {
    db.with(|conn| {
        let seq: i64 = conn.query_row(
            "SELECT COUNT(*) FROM session_events WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO session_events (session_id, seq, ts_ms, project_id, kind, label, text, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, '{}')",
            params![session_id, seq, ts_ms, project_id, kind, label, text],
        )?;
        conn.execute(
            "INSERT INTO session_events_fts (session_id, ts_ms, project_id, kind, label, text)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![session_id, ts_ms, project_id, kind, label, text],
        )?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn a_query_shorter_than_two_characters_returns_nothing() {
    let db = db();
    add_knowledge_for_test(&db, "p1", "a very specific gotcha about ports");
    assert!(search(&db, "a").unwrap().is_empty());
    assert!(search(&db, "  ").unwrap().is_empty());
}

#[test]
fn finds_knowledge_case_insensitively() {
    let db = db();
    add_knowledge_for_test(&db, "p1", "Port 5173 is reserved for the Vite dev server");

    let results = search(&db, "vite").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].kind, Kind::Knowledge);
    assert_eq!(results[0].project_name.as_deref(), Some("Aionix"));
    assert!(results[0].snippet.contains("Vite"));
}

#[test]
fn an_archived_knowledge_entry_is_excluded() {
    let db = db();
    let id = add_knowledge_for_test(&db, "p1", "a note about staging deploys");
    db.with(|conn| {
        conn.execute("UPDATE project_knowledge SET archived = 1 WHERE id = ?1", [&id])?;
        Ok(())
    })
    .unwrap();

    assert!(search(&db, "staging").unwrap().is_empty());
}

#[test]
fn finds_a_note() {
    let db = db();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO project_notes (id, project_id, body, created_at, updated_at)
             VALUES ('n1', 'p1', 'remember to rotate the webhook secret', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let results = search(&db, "webhook").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].kind, Kind::Note);
}

#[test]
fn finds_a_mission_and_prefers_the_matching_field_for_the_snippet() {
    let db = db();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO missions (id, project_id, title, goal, description, created_at, updated_at)
             VALUES ('m1', 'p1', 'Ship the exporter', 'Reduce churn',
                     'The exporter must stream CSV without buffering the whole table', 0, 0)",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let results = search(&db, "csv").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].heading, "Ship the exporter");
    assert!(results[0].snippet.to_lowercase().contains("csv"));
    assert_eq!(results[0].mission_id.as_deref(), Some("m1"));
}

#[test]
fn finds_an_activity_entry_across_projects() {
    let db = db();
    activity::record(
        &db,
        "mission.blocked",
        activity::Severity::Attention,
        "Ship the exporter",
        Some("waiting on production credentials".into()),
        Some("p2"),
        None,
        None,
    );

    let results = search(&db, "credentials").unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].kind, Kind::Activity);
    assert_eq!(results[0].project_name.as_deref(), Some("Casco"));
}

#[test]
fn finds_conversation_content_via_fts() {
    let db = db();
    insert_session(&db, "s1", "p1", "claude-code");
    insert_event(&db, "s1", "p1", 100, "message", Some("assistant"), "I ran git status and the tree is clean");
    insert_event(&db, "s1", "p1", 200, "toolCall", Some("Bash"), "Bash: git status");

    let results = search(&db, "status").unwrap();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.kind == Kind::Conversation));
    assert!(results.iter().all(|r| r.session_provider.as_deref() == Some("claude-code")));
}

#[test]
fn a_double_quote_in_the_query_does_not_break_conversation_search() {
    let db = db();
    insert_session(&db, "s1", "p1", "claude-code");
    insert_event(&db, "s1", "p1", 100, "message", Some("assistant"), "the field is called \"status\"");

    // A bare `"` is FTS5 phrase-query syntax; the query builder must quote it
    // as literal text rather than let it open an unterminated phrase.
    let results = search(&db, "\"status\"").unwrap();
    assert!(!results.is_empty(), "a quote character must not silently break the search");
}

#[test]
fn a_percent_or_underscore_in_the_query_is_matched_literally() {
    let db = db();
    add_knowledge_for_test(&db, "p1", "throughput dropped by 50% after the change");

    // `%` is a LIKE wildcard; unescaped, "50%" would match anything at all.
    let results = search(&db, "50%").unwrap();
    assert_eq!(results.len(), 1);

    let none = search(&db, "50% after the wrong text").unwrap();
    assert!(none.is_empty());
}

#[test]
fn results_are_sorted_newest_first_across_sources() {
    let db = db();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO project_knowledge (id, project_id, kind, body, source, created_at, updated_at)
             VALUES ('k1', 'p1', 'what', 'apples are old', 'human', 0, 1000)",
            [],
        )?;
        conn.execute(
            "INSERT INTO project_notes (id, project_id, body, created_at, updated_at)
             VALUES ('n1', 'p1', 'apples are new', 0, 5000)",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let results = search(&db, "apples").unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].kind, Kind::Note, "the more recently updated match must come first");
}

/// A test-only shortcut so these tests do not have to spell out every column
/// of `project_knowledge` to check the search query against it.
fn add_knowledge_for_test(db: &Database, project_id: &str, body: &str) -> String {
    let id = uuid::Uuid::now_v7().to_string();
    let stored = id.clone();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO project_knowledge (id, project_id, kind, body, source, created_at, updated_at)
             VALUES (?1, ?2, 'what', ?3, 'human', 0, 0)",
            params![stored, project_id, body],
        )?;
        Ok(())
    })
    .unwrap();
    id
}
