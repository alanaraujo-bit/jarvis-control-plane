use super::*;

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

#[test]
fn a_new_note_is_empty_and_unfiled_until_it_is_filed() {
    let db = db();
    let id = create_note(&db, None).unwrap();
    let report = report(&db).unwrap();

    assert_eq!(report.notes.len(), 1);
    let note = &report.notes[0];
    assert_eq!(note.id, id);
    assert_eq!(note.notebook_id, None, "unfiled, not orphaned");
    assert_eq!(note.title, "");
    assert_eq!(note.body, "");
    assert!(!note.pinned);
}

#[test]
fn deleting_a_folder_keeps_its_notes_and_unfiles_them() {
    // The whole reason `notebook_id` is nullable. Somebody who has kept forty
    // prompts for a year must not lose them to one mis-click, and the schema
    // is a stronger guarantee of that than a confirmation dialog.
    let db = db();
    let book = create_notebook(&db, "Prompts").unwrap();
    let note = create_note(&db, Some(&book)).unwrap();
    update_note(&db, &note, "Refactor", "Reescreva mantendo o comportamento.").unwrap();

    delete_notebook(&db, &book).unwrap();

    let report = report(&db).unwrap();
    assert!(report.notebooks.is_empty());
    assert_eq!(report.notes.len(), 1, "the note outlived its folder");
    assert_eq!(report.notes[0].notebook_id, None);
    assert_eq!(report.notes[0].body, "Reescreva mantendo o comportamento.");
}

#[test]
fn pinning_and_filing_do_not_count_as_edits() {
    // Both would otherwise bump `updated_at` and reorder the list under the
    // person who just pinned something — the list is sorted by edit time.
    let db = db();
    let book = create_notebook(&db, "Ideias").unwrap();
    let note = create_note(&db, None).unwrap();
    update_note(&db, &note, "t", "b").unwrap();

    let before = report(&db).unwrap().notes[0].updated_at;
    set_note_pinned(&db, &note, true).unwrap();
    move_note(&db, &note, Some(&book)).unwrap();
    let after = report(&db).unwrap().notes[0].clone();

    assert_eq!(after.updated_at, before);
    assert!(after.pinned);
    assert_eq!(after.notebook_id.as_deref(), Some(book.as_str()));
}

#[test]
fn the_list_is_pinned_first_then_most_recently_edited() {
    // This test was flaky before `report` gained its `id` tiebreak: both notes
    // land in the same millisecond, `updated_at` and `created_at` tie, and the
    // order was whatever SQLite chose. That was a real defect rather than a
    // test artefact — a list of unchanged notes must not reshuffle between two
    // reads of it.
    let db = db();
    let older = create_note(&db, None).unwrap();
    update_note(&db, &older, "older", "a").unwrap();
    let newer = create_note(&db, None).unwrap();
    update_note(&db, &newer, "newer", "b").unwrap();

    // Nothing pinned: newest edit leads.
    let ids: Vec<_> = report(&db)
        .unwrap()
        .notes
        .iter()
        .map(|n| n.id.clone())
        .collect();
    assert_eq!(ids.first().unwrap(), &newer);

    // Pinning the older one lifts it above a more recent edit.
    set_note_pinned(&db, &older, true).unwrap();
    let ids: Vec<_> = report(&db)
        .unwrap()
        .notes
        .iter()
        .map(|n| n.id.clone())
        .collect();
    assert_eq!(ids.first().unwrap(), &older);
}

#[test]
fn a_duplicate_carries_the_words_and_the_folder_but_never_the_pin() {
    // A copy made to be edited must not look like the original somebody had
    // already decided to trust.
    let db = db();
    let book = create_notebook(&db, "Prompts").unwrap();
    let note = create_note(&db, Some(&book)).unwrap();
    update_note(&db, &note, "Revisão", "Revise este diff.").unwrap();
    set_note_pinned(&db, &note, true).unwrap();

    let copy_id = duplicate_note(&db, &note).unwrap();
    let report = report(&db).unwrap();
    let copy = report.notes.iter().find(|n| n.id == copy_id).unwrap();

    assert_eq!(copy.title, "Revisão");
    assert_eq!(copy.body, "Revise este diff.");
    assert_eq!(copy.notebook_id.as_deref(), Some(book.as_str()));
    assert!(!copy.pinned);
}

#[test]
fn a_folder_needs_a_name_but_a_note_never_does() {
    // The gesture is "I have an idea". A required field between that and a
    // cursor is how a scratchpad stops being used.
    let db = db();
    assert!(create_notebook(&db, "   ").is_err());
    assert!(create_note(&db, None).is_ok());
}

#[test]
fn folders_are_appended_rather_than_pushing_the_list_down() {
    let db = db();
    let first = create_notebook(&db, "Um").unwrap();
    let second = create_notebook(&db, "Dois").unwrap();
    let ids: Vec<_> = report(&db)
        .unwrap()
        .notebooks
        .iter()
        .map(|b| b.id.clone())
        .collect();
    assert_eq!(
        ids,
        vec![first, second],
        "a new folder must not push the one being looked at down the list"
    );
}

// ---------------------------------------------------------------------------
// Carrying the library between machines (M23)
// ---------------------------------------------------------------------------
//
// Three machines is the honest way to test a sync and not a thing anybody has.
// These stand in for it: one database is "here", a `SyncPayload` built by hand
// is "there", and `merge` is the only thing that decides who wins.

use super::sync::{self, SyncNote, SyncNotebook, SyncPayload};

const THEM: &str = "account-them";

fn touched(db: &Database, id: &str) -> i64 {
    db.with(|conn| {
        conn.query_row(
            "SELECT touched_at FROM notebook_notes WHERE id = ?1",
            [id],
            |row| row.get(0),
        )
    })
    .unwrap()
}

fn note_at(id: &str, body: &str, touched_at: i64) -> SyncNote {
    SyncNote {
        id: id.into(),
        body: body.into(),
        created_at: touched_at,
        updated_at: touched_at,
        touched_at,
        ..Default::default()
    }
}

#[test]
fn pinning_moves_the_sync_clock_and_leaves_the_list_order_alone() {
    // The reason `touched_at` exists at all. Pinning deliberately does not
    // touch `updated_at`, so a sync keyed on `updated_at` would never carry a
    // pin — and the next pull would quietly unpin it again.
    let db = db();
    let id = create_note(&db, None).unwrap();
    let before = report(&db).unwrap().notes[0].clone();

    set_note_pinned(&db, &id, true).unwrap();
    let after = report(&db).unwrap().notes[0].clone();

    assert_eq!(after.updated_at, before.updated_at, "the list must not reorder");
    assert!(after.pinned);

    let carried = sync::payload(&db).unwrap();
    let sent = carried.notes.iter().find(|note| note.id == id).unwrap();
    assert!(sent.pinned, "the pin is in the payload");
    assert!(sent.touched_at >= before.updated_at, "and the clock moved");
}

#[test]
fn the_newer_side_wins_in_both_directions() {
    let db = db();
    let id = create_note(&db, None).unwrap();
    update_note(&db, &id, "Here", "written here").unwrap();
    let here = touched(&db, &id);

    // Older remote edit: ignored.
    let stale = SyncPayload {
        notes: vec![note_at(&id, "written there, earlier", here - 1_000)],
        ..Default::default()
    };
    sync::merge(&db, THEM, &stale).unwrap();
    assert_eq!(report(&db).unwrap().notes[0].body, "written here");

    // Newer remote edit: taken.
    let fresh = SyncPayload {
        notes: vec![note_at(&id, "written there, later", here + 1_000)],
        ..Default::default()
    };
    sync::merge(&db, THEM, &fresh).unwrap();
    assert_eq!(report(&db).unwrap().notes[0].body, "written there, later");
}

#[test]
fn a_deletion_here_is_not_undone_by_a_pull() {
    // Without a tombstone the server's copy looks exactly like a note this
    // machine has never seen, so every pull restores what somebody deleted and
    // deleting it again is a loop with no exit.
    let db = db();
    let id = create_note(&db, None).unwrap();
    update_note(&db, &id, "Gone", "delete me").unwrap();
    let alive = touched(&db, &id);
    delete_note(&db, &id).unwrap();

    let remote = SyncPayload {
        notes: vec![note_at(&id, "delete me", alive)],
        ..Default::default()
    };
    sync::merge(&db, THEM, &remote).unwrap();

    assert!(report(&db).unwrap().notes.is_empty(), "it stays deleted");
    let carried = sync::payload(&db).unwrap();
    let grave = carried.notes.iter().find(|note| note.id == id).unwrap();
    assert!(grave.deleted_at.is_some(), "and the tombstone travels");
}

#[test]
fn an_edit_made_after_a_deletion_wins_over_it() {
    // The other half of the same rule. A tombstone is not permanent authority:
    // somebody who deleted a note here at noon and rewrote it there at one
    // o'clock meant the rewrite.
    let db = db();
    let id = create_note(&db, None).unwrap();
    delete_note(&db, &id).unwrap();
    let buried = sync::payload(&db)
        .unwrap()
        .notes
        .iter()
        .find(|note| note.id == id)
        .unwrap()
        .touched_at;

    let remote = SyncPayload {
        notes: vec![note_at(&id, "rewritten later", buried + 1_000)],
        ..Default::default()
    };
    sync::merge(&db, THEM, &remote).unwrap();

    let notes = report(&db).unwrap().notes;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body, "rewritten later");
}

#[test]
fn a_note_whose_folder_never_arrived_lands_unfiled() {
    // Rather than failing the whole merge on a foreign key. Unfiled is a real
    // place somebody can look in; a note lost to a rolled-back transaction is
    // not.
    let db = db();
    let remote = SyncPayload {
        notes: vec![SyncNote {
            id: "note-1".into(),
            notebook_id: Some("a-folder-that-never-came".into()),
            body: "still mine".into(),
            touched_at: 10,
            ..Default::default()
        }],
        ..Default::default()
    };
    sync::merge(&db, THEM, &remote).unwrap();

    let notes = report(&db).unwrap().notes;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].notebook_id, None);
    assert_eq!(notes[0].body, "still mine");
}

#[test]
fn a_second_account_does_not_inherit_the_first_ones_library() {
    // `notebooks` is machine-scoped, like `settings`. Merging one person's
    // prompt library into another's is the one outcome here that cannot be
    // undone, so a pull for a different account replaces rather than merges.
    let db = db();
    let mine = create_note(&db, None).unwrap();
    update_note(&db, &mine, "Mine", "my prompt").unwrap();
    sync::merge(&db, "account-me", &SyncPayload::default()).unwrap();

    let theirs = SyncPayload {
        notes: vec![note_at("note-theirs", "their prompt", 10)],
        ..Default::default()
    };
    sync::merge(&db, THEM, &theirs).unwrap();

    let notes = report(&db).unwrap().notes;
    assert_eq!(notes.len(), 1, "one library, not two stirred together");
    assert_eq!(notes[0].body, "their prompt");
}

#[test]
fn a_library_that_has_never_synced_is_adopted_by_the_first_account() {
    // The mirror of `prefs::adopt_machine_settings`: somebody who used the
    // notebook before signing up keeps what they wrote.
    let db = db();
    let mine = create_note(&db, None).unwrap();
    update_note(&db, &mine, "Mine", "written before signing up").unwrap();

    sync::merge(&db, "account-me", &SyncPayload::default()).unwrap();

    let notes = report(&db).unwrap().notes;
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body, "written before signing up");
}

#[test]
fn deleting_a_folder_tells_the_other_machine_its_notes_were_unfiled() {
    // `ON DELETE SET NULL` edits the notes without touching them, which is
    // invisible to a sync that only looks at what changed.
    let db = db();
    let book = create_notebook(&db, "Prompts").unwrap();
    let note = create_note(&db, Some(&book)).unwrap();
    let before = touched(&db, &note);
    // `now_ms` has millisecond resolution and the two writes above land inside
    // one, so without this the clock is *equal* rather than later and the
    // assertion measures the machine's speed instead of the behaviour.
    std::thread::sleep(std::time::Duration::from_millis(2));

    delete_notebook(&db, &book).unwrap();

    assert!(touched(&db, &note) > before, "the unfiling is carried");
    let carried = sync::payload(&db).unwrap();
    assert!(carried
        .notebooks
        .iter()
        .any(|entry| entry.id == book && entry.deleted_at.is_some()));
    let unfiled = carried.notes.iter().find(|entry| entry.id == note).unwrap();
    assert_eq!(unfiled.notebook_id, None, "and so is where it ended up");
}
