//! Identity in the cloud, and the sync boundary (M20, widened by M23).
//!
//! The Google client secret lives only on Railway. The desktop opens the system
//! browser, waits on an opaque one-time flow, and receives a revocable
//! J.A.R.V.I.S. session. Provider credentials and configuration directories
//! never cross this boundary — that rule is older than this module and M23 did
//! not relax it.
//!
//! ## What M23 changed
//!
//! Three things were true and are no longer:
//!
//! * **only Google reached the server.** `identity::sign_up`/`sign_in` were
//!   Argon2 and SQLite and nothing else, so reinstalling lost the *account*,
//!   not merely its contents. A password account now exists on the server too,
//!   and the machine keeps its own hash so that signing in still works with no
//!   network;
//! * **nothing ever pulled.** `GET /v1/sync/state` had no caller in the whole
//!   application. What went up was written and never read back, which is a
//!   backup nobody can restore from;
//! * **only preferences went up**, and in practice not even those — see
//!   `docs/M23-CLOUD-SYNC.md` for the measurement. The notebook goes now.
//!
//! ## Failing is silent, deliberately
//!
//! Every push is fire-and-forget and the launch pull swallows its errors. A
//! machine with no network is exactly the product it was before any of this
//! existed, and a sync that could interrupt somebody to report itself would be
//! a worse thing than a sync that is quietly late.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri_plugin_opener::OpenerExt;

use super::{prefs, IdentityReport, Result, SignInOutcome, SignUpOutcome};
use crate::db::Database;
use crate::notebook::sync::{self as nbsync, SyncPayload};

const ORIGIN: &str = "https://social-api-production-edb6.up.railway.app";
const SESSION_KEY: &str = "identity.cloudSession";

/// How long a notebook push waits for the typing to stop.
///
/// The editor autosaves 500ms after the last keystroke and every mutation
/// returns the whole library, so a push per mutation means a full-library PUT
/// twice a second while somebody writes a paragraph. Pushes coalesce into one
/// request this long after the last of them.
const COALESCE: Duration = Duration::from_secs(3);

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    flow_id: String,
    poll_secret: String,
    authorization_url: String,
    expires_in_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteAccount {
    id: String,
    email: String,
    display_name: String,
}

/// One answer shape for every way in: the Google poll, a password signup, a
/// password sign-in and the launch pull all carry the same account and the same
/// state, because four assemblies of the same thing is four chances for one of
/// them to forget a field.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    #[serde(default)]
    status: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(default)]
    account: Option<RemoteAccount>,
    #[serde(default)]
    settings: HashMap<String, serde_json::Value>,
    #[serde(default)]
    notebook: SyncPayload,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    minimum: Option<u32>,
    #[serde(default)]
    attempts_left: Option<u32>,
    #[serde(default)]
    retry_in_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSignIn {
    pub report: IdentityReport,
    pub carried: prefs::Carried,
}

/// The event the surface listens for when a launch pull actually changed
/// something. Nothing is emitted when the pull was a no-op — a screen that
/// reloads itself for no visible reason is a screen that flickers.
pub const SYNC_EVENT: &str = "sync://state";

fn read_json<T: for<'de> Deserialize<'de>>(response: ureq::Response) -> Result<T> {
    serde_json::from_reader(response.into_reader()).map_err(|error| error.to_string())
}

fn post_json<T: for<'de> Deserialize<'de>>(path: &str, value: serde_json::Value) -> Result<T> {
    let response = ureq::post(&format!("{ORIGIN}{path}"))
        .set("content-type", "application/json")
        .send_string(&value.to_string())
        .map_err(|error| error.to_string())?;
    read_json(response)
}

/// A POST whose refusals are answers rather than failures.
///
/// `ureq` turns any non-2xx into an `Err`, which is right for a transport and
/// wrong here: "that e-mail is taken" is the server doing its job, and it has
/// to be told apart from "there is no network", because one of them is a
/// verdict to show somebody and the other is a reason to fall back to working
/// offline.
fn post_answer(path: &str, value: serde_json::Value) -> Result<Answer> {
    match ureq::post(&format!("{ORIGIN}{path}"))
        .set("content-type", "application/json")
        .send_string(&value.to_string())
    {
        Ok(response) => read_json(response),
        Err(ureq::Error::Status(_, response)) => read_json(response),
        Err(error) => Err(error.to_string()),
    }
}

fn session(db: &Database) -> Option<String> {
    crate::settings::get::<String>(db, SESSION_KEY)
}

// ---------------------------------------------------------------------------
// Applying what arrived
// ---------------------------------------------------------------------------

/// Fold a server answer into this machine.
///
/// `local_id` is the row here; `remote_id` is the account as the server knows
/// it. They are usually the same string and deliberately not required to be —
/// `upsert_google` links a pre-existing local account in place and keeps its
/// id, so the notebook's owner marker uses the remote one, which is the only id
/// that means the same thing on two machines.
fn apply(
    db: &Database,
    local_id: &str,
    remote_id: &str,
    settings: &HashMap<String, serde_json::Value>,
    notebook: &SyncPayload,
) -> Result<(prefs::Carried, bool)> {
    for (key, value) in settings {
        if prefs::is_carried(key) {
            prefs::put_raw(db, local_id, key, &value.to_string())?;
        }
    }
    prefs::apply_to_machine(db, local_id)?;
    let changed = nbsync::merge(db, remote_id, notebook)?;
    Ok((prefs::carried(db, local_id)?, changed))
}

/// Everything this machine has, sent up in one request.
///
/// Called right after a merge so the server ends up holding the union rather
/// than whichever half it started with — a merge that only ever pulled would
/// leave the other machine's copy permanently missing the rows this one had.
fn push_state(db: &Arc<Database>) {
    push_all_settings(db);
    push_notebook_now(db);
}

// ---------------------------------------------------------------------------
// Signing in
// ---------------------------------------------------------------------------

pub async fn sign_in(app: tauri::AppHandle, db: Arc<Database>) -> Result<GoogleSignIn> {
    let start: Start = post_json("/v1/auth/google/start", serde_json::json!({}))?;
    app.opener()
        .open_url(&start.authorization_url, None::<&str>)
        .map_err(|error| error.to_string())?;

    let poll = tauri::async_runtime::spawn_blocking(move || {
        let attempts = (start.expires_in_ms / 1_000).clamp(1, 600);
        for _ in 0..attempts {
            std::thread::sleep(Duration::from_secs(1));
            let answer: Answer = post_json(
                "/v1/auth/google/poll",
                serde_json::json!({"flowId": start.flow_id, "pollSecret": start.poll_secret}),
            )?;
            match answer.status.as_str() {
                "pending" => continue,
                "complete" => return Ok(answer),
                _ => return Err(answer.error.unwrap_or_else(|| "identity.googleFailed".into())),
            }
        }
        Err("identity.googleExpired".into())
    })
    .await
    .map_err(|error| error.to_string())??;

    let session = poll.token.ok_or_else(|| "identity.googleFailed".to_string())?;
    let account = poll.account.ok_or_else(|| "identity.googleFailed".to_string())?;
    crate::settings::set(&db, SESSION_KEY, &session)?;

    let local_id = super::upsert_google(&db, &account.id, &account.email, &account.display_name)?;
    let (carried, _) = apply(&db, &local_id, &account.id, &poll.settings, &poll.notebook)?;

    super::seat(&db, &local_id)?;
    push_state(&db);
    Ok(GoogleSignIn {
        report: super::report(&db)?,
        carried,
    })
}

/// Create an account, on the server when there is one to reach.
///
/// The order matters. The server is asked **first**, because it is the only
/// thing that can say whether an address is already an account somewhere other
/// than this laptop, and because its id is the one both machines will use. Only
/// then does the local row get written. With no network the local row is all
/// there is, which is the product working exactly as it did before M23 — and
/// the next sign-in with a network promotes it.
pub async fn sign_up(
    db: Arc<Database>,
    display_name: String,
    email: String,
    password: String,
) -> Result<SignUpOutcome> {
    // Asked here, before the server, and not only to save a round trip: the
    // server would happily create an account for an address this machine
    // already has a row for, and `sign_up_as` would then refuse to write the
    // local half -- leaving a server account nobody can ever sign into from
    // here. One refusal, at the first place that can give it.
    if super::find_by_email(&db, &email)?.is_some() {
        return Ok(SignUpOutcome::EmailTaken);
    }

    let payload = serde_json::json!({
        "displayName": display_name, "email": email, "password": password,
    });
    let remote = tauri::async_runtime::spawn_blocking(move || post_answer("/v1/auth/sign-up", payload))
        .await
        .map_err(|error| error.to_string())?;

    let remote = match remote {
        Ok(answer) => match answer.status.as_str() {
            "ok" => Some(answer),
            "nameRequired" => return Ok(SignUpOutcome::NameRequired),
            "invalidEmail" => return Ok(SignUpOutcome::InvalidEmail),
            "emailTaken" => return Ok(SignUpOutcome::EmailTaken),
            "passwordTooShort" => {
                return Ok(SignUpOutcome::PasswordTooShort {
                    minimum: answer.minimum.unwrap_or(super::MIN_PASSWORD as u32),
                })
            }
            // An answer this build does not recognise is not a refusal to show
            // somebody. Fall through to the local path, which has its own
            // opinion about every one of these cases.
            _ => None,
        },
        Err(_) => None,
    };

    let id = remote.as_ref().and_then(|answer| answer.account.as_ref()).map(|a| a.id.clone());
    let outcome = match id.as_deref() {
        // The server issued the id, so the local row takes it.
        Some(id) => super::sign_up_as(&db, &display_name, &email, &password, Some(id))?,
        // Nobody answered. The local path owns the whole decision, id included,
        // exactly as it did before there was a server to ask.
        None => super::sign_up(&db, &display_name, &email, &password)?,
    };

    if let (SignUpOutcome::Ok { .. }, Some(answer)) = (&outcome, &remote) {
        if let (Some(token), Some(account)) = (&answer.token, &answer.account) {
            crate::settings::set(&db, SESSION_KEY, token)?;
            let (carried, _) = apply(&db, &account.id, &account.id, &answer.settings, &answer.notebook)?;
            push_state(&db);
            return Ok(SignUpOutcome::Ok {
                report: super::report(&db)?,
                carried,
            });
        }
    }
    Ok(outcome)
}

/// Sign in, trying this machine first and the server second.
///
/// Local first is not an optimisation, it is the local-first rule: somebody on
/// a plane opens their own laptop with their own password. The server is then
/// asked anyway — a successful local sign-in still needs a session to sync
/// against, and that is the request that makes a reinstall restore itself.
///
/// The interesting case is the one the local half **refuses**: an unknown
/// address (a fresh install), or a password this machine's hash rejects but the
/// server accepts (it was changed elsewhere). Both are the server's answer to
/// give, and both end with a local row that works offline afterwards.
pub async fn sign_in_password(
    db: Arc<Database>,
    email: String,
    password: String,
) -> Result<SignInOutcome> {
    let local = super::sign_in(&db, &email, &password)?;
    if matches!(local, SignInOutcome::LockedOut { .. }) {
        return Ok(local);
    }

    let payload = serde_json::json!({"email": email, "password": password});
    let remote = tauri::async_runtime::spawn_blocking(move || post_answer("/v1/auth/sign-in", payload))
        .await
        .map_err(|error| error.to_string())?;

    let Ok(answer) = remote else { return Ok(local) };
    let (Some(token), Some(account), "ok") =
        (&answer.token, &answer.account, answer.status.as_str())
    else {
        // On a machine that has never seen this account, "unknownEmail" is the
        // local half saying it has no row -- which is true and unhelpful, since
        // the reason somebody is signing in on a fresh install is that the
        // account lives somewhere else. Where the server has an opinion, it is
        // the one worth showing.
        return Ok(match (&local, answer.status.as_str()) {
            (SignInOutcome::UnknownEmail, "wrongPassword") => SignInOutcome::WrongPassword {
                attempts_left: answer.attempts_left.unwrap_or(super::MAX_ATTEMPTS),
            },
            (SignInOutcome::UnknownEmail, "lockedOut") => SignInOutcome::LockedOut {
                retry_in_ms: answer.retry_in_ms.unwrap_or(super::LOCKOUT_MS),
            },
            _ => local,
        });
    };

    crate::settings::set(&db, SESSION_KEY, token)?;
    let local_id = match &local {
        // The machine already knew this account; keep its row and its id.
        SignInOutcome::Ok { .. } => super::current(&db).map(|a| a.id).unwrap_or_else(|| account.id.clone()),
        // It did not, or would not. The server just proved the password, so
        // write a row that can prove it again offline.
        _ => super::adopt_remote(&db, &account.id, &account.email, &account.display_name, &password)?,
    };

    let (carried, _) = apply(&db, &local_id, &account.id, &answer.settings, &answer.notebook)?;
    super::seat(&db, &local_id)?;
    push_state(&db);
    Ok(SignInOutcome::Ok {
        report: super::report(&db)?,
        carried,
    })
}

pub fn sign_out(db: &Database) {
    let Some(session) = session(db) else {
        return;
    };
    let _ = crate::settings::clear(db, SESSION_KEY);
    std::thread::spawn(move || {
        let _ = ureq::post(&format!("{ORIGIN}/v1/auth/sign-out"))
            .set("authorization", &format!("Bearer {session}"))
            .call();
    });
}

// ---------------------------------------------------------------------------
// The launch pull
// ---------------------------------------------------------------------------

/// Read the account's state back and fold it in, once, at startup.
///
/// Off the startup path in a thread of its own, like `search::backfill` and for
/// the same reason: nothing on screen may wait for a network round trip. The
/// surface hears about it through `SYNC_EVENT`, and only when something
/// actually changed.
pub fn spawn_pull(app: tauri::AppHandle, db: Arc<Database>) {
    std::thread::spawn(move || {
        let Some(session) = session(&db) else { return };
        let Some(account) = super::current(&db) else { return };

        let answer: std::result::Result<Answer, String> =
            match ureq::get(&format!("{ORIGIN}/v1/sync/state"))
                .set("authorization", &format!("Bearer {session}"))
                .call()
            {
                Ok(response) => read_json(response),
                // A 401 is a session the server has revoked or expired. Clearing
                // it is the honest response — it stops every later push retrying
                // with a credential that will never work again — and it does not
                // sign anybody out here, because being signed in locally never
                // depended on the cloud in the first place.
                Err(ureq::Error::Status(401, _)) => {
                    let _ = crate::settings::clear(&db, SESSION_KEY);
                    return;
                }
                Err(error) => Err(error.to_string()),
            };

        let Ok(answer) = answer else { return };
        let Some(remote) = answer.account.as_ref() else { return };

        match apply(&db, &account.id, &remote.id, &answer.settings, &answer.notebook) {
            Ok((carried, changed)) => {
                // Push after the merge, always: this machine may hold rows the
                // server has never seen, and a pull that did not push them back
                // would leave them stranded here.
                push_state(&db);
                if changed {
                    use tauri::Emitter;
                    let _ = app.emit(SYNC_EVENT, &carried);
                }
            }
            Err(error) => tracing::warn!(%error, "could not apply synced state"),
        }
    });
}

// ---------------------------------------------------------------------------
// Pushing
// ---------------------------------------------------------------------------

pub fn push_preference(db: &Database, key: &str, value: &serde_json::Value) {
    let Some(session) = session(db) else {
        return;
    };
    let key = key.to_string();
    let value = value.clone();
    std::thread::spawn(move || {
        let payload = serde_json::json!({"settings": {key: value}}).to_string();
        let _ = ureq::put(&format!("{ORIGIN}/v1/sync/settings"))
            .set("authorization", &format!("Bearer {session}"))
            .set("content-type", "application/json")
            .send_string(&payload);
    });
}

fn push_all_settings(db: &Database) {
    let Some(session) = session(db) else {
        return;
    };
    let Some(account) = super::current(db) else {
        return;
    };
    let settings = prefs::all(db, &account.id);
    if settings.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        let payload = serde_json::json!({"settings": settings}).to_string();
        let _ = ureq::put(&format!("{ORIGIN}/v1/sync/settings"))
            .set("authorization", &format!("Bearer {session}"))
            .set("content-type", "application/json")
            .send_string(&payload);
    });
}

fn push_notebook_now(db: &Database) {
    let Some(session) = session(db) else {
        return;
    };
    let Ok(payload) = nbsync::payload(db) else {
        return;
    };
    let body = match serde_json::to_string(&payload) {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(%error, "could not encode the notebook");
            return;
        }
    };
    if let Err(error) = ureq::put(&format!("{ORIGIN}/v1/sync/notebook"))
        .set("authorization", &format!("Bearer {session}"))
        .set("content-type", "application/json")
        .send_string(&body)
    {
        tracing::debug!(%error, "notebook push deferred");
    }
}

/// The coalescing pusher: one thread, one request per quiet period.
///
/// A `Sender` rather than a dirty flag and a timer, because the thread has to
/// hold a `Database` handle and the channel is what carries it. Everything that
/// arrives during `COALESCE` collapses into the send that follows, so a
/// paragraph typed at speed is one PUT rather than forty.
static PUSHER: OnceLock<std::sync::mpsc::Sender<Arc<Database>>> = OnceLock::new();

pub fn push_notebook(db: &Arc<Database>) {
    if session(db).is_none() {
        return;
    }
    let sender = PUSHER.get_or_init(|| {
        let (sender, receiver) = std::sync::mpsc::channel::<Arc<Database>>();
        std::thread::spawn(move || {
            while let Ok(mut db) = receiver.recv() {
                while let Ok(next) = receiver.recv_timeout(COALESCE) {
                    db = next;
                }
                push_notebook_now(&db);
            }
        });
        sender
    });
    let _ = sender.send(Arc::clone(db));
}

pub fn push_quota(db: &Database, report: &crate::accounts::commands::AccountsReport) {
    let Some(session) = session(db) else {
        return;
    };
    let accounts: Vec<_> = report
        .accounts
        .iter()
        .map(|card| {
            serde_json::json!({
                "provider": card.account.provider,
                "label": card.account.label,
                "email": card.account.email,
                "plan": card.account.plan,
                "signedIn": card.account.signed_in,
                "active": card.account.active,
                "paused": card.account.paused,
                "quota": card.quota,
                "sharedWith": card.shared_with,
            })
        })
        .collect();
    let snapshot = serde_json::json!({
        "accounts": accounts,
        "autoSwitch": report.auto_switch,
        "thresholdPercent": report.threshold_percent,
    });
    std::thread::spawn(move || {
        let payload = serde_json::json!({"snapshot": snapshot}).to_string();
        let _ = ureq::put(&format!("{ORIGIN}/v1/sync/quota"))
            .set("authorization", &format!("Bearer {session}"))
            .set("content-type", "application/json")
            .send_string(&payload);
    });
}
