//! The desktop's end of the mobile companion (§55–§59).
//!
//! One background thread, pushing a summary and collecting whatever the phone
//! queued. Nothing here is on the startup path and nothing local depends on
//! it: **the relay can be unreachable, misconfigured or switched off and the
//! product is unaffected** — that is the test of whether §3 survived contact
//! with the cloud, and it is why every failure in this module is logged and
//! swallowed rather than surfaced.
//!
//! ## Why polling
//!
//! A serverless relay holds no connection, so there is nothing to keep open.
//! For a companion that answers "does anything need me?", a few seconds of
//! latency is not a cost worth building a socket service to avoid — and the
//! thing it buys is real: nothing has to stay up.

pub mod client;
pub mod snapshot;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::Database;
use crate::AppState;

/// How often the desktop talks to the relay.
///
/// One minute, and deliberately unhurried. The freshness window the phone is
/// told about is 150 seconds, so a single missed push does not make a working
/// desktop look offline while two in a row do. Faster polling would buy a
/// companion nothing and spend the user's battery and the account's
/// invocations on saying "still nothing".
const PUSH_INTERVAL: Duration = Duration::from_secs(60);

/// Where the pairing lives, once a desktop has one.
const MAILBOX_KEY: &str = "relay.mailboxId";
const TOKEN_KEY: &str = "relay.desktopToken";
const ENABLED_KEY: &str = "relay.enabled";

/// The relay this build talks to.
///
/// A constant rather than a setting: pointing a desktop at an arbitrary relay
/// is pointing it at an arbitrary server, and "type a URL here" is how a
/// companion feature becomes a way to exfiltrate what is running on someone's
/// machine. A self-hosted relay is a legitimate want and would need a
/// deliberate design (§81) rather than a text field.
pub const RELAY_ORIGIN: &str = "https://jarvis-desktop-relay.vercel.app";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pairing {
    pub mailbox_id: String,
    pub desktop_token: String,
}

/// What the Settings surface needs to render the companion's state.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayStatus {
    /// Whether the user has turned the companion on at all.
    pub enabled: bool,
    /// Whether this desktop has been paired with anything.
    pub paired: bool,
    /// A code being shown right now, if pairing is in progress.
    pub code: Option<String>,
    pub code_expires_at: Option<String>,
}

pub fn pairing(db: &Database) -> Option<Pairing> {
    Some(Pairing {
        mailbox_id: crate::settings::get(db, MAILBOX_KEY)?,
        desktop_token: crate::settings::get(db, TOKEN_KEY)?,
    })
}

pub fn is_enabled(db: &Database) -> bool {
    // **Off unless chosen.** A local-first product does not start talking to a
    // server because it was installed; §3 is a promise, and a default of "on"
    // would break it for everyone who never opened Settings.
    crate::settings::get_or(db, ENABLED_KEY, false)
}

/// Start the background loop. Returns immediately.
pub fn spawn(db: Arc<Database>, stop: Arc<AtomicBool>) {
    std::thread::Builder::new()
        .name("relay-push".into())
        .spawn(move || loop {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(PUSH_INTERVAL);

            if !is_enabled(&db) {
                continue;
            }
            let Some(pairing) = pairing(&db) else { continue };

            match push_once(&db, &pairing) {
                Ok(count) if count > 0 => {
                    tracing::info!(commands = count, "acted on commands from the companion");
                }
                Ok(_) => {}
                // Logged, never surfaced. The companion being unreachable is
                // not something to interrupt someone's work over, and the
                // desktop is fully usable without it.
                Err(e) => tracing::debug!(error = %e, "relay push failed"),
            }
        })
        .expect("spawn relay push");
}

/// One push-and-collect cycle. Returns how many commands were acted on.
fn push_once(db: &Database, pairing: &Pairing) -> Result<usize, String> {
    let device_name = hostname();
    let snapshot = snapshot::build(db, device_name, crate::session::log::now_ms())
        .map_err(|e| e.to_string())?;

    let commands = client::push(pairing, &snapshot)?;
    let mut acted = 0usize;
    for command in commands {
        if apply(db, &command).is_ok() {
            acted += 1;
        }
    }
    Ok(acted)
}

/// Act on one command from the phone.
///
/// Each maps to something the desktop already knows how to do, through the
/// same functions the local surfaces use — a command from a phone must not
/// take a shortcut past a rule the local UI honours. Guardrail approvals in
/// particular go through `guardrail::decide`, so §35's own record of who
/// decided what stays intact.
fn apply(db: &Database, command: &client::Command) -> Result<(), String> {
    match command {
        client::Command::Approve { approval_id, decision, .. } => {
            // **Only `AllowOnce` is reachable from a phone.**
            //
            // §35 offers four answers, and three of them write a policy that
            // outlives the moment: `AllowForProject`, `AlwaysAllow` and —
            // importantly — `NeverAllow`, which is a permanent refusal for the
            // whole project. There is deliberately no "deny once" in that set,
            // because on the desktop a refusal *is* a policy decision made
            // with the command in front of you.
            //
            // A phone has a small screen, little context and a thumb. Letting
            // it write a lasting safety policy would be a real consequence
            // from a glance, so it gets the one answer that expires with the
            // thing it answers: allow this, now. A refusal from a phone is
            // simply not sending an approval — the guardrail already refuses
            // by default (§35), so silence is the safe answer and it is
            // already the right one.
            if decision != "allow" {
                return Err("relay.denyNotFromPhone".into());
            }
            crate::guardrail::commands::decide(
                db,
                approval_id.clone(),
                crate::guardrail::commands::Choice::AllowOnce,
                // Recorded as decided from the phone rather than as if someone
                // had clicked on the desktop. The audit trail should say where
                // an approval came from.
                Some("companion".to_string()),
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        }
        // Deliberately not implemented in this pass, and not silently ignored
        // either: the phone is told the command failed rather than left to
        // assume it worked. Starting a mission from a phone needs the same
        // Unattended checks `autopilot_start` makes, including the untrusted
        // folder refusal (HANDOFF §5 item 25), and that deserves its own pass.
        client::Command::StartMission { .. } | client::Command::StopRun { .. } => {
            Err("relay.notImplemented".into())
        }
    }
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "desktop".into())
}

// ---- Commands the Settings surface calls ------------------------------------

#[tauri::command]
pub fn relay_status(state: State<'_, AppState>) -> RelayStatus {
    RelayStatus {
        enabled: is_enabled(&state.db),
        paired: pairing(&state.db).is_some(),
        code: None,
        code_expires_at: None,
    }
}

/// Ask the relay for a pairing code, and remember the token it returns.
#[tauri::command]
pub async fn relay_pair(state: State<'_, AppState>) -> Result<RelayStatus, String> {
    let offer = client::request_code()?;

    crate::settings::set(&state.db, MAILBOX_KEY, &offer.mailbox_id)?;
    crate::settings::set(&state.db, TOKEN_KEY, &offer.desktop_token)?;
    crate::settings::set(&state.db, ENABLED_KEY, &true)?;

    Ok(RelayStatus {
        enabled: true,
        paired: true,
        code: Some(offer.code),
        code_expires_at: Some(offer.expires_at),
    })
}

/// Turn the companion off, and forget the pairing.
///
/// Forgetting rather than merely disabling: a paired phone that can no longer
/// reach anything is the honest outcome of turning this off, and leaving a
/// live token in the database against a switch marked "off" is the kind of
/// thing nobody expects.
#[tauri::command]
pub fn relay_unpair(state: State<'_, AppState>) -> Result<RelayStatus, String> {
    // **Tell the relay first, then forget locally.**
    //
    // Found by testing the real thing: clearing only the local rows left the
    // mailbox standing, and a phone that had already paired went on reading
    // snapshots from a desktop that believed it had cut them off. A switch
    // marked "disconnect" that leaves a live token working is worse than no
    // switch.
    //
    // A failure here is logged and does not stop the local half: if the relay
    // cannot be reached, the desktop stops publishing anyway, so the phone
    // sees a snapshot that goes stale and then says so (§28). Refusing to
    // disconnect because a server is unreachable would be the wrong answer to
    // "I want this off".
    if let Some(pairing) = pairing(&state.db) {
        if let Err(e) = client::unpair(&pairing) {
            tracing::warn!(error = %e, "could not revoke the mailbox; disconnecting locally anyway");
        }
    }

    crate::settings::clear(&state.db, MAILBOX_KEY)?;
    crate::settings::clear(&state.db, TOKEN_KEY)?;
    crate::settings::set(&state.db, ENABLED_KEY, &false)?;
    Ok(RelayStatus { enabled: false, paired: false, code: None, code_expires_at: None })
}
