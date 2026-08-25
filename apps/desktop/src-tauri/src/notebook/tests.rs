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
