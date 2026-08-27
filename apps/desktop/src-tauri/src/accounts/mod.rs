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
pub mod live;
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
    /// The provider's own identifier for the subscription behind this
    /// directory, where it publishes one.
    ///
    /// This is what makes "are these two cards the same allowance?" answerable
    /// rather than guessed. An e-mail is a label — it changes, it has aliases,
    /// it is written in whatever casing the person typed — while this string is
    /// identical in every directory signed into one account. `None` for Codex,
    /// which publishes no equivalent, and for any directory not signed in.
    pub account_uuid: Option<String>,
    pub org_id: Option<String>,
    pub org_name: Option<String>,
    /// `pro`, `max`, `team`… exactly as the provider spells it.
    pub plan: Option<String>,
    /// Whether the last identity check found credentials that work.
    pub signed_in: bool,
    /// When an identity was last successfully **read**.
    pub checked_at: Option<i64>,
    /// When an identity read was last **attempted**, successfully or not.
    ///
    /// Separate from `checked_at` because a failed read used to be silent: the
    /// function returned early without writing anything, so "the CLI could not
    /// answer" and "nothing has changed" were one state. A card that cannot
    /// tell them apart shows a confident identity that has not been verified
    /// since some hour it does not name.
    pub identity_attempted_at: Option<i64>,
    /// When this row's current subscription was first seen on it.
    ///
    /// Quota history before this instant belongs to whoever was signed into the
    /// directory before, and is excluded from every window, calibration and
    /// sparkline this account draws. See migration 18.
    pub subscription_since: i64,
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

/// The subscription an account draws on, as a comparable key.
///
/// An account in this product is a configuration directory, and **two
/// directories can be signed into the same provider account**. That is not a
/// corner case, it is what happens by default: `claude auth login` in an empty
/// directory reuses the claude.ai session the browser already holds and returns
/// "Login successful" in about a second, having asked nothing. Add a second
/// account the obvious way and you get a second directory on the *first*
/// account — two cards, two names, one allowance, both dials moving together.
///
/// Two keys, in order of authority:
///
/// * **`Uuid`** — the provider's own identifier for the account, read from
///   `oauthAccount.accountUuid`. Where both sides have one it decides on its
///   own and overrides e-mail: it is the same string in every directory signed
///   into one account, and it survives an alias, a rename and a change of
///   casing that would each defeat the e-mail comparison.
/// * **`Email`** — the fallback where a uuid is not published, which is every
///   Codex account and any directory whose config has not been written yet.
///   Not `org_id`: a personal organisation maps one-to-one onto a Pro account
///   and would be right on this machine, but a Team plan puts several people's
///   separate allowances under one org, and calling colleagues twins is a wrong
///   answer that gets worse the larger the team.
///
/// **Absent is unknown, never same.** Codex 0.149.1 stopped writing
/// `id_token_claims`, which leaves accounts with no e-mail at all (see
/// `live::identity`); treating `None == None` would make every nameless account
/// a twin of every other one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubscriptionKey {
    Uuid(String),
    Email(String),
}

/// The strongest key this account can be compared by, with its provider.
pub fn subscription_key(account: &Account) -> Option<(String, SubscriptionKey)> {
    let clean = |value: &Option<String>| {
        value
            .as_deref()
            .map(|v| v.trim().to_lowercase())
            .filter(|v| !v.is_empty())
    };
    let key = match clean(&account.account_uuid) {
        Some(uuid) => SubscriptionKey::Uuid(uuid),
        None => SubscriptionKey::Email(clean(&account.email)?),
    };
    Some((account.provider.clone(), key))
}

/// Whether two accounts are two views of one allowance.
///
/// Derived on every call rather than stored in a column: the adopted row is
/// read with the ambient environment, so signing `~/.claude` into a different
/// account changes the answer without anything in this product being told.
///
/// A uuid on **both** sides is the whole answer, including when it says *no*.
/// Two directories on one subscription always agree on it, so differing uuids
/// mean different accounts however the e-mails read — which is the case that
/// matters when one row's e-mail is stale and the other's is current.
pub fn same_subscription(a: &Account, b: &Account) -> bool {
    if a.provider != b.provider {
        return false;
    }
    match (subscription_key(a), subscription_key(b)) {
        (Some((_, x)), Some((_, y))) => match (&x, &y) {
            (SubscriptionKey::Uuid(_), SubscriptionKey::Uuid(_)) => x == y,
            // One side has a uuid and the other does not, or neither does:
            // e-mail is the only shared ground there is.
            _ => match (
                a.email.as_deref().map(str::trim).filter(|v| !v.is_empty()),
                b.email.as_deref().map(str::trim).filter(|v| !v.is_empty()),
            ) {
                (Some(x), Some(y)) => x.eq_ignore_ascii_case(y),
                _ => false,
            },
        },
        _ => false,
    }
}

/// Every account on the same subscription as this one.
pub fn twins_of<'a>(account: &Account, all: &'a [Account]) -> Vec<&'a Account> {
    all.iter()
        .filter(|other| other.id != account.id && same_subscription(account, other))
        .collect()
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// Who a configuration directory is signed in as.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Identity {
    pub signed_in: bool,
    pub email: Option<String>,
    /// `oauthAccount.accountUuid`, where the provider writes one.
    pub account_uuid: Option<String>,
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
        // `auth status` does not publish it; `read_identity` fills it in from
        // the config file, and only when the two agree on the e-mail.
        account_uuid: None,
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
        // Codex publishes no stable account identifier of its own, so its
        // accounts are compared by e-mail alone. Absent stays unknown, never
        // "same as the other nameless one" — see `same_subscription`.
        account_uuid: None,
        org_id: text(auth, "organization_id"),
        org_name: None,
        plan: text(auth, "chatgpt_plan_type"),
    })
}

/// Where Claude Code keeps the non-secret half of an account's config.
///
/// **The adopted account's file is not inside its config directory.** With
/// `CLAUDE_CONFIG_DIR` set, `.claude.json` travels with the directory; with it
/// unset — which is exactly how the adopted account is run, deliberately, see
/// `session_env` — the file stays at `$HOME/.claude.json` and the one inside
/// `~/.claude` is a small unrelated stub with no `oauthAccount` in it. Measured
/// on this machine: 87 KB with the account against 343 bytes without.
///
/// Joining `config_dir` unconditionally would therefore read the stub, find no
/// uuid, fall back to the e-mail, and quietly reinstate the very bug this
/// exists to close.
pub fn claude_identity_file(config_dir: &Path, adopted: bool) -> Option<PathBuf> {
    if adopted {
        Some(home()?.join(".claude.json"))
    } else {
        Some(config_dir.join(".claude.json"))
    }
}

/// Read `oauthAccount` out of a Claude Code config file.
///
/// Identity, not credentials: this file holds the account's e-mail, uuid and
/// organisation beside a large amount of unrelated UI state, and no token. The
/// tokens live in `.credentials.json`, which this product never opens (§60/§61).
pub fn parse_claude_oauth_account(json: &str) -> Option<(Option<String>, Option<String>)> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let account = value.get("oauthAccount").filter(|v| !v.is_null())?;
    let text = |key: &str| {
        account
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    Some((text("accountUuid"), text("emailAddress")))
}

/// Whether this account's stored identity disagrees with what is on disk.
///
/// This is the gate in front of every identity read, and it decides whether the
/// cheap path stays cheap. It answers by **comparing the identity itself**, not
/// by looking at modification times.
///
/// The mtime version of this was written first and was wrong in a way that only
/// the real machine shows. Claude Code rewrites `.claude.json` roughly every ten
/// minutes while a session is running — the `.claude.json.backup.*` files in a
/// live account directory are stamped exactly 600 seconds apart — and it
/// rewrites `.credentials.json` on every token refresh. Neither is an identity
/// change. Keyed on mtime, the gate would open on essentially every panel paint
/// and on every mutation the surface makes (`load("cached")` runs after rename,
/// pause, remove, activate), spawning a CLI per account each time. Pausing an
/// account would freeze the window.
///
/// Reading the config file and comparing `oauthAccount` costs a file read and a
/// JSON parse, runs entirely in this process, and is *exact*: it is true when
/// the account really has changed and false when a file was merely touched.
/// `claude auth status` stays the authority on `signed_in` and the
/// organisation — this only decides whether it is worth asking.
pub fn identity_is_stale(account: &Account) -> bool {
    if account.checked_at.is_none() {
        return true;
    }
    let dir = Path::new(&account.config_dir);
    match account.provider.as_str() {
        "claude-code" => {
            // A directory the row believes is signed in, with no credentials
            // beside it, has been signed out somewhere else.
            if account.signed_in && !dir.join(".credentials.json").exists() {
                return true;
            }
            let Some(file) = claude_identity_file(dir, account.adopted) else {
                return false;
            };
            let Ok(text) = std::fs::read_to_string(file) else {
                // No config to compare against. Nothing here contradicts the
                // row, and asking a CLI on every paint to be told the same is
                // the cost this gate exists to avoid.
                return false;
            };
            match parse_claude_oauth_account(&text) {
                Some((uuid, email)) => !claude_identity_matches(account, &uuid, &email),
                // The file names nobody. That contradicts a row that claims an
                // identity, and agrees with one that does not.
                None => account.signed_in,
            }
        }
        "codex" => match std::fs::read_to_string(dir.join("auth.json"))
            .ok()
            .and_then(|text| parse_codex_identity(&text))
        {
            Some(identity) => {
                identity.signed_in != account.signed_in
                    || !same_text(&identity.email, &account.email)
            }
            None => account.signed_in,
        },
        _ => false,
    }
}

/// Whether the row already says what the config file says.
///
/// The uuid decides when both sides have one; otherwise the e-mail does. A
/// backfill — a row that had an e-mail and now gains the uuid for the same
/// account — must read as a *match*, or every existing installation would be
/// told its directory changed hands on the first launch after upgrading.
fn claude_identity_matches(
    account: &Account,
    uuid: &Option<String>,
    email: &Option<String>,
) -> bool {
    if !same_text(email, &account.email) {
        return false;
    }
    match (uuid, &account.account_uuid) {
        (Some(disk), Some(stored)) => disk.eq_ignore_ascii_case(stored),
        // The uuid is present on disk and missing from the row: the e-mails
        // agree, so this is the same account with a column to fill in, and a
        // refresh is worth one CLI start exactly once.
        (Some(_), None) => false,
        _ => true,
    }
}

/// Case-insensitive equality where absent equals absent.
fn same_text(a: &Option<String>, b: &Option<String>) -> bool {
    let clean = |v: &Option<String>| {
        v.as_deref()
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
    };
    clean(a) == clean(b)
}

/// Ask a configuration directory who it is signed in as.
///
/// Never fails a caller: an unknown identity is `None`, which the surface shows
/// as "not checked" rather than as "signed out". Guessing the difference is
/// exactly what §28 forbids one level up.
///
/// Two sources for Claude Code, and the order matters. `claude auth status`
/// is authoritative for *whether* the directory is signed in and for the
/// organisation, because it is the provider answering a question rather than a
/// file that may predate a logout. The uuid comes from the config file, which
/// is the only place it is published — and is accepted **only when the two
/// agree on the e-mail**, so a config file left behind by a previous account
/// can never attach the wrong subscription to a directory.
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
            let mut identity = parse_claude_identity(&out)?;
            identity.account_uuid = claude_account_uuid(config_dir, adopted, &identity);
            Some(identity)
        }
        "codex" => {
            let text = std::fs::read_to_string(config_dir.join("auth.json")).ok()?;
            parse_codex_identity(&text)
        }
        _ => None,
    }
}

/// The uuid for a directory, when the config file describes the same account
/// the provider just named.
fn claude_account_uuid(config_dir: &Path, adopted: bool, identity: &Identity) -> Option<String> {
    if !identity.signed_in {
        return None;
    }
    let file = claude_identity_file(config_dir, adopted)?;
    let text = std::fs::read_to_string(file).ok()?;
    let (uuid, email) = parse_claude_oauth_account(&text)?;
    match (&identity.email, &email) {
        // Both named an e-mail and they differ: the file is describing somebody
        // else, so its uuid is not this directory's.
        (Some(status), Some(config)) if !status.eq_ignore_ascii_case(config) => None,
        _ => uuid,
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
        account_uuid: row.get("account_uuid")?,
        org_id: row.get("org_id")?,
        org_name: row.get("org_name")?,
        plan: row.get("plan")?,
        signed_in: row.get::<_, i64>("signed_in")? != 0,
        checked_at: row.get("checked_at")?,
        identity_attempted_at: row.get("identity_attempted_at")?,
        subscription_since: row.get("subscription_since")?,
        active: row.get::<_, i64>("active")? != 0,
        paused: row.get::<_, i64>("paused")? != 0,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        last_used_at: row.get("last_used_at")?,
    })
}

const SELECT: &str = "SELECT id, provider, label, config_dir, adopted, email, account_uuid,
                             org_id, org_name, plan, signed_in, checked_at,
                             identity_attempted_at, subscription_since, active, paused, position,
                             created_at, last_used_at
                      FROM provider_accounts";

pub fn list(db: &Database) -> Result<Vec<Account>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY provider, position"))?;
        let rows: rusqlite::Result<Vec<Account>> = stmt.query_map([], row_to_account)?.collect();
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

    let observed_identity = read_identity(provider, &dir, true);
    let identity = observed_identity.clone().unwrap_or_default();
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
                 (id, provider, label, config_dir, adopted, email, account_uuid, org_id, org_name,
                  plan, signed_in, checked_at, identity_attempted_at, subscription_since,
                  active, paused, position, created_at)
             VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 0, ?15, ?16)",
            params![
                id,
                provider,
                label,
                dir_text,
                identity.email,
                identity.account_uuid,
                identity.org_id,
                identity.org_name,
                identity.plan,
                identity.signed_in as i64,
                observed_identity.as_ref().map(|_| ts),
                ts,
                // The machine account is adopted, not created: whatever it has
                // already spent this week was spent on the account it is signed
                // into now, and hiding that history would understate the very
                // window the person opened the screen to read.
                0,
                (count == 0) as i64,
                count,
                ts,
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
                  position, created_at, subscription_since)
             VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, 0, ?6, ?7, ?7)",
            params![
                id,
                provider,
                label,
                dir_text,
                (count == 0) as i64,
                count,
                ts
            ],
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
    let outcome = db
        .with(|conn| {
            let Some((provider, was_active)) = conn
                .query_row(
                    "SELECT provider, active FROM provider_accounts WHERE id = ?1",
                    [id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
                )
                .optional()?
            else {
                return Ok(Ok(()));
            };

            let replacement = if paused && was_active {
                conn.query_row(
                    "SELECT id FROM provider_accounts
                 WHERE provider = ?1 AND id <> ?2 AND signed_in = 1 AND paused = 0
                 ORDER BY position LIMIT 1",
                    params![provider, id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
            } else {
                None
            };
            if paused && was_active && replacement.is_none() {
                return Ok(Err("accounts.lastAvailableCannotPause".to_string()));
            }

            let tx = conn.unchecked_transaction()?;
            tx.execute(
                "UPDATE provider_accounts SET paused = ?2 WHERE id = ?1",
                params![id, paused as i64],
            )?;
            if let Some(replacement) = replacement {
                tx.execute(
                    "UPDATE provider_accounts SET active = (id = ?2)
                 WHERE provider = ?1",
                    params![provider, replacement],
                )?;
            }
            tx.commit()?;
            Ok(Ok(()))
        })
        .map_err(|e| e.to_string())?;
    outcome
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
///
/// The attempt is stamped whether or not it succeeds. Before, a failed read
/// returned early having written nothing, so a card kept showing an identity
/// from hours ago with no way to know it had not been confirmed since; on this
/// machine the adopted account read `alanvitoraraujo1@icloud.com` for eleven
/// hours after the directory had been signed into a different account, which is
/// what let two cards on one subscription look like two subscriptions.
pub fn refresh_identity(db: &Database, id: &str) -> Result<Option<Account>> {
    let Some(account) = get(db, id)? else {
        return Ok(None);
    };
    let ts = now_ms();
    let observed = read_identity(
        &account.provider,
        Path::new(&account.config_dir),
        account.adopted,
    );

    let Some(identity) = observed else {
        // The question could not be put — the CLI is missing, mid-upgrade, or
        // was interrupted. Record that we tried and leave the last known
        // identity alone; the surface says when it was last confirmed.
        db.with(|conn| {
            conn.execute(
                "UPDATE provider_accounts SET identity_attempted_at = ?2 WHERE id = ?1",
                params![id, ts],
            )?;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
        return get(db, id);
    };

    // A provider that has stopped naming the account has not renamed it.
    //
    // Codex 0.149.1 stopped writing `id_token_claims`, so `read_identity`
    // returns no e-mail for a directory that is plainly signed in and whose
    // e-mail this product already knows. Overwriting the known value with
    // nothing loses the only key a Codex account can be compared by — and, in
    // the version of this function before the guard below, it also read as
    // "this directory now belongs to somebody else" and wiped the account's
    // quota history. Caught by running the diagnostic against the real
    // registry, where the Codex card lost its address on every refresh.
    //
    // So an absent field is treated the way it is treated everywhere else in
    // this module: unknown, not a statement. A directory the provider says is
    // signed *out* is a statement, and clears both.
    let keep = |fresh: Option<String>, known: &Option<String>| -> Option<String> {
        match (fresh, identity.signed_in) {
            (Some(value), _) => Some(value),
            (None, true) => known.clone(),
            (None, false) => None,
        }
    };
    let email = keep(identity.email.clone(), &account.email);
    let account_uuid = keep(identity.account_uuid.clone(), &account.account_uuid);

    // Has the directory changed hands? Compare on the same rule the surface
    // groups by, so "this is a different subscription now" and "these two cards
    // are one subscription" can never disagree.
    let before = account.clone();
    let after = Account {
        email: email.clone(),
        account_uuid: account_uuid.clone(),
        ..account.clone()
    };
    // **Both sides must be known.** `changed` moves `subscription_since` and
    // drops the cached reading, which is irreversible for the history it
    // excludes, so it may only fire on a positive statement that this is a
    // different account — never on the absence of one.
    let changed = subscription_key(&before).is_some()
        && subscription_key(&after).is_some()
        && !same_subscription(&before, &after);
    let had_identity = subscription_key(&before).is_some();

    db.with(|conn| {
        conn.execute(
            "UPDATE provider_accounts
                SET email = ?2, account_uuid = ?3, org_id = ?4, org_name = ?5, plan = ?6,
                    signed_in = ?7, checked_at = ?8, identity_attempted_at = ?8
              WHERE id = ?1",
            params![
                id,
                email,
                account_uuid,
                identity.org_id,
                identity.org_name,
                identity.plan,
                identity.signed_in as i64,
                ts,
            ],
        )?;
        // An account the user never named takes the identity's own name once
        // one arrives. A label they *did* choose is theirs and is left alone.
        if let Some(email) = email.as_deref() {
            conn.execute(
                "UPDATE provider_accounts SET label = ?2 WHERE id = ?1 AND label = ''",
                params![id, email],
            )?;
            // A label that is just the *previous* identity's address was never
            // a name anybody chose — it was filled in by the branch above — and
            // leaving it in place after the directory changes hands is how a
            // card ends up titled `alanvitoraraujo1@icloud.com` with a
            // different address printed underneath it. Found exactly that way.
            // A label the person typed does not match this test and survives.
            if let Some(previous) = before.email.as_deref() {
                if before.label.eq_ignore_ascii_case(previous.trim()) {
                    conn.execute(
                        "UPDATE provider_accounts SET label = ?2 WHERE id = ?1",
                        params![id, email],
                    )?;
                }
            }
        }
        if changed {
            // Both halves of "this directory changed hands" or neither: a
            // moved boundary with the old reading still cached, or a dropped
            // reading with the old history still counted, are each worse than
            // the state before.
            let tx = conn.unchecked_transaction()?;
            // Everything recorded against this row until now was somebody
            // else's allowance. Drawing this account's window over it would
            // report one person's spend as another's — the "it merged my two
            // accounts" failure, arriving through the back door.
            tx.execute(
                "UPDATE provider_accounts SET subscription_since = ?2 WHERE id = ?1",
                params![id, ts],
            )?;
            // The cached reading describes the previous account. Dropping it
            // leaves the card honestly blank until the next probe rather than
            // confidently wrong.
            tx.execute(
                "DELETE FROM account_live_readings WHERE account_id = ?1",
                params![id],
            )?;
            tx.commit()?;
        }
        // A directory that has never had an identity starts its history here,
        // so a row adopted before this column existed does not inherit "0".
        if !had_identity && identity.signed_in {
            conn.execute(
                "UPDATE provider_accounts SET subscription_since = ?2
                  WHERE id = ?1 AND subscription_since = 0",
                params![id, ts],
            )?;
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;

    if changed {
        tracing::info!(
            account = %id,
            "provider account directory is signed into a different subscription; \
             its earlier quota history no longer counts towards it"
        );
    }

    get(db, id)
}

/// Re-read identity only when something on disk says it may have changed.
///
/// This is the one that runs on every report. A login done outside this product
/// writes `.credentials.json` and `.claude.json`; comparing their timestamps to
/// `checked_at` turns "we find out when the person presses Check now" into "we
/// find out the next time they look at the screen", for the cost of a `stat`.
pub fn refresh_identity_if_stale(db: &Database, account: &Account) -> Result<Option<Account>> {
    if identity_is_stale(account) {
        refresh_identity(db, &account.id)
    } else {
        Ok(Some(account.clone()))
    }
}

/// Re-read every account of one provider.
///
/// Used after a sign-in, and it is deliberately *every* account rather than the
/// one that signed in. The interesting outcome of a login is often about a
/// different row: signing directory B into the account directory A already
/// holds is invisible from B alone, and is the single most likely way to end up
/// with two cards drawing one allowance.
pub fn refresh_provider_identities(db: &Database, provider: &str) {
    let Ok(accounts) = list(db) else { return };
    for account in accounts.iter().filter(|a| a.provider == provider) {
        let _ = refresh_identity(db, &account.id);
    }
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
