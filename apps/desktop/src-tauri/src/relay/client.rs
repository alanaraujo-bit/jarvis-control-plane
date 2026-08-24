//! Talking to the relay.
//!
//! Thin on purpose: every failure here is one the caller swallows, because the
//! companion being unreachable must never be something the desktop reports as
//! a problem with itself (§3).

use serde::Deserialize;

use super::{Pairing, RELAY_ORIGIN};

/// How long any relay call may take before it is abandoned.
///
/// Short. This runs on a timer, so a hung request would hold the thread until
/// the next tick and quietly turn a one-minute cadence into something else. A
/// relay that has not answered in ten seconds is one to try again later.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeOffer {
    pub code: String,
    pub expires_at: String,
    pub mailbox_id: String,
    pub desktop_token: String,
}

/// A command the phone queued.
///
/// Deserialised into a **closed enum**, so anything the relay sends that this
/// build does not recognise fails to parse and is dropped rather than being
/// passed along to be interpreted. The relay is not trusted to only send known
/// shapes — it is a server, and this is the desktop.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Command {
    #[serde(rename_all = "camelCase")]
    Approve {
        #[allow(dead_code)]
        id: String,
        approval_id: String,
        decision: String,
    },
    #[serde(rename_all = "camelCase")]
    StartMission {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        project_id: String,
        #[allow(dead_code)]
        mission_id: String,
    },
    #[serde(rename_all = "camelCase")]
    StopRun {
        #[allow(dead_code)]
        id: String,
        #[allow(dead_code)]
        mission_id: String,
    },
}

#[derive(Debug, Deserialize)]
struct CollectResponse {
    commands: Vec<Command>,
}

/// Ask the relay for a pairing code.
pub fn request_code() -> Result<CodeOffer, String> {
    let response = ureq::post(&format!("{RELAY_ORIGIN}/api/pair"))
        .timeout(TIMEOUT)
        .send_string("")
        .map_err(|e| e.to_string())?;
    serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string())
}

/// Push a snapshot and collect whatever is waiting.
pub fn push<T: serde::Serialize>(pairing: &Pairing, snapshot: &T) -> Result<Vec<Command>, String> {
    // Serialised by hand rather than with `send_json`: that method is behind
    // ureq's `json` feature, which is not enabled here — the same trap the
    // voice work hit with `into_json` (see `voice::model`, which sidesteps it
    // the same way).
    let body = serde_json::to_string(&serde_json::json!({ "snapshot": snapshot }))
        .map_err(|e| e.to_string())?;
    let response = ureq::post(&format!(
        "{RELAY_ORIGIN}/api/desktop?mailbox={}",
        pairing.mailbox_id
    ))
    .timeout(TIMEOUT)
    .set("authorization", &format!("Bearer {}", pairing.desktop_token))
    .set("content-type", "application/json")
    .send_string(&body)
    .map_err(|e| e.to_string())?;

    let collected: CollectResponse =
        serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string())?;
    Ok(collected.commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire shape the relay actually sends, pinned.
    ///
    /// Copied from a real response captured against the deployed relay, not
    /// written from the type: `camelCase` on the wire against `snake_case` in
    /// Rust is exactly the mismatch HANDOFF §5 item 17 records costing real
    /// time, and a test that asserts the type against itself would not catch
    /// it.
    #[test]
    fn a_real_pairing_response_deserialises() {
        let real = r#"{"code":"FZ4AT2","expiresAt":"2026-08-24T01:48:24.547Z","mailboxId":"b4a3303cec9167737f42e44102469ba9789f737795413181d54d6c22a4b67b2c","desktopToken":"99f09512fba055914d15d365f5c5d9081967ae82ccd837491dd600ef75896209"}"#;
        let offer: CodeOffer = serde_json::from_str(real).unwrap();
        assert_eq!(offer.code, "FZ4AT2");
        assert_eq!(offer.mailbox_id.len(), 64);
        assert_eq!(offer.desktop_token.len(), 64);
    }

    /// Also a real response, captured after queueing an approval from the
    /// phone side against the deployed relay.
    #[test]
    fn a_real_collect_response_deserialises() {
        let real = r#"{"commands":[{"kind":"approve","id":"cmd-1","approvalId":"ap1","decision":"allow"}]}"#;
        let collected: CollectResponse = serde_json::from_str(real).unwrap();
        assert_eq!(collected.commands.len(), 1);
        match &collected.commands[0] {
            Command::Approve { approval_id, decision, .. } => {
                assert_eq!(approval_id, "ap1");
                assert_eq!(decision, "allow");
            }
            other => panic!("expected an approval, got {other:?}"),
        }
    }

    #[test]
    fn an_empty_collect_is_the_ordinary_case() {
        let collected: CollectResponse = serde_json::from_str(r#"{"commands":[]}"#).unwrap();
        assert!(collected.commands.is_empty());
    }

    /// A shape this build does not know must not be acted on. If a later relay
    /// grows a command this desktop has never heard of, the honest response is
    /// to ignore it rather than to guess.
    #[test]
    fn an_unknown_command_is_refused_rather_than_guessed_at() {
        let hostile = r#"{"commands":[{"kind":"runShell","id":"x","cmd":"rm -rf /"}]}"#;
        assert!(serde_json::from_str::<CollectResponse>(hostile).is_err());
    }
}
