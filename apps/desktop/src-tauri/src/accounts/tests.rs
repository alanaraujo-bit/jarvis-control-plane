use super::*;

fn account(id: &str, provider: &str, position: i64, active: bool) -> Account {
    Account {
        id: id.into(),
        provider: provider.into(),
        label: id.into(),
        config_dir: format!("C:/accounts/{id}"),
        adopted: false,
        email: Some(format!("{id}@example.test")),
        account_uuid: None,
        org_id: None,
        org_name: None,
        plan: Some("pro".into()),
        signed_in: true,
        checked_at: Some(1),
        identity_attempted_at: Some(1),
        subscription_since: 0,
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
                 (id, provider, label, config_dir, adopted, email, account_uuid, org_id, org_name,
                  plan, signed_in, checked_at, identity_attempted_at, subscription_since,
                  active, paused, position, created_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                     ?18, ?19)",
            rusqlite::params![
                value.id,
                value.provider,
                value.label,
                value.config_dir,
                value.adopted as i64,
                value.email,
                value.account_uuid,
                value.org_id,
                value.org_name,
                value.plan,
                value.signed_in as i64,
                value.checked_at,
                value.identity_attempted_at,
                value.subscription_since,
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

/// The failure this whole area exists to prevent, in the shape it actually had.
///
/// Two directories were signed into one Claude account. One card's stored
/// e-mail was hours out of date, from the person who had been signed into that
/// directory before, so the twin check compared two different strings, found no
/// twin, and let the panel present one allowance as two — the user's report of
/// "two accounts, identical statistics, and I can never actually use the other
/// one".
#[test]
fn a_stale_email_cannot_hide_two_directories_on_one_subscription() {
    let mut machine = account("machine", "claude-code", 0, true);
    machine.email = Some("previous-owner@example.test".into());
    machine.account_uuid = Some("bddb9ea1-8777-4499-b8c2-3fb4c5429acd".into());

    let mut second = account("second", "claude-code", 1, false);
    second.email = Some("current@example.test".into());
    second.account_uuid = Some("bddb9ea1-8777-4499-b8c2-3fb4c5429acd".into());

    assert!(
        same_subscription(&machine, &second),
        "two directories carrying one account uuid are one allowance, whatever \
         e-mail the rows happen to be carrying"
    );
    assert_eq!(twins_of(&machine, &[machine.clone(), second.clone()]).len(), 1);
}

/// The uuid decides in both directions, including when it says "different".
#[test]
fn a_matching_email_does_not_override_two_different_account_uuids() {
    // A person with one address on two subscriptions — a personal plan and a
    // seat somebody bought them — is two allowances, and rotating between them
    // is exactly what this feature is for.
    let mut left = account("a", "claude-code", 0, true);
    left.email = Some("someone@example.test".into());
    left.account_uuid = Some("11111111-1111-1111-1111-111111111111".into());
    let mut right = account("b", "claude-code", 1, false);
    right.email = Some("someone@example.test".into());
    right.account_uuid = Some("22222222-2222-2222-2222-222222222222".into());

    assert!(!same_subscription(&left, &right));
    assert!(twins_of(&left, &[left.clone(), right.clone()]).is_empty());

    // …and a rotation may move between them, which is the point.
    let db = Database::open_in_memory().unwrap();
    insert(&db, &left);
    insert(&db, &right);
    assert_eq!(
        switch::next_available(&db, &left).map(|a| a.id),
        Some("b".to_string())
    );
}

/// A uuid on only one side falls back to e-mail rather than declaring a match.
#[test]
fn a_missing_uuid_falls_back_to_email_and_never_matches_on_absence() {
    let mut known = account("a", "claude-code", 0, true);
    known.account_uuid = Some("11111111-1111-1111-1111-111111111111".into());
    known.email = Some("shared@example.test".into());
    let mut unknown = account("b", "claude-code", 1, false);
    unknown.account_uuid = None;
    unknown.email = Some("shared@example.test".into());
    assert!(same_subscription(&known, &unknown));

    // Two accounts with neither uuid nor e-mail are unknown, never the same.
    let mut blank_a = account("c", "codex", 2, false);
    blank_a.account_uuid = None;
    blank_a.email = None;
    let mut blank_b = account("d", "codex", 3, false);
    blank_b.account_uuid = None;
    blank_b.email = None;
    assert!(!same_subscription(&blank_a, &blank_b));
}

/// A rotation must never move work onto the allowance it is leaving.
#[test]
fn rotation_skips_a_directory_on_the_same_subscription() {
    let db = Database::open_in_memory().unwrap();
    let mut current = account("a", "claude-code", 0, true);
    current.account_uuid = Some("same-account".into());
    let mut twin = account("b", "claude-code", 1, false);
    twin.account_uuid = Some("same-account".into());
    twin.email = Some("looks-different@example.test".into());
    insert(&db, &current);
    insert(&db, &twin);

    assert!(
        switch::next_available(&db, &current).is_none(),
        "moving to a second directory on the same subscription buys nothing: \
         same allowance, same window, same reset — and under the threshold \
         policy it would bounce straight back"
    );
}

/// `.claude.json` is not inside the adopted account's configuration directory.
#[test]
fn the_adopted_accounts_config_file_is_in_the_home_directory() {
    let dir = std::path::Path::new("C:/Users/someone/.claude");
    let adopted = claude_identity_file(dir, true).unwrap();
    assert!(
        !adopted.starts_with(dir),
        "the machine account runs with CLAUDE_CONFIG_DIR unset, so its config \
         file stays at $HOME/.claude.json; reading the stub inside ~/.claude \
         finds no account uuid and silently restores the bug (got {adopted:?})"
    );
    assert!(adopted.ends_with(".claude.json"));

    let created = claude_identity_file(dir, false).unwrap();
    assert_eq!(created, dir.join(".claude.json"));
}

#[test]
fn an_account_uuid_is_read_from_the_provider_config() {
    let json = r#"{
        "numStartups": 4,
        "oauthAccount": {
            "accountUuid": "bddb9ea1-8777-4499-b8c2-3fb4c5429acd",
            "emailAddress": "someone@example.test",
            "organizationName": "someone@example.test's Organization"
        }
    }"#;
    assert_eq!(
        parse_claude_oauth_account(json),
        Some((
            Some("bddb9ea1-8777-4499-b8c2-3fb4c5429acd".to_string()),
            Some("someone@example.test".to_string()),
        ))
    );

    // The stub that lives inside `~/.claude` has no account in it at all, and
    // must read as "nothing here" rather than as a parse failure.
    let stub = r#"{"firstStartTime":"2026-01-01","migrationVersion":3}"#;
    assert_eq!(parse_claude_oauth_account(stub), None);
}

/// The gate in front of every identity read opens on a *changed account*, and
/// stays shut for a file that was merely rewritten.
///
/// The distinction is the whole design. Claude Code rewrites `.claude.json`
/// about every ten minutes while a session runs and `.credentials.json` on
/// every token refresh; an earlier version of this gate compared modification
/// times and would therefore have started a CLI per account on almost every
/// paint — and `load("cached")` runs after every rename, pause, remove and
/// activate, so pausing an account would have frozen the window.
#[test]
fn the_identity_gate_opens_on_a_changed_account_not_a_rewritten_file() {
    let root = std::env::temp_dir().join(format!("jarvis-identity-{}", std::process::id()));
    let dir = root.join("account");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(".credentials.json"), "{}").unwrap();

    let config = dir.join(".claude.json");
    let write_account = |uuid: &str, email: &str| {
        std::fs::write(
            &config,
            format!(
                r#"{{"numStartups":1,"oauthAccount":{{"accountUuid":"{uuid}",
                    "emailAddress":"{email}"}}}}"#
            ),
        )
        .unwrap();
    };

    let mut row = account("a", "claude-code", 0, true);
    row.config_dir = dir.to_string_lossy().to_string();
    row.checked_at = Some(now_ms());
    row.email = Some("someone@example.test".into());
    row.account_uuid = Some("uuid-one".into());

    write_account("uuid-one", "someone@example.test");
    assert!(
        !identity_is_stale(&row),
        "the row already says what the file says — asking a CLI to be told the \
         same costs a second a card on every paint"
    );

    // The provider rewrites the file with the same account in it, as it does
    // every ten minutes. Nothing has changed.
    std::thread::sleep(std::time::Duration::from_millis(10));
    write_account("uuid-one", "someone@example.test");
    row.checked_at = Some(now_ms() - 60_000);
    assert!(
        !identity_is_stale(&row),
        "a rewritten file is not a changed account; keying this on mtime is \
         what made the gate open on every paint"
    );

    // Now the directory is genuinely signed into somebody else.
    write_account("uuid-two", "other@example.test");
    assert!(
        identity_is_stale(&row),
        "a different account in the directory is exactly what this must catch — \
         it is the only trace a login performed outside this product leaves"
    );

    // Signed out elsewhere: credentials gone while the row still claims one.
    write_account("uuid-one", "someone@example.test");
    std::fs::remove_file(dir.join(".credentials.json")).unwrap();
    assert!(identity_is_stale(&row));

    // Never checked at all is stale by definition.
    std::fs::write(dir.join(".credentials.json"), "{}").unwrap();
    let mut never = row.clone();
    never.account_uuid = Some("uuid-one".into());
    never.email = Some("someone@example.test".into());
    never.checked_at = None;
    assert!(identity_is_stale(&never));

    let _ = std::fs::remove_dir_all(&root);
}

/// Upgrading must not look like the directory changed hands.
///
/// Every existing row reaches this build with `account_uuid` NULL. The first
/// identity read fills it in, and if that counted as a change of subscription
/// then `subscription_since` would jump to now on **every account on every
/// installation** — silently erasing everybody's quota history, sparkline and
/// calibration on the first launch after upgrading.
///
/// The rule that prevents it lives in `same_subscription`: a uuid on only one
/// side falls through to the e-mail comparison. This test exists because that
/// is a subtle branch guarding an expensive, irreversible outcome.
#[test]
fn backfilling_the_account_uuid_is_not_a_change_of_subscription() {
    let before = {
        let mut row = account("a", "claude-code", 0, true);
        row.email = Some("someone@example.test".into());
        row.account_uuid = None; // as every row arrives from migration 18
        row
    };
    let after = Account {
        account_uuid: Some("uuid-one".into()),
        ..before.clone()
    };

    assert!(
        same_subscription(&before, &after),
        "gaining a uuid for the account the row already named is a column being \
         filled in, not a directory changing hands — if this ever flips, every \
         user loses their whole quota history on upgrade"
    );

    // And the real thing still registers as a change.
    let elsewhere = Account {
        email: Some("different@example.test".into()),
        account_uuid: Some("uuid-two".into()),
        ..before.clone()
    };
    assert!(!same_subscription(&before, &elsewhere));
}

/// A provider that stops naming the account has not renamed it.
///
/// Found by running the real-machine diagnostic: Codex 0.149.1 no longer writes
/// `id_token_claims`, so `read_identity` returns no e-mail for a directory that
/// is plainly signed in. The first version of this code wrote that `None` over
/// the known address — which not only lost the only key a Codex account can be
/// compared by, but read as *"this directory now belongs to somebody else"* and
/// moved `subscription_since`, discarding the account's entire quota history.
/// Every refresh. Silently.
#[test]
fn losing_the_email_is_not_a_change_of_subscription() {
    let known = {
        let mut row = account("a", "codex", 0, true);
        row.email = Some("someone@example.test".into());
        row
    };
    // What the provider now reports: signed in, and nothing else.
    let nameless = Account {
        email: None,
        ..known.clone()
    };

    assert!(
        subscription_key(&nameless).is_none(),
        "an account with no identity has no key — that is the premise"
    );
    // The guard is that `changed` requires a key on *both* sides. Absence is
    // unknown, and `subscription_since` may only move on a positive statement
    // that this is a different account.
    assert!(
        subscription_key(&known).is_some() && subscription_key(&nameless).is_none(),
        "which is exactly the shape the guard in `refresh_identity` tests for"
    );
}

/// The same guarantee, through the function that actually writes the column.
#[test]
fn refresh_identity_keeps_history_when_it_only_learns_the_uuid() {
    let root = std::env::temp_dir().join(format!("jarvis-upgrade-{}", std::process::id()));
    let dir = root.join("account");
    std::fs::create_dir_all(&dir).unwrap();

    let db = Database::open_in_memory().unwrap();
    let mut row = account("a", "claude-code", 0, true);
    row.config_dir = dir.to_string_lossy().to_string();
    row.subscription_since = 1_234;
    insert(&db, &row);

    // `read_identity` cannot answer for a directory with no provider behind it,
    // so this exercises the failure branch — which must also leave the
    // boundary alone.
    let after = refresh_identity(&db, "a").unwrap().unwrap();
    assert_eq!(
        after.subscription_since, 1_234,
        "a read that could not happen must never move the boundary that decides \
         which history belongs to this account"
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// A failed identity read is distinguishable from an unchanged one.
#[test]
fn a_failed_identity_read_records_the_attempt_and_keeps_the_last_answer() {
    let db = Database::open_in_memory().unwrap();
    let mut row = account("a", "claude-code", 0, true);
    // A directory no provider can answer for: nothing is installed at this path
    // and `read_identity` returns `None`.
    row.config_dir = "C:/does-not-exist/jarvis-test".into();
    row.email = Some("known@example.test".into());
    row.checked_at = Some(1);
    row.identity_attempted_at = Some(1);
    insert(&db, &row);

    let after = refresh_identity(&db, "a").unwrap().unwrap();
    assert_eq!(
        after.email.as_deref(),
        Some("known@example.test"),
        "a read that could not happen must not erase what was last known"
    );
    assert!(
        after.identity_attempted_at.unwrap() > 1,
        "the attempt is recorded even when it fails, so a card can say the \
         identity has not been confirmed since — before this, the two states \
         were indistinguishable and one account showed a previous owner's \
         e-mail for eleven hours"
    );
    assert_eq!(
        after.checked_at,
        Some(1),
        "`checked_at` still means the last time an identity was actually read"
    );
}

/// Quota history from a previous occupant is not this account's.
#[test]
fn history_before_a_change_of_subscription_is_not_counted() {
    let db = Database::open_in_memory().unwrap();
    let now = now_ms();
    let day = 86_400_000;
    let mut row = account("a", "claude-code", 0, true);
    // The directory changed hands two days ago.
    row.subscription_since = now - 2 * day;
    insert(&db, &row);

    let spend = |ts: i64, tokens: i64| {
        db.with(|conn| {
            conn.execute(
                "INSERT INTO usage_samples
                     (session_id, project_id, provider, model, ts_ms, input_tokens,
                      output_tokens, cache_read_tokens, cache_write_tokens, confidence,
                      account_id)
                 VALUES (NULL, NULL, 'claude-code', 'opus', ?1, ?2, 0, 0, 0, 'official', 'a')",
                params![ts, tokens],
            )?;
            Ok(())
        })
        .unwrap();
    };
    spend(now - 4 * day, 900_000); // the previous account's week
    spend(now - 3_600_000, 1_000); // this one's

    let quota = quota::for_account(&db, &get(&db, "a").unwrap().unwrap());
    let counted: i64 = quota.daily_tokens.iter().sum();
    assert_eq!(
        counted, 1_000,
        "a directory that has been signed into a different account since carries \
         somebody else's spend; counting it is the 'it merged my two accounts' \
         complaint arriving from the other direction"
    );
}

/// The authorisation link is what makes choosing a different account possible.
#[test]
fn the_authorisation_link_is_recovered_from_the_login_output() {
    // Measured output of `claude auth login --claudeai` on this machine.
    let line = "If the browser didn't open, visit: \
                https://claude.com/cai/oauth/authorize?code=true&client_id=9d1c250a\
                &response_type=code&code_challenge_method=S256";
    let url = switch::authorize_url(line).expect("the link is the whole point");
    assert!(url.starts_with("https://claude.com/cai/oauth/authorize?"));
    assert!(!url.contains(' '));

    // Ordinary chatter carries no link, and a non-OAuth URL is not one.
    assert_eq!(switch::authorize_url("Login successful."), None);
    assert_eq!(
        switch::authorize_url("Read more at https://docs.claude.com/cli"),
        None
    );
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
            account_uuid: None,
            org_id: None,
            org_name: None,
            plan: None,
            signed_in: true,
            checked_at: Some(1),
            identity_attempted_at: Some(1),
            subscription_since: 0,
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
            account_uuid: None,
            org_id: None,
            org_name: None,
            plan: None,
            signed_in: false,
            checked_at: Some(1),
            identity_attempted_at: Some(1),
            subscription_since: 0,
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

/// Point the real machine's registry at this build and see what it says.
///
/// `#[ignore]`d because it reads a database that only exists on a machine that
/// has actually run the app, and because it starts a CLI per account. It is
/// kept because it is the only check that exercises the whole chain against
/// real signed-in directories: migrate, re-read every identity, and ask whether
/// two cards are the same allowance.
///
/// **It works on a copy.** The installed app holds the original open, and a
/// diagnostic that writes to the database somebody is working in is not a
/// diagnostic.
///
/// Run with:
/// `cargo test real_machine_registry_tells_its_accounts_apart -- --ignored --nocapture`
#[test]
#[ignore]
fn real_machine_registry_tells_its_accounts_apart() {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return;
    };
    let live = std::path::Path::new(&appdata)
        .join("dev.jarvis.desktop")
        .join("jarvis.db");
    if !live.exists() {
        return;
    }

    let copy = std::env::temp_dir().join(format!("jarvis-registry-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&copy);
    std::fs::copy(&live, &copy).unwrap();
    let db = Database::open(&copy).unwrap();

    for account in list(&db).unwrap() {
        println!("{:<30} stale_before={}", account.label, identity_is_stale(&account));
        let refreshed = refresh_identity(&db, &account.id).unwrap().unwrap();
        println!(
            "{:<30} {:<12} email={:?} uuid={:?} signed_in={}",
            refreshed.label,
            refreshed.provider,
            refreshed.email,
            refreshed.account_uuid,
            refreshed.signed_in,
        );
        // The gate must be shut immediately after a refresh. If it is not, it
        // is comparing something that changes on its own, and every paint of
        // the panel would start a CLI per account.
        assert!(
            !identity_is_stale(&refreshed),
            "{}: the identity gate is still open right after a successful read \
             — it is keyed on something that moves by itself, and the panel \
             would spawn a CLI per account on every paint",
            refreshed.label,
        );
    }

    let all = list(&db).unwrap();
    for account in &all {
        let twins = twins_of(account, &all);
        if twins.is_empty() {
            continue;
        }
        println!(
            "SHARED: {} draws on the same subscription as {:?}",
            account.label,
            twins.iter().map(|t| &t.label).collect::<Vec<_>>(),
        );
        // Two views of one allowance must never be offered as somewhere to move
        // work to — that is the ping-pong this whole area exists to prevent.
        assert!(
            switch::next_available(&db, account)
                .map(|next| !same_subscription(account, &next))
                .unwrap_or(true),
            "rotation offered a directory on the same subscription"
        );
    }

    let _ = std::fs::remove_file(&copy);
}
