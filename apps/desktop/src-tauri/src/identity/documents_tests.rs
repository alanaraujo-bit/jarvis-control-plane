use super::*;
use crate::db::Database;

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

fn remote(kind: &str, id: &str, body: Value, touched_at: i64) -> Document {
    Document { kind: kind.into(), id: id.into(), body, touched_at, deleted_at: None }
}

fn find<'a>(docs: &'a [Document], kind: &str, id: &str) -> Option<&'a Document> {
    docs.iter().find(|d| d.kind == kind && d.id == id)
}

fn project_count(db: &Database) -> i64 {
    db.with(|c| c.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))).unwrap()
}

#[test]
fn outgoing_marks_what_changed_and_what_disappeared() {
    let db = db();
    crate::settings::set(&db, "autonomy.global", &"supervised").unwrap();
    let first = outgoing(&db, 100).unwrap();
    assert_eq!(find(&first, "setting", "autonomy.global").unwrap().touched_at, 100);

    let again = outgoing(&db, 200).unwrap();
    assert_eq!(find(&again, "setting", "autonomy.global").unwrap().touched_at, 100, "unchanged keeps its clock");

    crate::settings::set(&db, "autonomy.global", &"unattended").unwrap();
    let changed = outgoing(&db, 300).unwrap();
    assert_eq!(find(&changed, "setting", "autonomy.global").unwrap().touched_at, 300);

    crate::settings::clear(&db, "autonomy.global").unwrap();
    let gone = outgoing(&db, 400).unwrap();
    let tomb = find(&gone, "setting", "autonomy.global").unwrap();
    assert_eq!((tomb.touched_at, tomb.deleted_at), (400, Some(400)));
}

#[test]
fn only_a_newer_remote_document_is_applied() {
    let db = db();
    let dir = tempfile::tempdir().unwrap();
    crate::settings::set(&db, "startup.minimized", &true).unwrap();
    outgoing(&db, 100).unwrap();

    apply(&db, dir.path(), &[remote("setting", "startup.minimized", json!("false"), 50)]).unwrap();
    assert_eq!(crate::settings::get::<bool>(&db, "startup.minimized"), Some(true), "older loses");

    apply(&db, dir.path(), &[remote("setting", "startup.minimized", json!("false"), 150)]).unwrap();
    assert_eq!(crate::settings::get::<bool>(&db, "startup.minimized"), Some(false), "newer wins");
    let after = outgoing(&db, 200).unwrap();
    assert_eq!(find(&after, "setting", "startup.minimized").unwrap().touched_at, 150, "applying is not a local change");
}

#[test]
fn a_project_comes_back_only_where_its_folder_exists() {
    let db = db();
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let missing = folder.path().join("not-here");
    let project = |id: &str, path: &std::path::Path| {
        remote("project", id, json!({"name": id, "path": path.to_string_lossy(), "autonomy": null, "archived": false}), 10)
    };

    apply(&db, data.path(), &[project("p-missing", &missing), project("p-here", folder.path())]).unwrap();
    assert_eq!(project_count(&db), 1);
    let out = outgoing(&db, 20).unwrap();
    assert!(find(&out, "project", "p-missing").map_or(true, |d| d.deleted_at.is_none()), "a held document is never tombstoned");

    std::fs::create_dir_all(&missing).unwrap();
    apply(&db, data.path(), &[]).unwrap();
    assert_eq!(project_count(&db), 2, "held documents are retried on the next pull");
}

#[test]
fn notes_and_knowledge_land_on_the_project_with_that_path() {
    let db = db();
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().to_string_lossy().to_string();
    apply(
        &db,
        data.path(),
        &[
            remote("projectNote", "n1", json!({"projectPath": path, "body": "remember", "pinned": true, "createdAt": 1, "updatedAt": 2}), 10),
            remote("projectKnowledge", "k1", json!({"projectPath": path, "kind": "gotcha", "body": "slow tests", "source": "human", "archived": false, "createdAt": 1, "updatedAt": 2}), 10),
            remote("project", "p1", json!({"name": "repo", "path": path, "autonomy": null, "archived": false}), 10),
        ],
    )
    .unwrap();
    let note: (String, String, i64) = db
        .with(|c| c.query_row("SELECT project_id, body, pinned FROM project_notes WHERE id = 'n1'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))
        .unwrap();
    assert_eq!(note, ("p1".to_string(), "remember".to_string(), 1));
    let knowledge: String = db
        .with(|c| c.query_row("SELECT body FROM project_knowledge WHERE id = 'k1'", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(knowledge, "slow tests");
}

#[test]
fn a_provider_account_comes_back_signed_out_with_an_empty_directory() {
    let db = db();
    let data = tempfile::tempdir().unwrap();
    apply(
        &db,
        data.path(),
        &[remote("providerAccount", "a1", json!({"provider": "claude-code", "label": "Claude 2", "email": "a@b.c", "position": 1, "paused": false}), 10)],
    )
    .unwrap();
    let account = crate::accounts::get(&db, "a1").unwrap().expect("restored with the same id");
    assert!(!account.signed_in && !account.adopted);
    assert_eq!(account.credential_origin, "own");
    assert_eq!(account.label, "Claude 2");
    let dir = std::path::Path::new(&account.config_dir);
    assert!(dir.starts_with(data.path()) && dir.is_dir());
    assert_eq!(std::fs::read_dir(dir).unwrap().count(), 0, "no credential arrives with it");
}

#[test]
fn an_account_tombstone_never_deletes_a_local_account() {
    let db = db();
    let data = tempfile::tempdir().unwrap();
    let local = crate::accounts::create(&db, "claude-code", data.path(), "mine").unwrap();
    let mut tomb = remote("providerAccount", &local.id, json!(null), 99);
    tomb.deleted_at = Some(99);
    apply(&db, data.path(), &[tomb]).unwrap();
    assert!(crate::accounts::get(&db, &local.id).unwrap().is_some());
}

#[test]
fn a_setting_stored_as_raw_text_travels_verbatim() {
    // `autonomy.global` is written as bare text, not JSON (mission::store).
    let db = db();
    let data = tempfile::tempdir().unwrap();
    db.with(|c| c.execute("INSERT INTO settings (key, value) VALUES ('autonomy.global', 'unattended')", [])).unwrap();
    let out = outgoing(&db, 10).unwrap();
    assert_eq!(find(&out, "setting", "autonomy.global").unwrap().body, json!("unattended"));

    let other = self::db();
    apply(&other, data.path(), &[remote("setting", "autonomy.global", json!("unattended"), 10)]).unwrap();
    let raw: String = other.with(|c| c.query_row("SELECT value FROM settings WHERE key = 'autonomy.global'", [], |r| r.get(0))).unwrap();
    assert_eq!(raw, "unattended", "written back exactly as it was stored");
}

#[test]
fn a_machine_adopted_account_is_not_carried() {
    // Every installation adopts its own machine accounts; carrying them would
    // add a duplicate on each restore.
    let db = db();
    db.with(|c| c.execute(
        "INSERT INTO provider_accounts (id, provider, label, config_dir, adopted, position, created_at) VALUES ('m', 'codex', '', 'C:/m', 1, 0, 0)",
        [],
    ))
    .unwrap();
    let out = outgoing(&db, 10).unwrap();
    assert!(find(&out, "providerAccount", "m").is_none());
}

#[test]
fn a_copied_login_is_not_carried() {
    let db = db();
    let data = tempfile::tempdir().unwrap();
    let local = crate::accounts::create(&db, "claude-code", data.path(), "machine").unwrap();
    crate::accounts::set_credential_origin(&db, &local.id, "copied").unwrap();
    let out = outgoing(&db, 10).unwrap();
    assert!(find(&out, "providerAccount", &local.id).is_none());
}
