//! Identity (M20) — the account that belongs to a *person*.
//!
//! ## What this is, and what it is emphatically not
//!
//! `accounts` (M13/M16) is a **provider subscription**: four Claude Pro plans,
//! each one a configuration directory on disk, each with its own five-hour
//! quota. This module is the other kind of account — the person sitting in
//! front of the product, their name, their password, and the preferences that
//! should follow them rather than belong to a machine. Two different things,
//! deliberately two different names; see `docs/M20-IDENTITY.md` §3.
//!
//! ## Signing in is not a gate, and this module must never become one
//!
//! Nothing in the core asks who is signed in before deciding whether work may
//! happen. That is not an oversight to be tidied up later — it is the design:
//!
//! * the product is local-first (§3), and its projects, sessions and
//!   credentials are on this machine already;
//! * half of it runs with nobody present. Unattended runs (§32) drive an agent
//!   turn by turn, `search::backfill` walks the logs five seconds after launch,
//!   and the notification feed raises things while the window is behind
//!   something else. Ask the concrete question — what does `autopilot_start`
//!   do at 3am when nobody is signed in? — and the answer has to be "exactly
//!   what it does today";
//! * there are installations with real work in them already. An update that
//!   demands a signup before somebody can reach their own projects is the
//!   least reversible thing this feature could do.
//!
//! So an account is **additive**. `current` exists for the surface, and for
//! nothing else.
//!
//! ## Passwords
//!
//! Argon2id, through the RustCrypto `argon2` crate, with the PHC string stored
//! whole — algorithm, parameters and salt travel with the hash, so raising the
//! cost later verifies old passwords rather than locking everyone out.
//!
//! The lockout is worth being honest about. Somebody holding `jarvis.db` does
//! not have to guess anything; the hash is not what protects them, and this
//! module never pretends otherwise. What a lockout does protect against is the
//! realistic threat to a *desktop* account — a person at this keyboard trying
//! passwords — so it is short (a minute) rather than punitive, and a correct
//! password clears it.

pub mod commands;
pub mod prefs;

#[cfg(test)]
mod tests;

use argon2::password_hash::{
    rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::session::log::now_ms;

pub type Result<T> = std::result::Result<T, String>;

/// Which account is signed in **on this machine**.
///
/// A machine fact, so it lives in `settings` rather than in `identity_*`. The
/// same person on two machines is signed in on each independently, which is
/// what anyone would expect and what keeps signing out on one from being a
/// remote action on the other.
pub const SIGNED_IN_KEY: &str = "identity.signedIn";

/// Whether the welcome screen has already had its one chance to ask.
///
/// Separate from `onboarding.seen` on purpose: they answer different questions
/// ("has this person been offered an account" against "has this install been
/// shown the environment scan"), and folding them together would mean an
/// existing installation could never be offered an account at all.
pub const PROMPTED_KEY: &str = "identity.prompted";

/// How many wrong passwords in a row before the account rests for a minute.
pub const MAX_ATTEMPTS: u32 = 5;

/// How long that rest is. Short deliberately — see the module note.
pub const LOCKOUT_MS: i64 = 60_000;

/// The shortest password this will accept.
///
/// Eight, and no composition rules. A required symbol produces `Password1!`
/// across the whole world and buys nothing; length is the property that
/// actually costs an attacker something. The surface still *shows* strength,
/// because telling somebody their password is weak is useful — refusing it for
/// failing a rule they did not choose is not.
pub const MIN_PASSWORD: usize = 8;

/// An account, as everything outside this module sees it.
///
/// There is no `password_hash` field, and there is not going to be one. A
/// struct that carries a credential is a struct that eventually gets logged,
/// serialised to the webview, or written into an activity row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub email: String,
    pub display_name: String,
    /// `local` today; `google` when B7 is unblocked.
    pub auth_provider: String,
    /// Whether a password can be used to sign in to this account at all. An
    /// account linked to an external provider has none, and the surface has to
    /// draw a different form rather than a password field that can never work.
    pub has_password: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_signed_in_at: Option<i64>,
}

/// Enough of an account to offer it on the sign-in screen, and no more.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KnownAccount {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub last_signed_in_at: Option<i64>,
}

/// What the surface needs to draw itself, in one answer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IdentityReport {
    /// Who is signed in on this machine, if anybody.
    pub account: Option<Account>,
    /// Accounts this machine already knows, most recently used first, so
    /// signing back in is a click rather than typing an address again.
    pub known: Vec<KnownAccount>,
    /// Whether the welcome screen has already been offered.
    pub prompted: bool,
    /// Whether signing in with Google can actually work. Always `false` today,
    /// and the surface says why rather than drawing a button that lies (§81).
    /// See B7.
    pub google_available: bool,
}

/// The verdict on an attempt to sign in.
///
/// A tagged enum crossing to the webview, which is the exact shape that has bit
/// this repository twice (HANDOFF items 17 and 61): `rename_all` on an enum
/// renames the *variants*, not the fields inside them, and both sides compile
/// while the surface reads `undefined`. `rename_all_fields` is the missing
/// half, and `every_outcome_variant_serialises_in_camel_case` pins it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SignInOutcome {
    /// Signed in. The report comes with it so the surface never has to make a
    /// second call to find out what changed.
    Ok {
        report: IdentityReport,
        /// The preferences this account carries. The ones the core owns have
        /// already been applied by the time this returns; these are the ones
        /// only the webview can act on.
        carried: prefs::Carried,
    },
    /// No account with that address. Deliberately distinguished from a wrong
    /// password: this is a personal machine, not a login page facing the
    /// internet, and "you do not have an account here" is the single most
    /// useful thing the screen can say to somebody who is stuck.
    UnknownEmail,
    WrongPassword {
        attempts_left: u32,
    },
    /// Too many wrong guesses. Carries when it comes back, so the surface can
    /// count down rather than say "try again later" and leave somebody
    /// refreshing.
    LockedOut {
        retry_in_ms: i64,
    },
    /// The account has no local password — created against an external
    /// provider, which is the only way that can happen.
    NoPassword,
}

/// Why a signup was refused, or that it was not.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "status", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum SignUpOutcome {
    Ok {
        report: IdentityReport,
        carried: prefs::Carried,
    },
    NameRequired,
    InvalidEmail,
    EmailTaken,
    /// The password is shorter than `MIN_PASSWORD`. The minimum travels with
    /// the refusal, so the message can name it without the surface keeping its
    /// own copy of a number the core owns.
    PasswordTooShort {
        minimum: u32,
    },
}

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    let hash: Option<String> = row.get("password_hash")?;
    Ok(Account {
        id: row.get("id")?,
        email: row.get("email")?,
        display_name: row.get("display_name")?,
        auth_provider: row.get("auth_provider")?,
        has_password: hash.is_some(),
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        last_signed_in_at: row.get("last_signed_in_at")?,
    })
}

/// Trim, lower-case, and that is all.
///
/// Deliberately *not* clever: stripping dots or `+tags` the way one particular
/// provider treats them would make two addresses the same account here and two
/// different accounts everywhere else in the person's life.
pub fn normalise_email(raw: &str) -> String {
    raw.trim().to_lowercase()
}

/// Is this shaped like an address?
///
/// One `@`, something on each side, a dot in the domain, no whitespace. Not a
/// grammar for RFC 5322 — the only thing this product can honestly check is
/// that the string is not a typo, because there is nothing here that could send
/// a confirmation mail. Rejecting an address a real mail server would accept is
/// a worse failure than accepting one it would not.
pub fn looks_like_email(email: &str) -> bool {
    let mut parts = email.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.chars().any(char::is_whitespace)
}

fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|e| format!("identity.hashFailed: {e}"))
}

fn verify_password(password: &str, stored: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(stored) else {
        // A hash this build cannot parse is not a password anybody can match.
        // Refusing is the only safe reading; treating it as a match would turn
        // a corrupt row into a way in.
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Everything the surface needs, in one call.
pub fn report(db: &Database) -> Result<IdentityReport> {
    let signed_in: Option<String> = crate::settings::get(db, SIGNED_IN_KEY);
    let account = match signed_in.as_deref() {
        Some(id) => find_by_id(db, id)?,
        None => None,
    };

    // A signed-in id pointing at an account that no longer exists is not an
    // error worth failing a screen over — it is a deleted account and a stale
    // pointer. Clear it and carry on signed out.
    if account.is_none() && signed_in.is_some() {
        crate::settings::clear(db, SIGNED_IN_KEY)?;
    }

    let known = db
        .with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, email, display_name, last_signed_in_at
                   FROM identity_accounts
                  ORDER BY last_signed_in_at IS NULL, last_signed_in_at DESC,
                           created_at DESC, id",
            )?;
            let rows: rusqlite::Result<Vec<KnownAccount>> = stmt
                .query_map([], |row| {
                    Ok(KnownAccount {
                        id: row.get("id")?,
                        email: row.get("email")?,
                        display_name: row.get("display_name")?,
                        last_signed_in_at: row.get("last_signed_in_at")?,
                    })
                })?
                .collect();
            rows
        })
        .map_err(|e| e.to_string())?;

    Ok(IdentityReport {
        account,
        known,
        prompted: crate::settings::get_or(db, PROMPTED_KEY, false),
        // B7. When a Google client id exists this becomes a real check for it,
        // and the button stops explaining itself.
        google_available: false,
    })
}

pub fn find_by_id(db: &Database, id: &str) -> Result<Option<Account>> {
    db.with(|conn| {
        conn.query_row(
            "SELECT * FROM identity_accounts WHERE id = ?1",
            [id],
            row_to_account,
        )
        .optional()
    })
    .map_err(|e| e.to_string())
}

pub fn find_by_email(db: &Database, email: &str) -> Result<Option<Account>> {
    let email = normalise_email(email);
    db.with(|conn| {
        conn.query_row(
            "SELECT * FROM identity_accounts WHERE email = ?1",
            [&email],
            row_to_account,
        )
        .optional()
    })
    .map_err(|e| e.to_string())
}

/// Who is signed in, if anybody. The one question the rest of the product is
/// allowed to ask — and, per the module note, it asks in order to *draw*
/// something, never to decide whether work may happen.
pub fn current(db: &Database) -> Option<Account> {
    let id: String = crate::settings::get(db, SIGNED_IN_KEY)?;
    find_by_id(db, &id).ok().flatten()
}

/// Create an account and sign into it.
///
/// Signing in as part of creating is not a convenience shortcut: an account
/// that exists and is not signed into is a state nobody asked for, reachable
/// only by a failure between two calls.
pub fn sign_up(
    db: &Database,
    display_name: &str,
    email: &str,
    password: &str,
) -> Result<SignUpOutcome> {
    let name = display_name.trim();
    if name.is_empty() {
        return Ok(SignUpOutcome::NameRequired);
    }
    let email = normalise_email(email);
    if !looks_like_email(&email) {
        return Ok(SignUpOutcome::InvalidEmail);
    }
    if password.chars().count() < MIN_PASSWORD {
        return Ok(SignUpOutcome::PasswordTooShort {
            minimum: MIN_PASSWORD as u32,
        });
    }
    if find_by_email(db, &email)?.is_some() {
        return Ok(SignUpOutcome::EmailTaken);
    }

    let hash = hash_password(password)?;
    let id = uuid::Uuid::now_v7().to_string();
    let now = now_ms();

    db.with(|conn| {
        conn.execute(
            "INSERT INTO identity_accounts
                 (id, email, display_name, password_hash, auth_provider,
                  created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'local', ?5, ?5)",
            params![&id, &email, name, &hash, now],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    // A brand-new account inherits whatever this machine is already set to, so
    // signing up never resets the app of somebody who has been using it. The
    // alternative — defaults — would make the first thing an account ever does
    // be undoing the person's own choices.
    prefs::adopt_machine_settings(db, &id)?;

    seat(db, &id)?;
    Ok(SignUpOutcome::Ok {
        report: report(db)?,
        carried: prefs::carried(db, &id)?,
    })
}

/// Put an account in the seat: record the sign-in and apply what it carries.
///
/// This is also where the one-time offer is marked as answered, and it has to
/// happen **here** rather than in the command. The command builds the report it
/// returns from `sign_up`/`sign_in`, so marking it afterwards handed the
/// surface a report saying "signed in, and still owed the welcome screen" — and
/// the auth screen, which decides what to draw from exactly that pair, stayed
/// exactly where it was over an account that had just been created correctly.
/// Being signed in *is* the offer having been made and answered.
fn seat(db: &Database, account_id: &str) -> Result<()> {
    mark_prompted(db)?;
    let now = now_ms();
    db.with(|conn| {
        conn.execute(
            "UPDATE identity_accounts
                SET last_signed_in_at = ?2, failed_attempts = 0, locked_until = NULL,
                    updated_at = ?2
              WHERE id = ?1",
            params![account_id, now],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    crate::settings::set(db, SIGNED_IN_KEY, &account_id.to_string())?;
    prefs::apply_to_machine(db, account_id)
}

pub fn sign_in(db: &Database, email: &str, password: &str) -> Result<SignInOutcome> {
    let Some(account) = find_by_email(db, email)? else {
        return Ok(SignInOutcome::UnknownEmail);
    };

    let (stored, failed, locked_until): (Option<String>, u32, Option<i64>) = db
        .with(|conn| {
            conn.query_row(
                "SELECT password_hash, failed_attempts, locked_until
                   FROM identity_accounts WHERE id = ?1",
                [&account.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .map_err(|e| e.to_string())?;

    let now = now_ms();
    if let Some(until) = locked_until {
        if until > now {
            return Ok(SignInOutcome::LockedOut {
                retry_in_ms: until - now,
            });
        }
    }

    let Some(stored) = stored else {
        return Ok(SignInOutcome::NoPassword);
    };

    if verify_password(password, &stored) {
        seat(db, &account.id)?;
        return Ok(SignInOutcome::Ok {
            report: report(db)?,
            carried: prefs::carried(db, &account.id)?,
        });
    }

    // The count restarts once a lockout has expired, so five wrong guesses a
    // week apart never add up to a lockout — which is exactly what a counter
    // that only ever goes up would do.
    let expired = locked_until.map(|until| until <= now).unwrap_or(false);
    let attempts = if expired { 1 } else { failed + 1 };
    let lock = if attempts >= MAX_ATTEMPTS {
        Some(now + LOCKOUT_MS)
    } else {
        None
    };

    db.with(|conn| {
        conn.execute(
            "UPDATE identity_accounts SET failed_attempts = ?2, locked_until = ?3 WHERE id = ?1",
            params![&account.id, attempts, lock],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    if lock.is_some() {
        return Ok(SignInOutcome::LockedOut {
            retry_in_ms: LOCKOUT_MS,
        });
    }
    Ok(SignInOutcome::WrongPassword {
        attempts_left: MAX_ATTEMPTS - attempts,
    })
}

/// Leave the seat empty.
///
/// What this deliberately does **not** do is put the machine's own preferences
/// back. Signing out is not a reason for the interface to change appearance
/// while somebody is looking at it, and the account's values are still in
/// `identity_settings` waiting for the next sign-in. See M20 §5.
pub fn sign_out(db: &Database) -> Result<IdentityReport> {
    crate::settings::clear(db, SIGNED_IN_KEY)?;
    report(db)
}

/// Record that the welcome screen has had its turn.
pub fn mark_prompted(db: &Database) -> Result<()> {
    crate::settings::set(db, PROMPTED_KEY, &true)
}

/// Rename an account, or move it to another address.
pub fn update_profile(db: &Database, id: &str, display_name: &str, email: &str) -> Result<Account> {
    let name = display_name.trim();
    if name.is_empty() {
        return Err("identity.nameRequired".into());
    }
    let email = normalise_email(email);
    if !looks_like_email(&email) {
        return Err("identity.invalidEmail".into());
    }
    if let Some(other) = find_by_email(db, &email)? {
        if other.id != id {
            return Err("identity.emailTaken".into());
        }
    }

    db.with(|conn| {
        conn.execute(
            "UPDATE identity_accounts
                SET display_name = ?2, email = ?3, updated_at = ?4
              WHERE id = ?1",
            params![id, name, &email, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    find_by_id(db, id)?.ok_or_else(|| "identity.notFound".to_string())
}

/// Change a password, proving the old one first.
pub fn change_password(db: &Database, id: &str, current_password: &str, next: &str) -> Result<()> {
    let stored: Option<String> = db
        .with(|conn| {
            conn.query_row(
                "SELECT password_hash FROM identity_accounts WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()
            .map(Option::flatten)
        })
        .map_err(|e| e.to_string())?;

    let Some(stored) = stored else {
        return Err("identity.noPassword".into());
    };
    if !verify_password(current_password, &stored) {
        return Err("identity.wrongPassword".into());
    }
    if next.chars().count() < MIN_PASSWORD {
        return Err("identity.passwordTooShort".into());
    }

    let hash = hash_password(next)?;
    db.with(|conn| {
        conn.execute(
            "UPDATE identity_accounts SET password_hash = ?2, updated_at = ?3 WHERE id = ?1",
            params![id, &hash, now_ms()],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Remove an account and everything it carries.
///
/// The cascade takes `identity_settings` with it. Nothing else in the database
/// references an account — no project, no session, no mission — which is what
/// makes this safe to offer at all, and is worth keeping true: the day a
/// session belongs to an account, deleting one becomes deleting work.
pub fn delete_account(db: &Database, id: &str) -> Result<IdentityReport> {
    db.with(|conn| {
        conn.execute("DELETE FROM identity_accounts WHERE id = ?1", [id])?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    if crate::settings::get::<String>(db, SIGNED_IN_KEY).as_deref() == Some(id) {
        crate::settings::clear(db, SIGNED_IN_KEY)?;
    }
    report(db)
}
