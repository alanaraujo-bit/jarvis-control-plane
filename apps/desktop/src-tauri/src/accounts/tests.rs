use super::*;

fn account(id: &str, provider: &str, position: i64, active: bool) -> Account {
    Account {
        id: id.into(),
        provider: provider.into(),
        label: id.into(),
        config_dir: format!("C:/accounts/{id}"),
        adopted: false,
        email: Some(format!("{id}@example.test")),
        org_id: None,
        org_name: None,
        plan: Some("pro".into()),
        signed_in: true,
        checked_at: Some(1),
        active,
        paused: false,
        position,
        created_at: 1,
        last_used_at: None,
    }
}

fn insert(db: &Database, value: &Account) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO provider_accounts
                 (id, provider, label, config_dir, adopted, email, org_id, org_name, plan,
                  signed_in, checked_at, active, paused, position, created_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            rusqlite::params![
                value.id,
                value.provider,
                value.label,
                value.config_dir,
                value.adopted as i64,
                value.email,
                value.org_id,
                value.org_name,
                value.plan,
                value.signed_in as i64,
                value.checked_at,
                value.active as i64,
                value.paused as i64,
                value.position,
                value.created_at,
                value.last_used_at,
            ],
        )?;
        Ok(())
    })
    .unwrap();
}

fn insert_session(db: &Database, account_id: &str) {
    db.with(|conn| {
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p1', 'demo', 'C:/demo', 1, 1)",
            [],
        )?;
        conn.execute(
            "INSERT INTO sessions
                 (id, project_id, provider, cwd, state, log_dir, created_at, updated_at, account_id)
             VALUES ('s1', 'p1', 'claude-code', 'C:/demo', 'running', 'C:/logs/s1', 1, 1, ?1)",
            [account_id],
        )?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn an_adopted_account_keeps_provider_defaults_while_an_added_one_gets_an_override() {
    let mut adopted = account("machine", "claude-code", 0, true);
    adopted.adopted = true;
    assert!(session_env(&adopted).is_empty());

    let added = account("second", "claude-code", 1, false);
    assert_eq!(
        session_env(&added),
        vec![("CLAUDE_CONFIG_DIR".into(), "C:/accounts/second".into())]
    );
}

#[test]
fn switching_changes_only_one_active_row_for_the_provider() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &account("a", "claude-code", 0, true));
    insert(&db, &account("b", "claude-code", 1, false));
    insert(&db, &account("c", "codex", 0, true));

    switch::set_active(&db, "b").unwrap();

    assert_eq!(active(&db, "claude-code").unwrap().id, "b");
    assert_eq!(active(&db, "codex").unwrap().id, "c");
}

#[test]
fn a_signed_out_or_paused_account_cannot_become_active() {
    let db = Database::open_in_memory().unwrap();
    let mut signed_out = account("out", "claude-code", 0, false);
    signed_out.signed_in = false;
    insert(&db, &signed_out);
    let mut paused = account("paused", "claude-code", 1, false);
    paused.paused = true;
    insert(&db, &paused);

    assert_eq!(
        switch::set_active(&db, "out").unwrap_err(),
        "accounts.signedOutCannotActivate"
    );
    assert_eq!(
        switch::set_active(&db, "paused").unwrap_err(),
        "accounts.pausedCannotActivate"
    );
}

#[test]
fn pausing_the_active_account_promotes_a_ready_account() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &account("a", "claude-code", 0, true));
    insert(&db, &account("b", "claude-code", 1, false));

    set_paused(&db, "a", true).unwrap();

    assert_eq!(active(&db, "claude-code").unwrap().id, "b");
    assert!(get(&db, "a").unwrap().unwrap().paused);
}

#[test]
fn the_only_ready_account_cannot_be_paused_while_active() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &account("only", "claude-code", 0, true));

    assert_eq!(
        set_paused(&db, "only", true).unwrap_err(),
        "accounts.lastAvailableCannotPause"
    );
    let unchanged = active(&db, "claude-code").unwrap();
    assert_eq!(unchanged.id, "only");
    assert!(!unchanged.paused);
}

#[test]
fn automatic_switch_policy_round_trips_as_a_stable_code() {
    let db = Database::open_in_memory().unwrap();
    assert_eq!(switch::policy(&db), switch::AutoSwitchPolicy::Off);
    switch::set_policy(&db, switch::AutoSwitchPolicy::OnThreshold).unwrap();
    assert_eq!(switch::policy(&db), switch::AutoSwitchPolicy::OnThreshold);
}

#[test]
fn an_official_rejection_rotates_when_automatic_switching_is_enabled() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &account("a", "claude-code", 0, true));
    insert(&db, &account("b", "claude-code", 1, false));
    insert_session(&db, "a");
    switch::set_policy(&db, switch::AutoSwitchPolicy::OnExhaustion).unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    db.with(|conn| {
        conn.execute(
            "INSERT INTO account_limit_events
                 (account_id, ts_ms, window, status, resets_at_ms)
             VALUES ('a', ?1, 'five_hour', 'rejected', ?2)",
            rusqlite::params![now, now + 60 * 60 * 1_000],
        )?;
        Ok(())
    })
    .unwrap();

    let next = switch::maybe_rotate(&db, "a").expect("another account is ready");
    assert_eq!(next.id, "b");
    assert_eq!(active(&db, "claude-code").unwrap().id, "b");
    let relay = switch::relay_needed(&db, "s1").expect("the driven session now needs a relay");
    assert_eq!(relay.from.id, "a");
    assert_eq!(relay.to.id, "b");
}

#[test]
fn a_manual_switch_never_relays_a_running_session() {
    let db = Database::open_in_memory().unwrap();
    insert(&db, &account("a", "claude-code", 0, true));
    insert(&db, &account("b", "claude-code", 1, false));
    insert_session(&db, "a");

    switch::set_active(&db, "b").unwrap();
    assert!(
        switch::relay_needed(&db, "s1").is_none(),
        "manual activation affects new sessions only"
    );
}

#[test]
fn account_wire_data_contains_identity_but_no_credential_field() {
    let json = serde_json::to_value(account("a", "claude-code", 0, true)).unwrap();
    assert_eq!(json["email"], "a@example.test");
    let object = json.as_object().unwrap();
    for forbidden in [
        "token",
        "accessToken",
        "refreshToken",
        "apiKey",
        "credential",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "secret-shaped field {forbidden} crossed the boundary"
        );
    }
}
