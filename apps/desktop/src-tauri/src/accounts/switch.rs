//! Account rotation policy (§66).
//!
//! Switching changes only which configuration directory the *next* session
//! receives. A process that is already running keeps the credentials it
//! loaded at startup; touching it would break the continuity this feature is
//! meant to preserve.



use crate::db::Database;

use super::{get, Account, Result};


/// Build the provider's own persistent sign-in flow for one configuration
/// directory. The caller spawns it off the Tauri command thread.
pub fn sign_in_command(account: &Account, email: Option<&str>) -> Result<std::process::Command> {
    let (bin, mut args): (&str, Vec<&str>) = match account.provider.as_str() {
        "claude-code" => ("claude", vec!["auth", "login", "--claudeai"]),
        "codex" => ("codex", vec!["login"]),
        _ => return Err("accounts.unknownProvider".into()),
    };
    if account.provider == "claude-code" {
        if let Some(email) = email.filter(|value| !value.trim().is_empty()) {
            args.extend(["--email", email]);
        }
    }
    let mut command = crate::envscan::tool_command(bin, &args)
        .ok_or_else(|| "accounts.providerMissing".to_string())?;
    super::apply_env(&mut command, &super::provider_env(account));
    Ok(command)
}

/// Sign one configuration directory out, leaving the directory itself in place.
pub fn sign_out_command(account: &Account) -> Result<std::process::Command> {
    let (bin, args): (&str, Vec<&str>) = match account.provider.as_str() {
        "claude-code" => ("claude", vec!["auth", "logout"]),
        "codex" => ("codex", vec!["logout"]),
        _ => return Err("accounts.unknownProvider".into()),
    };
    let mut command = crate::envscan::tool_command(bin, &args)
        .ok_or_else(|| "accounts.providerMissing".to_string())?;
    super::apply_env(&mut command, &super::provider_env(account));
    Ok(command)
}

/// The authorisation link out of a line the provider's login printed.
///
/// Worth having because it is the difference between "sign in again and hope"
/// and a person being able to choose. The CLI opens a browser itself, and that
/// browser carries an existing claude.ai session which the flow accepts without
/// asking — so the same link, opened in a private window, is the only ordinary
/// route to a *different* account.
///
/// Matched by shape rather than by the sentence around it: the surrounding
/// prose is English-only, versioned, and none of this product's business, while
/// the URL is a stable contract with an OAuth endpoint. Trailing punctuation is
/// trimmed so a link at the end of a sentence still opens.
pub fn authorize_url(line: &str) -> Option<String> {
    let start = line.find("https://")?;
    let rest = &line[start..];
    let end = rest
        .find(char::is_whitespace)
        .unwrap_or(rest.len());
    let url = rest[..end].trim_end_matches(['.', ',', ')', '"', '\'']);
    let looks_like_authorisation =
        url.contains("/oauth/") || url.contains("/authorize") || url.contains("code_challenge");
    looks_like_authorisation.then(|| url.to_string())
}

/// When J.A.R.V.I.S. may move new work away from an account.
/// Make one account the destination for future sessions.
pub fn set_active(db: &Database, account_id: &str) -> Result<Account> {
    let account = get(db, account_id)?.ok_or_else(|| "accounts.notFound".to_string())?;
    if !account.signed_in {
        return Err("accounts.signedOutCannotActivate".into());
    }
    if account.paused {
        return Err("accounts.pausedCannotActivate".into());
    }

    db.with(|conn| {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE provider_accounts SET active = 0 WHERE provider = ?1",
            [&account.provider],
        )?;
        tx.execute(
            "UPDATE provider_accounts SET active = 1 WHERE id = ?1",
            [account_id],
        )?;
        tx.commit()?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    get(db, account_id)?.ok_or_else(|| "accounts.notFound".to_string())
}

/// Stable audit detail for an automatic switch. Prose is localised by the UI.
pub fn record_rotation(db: &Database, from: &Account, to: &Account, estimated: bool) {
    let detail = serde_json::json!({
        "fromAccountId": from.id,
        "toAccountId": to.id,
        "estimated": estimated,
    })
    .to_string();
    crate::activity::record(
        db,
        "account.autoSwitched",
        crate::activity::Severity::Info,
        &to.provider,
        Some(detail),
        None,
        None,
        None,
    );
}
