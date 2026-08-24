//! Provider accounts (§66).
//!
//! Four Claude Pro subscriptions, each with its own five-hour allowance, and
//! work that should move to the next one rather than stop. That is the whole
//! feature, and everything here follows from one decision about *how* an
//! account is switched.
//!
//! ## Config directories, not a credential swap
//!
//! Claude Code keeps its credentials, its settings and its transcripts under
//! one configuration root — `~/.claude` by default, overridable with
//! `CLAUDE_CONFIG_DIR`. Codex does the same under `~/.codex` / `CODEX_HOME`.
//! An account in this product **is** such a directory, and switching accounts
//! means the next session is started with a different one in its environment.
//!
//! The alternative — rewriting `~/.claude/.credentials.json` on each switch —
//! was rejected for two reasons, both concrete:
//!
//! 1. The user is signed in to that file *right now*, very possibly in a Claude
//!    Code session they are using to build this product. A switch that rewrites
//!    it logs them out of the thing they are sitting in front of.
//! 2. There is only one such file, so a running session and a new session
//!    cannot be on different accounts. "Keep working on the old account while
//!    new work starts on the next one" — the thing this feature exists for —
//!    is mechanically impossible with a single global credential file.
//!
//! ## The account already on this machine is adopted, never copied
//!
//! The first row this product creates points at the real `~/.claude`. Nothing
//! is copied out of it, nothing is written into it, and removing that account
//! from J.A.R.V.I.S. never deletes the directory. Accounts added afterwards get
//! a directory of ours under the app data dir, and the person signs into it
//! through the provider's own login flow — we never handle a password, and no
//! secret from any of these directories is read, stored, or shown (§60/§61).

pub mod commands;
pub mod quota;
pub mod switch;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::Database;

pub type Result<T> = std::result::Result<T, String>;

/// Providers that have accounts at all.
///
/// A plain shell does not, and asking the capability model rather than
/// hardcoding a list here would be circular — this *is* where the answer for
/// `account_switching` comes from.
pub const PROVIDERS: &[&str] = &["claude-code", "codex"];

/// A signed-in provider account, as the UI sees it.
///
/// Identity only. There is no token, no session key and no path to a secret in
/// this struct, because it crosses into the webview.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub provider: String,
    /// What the user calls this account, or empty when they have not named it.
    ///
    /// Empty rather than a stand-in sentence: the placeholder a person reads
    /// before an account has an identity is a *string in their own language*
    /// (§65), so it belongs in the surface, not in a database column that
    /// would freeze one language into the record forever.
    pub label: String,
    /// Where the provider keeps this account's configuration. Shown because a
    /// person debugging a stuck account needs to know which directory it is,
    /// and it is a path, not a credential.
    pub config_dir: String,
    /// True for the machine's own directory, which this product did not create
    /// and will not delete.
    pub adopted: bool,
    pub email: Option<String>,
    pub org_id: Option<String>,
    pub org_name: Option<String>,
    /// `pro`, `max`, `team`… exactly as the provider spells it.
    pub plan: Option<String>,
    /// Whether the last identity check found credentials that work.
    pub signed_in: bool,
    pub checked_at: Option<i64>,
    /// The account new sessions on this provider start on.
    pub active: bool,
    /// Taken out of the rotation by the user — a decision, not a measurement.
    pub paused: bool,
    pub position: i64,
    pub created_at: i64,
    pub last_used_at: Option<i64>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The configuration directory a provider uses when nothing overrides it.
pub fn machine_config_dir(provider: &str) -> Option<PathBuf> {
    let home = home()?;
    match provider {
        "claude-code" => Some(home.join(".claude")),
        "codex" => Some(home.join(".codex")),
        _ => None,
    }
}

/// The environment variable that points a provider at a configuration root.
pub fn config_env_key(provider: &str) -> Option<&'static str> {
    match provider {
        "claude-code" => Some("CLAUDE_CONFIG_DIR"),
        "codex" => Some("CODEX_HOME"),
        _ => None,
    }
}

/// Environment a session needs to run on this account.
///
/// Empty for the adopted account **on purpose**: leaving the variable unset is
/// not the same as setting it to the same path. Claude Code treats an unset
/// `CLAUDE_CONFIG_DIR` as its own default and takes a different code path for
/// an explicit one, and a product that always sets it would change the
/// behaviour of the account that was working perfectly before this feature
/// existed. The default stays the default.
pub fn session_env(account: &Account) -> Vec<(String, String)> {
    if account.adopted {
        return Vec::new();
    }
    match config_env_key(&account.provider) {
        Some(key) => vec![(key.to_string(), account.config_dir.clone())],
        None => Vec::new(),
    }
}

/// Where this provider writes the transcripts for sessions on this account.
///
/// This is the reason `account_id` is on the sessions table. The transcript
/// tailer, Conversation View, usage, evidence and Global Search all hang off
/// finding this file, and a session started on a non-default configuration
/// directory writes it somewhere the old hardcoded `~/.claude/projects` would
/// never look — which fails as a permanently empty conversation rather than as
/// an error anyone would notice.
pub fn transcript_root(provider: &str, config_dir: &Path) -> Option<PathBuf> {
    match provider {
        "claude-code" => Some(config_dir.join("projects")),
        "codex" => Some(config_dir.join("sessions")),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Who a configuration directory is signed in as.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identity {
    pub signed_in: bool,
    pub email: Option<String>,
    pub org_id: Option<String>,
    pub org_name: Option<String>,
    pub plan: Option<String>,
}

/// Parse `claude auth status --json`.
///
/// Separated from running it so it can be tested against captured output —
/// spawning the real CLI in a unit test would make the suite depend on a login.
pub fn parse_claude_identity(json: &str) -> Option<Identity> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some(Identity {
        signed_in: value
            .get("loggedIn")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        email: text("email"),
        org_id: text("orgId"),
        org_name: text("orgName"),
        plan: text("subscriptionType"),
    })
}

/// Parse the identity Codex records in its own `auth.json`.
///
/// Codex has no `auth status --json` equivalent on 0.149.0, so identity comes
/// from the id-token claims it already stores. **Only the claims are read** —
/// the tokens beside them are never touched, and an unparseable file reads as
/// "signed out", never as an error.
pub fn parse_codex_identity(json: &str) -> Option<Identity> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tokens = value.get("tokens");
    let claims = tokens.and_then(|t| t.get("id_token_claims"));

    let text = |source: Option<&serde_json::Value>, key: &str| {
        source
            .and_then(|v| v.get(key))
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    // A file with an API key and no OAuth tokens is still signed in.
    let has_api_key = value
        .get("OPENAI_API_KEY")
        .map(|v| !v.is_null())
        .unwrap_or(false);
    let has_tokens = tokens.map(|t| !t.is_null()).unwrap_or(false);

    let auth = claims.and_then(|c| c.get("https://api.openai.com/auth"));

    Some(Identity {
        signed_in: has_api_key || has_tokens,
        email: text(claims, "email"),
        org_id: text(auth, "organization_id"),
        org_name: None,
        plan: text(auth, "chatgpt_plan_type"),
    })
}

/// Ask a configuration directory who it is signed in as.
///
/// Never fails a caller: an unknown identity is `None`, which the surface shows
/// as "not checked" rather than as "signed out". Guessing the difference is
/// exactly what §28 forbids one level up.
pub fn read_identity(provider: &str, config_dir: &Path, adopted: bool) -> Option<Identity> {
    match provider {
        "claude-code" => {
            let env = (!adopted).then(|| {
                (
                    "CLAUDE_CONFIG_DIR".to_string(),
                    config_dir.to_string_lossy().to_string(),
                )
            });
            let out = crate::envscan::run_tool("claude", &["auth", "status", "--json"], env)?;
            parse_claude_identity(&out)
        }
        "codex" => {
            let text = std::fs::read_to_string(config_dir.join("auth.json")).ok()?;
            parse_codex_identity(&text)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Registry
// ---------------------------------------------------------------------------

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

fn row_to_account(row: &rusqlite::Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get("id")?,
        provider: row.get("provider")?,
        label: row.get("label")?,
        config_dir: row.get("config_dir")?,
        adopted: row.get::<_, i64>("adopted")? != 0,
        email: row.get("email")?,
        org_id: row.get("org_id")?,
        org_name: row.get("org_name")?,
        plan: row.get("plan")?,
        signed_in: row.get::<_, i64>("signed_in")? != 0,
        checked_at: row.get("checked_at")?,
        active: row.get::<_, i64>("active")? != 0,
        paused: row.get::<_, i64>("paused")? != 0,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        last_used_at: row.get("last_used_at")?,
    })
}

const SELECT: &str = "SELECT id, provider, label, config_dir, adopted, email, org_id, org_name,
                             plan, signed_in, checked_at, active, paused, position, created_at,
                             last_used_at
                      FROM provider_accounts";

pub fn list(db: &Database) -> Result<Vec<Account>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY provider, position"))?;
        let rows: rusqlite::Result<Vec<Account>> =
            stmt.query_map([], row_to_account)?.collect();
        Ok(rows?)
    })
    .map_err(|e| e.to_string())
}

pub fn get(db: &Database, id: &str) -> Result<Option<Account>> {
    db.with(|conn| {
        Ok(conn
            .query_row(&format!("{SELECT} WHERE id = ?1"), [id], row_to_account)
            .optional()?)
    })
    .map_err(|e| e.to_string())
}

/// The account new sessions on this provider start on.
///
/// `None` is a real answer and means "no account is registered for this
/// provider" — every session then runs exactly as it did before this feature
/// existed, on the machine's own configuration. That fallback is deliberate:
/// nothing about starting an agent may depend on accounts having been set up.
pub fn active(db: &Database, provider: &str) -> Option<Account> {
    db.with(|conn| {
        Ok(conn
            .query_row(
                &format!("{SELECT} WHERE provider = ?1 AND active = 1"),
                [provider],
                row_to_account,
            )
            .optional()?)
    })
    .ok()
    .flatten()
}

/// Register the account already signed in on this machine, once.
///
/// Idempotent by `config_dir`, so running it on every launch adopts the machine
/// account the first time and does nothing afterwards. Returns the id when a
/// row was created.
///
/// It does not require the provider to be signed in: an installed-but-logged-out
/// Claude Code still gets a row, showing as signed out with a way to sign in.
/// The alternative — no row at all — leaves the Accounts screen empty on a
/// machine that plainly has Claude Code on it, which reads as a broken surface.
pub fn adopt_machine_account(db: &Database, provider: &str) -> Result<Option<String>> {
    let Some(dir) = machine_config_dir(provider) else {
        return Ok(None);
    };
    if !dir.exists() {
        return Ok(None);
    }
    let dir_text = dir.to_string_lossy().to_string();

    let existing: Option<String> = db
        .with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM provider_accounts WHERE provider = ?1 AND config_dir = ?2",
                    params![provider, dir_text],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .map_err(|e| e.to_string())?;
    if existing.is_some() {
        return Ok(None);
    }

    let identity = read_identity(provider, &dir, true).unwrap_or_default();
    let id = crate::session::manager::new_session_id();
    // Empty when the provider is installed but logged out; the surface names
    // it in the reader's language until an identity arrives.
    let label = identity.email.clone().unwrap_or_default();
    let ts = now_ms();

    db.with(|conn| {
        // The first account for a provider is active by definition: something
        // has to be, and it is the one already in use.
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM provider_accounts WHERE provider = ?1",
            [provider],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO provider_accounts
                 (id, provider, label, config_dir, adopted, email, org_id, org_name, plan,
                  signed_in, checked_at, active, paused, position, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0, ?12, ?10)",
            params![
                id,
                provider,
                label,
                dir_text,
                identity.email,
                identity.org_id,
                identity.org_name,
                identity.plan,
                identity.signed_in as i64,
                ts,
                (count == 0) as i64,
                count,
            ],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    Ok(Some(id))
}

/// Create a new account, with its own configuration directory.
///
/// The directory is created empty. Nothing is copied from another account —
/// copying credentials would put the same subscription in two rows wearing
/// different names, which is worse than useless for a feature whose whole job
/// is to tell allowances apart. The person signs in through the provider's own
/// flow afterwards; see `switch::sign_in_command`.
pub fn create(db: &Database, provider: &str, data_dir: &Path, label: &str) -> Result<Account> {
    if !PROVIDERS.contains(&provider) {
        return Err(format!("unknown provider: {provider}"));
    }
    let id = crate::session::manager::new_session_id();
    let dir = data_dir.join("accounts").join(provider).join(&id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dir_text = dir.to_string_lossy().to_string();
    let ts = now_ms();
    // An unnamed account is stored unnamed. See `Account::label`.
    let label = label.trim().to_string();

    db.with(|conn| {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM provider_accounts WHERE provider = ?1",
            [provider],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO provider_accounts
                 (id, provider, label, config_dir, adopted, signed_in, active, paused,
                  position, created_at)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, 0, ?6, ?7)",
            params![id, provider, label, dir_text, (count == 0) as i64, count, ts],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    get(db, &id)?.ok_or_else(|| "account vanished after insert".to_string())
}

pub fn rename(db: &Database, id: &str, label: &str) -> Result<()> {
    let label = label.trim();
    if label.is_empty() {
        return Err("a label cannot be empty".into());
    }
    db.with(|conn| {
        conn.execute(
            "UPDATE provider_accounts SET label = ?2 WHERE id = ?1",
            params![id, label],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub fn set_paused(db: &Database, id: &str, paused: bool) -> Result<()> {
    db.with(|conn| {
        conn.execute(
            "UPDATE provider_accounts SET paused = ?2 WHERE id = ?1",
            params![id, paused as i64],
        )?;
        Ok(())
    })
    .map_err(|e| e.to_string())
}

/// Forget an account.
///
/// An adopted account's directory is the machine's own and is left exactly
/// where it is. A directory this product created is removed, because leaving a
/// stranded credential store on disk after the user asked to forget the account
/// is the opposite of what they asked for.
///
/// Removing the active account promotes the next one rather than leaving the
/// provider with none active — a provider with rows but no active row would
/// silently fall back to the machine configuration, which is the account the
/// user may just have removed.
pub fn remove(db: &Database, id: &str) -> Result<()> {
    let Some(account) = get(db, id)? else {
        return Ok(());
    };

    db.with(|conn| {
        conn.execute("DELETE FROM provider_accounts WHERE id = ?1", [id])?;
        let still_active: i64 = conn.query_row(
            "SELECT COUNT(*) FROM provider_accounts WHERE provider = ?1 AND active = 1",
            [&account.provider],
            |row| row.get(0),
        )?;
        if still_active == 0 {
            conn.execute(
                "UPDATE provider_accounts SET active = 1
                 WHERE id = (SELECT id FROM provider_accounts WHERE provider = ?1
                             ORDER BY position LIMIT 1)",
                [&account.provider],
            )?;
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    if !account.adopted {
        let _ = std::fs::remove_dir_all(&account.config_dir);
    }
    Ok(())
}

/// Re-read who each account is signed in as.
///
/// Cheap enough to run when the Accounts screen opens, and the only way the
/// product ever learns that a sign-in it invited actually completed — the
/// provider's login happens in a browser and tells us nothing.
pub fn refresh_identity(db: &Database, id: &str) -> Result<Option<Account>> {
    let Some(account) = get(db, id)? else {
        return Ok(None);
    };
    let Some(identity) = read_identity(
        &account.provider,
        Path::new(&account.config_dir),
        account.adopted,
    ) else {
        return Ok(Some(account));
    };
    let ts = now_ms();

    db.with(|conn| {
        conn.execute(
            "UPDATE provider_accounts
                SET email = ?2, org_id = ?3, org_name = ?4, plan = ?5, signed_in = ?6,
                    checked_at = ?7
              WHERE id = ?1",
            params![
                id,
                identity.email,
                identity.org_id,
                identity.org_name,
                identity.plan,
                identity.signed_in as i64,
                ts,
            ],
        )?;
        // An account the user never named takes the identity's own name once
        // one arrives. A label they *did* choose is theirs and is left alone.
        if let Some(email) = identity.email.as_deref() {
            conn.execute(
                "UPDATE provider_accounts SET label = ?2 WHERE id = ?1 AND label = ''",
                params![id, email],
            )?;
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    get(db, id)
}

/// Record that a session started on this account.
pub fn stamp_used(db: &Database, id: &str) {
    let _ = db.with(|conn| {
        conn.execute(
            "UPDATE provider_accounts SET last_used_at = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    });
}
