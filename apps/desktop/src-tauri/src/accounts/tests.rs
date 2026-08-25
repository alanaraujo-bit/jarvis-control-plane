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
fn rotation_refuses_a_second_directory_signed_into_the_same_account() {
    // Found on this machine: three Claude cards, and two of them —
    // `~/.claude` and one added later — were signed in as the same person, so
    // they showed the same 74% and the same reset. Rotating between them moves
    // the work and changes nothing.
    let db = Database::open_in_memory().unwrap();
    let mut twin = account("b", "claude-code", 1, false);
    twin.email = Some("a@example.test".into());
    let mut other = account("c", "claude-code", 2, false);
    other.email = Some("someone.else@example.test".into());
    insert(&db, &account("a", "claude-code", 0, true));
    insert(&db, &twin);
    insert(&db, &other);
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

    let next = switch::maybe_rotate(&db, "a").expect("a different subscription is ready");
    assert_eq!(
        next.id, "c",
        "the twin sits earlier in rotation order and must still be skipped"
    );
}

#[test]
fn an_account_with_no_email_is_unknown_rather_than_everyone_else() {
    // Codex 0.149.1 stopped writing `id_token_claims`, so nameless accounts are
    // a real state. Grouping them together would strand rotation on a provider
    // where nothing can be told apart.
    let mut left = account("a", "codex", 0, true);
    left.email = None;
    let mut right = account("b", "codex", 1, false);
    right.email = Some("   ".into());

    assert!(subscription_key(&left).is_none());
    assert!(subscription_key(&right).is_none());
    assert!(!same_subscription(&left, &right));
}

#[test]
fn one_subscription_is_recognised_across_case_and_provider() {
    let mut lower = account("a", "claude-code", 0, true);
    lower.email = Some("Someone@Example.Test".into());
    let mut upper = account("b", "claude-code", 1, false);
    upper.email = Some("someone@example.test".into());
    assert!(same_subscription(&lower, &upper));

    // The same address at two providers is two subscriptions.
    let mut elsewhere = account("c", "codex", 2, false);
    elsewhere.email = Some("someone@example.test".into());
    assert!(!same_subscription(&lower, &elsewhere));
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

// ---------------------------------------------------------------------------
// Live quota, against the CLIs actually installed on this machine (M16)
// ---------------------------------------------------------------------------

/// The whole live-quota feature rests on a claim about two external programs,
/// and a claim about an external program is worth exactly as much as the last
/// time somebody ran it. These are `#[ignore]`d because they need a signed-in
/// account and a working network — run them when a provider ships a new
/// version, which is the moment the claim can quietly stop being true:
///
/// ```text
/// cargo test live_quota -- --ignored --nocapture
/// ```
mod live_cli {
    use super::*;
    use crate::accounts::live::{self, LiveStatus};

    /// The machine's own account, as `adopt_machine_account` would register it.
    /// `adopted` is true, so `session_env` is empty and the CLI runs against the
    /// default configuration — the account the person is signed into right now.
    fn machine(provider: &str) -> Option<Account> {
        let dir = super::super::machine_config_dir(provider)?;
        if !dir.exists() {
            return None;
        }
        Some(Account {
            id: "machine".into(),
            provider: provider.into(),
            label: String::new(),
            config_dir: dir.to_string_lossy().to_string(),
            adopted: true,
            email: None,
            org_id: None,
            org_name: None,
            plan: None,
            signed_in: true,
            checked_at: Some(1),
            active: true,
            paused: false,
            position: 0,
            created_at: 1,
            last_used_at: None,
        })
    }

    /// An account pointed at a directory that has never been signed into.
    fn empty(provider: &str, dir: &std::path::Path) -> Account {
        Account {
            id: "empty".into(),
            provider: provider.into(),
            label: String::new(),
            config_dir: dir.to_string_lossy().to_string(),
            adopted: false,
            email: None,
            org_id: None,
            org_name: None,
            plan: None,
            signed_in: false,
            checked_at: Some(1),
            active: false,
            paused: false,
            position: 1,
            created_at: 1,
            last_used_at: None,
        }
    }

    #[test]
    #[ignore = "runs the real Claude Code CLI and needs a signed-in account"]
    fn live_quota_claude_answers_with_official_numbers() {
        let Some(account) = machine("claude-code") else {
            eprintln!("no ~/.claude on this machine — nothing to probe");
            return;
        };
        let status = live::probe(&account);
        let LiveStatus::Ok { reading } = &status else {
            panic!("expected a reading from the machine's own account, got {status:?}");
        };

        assert!(
            !reading.windows.is_empty(),
            "a signed-in Pro account is rationed by at least one window"
        );
        assert_eq!(
            reading.windows.iter().filter(|w| w.binding).count(),
            1,
            "exactly one window binds — that is the answer the panel exists to give"
        );
        for window in &reading.windows {
            assert!(
                (0.0..=100.0).contains(&window.percent_used),
                "{}: {} is not a percentage",
                window.raw_kind,
                window.percent_used
            );
        }
        println!("claude: plan={:?} windows={:#?}", reading.plan, reading.windows);
    }

    #[test]
    #[ignore = "runs the real Codex CLI and needs a signed-in account"]
    fn live_quota_codex_answers_and_checks_which_home_it_opened() {
        let Some(account) = machine("codex") else {
            eprintln!("no ~/.codex on this machine — nothing to probe");
            return;
        };
        let status = live::probe(&account);
        let LiveStatus::Ok { reading } = &status else {
            panic!("expected a reading from the machine's own Codex account, got {status:?}");
        };
        assert!(!reading.windows.is_empty());

        // The reason this assertion exists: Codex 0.149.1 stopped writing
        // `id_token_claims` into `auth.json`, which is where
        // `parse_codex_identity` looks, and every Codex card in the product
        // went nameless without a single error. `account/read` is the supported
        // route and rides the app-server session the limits already opened.
        let identity = reading
            .identity
            .as_ref()
            .expect("the app-server states who this configuration directory is");
        assert!(
            identity.email.is_some(),
            "a signed-in ChatGPT account has an e-mail; a nameless card is how \
             the previous format change hid itself"
        );
        println!(
            "codex: plan={:?} identity={:?} windows={:#?}",
            reading.plan, identity, reading.windows
        );
    }

    /// The property the whole feature rests on: a probe reads the account in
    /// the directory it is pointed at, and an unsigned directory produces a
    /// definite "nobody is signed in here" rather than the ambient account's
    /// numbers under the wrong name.
    ///
    /// If this ever starts returning `Ok`, the panel is attributing one
    /// account's allowance to another and the feature must be turned off until
    /// it is understood.
    #[test]
    #[ignore = "runs both real CLIs"]
    fn live_quota_an_empty_directory_never_borrows_the_ambient_account() {
        let root = std::env::temp_dir().join(format!("jarvis-quota-probe-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        for provider in ["claude-code", "codex"] {
            let dir = root.join(provider);
            std::fs::create_dir_all(&dir).unwrap();
            let status = live::probe(&empty(provider, &dir));
            match &status {
                LiveStatus::Unavailable { reason, .. } => {
                    assert_eq!(reason, "signedOut", "{provider}");
                }
                LiveStatus::Failed { reason, .. } => {
                    // A missing CLI is a legitimate outcome on a machine that
                    // does not have it; anything else is not.
                    assert_eq!(reason, "toolMissing", "{provider}: {status:?}");
                }
                LiveStatus::Ok { .. } => panic!(
                    "{provider} reported quota for a directory that has never been \
                     signed into — the probe is reading the ambient account and \
                     every number in the panel is attributed to the wrong person"
                ),
            }
        }

        let _ = std::fs::remove_dir_all(&root);
    }
}
