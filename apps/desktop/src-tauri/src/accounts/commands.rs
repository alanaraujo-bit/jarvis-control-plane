//! Tauri boundary for provider accounts (§66).

use serde::Serialize;
use tauri::State;

use crate::AppState;

use super::quota::AccountQuota;
use super::switch::AutoSwitchPolicy;
use super::{Account, Result};

/// One account card. Trust is scoped to the project currently being viewed;
/// `None` means either not Claude Code, no project was supplied, or the
/// provider's configuration could not be understood.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountCard {
    pub account: Account,
    pub quota: AccountQuota,
    pub folder_trusted: Option<bool>,
    /// Ids of the other accounts drawing on this same subscription.
    ///
    /// Computed here rather than in the surface so there is exactly one rule for
    /// "is this the same allowance" — the core uses it to refuse a pointless
    /// rotation, the card uses it to say so out loud, and two implementations
    /// of it would eventually disagree about the very thing the card is
    /// asserting. Empty for the ordinary case of one directory per account.
    pub shared_with: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsReport {
    pub accounts: Vec<AccountCard>,
    pub auto_switch: AutoSwitchPolicy,
    pub threshold_percent: f64,
}

fn project_path(state: &AppState, project_id: Option<&str>) -> Option<String> {
    let id = project_id?;
    state
        .db
        .with(|conn| {
            conn.query_row("SELECT path FROM projects WHERE id = ?1", [id], |row| {
                row.get(0)
            })
        })
        .ok()
}

fn report(state: &AppState, project_id: Option<&str>) -> Result<AccountsReport> {
    let project = project_path(state, project_id);

    // Re-read any identity the filesystem says may have moved, before anything
    // is computed from it.
    //
    // This is a `stat` per account in the ordinary case and nothing else. It is
    // here, on the cheap path, because the expensive path was the only one that
    // ever refreshed identity — so a login performed anywhere but inside this
    // product stayed invisible until somebody happened to press "Check now".
    // That is not a hypothetical: it is how one account on this machine went on
    // being displayed under a previous owner's e-mail for eleven hours, which
    // in turn hid the fact that two cards were drawing on one subscription.
    for account in super::list(&state.db)? {
        let _ = super::refresh_identity_if_stale(&state.db, &account);
    }

    let all = super::list(&state.db)?;
    let accounts = all
        .iter()
        .cloned()
        .map(|account| {
            let shared_with = super::twins_of(&account, &all)
                .into_iter()
                .map(|twin| twin.id.clone())
                .collect();
            let folder_trusted = project.as_deref().and_then(|cwd| {
                (account.provider == "claude-code")
                    .then(|| {
                        crate::providers::claude::folder_is_trusted_in(
                            std::path::Path::new(&account.config_dir),
                            account.adopted,
                            std::path::Path::new(cwd),
                        )
                    })
                    .flatten()
            });
            let quota = super::quota::for_account(&state.db, &account);
            AccountCard {
                account,
                quota,
                folder_trusted,
                shared_with,
            }
        })
        .collect();

    Ok(AccountsReport {
        accounts,
        auto_switch: super::switch::policy(&state.db),
        threshold_percent: super::quota::NEARING_PERCENT,
    })
}

/// The cheap read: stored quota, plus an identity check that costs a file read.
///
/// `async` so Tauri runs it off the main thread. The identity gate is exact
/// rather than timestamp-based (see `accounts::identity_is_stale`), so it
/// almost never starts a CLI — but "almost never" is not "never", and this is
/// the command behind every paint and every mutation the surface makes. The one
/// time it *does* spawn is a login that just happened somewhere else, which is
/// precisely when the window must not freeze.
#[tauri::command]
pub async fn accounts_report(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<AccountsReport> {
    report(&state, project_id.as_deref())
}

/// Re-check identity **and** ask both providers for live quota.
///
/// Deliberately slow and deliberately explicit. A probe is a CLI startup per
/// account — a second or two each, run in parallel — which is fine behind a
/// spinner the person asked for and far too slow to sit in front of a panel
/// opening. `accounts_report` returns the stored reading instantly; this is
/// what replaces it.
///
/// `async` so Tauri runs it off the main thread: the same work on the command
/// thread would freeze the window for as long as the slowest CLI takes to
/// start, which is exactly the kind of thing that only shows up when you run
/// the real app and look at it.
/// Whether a refresh should also re-interrogate who each directory is.
///
/// It doubles the process spawns — `claude auth status` per account on top of
/// the probe — for a fact that changes when somebody signs in or out and at no
/// other time. Worth paying when a person presses "Check now" or a login just
/// finished; pure waste on the five-minute background tick behind the status
/// bar, which is the one that runs all day.
#[tauri::command]
pub async fn accounts_refresh(
    state: State<'_, AppState>,
    project_id: Option<String>,
    identity: Option<bool>,
) -> Result<AccountsReport> {
    let accounts = super::list(&state.db)?;
    if identity.unwrap_or(true) {
        for account in &accounts {
            let _ = super::refresh_identity(&state.db, &account.id);
        }
    }
    // Identity may have changed above — probe against the rows as they are now,
    // so an account that just finished signing in is asked rather than skipped.
    let accounts = super::list(&state.db)?;
    super::live::refresh_all(&state.db, &accounts);
    // The exhausted CLI may stop producing transcript lines, so a provider's
    // official live reading must be allowed to trigger the configured policy.
    // This changes only the destination for new work. A driven session still
    // performs its context-aware relay in the autopilot path.
    for account in accounts.iter().filter(|account| account.active) {
        let _ = super::switch::maybe_rotate_recorded(&state.db, &account.id);
    }
    let report = report(&state, project_id.as_deref())?;
    crate::identity::cloud::push_quota(&state.db, &report);
    Ok(report)
}

/// Ask one account's provider for its quota, now.
///
/// Exists so a single card can be retried without paying for every other
/// account's CLI startup — the case where one directory is mid-login and the
/// rest are fine.
#[tauri::command]
pub async fn account_refresh_live(
    state: State<'_, AppState>,
    account_id: String,
    project_id: Option<String>,
) -> Result<AccountsReport> {
    let _ = super::refresh_identity(&state.db, &account_id);
    if let Some(account) = super::get(&state.db, &account_id)? {
        super::live::refresh_all(&state.db, std::slice::from_ref(&account));
        if account.active {
            let _ = super::switch::maybe_rotate_recorded(&state.db, &account.id);
        }
    }
    let report = report(&state, project_id.as_deref())?;
    crate::identity::cloud::push_quota(&state.db, &report);
    Ok(report)
}

#[tauri::command]
pub fn account_create(
    state: State<'_, AppState>,
    provider: String,
    label: String,
) -> Result<Account> {
    super::create(&state.db, &provider, &state.data_dir, &label)
}

#[tauri::command]
pub fn account_rename(state: State<'_, AppState>, account_id: String, label: String) -> Result<()> {
    super::rename(&state.db, &account_id, &label)
}

#[tauri::command]
pub fn account_set_paused(
    state: State<'_, AppState>,
    account_id: String,
    paused: bool,
) -> Result<()> {
    super::set_paused(&state.db, &account_id, paused)
}

#[tauri::command]
pub fn account_remove(state: State<'_, AppState>, account_id: String) -> Result<()> {
    super::remove(&state.db, &account_id)
}

#[tauri::command]
pub fn account_set_active(state: State<'_, AppState>, account_id: String) -> Result<Account> {
    super::switch::set_active(&state.db, &account_id)
}

#[tauri::command]
pub fn account_set_auto_switch(state: State<'_, AppState>, policy: AutoSwitchPolicy) -> Result<()> {
    super::switch::set_policy(&state.db, policy)
}

/// Start the provider-owned OAuth flow and return immediately. The spawned CLI
/// owns browser login; J.A.R.V.I.S. never receives a password or token.
///
/// Two things happen here that did not before, and both exist because of one
/// measured fact: **`claude auth login` in an empty configuration directory
/// signs into whatever account the browser is already holding, and says "Login
/// successful" in about a second without asking anything.** Adding a second
/// account the obvious way therefore adds a second directory on the *first*
/// account. Nothing in this product could prevent that — the browser session is
/// not ours — so it does the two things it can:
///
/// 1. **The authorisation URL is captured and handed to the surface.** The CLI
///    prints it on stdout beside opening a browser itself. With the link in
///    hand a person can open it in a private window, where no claude.ai session
///    exists and the account chooser actually appears. This is the only route
///    to a genuinely different account that does not involve signing out of the
///    browser entirely.
/// 2. **Every account on the provider is re-read when the login finishes**, not
///    just this one. The interesting outcome is frequently about a different
///    row — the collision is only visible by comparing them — and the surface
///    is told, so the card can say "this is the same subscription as X" the
///    moment it becomes true rather than at the next manual check.
#[tauri::command]
pub fn account_begin_sign_in(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    email: Option<String>,
) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use tauri::Emitter;

    let account =
        super::get(&state.db, &account_id)?.ok_or_else(|| "accounts.notFound".to_string())?;
    let provider = account.provider.clone();
    let mut command = super::switch::sign_in_command(&account, email.as_deref())?;
    command.stdout(std::process::Stdio::piped());
    // **Stdin is piped and kept.** Measured on this machine: with the browser
    // prevented from completing the flow, `claude auth login` prints the
    // authorisation URL, prints `Paste code here if prompted > `, and then sits
    // waiting — still running after twelve seconds. It is a paste-the-code
    // flow, not a localhost callback: `redirect_uri` is
    // `platform.claude.com/oauth/code/callback`.
    //
    // So the private-window route this product recommends *cannot complete*
    // unless the code can be handed back. A windowed process has no console
    // stdin, and an inherited one would leave the child blocked for ever on a
    // prompt nobody can answer. The handle is parked in `PENDING_SIGN_INS` and
    // `account_submit_sign_in_code` writes into it.
    command.stdin(std::process::Stdio::piped());
    let db = std::sync::Arc::clone(&state.db);

    std::thread::Builder::new()
        .name(format!("account-login-{account_id}"))
        .spawn(move || {
            let mut child = match command.spawn() {
                Ok(child) => child,
                Err(error) => {
                    tracing::warn!(%error, %account_id, "could not start provider login");
                    let _ = app.emit(
                        SIGN_IN_EVENT,
                        SignInEvent {
                            account_id: account_id.clone(),
                            phase: "failed",
                            url: None,
                        },
                    );
                    return;
                }
            };

            if let Some(stdin) = child.stdin.take() {
                if let Ok(mut pending) = PENDING_SIGN_INS.lock() {
                    pending.insert(account_id.clone(), stdin);
                }
            }

            // Read stdout for the authorisation link. The CLI keeps printing
            // after it — "Paste code here if prompted", "Login successful" —
            // and the loop drains all of it, both to find the URL and because a
            // full pipe would block the child on a login the person is still
            // completing in a browser.
            if let Some(stdout) = child.stdout.take() {
                let mut announced = false;
                for line in BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
                    if announced {
                        continue;
                    }
                    if let Some(url) = super::switch::authorize_url(&line) {
                        announced = true;
                        let _ = app.emit(
                            SIGN_IN_EVENT,
                            SignInEvent {
                                account_id: account_id.clone(),
                                phase: "url",
                                url: Some(url),
                            },
                        );
                    }
                }
            }

            let _ = child.wait();
            if let Ok(mut pending) = PENDING_SIGN_INS.lock() {
                pending.remove(&account_id);
            }
            super::refresh_provider_identities(&db, &provider);
            let _ = app.emit(
                SIGN_IN_EVENT,
                SignInEvent {
                    account_id: account_id.clone(),
                    phase: "finished",
                    url: None,
                },
            );
        })
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Logins waiting for the code the person pastes back from the browser.
///
/// One entry per account, dropped when the CLI exits. A `Mutex` around a small
/// map rather than something in `AppState`: nothing else needs it, and the
/// lifetime is a single login rather than the application's.
static PENDING_SIGN_INS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, std::process::ChildStdin>>,
> = std::sync::LazyLock::new(Default::default);

/// Hand the provider the code the person copied out of the browser.
///
/// The other half of the private-window route. Without it the recommendation to
/// open the link where no session exists leads to a CLI blocked for ever on a
/// prompt with nobody at the keyboard.
#[tauri::command]
pub fn account_submit_sign_in_code(account_id: String, code: String) -> Result<()> {
    use std::io::Write;

    let code = code.trim().to_string();
    if code.is_empty() {
        return Err("accounts.signInCodeEmpty".into());
    }
    let mut pending = PENDING_SIGN_INS
        .lock()
        .map_err(|_| "accounts.signInUnavailable".to_string())?;
    let stdin = pending
        .get_mut(&account_id)
        .ok_or_else(|| "accounts.signInNotWaiting".to_string())?;
    stdin
        .write_all(code.as_bytes())
        .and_then(|()| stdin.write_all(b"\n"))
        .and_then(|()| stdin.flush())
        .map_err(|_| "accounts.signInNotWaiting".to_string())
}

/// What the surface hears while a provider login is running.
pub const SIGN_IN_EVENT: &str = "accounts:signIn";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignInEvent {
    account_id: String,
    /// `url` — the authorisation link is available; `finished` — the CLI
    /// exited and identities have been re-read; `failed` — it never started.
    phase: &'static str,
    url: Option<String>,
}

/// Sign one configuration directory out of its provider account.
///
/// The counterpart to discovering that two directories hold one subscription:
/// signing this one out is the first half of putting a *different* account in
/// it, and doing it from here means the person does not have to find the right
/// terminal incantation with the right `CLAUDE_CONFIG_DIR` to undo a mistake
/// the product's own button led them into.
///
/// The adopted directory is refused. It is the machine's own login — very
/// possibly the session the person is working in right now — and signing it out
/// from a panel about quota would log them out of the thing they are sitting in
/// front of. It is the same reasoning that made an account a directory in the
/// first place.
#[tauri::command]
pub async fn account_sign_out(state: State<'_, AppState>, account_id: String) -> Result<()> {
    let account =
        super::get(&state.db, &account_id)?.ok_or_else(|| "accounts.notFound".to_string())?;
    if account.adopted {
        return Err("accounts.adoptedCannotSignOut".into());
    }
    let mut command = super::switch::sign_out_command(&account)?;
    // Waiting on a process is blocking work; on the async runtime it would tie
    // up a worker for as long as the CLI takes to start.
    tauri::async_runtime::spawn_blocking(move || command.status())
        .await
        .map_err(|_| "accounts.signOutFailed".to_string())?
        .map_err(|error| format!("accounts.signOutFailed: {error}"))?;
    let _ = super::refresh_identity(&state.db, &account_id);
    Ok(())
}
