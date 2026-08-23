//! Brain tests, against a real database.

use super::*;

fn db() -> Database {
    let db = Database::open_in_memory().unwrap();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p1', 'demo', 'C:/demo', 1, 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p2', 'other', 'C:/other', 1, 1)",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    db
}

#[test]
fn knowledge_is_recorded_and_read_back() {
    let db = db();
    add_knowledge(
        &db,
        "p1",
        Kind::Gotcha,
        "The seed depends on ordering.",
        Source::Human,
        None,
        None,
    )
    .unwrap();

    let all = knowledge(&db, "p1").unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, Kind::Gotcha);
    assert_eq!(all[0].source, Source::Human);
    assert_eq!(all[0].body, "The seed depends on ordering.");
}

#[test]
fn one_projects_memory_is_not_anothers() {
    let db = db();
    add_knowledge(&db, "p1", Kind::What, "A desktop app.", Source::Human, None, None).unwrap();
    add_note(&db, "p1", "Ask about the logo.", None, None).unwrap();

    assert_eq!(knowledge(&db, "p2").unwrap().len(), 0);
    assert_eq!(notes(&db, "p2").unwrap().len(), 0);
}

/// Who said it changes how a reader should weigh it (§28), so it has to
/// survive storage.
#[test]
fn an_agents_claim_is_stored_as_an_agents_claim() {
    let db = db();
    add_knowledge(
        &db,
        "p1",
        Kind::Convention,
        "Migrations are never edited.",
        Source::Agent,
        None,
        None,
    )
    .unwrap();

    let all = knowledge(&db, "p1").unwrap();
    assert_eq!(all[0].source, Source::Agent);
}

#[test]
fn an_empty_body_is_refused_rather_than_stored() {
    let db = db();
    assert!(add_knowledge(&db, "p1", Kind::What, "   ", Source::Human, None, None).is_err());
    assert!(add_note(&db, "p1", "\n\t ", None, None).is_err());
    assert!(knowledge(&db, "p1").unwrap().is_empty());
    assert!(notes(&db, "p1").unwrap().is_empty());
}

#[test]
fn a_body_is_trimmed_before_it_is_stored() {
    let db = db();
    add_knowledge(&db, "p1", Kind::What, "  A CLI.  ", Source::Human, None, None).unwrap();
    assert_eq!(knowledge(&db, "p1").unwrap()[0].body, "A CLI.");
}

/// Retired knowledge leaves the surface but not the record.
#[test]
fn archived_knowledge_is_hidden_rather_than_destroyed() {
    let db = db();
    let id = add_knowledge(&db, "p1", Kind::What, "Was true once.", Source::Human, None, None)
        .unwrap();
    archive_knowledge(&db, &id).unwrap();

    assert!(knowledge(&db, "p1").unwrap().is_empty());
    let still_there: i64 = db
        .with(|conn| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM project_knowledge WHERE id = ?1",
                [&id],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(still_there, 1, "archived, not deleted");
}

/// A note is genuinely disposable — the asymmetry with knowledge is deliberate.
#[test]
fn a_deleted_note_is_gone() {
    let db = db();
    let id = add_note(&db, "p1", "Buy milk.", None, None).unwrap();
    delete_note(&db, &id).unwrap();

    assert!(notes(&db, "p1").unwrap().is_empty());
    let rows: i64 = db
        .with(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM project_notes", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 0);
}

#[test]
fn pinned_notes_come_first() {
    let db = db();
    add_note(&db, "p1", "first", None, None).unwrap();
    let second = add_note(&db, "p1", "second", None, None).unwrap();
    set_note_pinned(&db, &second, true).unwrap();

    let all = notes(&db, "p1").unwrap();
    assert_eq!(all[0].body, "second");
    assert!(all[0].pinned);
    assert!(!all[1].pinned);
}

/// The normal path for something learned mid-work.
#[test]
fn promoting_a_note_moves_it_into_knowledge_and_leaves_no_copy() {
    let db = db();
    let note = add_note(&db, "p1", "Clerk breaks on preview branches.", None, None).unwrap();
    promote_note(&db, &note, Kind::Gotcha).unwrap();

    let all = knowledge(&db, "p1").unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].kind, Kind::Gotcha);
    assert_eq!(all[0].body, "Clerk breaks on preview branches.");

    assert!(
        notes(&db, "p1").unwrap().is_empty(),
        "the same sentence must not exist in two places, or the two will drift"
    );
}

#[test]
fn promoting_a_note_that_is_not_there_says_so() {
    let db = db();
    assert!(promote_note(&db, "no-such-note", Kind::What).is_err());
}

/// Ordering is by creation, so re-wording an entry does not silently reshuffle
/// a document somebody is reading.
#[test]
fn editing_an_entry_does_not_move_it() {
    let db = db();
    let first = add_knowledge(&db, "p1", Kind::What, "One.", Source::Human, None, None).unwrap();
    add_knowledge(&db, "p1", Kind::What, "Two.", Source::Human, None, None).unwrap();

    update_knowledge(&db, &first, Kind::What, "One, reworded.").unwrap();

    let all = knowledge(&db, "p1").unwrap();
    assert_eq!(all[0].body, "One, reworded.");
    assert_eq!(all[1].body, "Two.");
    assert!(all[0].updated_at >= all[0].created_at);
}

#[test]
fn an_entry_can_be_moved_to_another_kind() {
    let db = db();
    let id = add_knowledge(&db, "p1", Kind::What, "Actually a gotcha.", Source::Human, None, None)
        .unwrap();
    update_knowledge(&db, &id, Kind::Gotcha, "Actually a gotcha.").unwrap();
    assert_eq!(knowledge(&db, "p1").unwrap()[0].kind, Kind::Gotcha);
}

/// One identity, one spelling — the rule D13 was written for.
#[test]
fn a_kind_serialises_as_the_id_used_everywhere_else() {
    for kind in KINDS {
        let json = serde_json::to_string(kind).unwrap();
        assert_eq!(json, format!("\"{}\"", kind.as_str()));
        assert_eq!(serde_json::from_str::<Kind>(&json).unwrap(), *kind);
    }
    assert!(serde_json::from_str::<Kind>("\"Gotcha\"").is_err());
}

#[test]
fn a_source_serialises_as_its_id() {
    for source in [Source::Human, Source::Agent] {
        let json = serde_json::to_string(&source).unwrap();
        assert_eq!(json, format!("\"{}\"", source.as_str()));
    }
}

/// The wire shape, checked as bytes rather than as a Rust type (D13).
#[test]
fn knowledge_serialises_with_the_field_names_the_webview_reads() {
    let entry = Knowledge {
        id: "k1".into(),
        project_id: "p1".into(),
        kind: Kind::Gotcha,
        body: "Careful.".into(),
        source: Source::Agent,
        session_id: Some("s1".into()),
        mission_id: None,
        created_at: 10,
        updated_at: 20,
    };
    let json = serde_json::to_string(&entry).unwrap();
    for expected in [
        r#""projectId":"p1""#,
        r#""sessionId":"s1""#,
        r#""missionId":null"#,
        r#""createdAt":10"#,
        r#""updatedAt":20"#,
        r#""kind":"gotcha""#,
        r#""source":"agent""#,
    ] {
        assert!(json.contains(expected), "missing {expected} in {json}");
    }
}

/// Deleting a project takes its memory with it, rather than leaving orphans
/// that a later project with a reused id could inherit.
#[test]
fn a_projects_memory_goes_when_the_project_does() {
    let db = db();
    add_knowledge(&db, "p1", Kind::What, "Something.", Source::Human, None, None).unwrap();
    add_note(&db, "p1", "A note.", None, None).unwrap();

    db.with(|conn| {
        conn.execute("DELETE FROM projects WHERE id = 'p1'", [])?;
        Ok(())
    })
    .unwrap();

    let (k, n): (i64, i64) = db
        .with(|conn| {
            Ok((
                conn.query_row("SELECT COUNT(*) FROM project_knowledge", [], |r| r.get(0))?,
                conn.query_row("SELECT COUNT(*) FROM project_notes", [], |r| r.get(0))?,
            ))
        })
        .unwrap();
    assert_eq!((k, n), (0, 0));
}

// ---- Derived ----------------------------------------------------------------

#[test]
fn a_project_with_no_history_derives_nothing_rather_than_zeroes() {
    let db = db();
    // A wall of "0 sessions, 0 missions, 0 refusals" is not knowledge, it is a
    // form with nothing filled in.
    assert!(derived::facts(&db, "p1").unwrap().is_empty());
    assert!(derived::timeline(&db, "p1").unwrap().is_empty());
}

#[test]
fn completed_and_revoked_are_both_reported() {
    let db = db();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO missions (id, project_id, title, status, created_at, updated_at)
             VALUES ('m1', 'p1', 'One', 'completed', 1, 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO activity (ts_ms, project_id, kind, severity, title)
             VALUES (5, 'p1', 'mission.completionRevoked', 'attention', 'One')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let codes: Vec<String> = derived::facts(&db, "p1")
        .unwrap()
        .into_iter()
        .map(|f| f.code)
        .collect();
    assert!(codes.contains(&"brain.fact.completed".to_string()));
    assert!(
        codes.contains(&"brain.fact.revoked".to_string()),
        "completion being taken back is the most informative thing recorded here"
    );
}

/// One edit is not a pattern.
#[test]
fn a_file_touched_once_is_not_reported_as_a_hot_spot() {
    let db = db();
    let now = crate::session::log::now_ms();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at)
             VALUES ('s1', 'p1', 'claude-code', 'C:/demo', 'ended', 'C:/logs', 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO file_changes (session_id, project_id, path, ts_ms)
             VALUES ('s1', 'p1', 'once.ts', ?1)",
            [now],
        )?;
        for _ in 0..3 {
            conn.execute(
                "INSERT INTO file_changes (session_id, project_id, path, ts_ms)
                 VALUES ('s1', 'p1', 'often.ts', ?1)",
                [now],
            )?;
        }
        Ok(())
    })
    .unwrap();

    let facts = derived::facts(&db, "p1").unwrap();
    let hot: Vec<&Vec<String>> = facts
        .iter()
        .filter(|f| f.code == "brain.fact.hotFile")
        .map(|f| &f.args)
        .collect();
    assert_eq!(hot.len(), 1, "only the repeated file is a hot spot");
    assert_eq!(hot[0][0], "often.ts");
    assert_eq!(hot[0][1], "3");
}

#[test]
fn the_timeline_is_this_projects_history_newest_first() {
    let db = db();
    db.with(|conn| {
        conn.execute(
            "INSERT INTO activity (ts_ms, project_id, kind, severity, title)
             VALUES (10, 'p1', 'session.started', 'info', 'older')",
            [],
        )?;
        conn.execute(
            "INSERT INTO activity (ts_ms, project_id, kind, severity, title)
             VALUES (20, 'p1', 'mission.completed', 'info', 'newer')",
            [],
        )?;
        conn.execute(
            "INSERT INTO activity (ts_ms, project_id, kind, severity, title)
             VALUES (30, 'p2', 'session.started', 'info', 'elsewhere')",
            [],
        )?;
        Ok(())
    })
    .unwrap();

    let timeline = derived::timeline(&db, "p1").unwrap();
    assert_eq!(timeline.len(), 2, "another project's history is not this one's");
    assert_eq!(timeline[0].title, "newer");
    assert_eq!(timeline[1].title, "older");
}

// ---- The brief --------------------------------------------------------------

/// The whole reason the two tables are separate.
#[test]
fn a_note_never_reaches_the_brief() {
    let db = db();
    add_knowledge(&db, "p1", Kind::What, "A desktop app.", Source::Human, None, None).unwrap();
    add_note(&db, "p1", "REMEMBER TO BUY MILK", None, None).unwrap();

    let brief = brief::compose(&knowledge(&db, "p1").unwrap()).unwrap();
    assert!(brief.contains("A desktop app."));
    assert!(
        !brief.contains("MILK"),
        "somebody's todo list must never be handed to an agent as context"
    );
}

#[test]
fn a_project_with_no_knowledge_has_no_brief() {
    let db = db();
    add_note(&db, "p1", "Only a note.", None, None).unwrap();
    assert_eq!(brief::compose(&knowledge(&db, "p1").unwrap()), None);
}
