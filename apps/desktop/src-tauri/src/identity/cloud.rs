//! Google identity and the small cloud-sync boundary.
//!
//! The Google client secret lives only on Railway. The desktop opens the
//! system browser, waits on an opaque one-time flow, and receives a revocable
//! J.A.R.V.I.S. session. Provider credentials and configuration directories
//! never cross this boundary.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri_plugin_opener::OpenerExt;

use super::{prefs, IdentityReport, Result};
use crate::db::Database;

const ORIGIN: &str = "https://social-api-production-edb6.up.railway.app";
const SESSION_KEY: &str = "identity.cloudSession";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    flow_id: String,
    poll_secret: String,
    authorization_url: String,
    expires_in_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemoteAccount {
    id: String,
    email: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Poll {
    status: String,
    token: Option<String>,
    account: Option<RemoteAccount>,
    #[serde(default)]
    settings: HashMap<String, serde_json::Value>,
    error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleSignIn {
    pub report: IdentityReport,
    pub carried: prefs::Carried,
}

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

pub async fn sign_in(app: tauri::AppHandle, db: std::sync::Arc<Database>) -> Result<GoogleSignIn> {
    let start: Start = post_json("/v1/auth/google/start", serde_json::json!({}))?;
    app.opener()
        .open_url(&start.authorization_url, None::<&str>)
        .map_err(|error| error.to_string())?;

    let poll = tauri::async_runtime::spawn_blocking(move || {
        let attempts = (start.expires_in_ms / 1_000).clamp(1, 600);
        for _ in 0..attempts {
            std::thread::sleep(Duration::from_secs(1));
            let answer: Poll = post_json(
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
    if poll.settings.is_empty() {
        push_all_settings(&db, &local_id);
    } else {
        for (key, value) in poll.settings {
            if prefs::is_carried(&key) {
                prefs::put_raw(&db, &local_id, &key, &value.to_string())?;
            }
        }
        prefs::apply_to_machine(&db, &local_id)?;
    }

    super::seat(&db, &local_id)?;
    Ok(GoogleSignIn {
        report: super::report(&db)?,
        carried: prefs::carried(&db, &local_id)?,
    })
}

pub fn sign_out(db: &Database) {
    let Some(session) = crate::settings::get::<String>(db, SESSION_KEY) else {
        return;
    };
    let _ = crate::settings::clear(db, SESSION_KEY);
    std::thread::spawn(move || {
        let _ = ureq::post(&format!("{ORIGIN}/v1/auth/sign-out"))
            .set("authorization", &format!("Bearer {session}"))
            .call();
    });
}

pub fn push_preference(db: &Database, key: &str, value: &serde_json::Value) {
    let Some(session) = crate::settings::get::<String>(db, SESSION_KEY) else {
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

fn push_all_settings(db: &Database, account_id: &str) {
    let Some(session) = crate::settings::get::<String>(db, SESSION_KEY) else {
        return;
    };
    let settings = prefs::all(db, account_id);
    std::thread::spawn(move || {
        let payload = serde_json::json!({"settings": settings}).to_string();
        let _ = ureq::put(&format!("{ORIGIN}/v1/sync/settings"))
            .set("authorization", &format!("Bearer {session}"))
            .set("content-type", "application/json")
            .send_string(&payload);
    });
}

pub fn push_quota(db: &Database, report: &crate::accounts::commands::AccountsReport) {
    let Some(session) = crate::settings::get::<String>(db, SESSION_KEY) else {
        return;
    };
    let accounts: Vec<_> = report.accounts.iter().map(|card| serde_json::json!({
        "provider": card.account.provider,
        "label": card.account.label,
        "email": card.account.email,
        "plan": card.account.plan,
        "signedIn": card.account.signed_in,
        "active": card.account.active,
        "paused": card.account.paused,
        "quota": card.quota,
        "sharedWith": card.shared_with,
    })).collect();
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
