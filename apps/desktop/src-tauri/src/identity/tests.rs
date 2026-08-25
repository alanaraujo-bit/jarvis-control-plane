use super::*;

fn db() -> Database {
    Database::open_in_memory().unwrap()
}

/// The password every test signs up with. Long enough to pass `MIN_PASSWORD`
/// and obviously not a real one.
const PASSWORD: &str = "correct horse battery";

fn account(db: &Database) -> Account {
    match sign_up(db, "Alan Araujo", "Alan@Example.com", PASSWORD).unwrap() {
        SignUpOutcome::Ok { report, .. } => report.account.unwrap(),
        other => panic!("expected a created account, got {other:?}"),
    }
}

#[test]
fn signing_up_creates_the_account_and_puts_it_in_the_seat() {
    let db = db();
    let created = account(&db);

    assert_eq!(created.display_name, "Alan Araujo");
    assert_eq!(created.email, "alan@example.com", "normalised on the way in");
    assert!(created.has_password);
    assert_eq!(created.auth_provider, "local");

    // Signed in, not merely created — see the note on `sign_up`.
    assert_eq!(current(&db).map(|a| a.id), Some(created.id));
}

#[test]
fn the_same_address_in_different_casing_is_the_same_account() {
    let db = db();
    account(&db);
    let second = sign_up(&db, "Someone Else", "ALAN@example.COM", PASSWORD).unwrap();
    assert_eq!(second, SignUpOutcome::EmailTaken);
}

#[test]
fn a_signup_is_refused_for_each_reason_separately() {
    let db = db();
    assert_eq!(
        sign_up(&db, "   ", "a@b.com", PASSWORD).unwrap(),
        SignUpOutcome::NameRequired
    );
    assert_eq!(
        sign_up(&db, "Alan", "not-an-address", PASSWORD).unwrap(),
        SignUpOutcome::InvalidEmail
    );
    assert_eq!(
        sign_up(&db, "Alan", "a@b.com", "short").unwrap(),
        SignUpOutcome::PasswordTooShort {
            minimum: MIN_PASSWORD as u32
        }
    );
    // None of the refusals wrote a row.
    assert!(report(&db).unwrap().known.is_empty());
}

/// A password is measured in characters, not bytes. Eight accented letters is
/// eight characters and sixteen bytes, and a byte-length check would have
/// accepted a four-character one for the same reason `type_text` sliced
/// "informação" in half (HANDOFF item 36).
#[test]
fn a_password_is_measured_in_characters() {
    let db = db();
    assert_eq!(
        sign_up(&db, "Alan", "a@b.com", "ãéíõçãé").unwrap(),
        SignUpOutcome::PasswordTooShort {
            minimum: MIN_PASSWORD as u32
        },
        "seven characters, fourteen bytes"
    );
}

#[test]
fn signing_in_with_the_right_password_works_and_with_the_wrong_one_counts_down() {
    let db = db();
    account(&db);
    sign_out(&db).unwrap();

    match sign_in(&db, "alan@example.com", "wrong password").unwrap() {
        SignInOutcome::WrongPassword { attempts_left } => {
            assert_eq!(attempts_left, MAX_ATTEMPTS - 1)
        }
        other => panic!("expected a wrong password, got {other:?}"),
    }
    assert!(current(&db).is_none(), "a failed attempt seats nobody");

    match sign_in(&db, "  ALAN@Example.com ", PASSWORD).unwrap() {
        SignInOutcome::Ok { report, .. } => assert!(report.account.is_some()),
        other => panic!("expected a sign-in, got {other:?}"),
    }
    assert!(current(&db).is_some());
}

/// A correct password clears the count, so somebody who mistypes twice and then
/// gets it right does not walk around one attempt from a lockout.
#[test]
fn a_correct_password_clears_the_failures_behind_it() {
    let db = db();
    account(&db);
    sign_out(&db).unwrap();

    sign_in(&db, "alan@example.com", "nope").unwrap();
    sign_in(&db, "alan@example.com", "nope").unwrap();
    sign_in(&db, "alan@example.com", PASSWORD).unwrap();
    sign_out(&db).unwrap();

    match sign_in(&db, "alan@example.com", "nope").unwrap() {
        SignInOutcome::WrongPassword { attempts_left } => {
            assert_eq!(attempts_left, MAX_ATTEMPTS - 1, "counting from zero again")
        }
        other => panic!("expected a wrong password, got {other:?}"),
    }
}

#[test]
fn enough_wrong_guesses_lock_the_account_for_a_minute() {
    let db = db();
    let created = account(&db);
    sign_out(&db).unwrap();

    for _ in 0..MAX_ATTEMPTS {
        sign_in(&db, "alan@example.com", "nope").unwrap();
    }

    match sign_in(&db, "alan@example.com", PASSWORD).unwrap() {
        SignInOutcome::LockedOut { retry_in_ms } => {
            assert!(retry_in_ms > 0 && retry_in_ms <= LOCKOUT_MS)
        }
        other => panic!("expected a lockout, got {other:?}"),
    }
    assert!(
        current(&db).is_none(),
        "the right password during a lockout still does not get in"
    );

    // Once it expires, the count starts over rather than resuming at five —
    // otherwise every subsequent mistake would relock instantly.
    db.with(|conn| {
        conn.execute(
            "UPDATE identity_accounts SET locked_until = 1 WHERE id = ?1",
            [&created.id],
        )?;
        Ok(())
    })
    .unwrap();

    match sign_in(&db, "alan@example.com", "nope").unwrap() {
        SignInOutcome::WrongPassword { attempts_left } => {
            assert_eq!(attempts_left, MAX_ATTEMPTS - 1)
        }
        other => panic!("expected the count to restart, got {other:?}"),
    }
}

#[test]
fn an_address_nobody_has_registered_says_so() {
    let db = db();
    account(&db);
    assert_eq!(
        sign_in(&db, "someone@else.com", PASSWORD).unwrap(),
        SignInOutcome::UnknownEmail
    );
}

/// A stored hash this build cannot parse must not be a way in. The empty string
/// is the shape a truncated write would leave.
#[test]
fn an_unparseable_hash_matches_nothing() {
    let db = db();
    let created = account(&db);
    db.with(|conn| {
        conn.execute(
            "UPDATE identity_accounts SET password_hash = '' WHERE id = ?1",
            [&created.id],
        )?;
        Ok(())
    })
    .unwrap();
    sign_out(&db).unwrap();

    assert!(matches!(
        sign_in(&db, "alan@example.com", PASSWORD).unwrap(),
        SignInOutcome::WrongPassword { .. }
    ));
}

#[test]
fn an_account_never_carries_its_hash_across_the_wire() {
    let db = db();
    let created = account(&db);
    let json = serde_json::to_string(&created).unwrap();

    assert!(!json.contains("password_hash"));
    assert!(!json.contains("passwordHash"));
    assert!(!json.contains("$argon2"));
    assert!(json.contains("\"hasPassword\":true"));
}

/// HANDOFF items 17 and 61, pinned. `rename_all` on an enum renames the
/// variants and **not** the fields inside them; both sides compile while the
/// surface reads `undefined`, and the visible symptom is a rendered `NaN`.
/// TypeScript cannot check a name that only exists at runtime, so this test is
/// the only guard there is.
#[test]
fn every_outcome_variant_serialises_in_camel_case() {
    let wrong = serde_json::to_string(&SignInOutcome::WrongPassword { attempts_left: 3 }).unwrap();
    assert!(wrong.contains("\"attemptsLeft\":3"), "got {wrong}");
    assert!(wrong.contains("\"status\":\"wrongPassword\""), "got {wrong}");

    let locked = serde_json::to_string(&SignInOutcome::LockedOut { retry_in_ms: 60 }).unwrap();
    assert!(locked.contains("\"retryInMs\":60"), "got {locked}");
    assert!(locked.contains("\"status\":\"lockedOut\""), "got {locked}");

    let unknown = serde_json::to_string(&SignInOutcome::UnknownEmail).unwrap();
    assert_eq!(unknown, "{\"status\":\"unknownEmail\"}");
    let none = serde_json::to_string(&SignInOutcome::NoPassword).unwrap();
    assert_eq!(none, "{\"status\":\"noPassword\"}");

    let short = serde_json::to_string(&SignUpOutcome::PasswordTooShort { minimum: 8 }).unwrap();
    assert!(short.contains("\"minimum\":8"), "got {short}");
    assert!(short.contains("\"status\":\"passwordTooShort\""), "got {short}");

    for (variant, expected) in [
        (SignUpOutcome::NameRequired, "nameRequired"),
        (SignUpOutcome::InvalidEmail, "invalidEmail"),
        (SignUpOutcome::EmailTaken, "emailTaken"),
    ] {
        let json = serde_json::to_string(&variant).unwrap();
        assert_eq!(json, format!("{{\"status\":\"{expected}\"}}"));
    }

    // And the `Ok` arms, which carry the two structs the surface reads most.
    let db = db();
    let created = account(&db);
    let ok = serde_json::to_string(&SignInOutcome::Ok {
        report: report(&db).unwrap(),
        carried: prefs::Carried {
            theme: Some("light".into()),
            locale: None,
        },
    })
    .unwrap();
    assert!(ok.contains("\"displayName\""), "got {ok}");
    assert!(ok.contains("\"hasPassword\""), "got {ok}");
    assert!(ok.contains("\"googleAvailable\""), "got {ok}");
    assert!(ok.contains("\"lastSignedInAt\""), "got {ok}");
    assert!(ok.contains("\"authProvider\""), "got {ok}");
    assert!(ok.contains(&created.id), "the report travels inside the Ok arm");
}

#[test]
fn what_the_machine_is_set_to_is_inherited_by_a_new_account() {
    let db = db();
    crate::settings::set(&db, crate::settings::TERMINAL_FONT_SIZE_KEY, &17u32).unwrap();
    let created = account(&db);

    assert_eq!(
        prefs::raw(&db, &created.id, crate::settings::TERMINAL_FONT_SIZE_KEY),
        Some("17".to_string()),
        "signing up must not reset an app somebody was already using"
    );
}

#[test]
fn preferences_follow_the_account_back_in() {
    let db = db();
    let created = account(&db);

    // The person changes two things while signed in.
    prefs::remember(&db, crate::settings::TERMINAL_FONT_SIZE_KEY, "19").unwrap();
    prefs::remember(&db, prefs::THEME_KEY, "\"light\"").unwrap();
    sign_out(&db).unwrap();

    // Somebody else — or the same person on a fresh machine — sets it low.
    crate::settings::set(&db, crate::settings::TERMINAL_FONT_SIZE_KEY, &11u32).unwrap();

    match sign_in(&db, "alan@example.com", PASSWORD).unwrap() {
        SignInOutcome::Ok { carried, .. } => {
            assert_eq!(carried.theme.as_deref(), Some("light"));
            assert_eq!(carried.locale, None, "never expressed, so never applied");
        }
        other => panic!("expected a sign-in, got {other:?}"),
    }
    assert_eq!(
        crate::settings::get::<u32>(&db, crate::settings::TERMINAL_FONT_SIZE_KEY),
        Some(19),
        "the account's own value came back"
    );
    assert_eq!(created.email, "alan@example.com");
}

/// M20 §5, stated as a test so nobody "fixes" it later: signing out leaves the
/// interface exactly as it is. Changing somebody's theme out from under them
/// because they signed out is a surprise, not a cleanup.
#[test]
fn signing_out_does_not_put_the_machine_back() {
    let db = db();
    account(&db);
    prefs::remember(&db, crate::settings::TERMINAL_FONT_SIZE_KEY, "19").unwrap();
    prefs::apply_to_machine(&db, &current(&db).unwrap().id).unwrap();

    sign_out(&db).unwrap();

    assert_eq!(
        crate::settings::get::<u32>(&db, crate::settings::TERMINAL_FONT_SIZE_KEY),
        Some(19)
    );
}

/// A key nobody decided is carried must not become a way to write arbitrary
/// rows from the webview — the same closed-list reasoning
/// `settings_set_preference` gives.
#[test]
fn a_key_that_is_not_carried_is_refused() {
    let db = db();
    account(&db);
    assert!(prefs::remember(&db, "onboarding.seen", "true").is_err());
    assert_eq!(crate::settings::get::<bool>(&db, "onboarding.seen"), None);
}

#[test]
fn remembering_with_nobody_signed_in_does_nothing_and_is_not_an_error() {
    let db = db();
    assert!(prefs::remember(&db, prefs::THEME_KEY, "\"light\"").is_ok());
}

#[test]
fn deleting_an_account_takes_its_preferences_and_the_seat_with_it() {
    let db = db();
    let created = account(&db);
    prefs::remember(&db, prefs::THEME_KEY, "\"light\"").unwrap();

    let after = delete_account(&db, &created.id).unwrap();

    assert!(after.account.is_none());
    assert!(after.known.is_empty());
    assert!(current(&db).is_none());
    let rows: i64 = db
        .with(|conn| conn.query_row("SELECT COUNT(*) FROM identity_settings", [], |r| r.get(0)))
        .unwrap();
    assert_eq!(rows, 0, "ON DELETE CASCADE");
}

/// A pointer at an account that is gone is a stale pointer, not a broken
/// screen. It reads as signed out and cleans itself up.
#[test]
fn a_signed_in_pointer_to_a_missing_account_reads_as_signed_out() {
    let db = db();
    crate::settings::set(&db, SIGNED_IN_KEY, &"nobody".to_string()).unwrap();

    let report = report(&db).unwrap();

    assert!(report.account.is_none());
    assert_eq!(crate::settings::get::<String>(&db, SIGNED_IN_KEY), None);
}

#[test]
fn the_welcome_screen_is_offered_once() {
    let db = db();
    assert!(!report(&db).unwrap().prompted);
    mark_prompted(&db).unwrap();
    assert!(report(&db).unwrap().prompted);
}

/// The report a sign-in hands back has to describe the world *after* it, not
/// the world one statement earlier.
///
/// When `mark_prompted` ran in the command rather than in `seat`, this field
/// came back `false` from a call that had just signed somebody in — and the
/// surface, which decides whether to draw the auth screen from exactly this
/// pair, went on drawing it over an account that had been created correctly.
/// Nothing errored, and no test here noticed. Found by pressing Enter in the
/// real app, seeing nothing happen, and being told on the second attempt that
/// the address was already taken.
#[test]
fn a_sign_in_hands_back_a_report_that_already_knows_it_happened() {
    let db = db();
    match sign_up(&db, "Alan", "alan@example.com", PASSWORD).unwrap() {
        SignUpOutcome::Ok { report, .. } => {
            assert!(report.account.is_some());
            assert!(report.prompted, "signed in, so the offer is answered");
        }
        other => panic!("expected a signup, got {other:?}"),
    }

    sign_out(&db).unwrap();
    match sign_in(&db, "alan@example.com", PASSWORD).unwrap() {
        SignInOutcome::Ok { report, .. } => {
            assert!(report.account.is_some());
            assert!(report.prompted);
        }
        other => panic!("expected a sign-in, got {other:?}"),
    }
}

#[test]
fn known_accounts_are_offered_most_recently_used_first() {
    let db = db();
    account(&db);
    sign_out(&db).unwrap();
    sign_up(&db, "Second", "second@example.com", PASSWORD).unwrap();

    let known = report(&db).unwrap().known;
    assert_eq!(known.len(), 2);
    assert_eq!(known[0].email, "second@example.com");
}

#[test]
fn a_profile_can_be_renamed_but_not_onto_another_accounts_address() {
    let db = db();
    let created = account(&db);
    sign_up(&db, "Second", "second@example.com", PASSWORD).unwrap();

    let updated = update_profile(&db, &created.id, " Alan V. Araujo ", "Alan@Example.com").unwrap();
    assert_eq!(updated.display_name, "Alan V. Araujo");
    assert_eq!(updated.email, "alan@example.com");

    assert_eq!(
        update_profile(&db, &created.id, "Alan", "second@example.com"),
        Err("identity.emailTaken".to_string())
    );
    assert_eq!(
        update_profile(&db, &created.id, "", "alan@example.com"),
        Err("identity.nameRequired".to_string())
    );
}

#[test]
fn changing_a_password_needs_the_old_one_and_then_the_new_one_works() {
    let db = db();
    let created = account(&db);

    assert_eq!(
        change_password(&db, &created.id, "not it", "a much longer one"),
        Err("identity.wrongPassword".to_string())
    );
    assert_eq!(
        change_password(&db, &created.id, PASSWORD, "short"),
        Err("identity.passwordTooShort".to_string())
    );

    change_password(&db, &created.id, PASSWORD, "a much longer one").unwrap();
    sign_out(&db).unwrap();

    assert!(matches!(
        sign_in(&db, "alan@example.com", PASSWORD).unwrap(),
        SignInOutcome::WrongPassword { .. }
    ));
    assert!(matches!(
        sign_in(&db, "alan@example.com", "a much longer one").unwrap(),
        SignInOutcome::Ok { .. }
    ));
}

#[test]
fn two_accounts_with_the_same_password_do_not_share_a_hash() {
    // Per-password salting, checked rather than assumed: an unsalted scheme
    // would make one leaked hash answer for every account that reused it.
    let db = db();
    account(&db);
    sign_up(&db, "Second", "second@example.com", PASSWORD).unwrap();

    let hashes: Vec<String> = db
        .with(|conn| {
            let mut stmt = conn.prepare("SELECT password_hash FROM identity_accounts")?;
            let rows: rusqlite::Result<Vec<String>> =
                stmt.query_map([], |row| row.get(0))?.collect();
            rows
        })
        .unwrap();

    assert_eq!(hashes.len(), 2);
    assert_ne!(hashes[0], hashes[1]);
    assert!(hashes.iter().all(|h| h.starts_with("$argon2id$")));
}

#[test]
fn an_address_is_checked_for_shape_and_not_for_a_grammar() {
    for good in [
        "a@b.co",
        "alan.araujo+jarvis@example.com.br",
        "x@sub.domain.io",
    ] {
        assert!(looks_like_email(good), "{good} should be accepted");
    }
    for bad in [
        "",
        "alan",
        "@example.com",
        "alan@",
        "alan@example",
        "alan@.com",
        "alan@example.",
        "alan @example.com",
        "a@b@c.com",
    ] {
        assert!(!looks_like_email(bad), "{bad} should be rejected");
    }
}
