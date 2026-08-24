//! Asking a provider, right now, how much allowance an account has left (§66, §28).
//!
//! ## Why this module exists at all
//!
//! M13 established — by reading all 115 transcripts on this machine — that
//! Claude Code states quota **only in the turn it refuses**. That finding was
//! true and it still is. It was also incomplete, and the gap is what made the
//! finished panel unusable: with no refusal on record an account showed
//! "allowance unknown" for every window, which is honest and useless. The
//! person then does the thing the product exists to prevent — opens the web UI
//! and checks by hand.
//!
//! Both providers do expose a live, official, on-demand reading. Neither of
//! them does it over an HTTP endpoint we would have to remember; both do it
//! through their own supported CLI protocol, which is why this is evidence
//! rather than recall. Every claim below was measured against the real binaries
//! on 2026-08-24 and is written up in `docs/M16-QUOTA.md`.
//!
//! * **Claude Code 2.1.241** answers a `control_request` of subtype
//!   `get_usage` on its stream-json stdio protocol. Its own description of the
//!   request ends "Experimental — the response shape may change", which is why
//!   every field here is read defensively and an unrecognised window renders
//!   rather than disappearing.
//! * **Codex 0.149.1** answers the JSON-RPC method `account/rateLimits/read`
//!   on `codex app-server`.
//!
//! ## The property that made the feature worth building
//!
//! Both probes read the account in the **configuration directory they are
//! pointed at**, not the ambient one. Measured with an empty directory:
//! Claude returns `rate_limits_available: false` and Codex returns JSON-RPC
//! `-32600 "codex account authentication required"`. Neither invents plausible
//! numbers belonging to another account — which was the real risk, because
//! wrong numbers under the right name are worse than no numbers at all.
//!
//! That is what lets four accounts each show their own live figure, which is
//! the whole of what was asked for.
//!
//! ## Three things this module refuses to do
//!
//! 1. **It never reads a credential.** It runs a CLI with one environment
//!    variable set and parses stdout. §60/§61 hold exactly as before.
//! 2. **It never spends a token.** Neither probe starts a turn; the measured
//!    `total_cost_usd` of a probe session is 0 and no transcript is written, so
//!    Session History and the §51 index stay clean.
//! 3. **It never consumes anything.** Codex reports free rate-limit reset
//!    credits; this module shows the count and never calls
//!    `account/rateLimitResetCredit/consume`, which is irreversible and belongs
//!    to a human with a finger on a button.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::db::Database;

use super::Account;

/// How long a probe may take before it is given up on and killed.
///
/// Generous because both CLIs do real startup work — plugin discovery, config
/// parsing — before they answer, and a cold first run on a slow disk is not a
/// failure. The surface never waits on this: it renders the stored reading
/// first and replaces it when the probe lands.
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);

/// How stale a stored reading may be before the surface asks for a new one.
const STALE_AFTER_MS: i64 = 90_000;

// ---------------------------------------------------------------------------
// The shape the surface renders
// ---------------------------------------------------------------------------

/// One allowance window as the provider stated it, just now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveWindow {
    /// Canonical id the surface localises: `session`, `weekly`, `weeklyOpus`,
    /// `weeklySonnet`, or — for anything this build has not seen — the
    /// provider's own spelling, rendered with its scope label instead of a
    /// translated name. Claude Code ships rotating codenames (`cinder_cove`,
    /// `nimbus_quill`, `tangelo`), so an unknown kind is an expected state, not
    /// an error, and dropping it would hide a window that is actually binding.
    pub kind: String,
    /// Exactly what the provider called it, kept for support and for the
    /// tooltip. Never translated.
    pub raw_kind: String,
    /// `session`, `weekly`, `monthly` or `other`. Drives grouping only.
    pub group: String,
    /// A provider-supplied label, such as the model bucket a weekly window is
    /// scoped to. Provider prose, shown verbatim, never localised.
    pub scope_label: Option<String>,
    /// 0–100 **used**. One direction, chosen once: the provider states used, so
    /// used is what is stored and the surface derives "remaining" from it. Two
    /// directions in the store is how a bar and its label end up disagreeing.
    pub percent_used: f64,
    pub resets_at_ms: Option<i64>,
    pub window_minutes: Option<i64>,
    /// True for the window that is actually rationing this account — the answer
    /// to "which quota am I waiting on".
    pub binding: bool,
    /// `provider` when the provider named the binding window itself (Claude
    /// Code's `is_active`), `derived` when we picked the fullest window because
    /// the provider does not say (Codex). §28: the surface must be able to tell
    /// these apart, so it is stored rather than flattened.
    pub binding_source: String,
    /// `normal` | `warning` | `critical` | `exhausted`.
    pub severity: String,
    /// `provider` or `derived`, same reasoning as `binding_source`.
    pub severity_source: String,
}

impl LiveWindow {
    /// Whether the provider is refusing work on this window right now.
    ///
    /// A provider's own severity vocabulary is kept verbatim rather than
    /// rewritten — Claude Code's worst band is `critical`, not `exhausted` —
    /// so exhaustion is read off the number, which both providers state the
    /// same way, and off the word, in case a provider adds one.
    pub fn exhausted(&self) -> bool {
        self.severity == "exhausted" || self.percent_used >= 100.0
    }
}

/// Paid overage sitting above the subscription — the "monthly" allowance.
///
/// Present because the refusal M13 recorded said, in the provider's own words,
/// "You've hit your **monthly spend limit**". A panel that shows only the
/// five-hour and weekly windows cannot explain that sentence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveSpend {
    pub enabled: bool,
    pub used: f64,
    pub limit: f64,
    /// ISO currency the provider bills this account in — `BRL` on this machine.
    /// Formatting is the surface's job; the code is the fact.
    pub currency: String,
    pub decimal_places: i64,
    pub percent_used: Option<f64>,
    /// The provider's stable reason id when overage is off, e.g.
    /// `org_level_disabled_until`. An id, not a sentence, so it can be
    /// translated (§65).
    pub disabled_reason: Option<String>,
    pub limit_reached: bool,
}

/// One complete answer from a provider about one account.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LiveReading {
    pub read_at_ms: i64,
    /// Which protocol produced this: `claudeGetUsage` or `codexAppServer`.
    pub source: String,
    pub plan: Option<String>,
    pub windows: Vec<LiveWindow>,
    pub spend: Option<LiveSpend>,
    /// Free rate-limit resets the provider says are available. Shown, never
    /// spent: consuming one is irreversible and is a human's decision.
    pub reset_credits: i64,
    /// Who the provider says this configuration directory is signed in as.
    ///
    /// Only Codex answers this, and only because its app-server session that is
    /// already open for the limits can be asked in the same round trip. It is
    /// how a reading proves it belongs to the account whose name is above it —
    /// see the note on identity in this module's header.
    pub identity: Option<ProbedIdentity>,
}

/// Identity a provider volunteered while being asked about quota.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProbedIdentity {
    pub email: Option<String>,
    pub plan: Option<String>,
}

/// What a probe attempt produced.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum LiveStatus {
    /// The provider answered with numbers.
    Ok { reading: LiveReading },
    /// The provider answered, and the answer was "no live quota for this
    /// account" — signed out, or on a plan where subscription limits do not
    /// apply. A definite negative, not a failure.
    Unavailable { reason: String, read_at_ms: i64 },
    /// The probe could not be completed. A stable reason id, never the raw
    /// error text, because it is read by a person in their own language.
    Failed { reason: String, read_at_ms: i64 },
}

impl LiveStatus {
    pub fn read_at_ms(&self) -> i64 {
        match self {
            LiveStatus::Ok { reading } => reading.read_at_ms,
            LiveStatus::Unavailable { read_at_ms, .. } | LiveStatus::Failed { read_at_ms, .. } => {
                *read_at_ms
            }
        }
    }

    pub fn reading(&self) -> Option<&LiveReading> {
        match self {
            LiveStatus::Ok { reading } => Some(reading),
            _ => None,
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Time, which arrives in three different shapes
// ---------------------------------------------------------------------------

/// Milliseconds since the epoch from any reset stamp a provider hands us.
///
/// Three formats are live at once and mixing them up is trap #1 of M13 §5
/// wearing a new hat: reading unix **seconds** as milliseconds puts a reset in
/// 1970, which makes an exhausted account look permanently recovered and
/// silently disables the whole feature.
///
/// * Claude Code's live probe → ISO 8601 with an offset.
/// * Claude Code's transcripts and Codex → unix **seconds**, as a number.
/// * Everything inside this product → milliseconds.
pub fn reset_to_ms(value: &Value) -> Option<i64> {
    if let Some(text) = value.as_str() {
        return crate::providers::conversation::parse_timestamp(text);
    }
    // A float is accepted because a JSON number with a fractional second is
    // still a valid unix stamp, and `as_i64` alone would silently drop it.
    let seconds = value
        .as_i64()
        .or_else(|| value.as_f64().map(|f| f as i64))?;
    // A stamp small enough to be seconds is seconds. The boundary is far below
    // any plausible reset time in either unit and far above zero, so it cannot
    // misclassify a real value in either direction.
    Some(if seconds < 100_000_000_000 {
        seconds * 1000
    } else {
        seconds
    })
}

/// Severity from a used percentage, when the provider does not state one.
///
/// The bands match what the surface colours, and 100 is `exhausted` rather than
/// `critical` because a full window is not a warning — it is a refusal.
fn severity_from_percent(percent: f64) -> &'static str {
    if percent >= 100.0 {
        "exhausted"
    } else if percent >= 90.0 {
        "critical"
    } else if percent >= super::quota::NEARING_PERCENT {
        "warning"
    } else {
        "normal"
    }
}

/// Map a provider's window name onto the id the surface knows how to translate.
///
/// Anything unrecognised keeps its own spelling. That is deliberate: Claude
/// Code's window set includes codenames that rotate between releases, and a
/// panel that only draws the four it recognises would omit whichever new one is
/// actually rationing the account.
fn canonical_kind(raw: &str) -> (&'static str, String) {
    match raw {
        "session" | "five_hour" => ("session", "session".to_string()),
        "weekly_all" | "seven_day" | "weekly" => ("weekly", "weekly".to_string()),
        "weekly_opus" | "seven_day_opus" => ("weekly", "weeklyOpus".to_string()),
        "weekly_sonnet" | "seven_day_sonnet" => ("weekly", "weeklySonnet".to_string()),
        "monthly" | "extra_usage" => ("monthly", "monthly".to_string()),
        other => ("other", other.to_string()),
    }
}

// ---------------------------------------------------------------------------
// Running a probe
// ---------------------------------------------------------------------------

/// Drive a line-oriented stdio protocol to a single answer.
///
/// Writes `requests` to the child's stdin, then reads stdout line by line and
/// hands each to `wants`, stopping at the first line it accepts. The child is
/// always killed — a probe that has its answer has no reason to keep a CLI
/// alive, and one that timed out must not leak a process.
///
/// The read runs on its own thread so the timeout is real. `BufRead::lines` on
/// a child that never speaks blocks forever, and a probe that hangs would hang
/// the refresh that a person is watching a spinner for.
fn drive<T, F>(
    mut command: std::process::Command,
    requests: &[String],
    wants: F,
) -> std::result::Result<T, String>
where
    T: Send + 'static,
    F: Fn(&str) -> Option<T> + Send + 'static,
{
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::null());

    let mut child = command.spawn().map_err(|_| "spawnFailed".to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        for line in requests {
            // A closed pipe here is not fatal on its own: the child may have
            // already answered and exited. The read below decides the outcome.
            if stdin.write_all(line.as_bytes()).is_err() || stdin.write_all(b"\n").is_err() {
                break;
            }
            let _ = stdin.flush();
        }
        // Dropping stdin would close it, which Codex's app-server treats as a
        // shutdown. It is held until the answer arrives instead.
        let (tx, rx) = mpsc::channel::<T>();
        let stdout = child.stdout.take().ok_or_else(|| "noOutput".to_string())?;
        let reader = std::thread::Builder::new()
            .name("quota-probe".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if let Some(found) = wants(&line) {
                        let _ = tx.send(found);
                        return;
                    }
                }
            })
            .map_err(|_| "spawnFailed".to_string())?;

        let outcome = rx
            .recv_timeout(PROBE_TIMEOUT)
            .map_err(|_| "timeout".to_string());
        let _ = child.kill();
        let _ = child.wait();
        drop(stdin);
        let _ = reader.join();
        return outcome;
    }

    let _ = child.kill();
    let _ = child.wait();
    Err("noInput".to_string())
}

/// A command for one CLI, pointed at one account's configuration directory.
///
/// Routed through `envscan::tool_command` so the Windows `.cmd` shim handling
/// that took real debugging to get right is not reimplemented here, and through
/// the same single place the environment variable is applied — so a probe can
/// never end up reading a directory the identity check did not read.
fn account_command(
    bin: &str,
    args: &[&str],
    account: &Account,
) -> std::result::Result<std::process::Command, String> {
    let mut command = crate::envscan::tool_command(bin, args).ok_or("toolMissing".to_string())?;
    for (key, value) in super::session_env(account) {
        command.env(key, value);
    }
    Ok(command)
}

// ---------------------------------------------------------------------------
// Claude Code
// ---------------------------------------------------------------------------

/// Read one Claude Code `get_usage` response into windows and spend.
///
/// Split from the probe so it can be tested against a captured response — the
/// same discipline `parse_claude_identity` follows, and the only way to have a
/// test for this at all without a signed-in account in CI.
pub fn parse_claude_usage(response: &Value) -> LiveStatus {
    let read_at_ms = now_ms();

    let available = response
        .get("rate_limits_available")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let plan = response
        .get("subscription_type")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let Some(limits) = response.get("rate_limits").filter(|v| !v.is_null()) else {
        // The provider answered and said there is nothing to report. A signed
        // out directory and an API-key session both land here, and they are
        // told apart by the plan the same answer carries.
        return LiveStatus::Unavailable {
            reason: if plan.is_none() {
                "signedOut".to_string()
            } else {
                "notApplicable".to_string()
            },
            read_at_ms,
        };
    };
    if !available {
        return LiveStatus::Unavailable {
            reason: "notApplicable".to_string(),
            read_at_ms,
        };
    }

    let mut windows = Vec::new();

    // `limits[]` is the uniform view and the one to read. The sibling keys
    // beside it (`five_hour`, `seven_day`, and a rotating set of codenames)
    // carry the same numbers in a shape that changes between releases.
    if let Some(rows) = limits.get("limits").and_then(Value::as_array) {
        for row in rows {
            let Some(percent) = row.get("percent").and_then(Value::as_f64) else {
                continue;
            };
            let raw_kind = row
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let (fallback_group, kind) = canonical_kind(&raw_kind);
            let group = row
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or(fallback_group)
                .to_string();
            let provider_severity = row
                .get("severity")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            windows.push(LiveWindow {
                kind,
                raw_kind,
                group,
                scope_label: row
                    .get("scope")
                    .and_then(|s| s.get("model"))
                    .and_then(|m| m.get("display_name"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                percent_used: percent.clamp(0.0, 100.0),
                resets_at_ms: row.get("resets_at").and_then(reset_to_ms),
                window_minutes: None,
                binding: row
                    .get("is_active")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                binding_source: "provider".to_string(),
                // The provider's word is kept exactly as sent, including a band
                // this build has never seen. Only a missing severity is derived.
                severity: provider_severity
                    .map(str::to_string)
                    .unwrap_or_else(|| severity_from_percent(percent).to_string()),
                severity_source: if provider_severity.is_some() {
                    "provider".to_string()
                } else {
                    "derived".to_string()
                },
            });
        }
    }

    // Older builds, and any build that stops sending `limits[]`, still send the
    // named windows. Only the stable names are read: a codename this build has
    // never heard of has no meaning we could render, and `limits[]` is where it
    // would appear anyway.
    if windows.is_empty() {
        for raw in ["five_hour", "seven_day", "seven_day_opus", "seven_day_sonnet"] {
            let Some(entry) = limits.get(raw).filter(|v| !v.is_null()) else {
                continue;
            };
            let Some(percent) = entry.get("utilization").and_then(Value::as_f64) else {
                continue;
            };
            let (group, kind) = canonical_kind(raw);
            windows.push(LiveWindow {
                kind,
                raw_kind: raw.to_string(),
                group: group.to_string(),
                scope_label: None,
                percent_used: percent.clamp(0.0, 100.0),
                resets_at_ms: entry.get("resets_at").and_then(reset_to_ms),
                window_minutes: None,
                binding: false,
                binding_source: "derived".to_string(),
                severity: severity_from_percent(percent).to_string(),
                severity_source: "derived".to_string(),
            });
        }
        mark_fullest_as_binding(&mut windows);
    }

    LiveStatus::Ok {
        reading: LiveReading {
            read_at_ms,
            source: "claudeGetUsage".to_string(),
            plan,
            spend: parse_extra_usage(limits.get("extra_usage")),
            windows,
            reset_credits: 0,
            // Claude Code's usage response carries no identity; the card leans
            // on `accounts::read_identity` instead, which reads the same
            // configuration directory through the same env-applying path.
            identity: None,
        },
    }
}

/// The paid overage block, when the provider sends one.
fn parse_extra_usage(value: Option<&Value>) -> Option<LiveSpend> {
    let extra = value.filter(|v| !v.is_null())?;
    let number = |key: &str| extra.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    Some(LiveSpend {
        enabled: extra
            .get("is_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        used: number("used_credits"),
        limit: number("monthly_limit"),
        currency: extra
            .get("currency")
            .and_then(Value::as_str)
            .unwrap_or("USD")
            .to_string(),
        decimal_places: extra
            .get("decimal_places")
            .and_then(Value::as_i64)
            .unwrap_or(2),
        percent_used: extra.get("utilization").and_then(Value::as_f64),
        disabled_reason: extra
            .get("disabled_reason")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        limit_reached: extra
            .get("spend_limit_reached")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// Pick the window nearest its limit when the provider does not say which one
/// is binding, and mark that choice as derived.
///
/// The fullest window is the one that will refuse first, so it is the right
/// answer — but it is *our* answer, and §28 says the surface must be able to
/// say so rather than presenting it the way it presents Claude's `is_active`.
fn mark_fullest_as_binding(windows: &mut [LiveWindow]) {
    let fullest = windows
        .iter()
        .enumerate()
        .max_by(|a, b| {
            a.1.percent_used
                .partial_cmp(&b.1.percent_used)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(index, _)| index);
    if let Some(index) = fullest {
        windows[index].binding = true;
        windows[index].binding_source = "derived".to_string();
    }
}

/// Ask Claude Code, in one account's configuration directory, for its usage.
///
/// `--safe-mode` is load-bearing rather than tidy: without it the probe runs
/// the user's own `SessionStart` hooks, which on this machine print terminal
/// escapes and one of which fails outright. A quota reading has no business
/// executing anybody's hooks.
pub fn probe_claude(account: &Account) -> LiveStatus {
    let read_at_ms = now_ms();
    let command = match account_command(
        "claude",
        &[
            "-p",
            "--safe-mode",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
        ],
        account,
    ) {
        Ok(command) => command,
        Err(reason) => return LiveStatus::Failed { reason, read_at_ms },
    };

    let request = serde_json::json!({
        "type": "control_request",
        "request_id": "jarvis-usage",
        "request": { "subtype": "get_usage" }
    })
    .to_string();

    let found = drive(command, &[request], |line| {
        // Every other line on this stream is session chatter. Only the control
        // response carries an answer, and matching on the request id keeps a
        // future unsolicited control response from being read as ours.
        if !line.contains("control_response") {
            return None;
        }
        let value: Value = serde_json::from_str(line).ok()?;
        if value.get("type").and_then(Value::as_str)? != "control_response" {
            return None;
        }
        let response = value.get("response")?;
        if response.get("request_id").and_then(Value::as_str) != Some("jarvis-usage") {
            return None;
        }
        Some(response.clone())
    });

    match found {
        Ok(response) => {
            if response.get("subtype").and_then(Value::as_str) == Some("error") {
                return LiveStatus::Failed {
                    reason: "providerError".to_string(),
                    read_at_ms,
                };
            }
            match response.get("response") {
                Some(payload) => parse_claude_usage(payload),
                None => LiveStatus::Failed {
                    reason: "unreadable".to_string(),
                    read_at_ms,
                },
            }
        }
        Err(reason) => LiveStatus::Failed { reason, read_at_ms },
    }
}

// ---------------------------------------------------------------------------
// Codex
// ---------------------------------------------------------------------------

/// Read one `account/read` result.
///
/// Exists because the shape Codex stores identity in **changed under us**, and
/// the old reader failed silently. `accounts::parse_codex_identity` reads
/// `tokens.id_token_claims` from `auth.json`; Codex 0.149.1 no longer writes
/// that key — it stores the raw JWT in `tokens.id_token` — so every Codex card
/// rendered as "Unnamed account / Identity not available". Nothing failed and
/// no test noticed; it was found by opening the panel and reading it.
///
/// The fix deliberately is **not** "decode the JWT". Decoding a bearer token to
/// scrape an e-mail out of it means this product handling a credential, which
/// §60/§61 rule out, and it would break again on the next format change. Codex
/// answers `account/read` on the app-server session that is already open for
/// the limits, which is both the supported route and one round trip cheaper.
pub fn parse_codex_account(result: &Value) -> ProbedIdentity {
    let account = result.get("account").filter(|v| !v.is_null());
    let text = |key: &str| {
        account
            .and_then(|a| a.get(key))
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };
    ProbedIdentity {
        email: text("email"),
        plan: text("planType"),
    }
}

/// Read one `account/rateLimits/read` result.
pub fn parse_codex_limits(result: &Value) -> LiveStatus {
    let read_at_ms = now_ms();
    let Some(limits) = result.get("rateLimits").filter(|v| !v.is_null()) else {
        return LiveStatus::Unavailable {
            reason: "signedOut".to_string(),
            read_at_ms,
        };
    };

    // Codex names the window it has actually hit, when it has hit one. That is
    // the only official statement it makes about which window binds.
    let reached = limits
        .get("rateLimitReachedType")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);

    let mut windows = Vec::new();
    for key in ["primary", "secondary"] {
        let Some(entry) = limits.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let Some(percent) = entry.get("usedPercent").and_then(Value::as_f64) else {
            continue;
        };
        let minutes = entry.get("windowDurationMins").and_then(Value::as_i64);
        // Codex describes a window by its length, not by a name. Six hours and
        // six days are the boundaries because they sit far from both the real
        // five-hour and seven-day windows, so a provider that nudges either one
        // does not reclassify it.
        let raw_kind = match minutes {
            Some(m) if m >= 6 * 24 * 60 => "weekly_all",
            Some(m) if m <= 6 * 60 => "session",
            _ => key,
        };
        let (group, kind) = canonical_kind(raw_kind);
        let binding = reached
            .as_deref()
            .map(|r| r.contains(key) || (r.contains("weekly") && group == "weekly"))
            .unwrap_or(false);
        windows.push(LiveWindow {
            kind,
            raw_kind: raw_kind.to_string(),
            group: group.to_string(),
            scope_label: None,
            percent_used: percent.clamp(0.0, 100.0),
            resets_at_ms: entry.get("resetsAt").and_then(reset_to_ms).or_else(|| {
                entry
                    .get("resetsInSeconds")
                    .and_then(Value::as_i64)
                    .map(|s| read_at_ms + s * 1000)
            }),
            window_minutes: minutes,
            binding,
            binding_source: if binding { "provider" } else { "derived" }.to_string(),
            severity: severity_from_percent(percent).to_string(),
            severity_source: "derived".to_string(),
        });
    }

    // With nothing reached, nobody has said which window binds — so the fullest
    // one is our inference and is labelled as one.
    if !windows.iter().any(|w| w.binding) {
        mark_fullest_as_binding(&mut windows);
    }

    let credits = limits.get("credits");
    let spend = credits.filter(|v| !v.is_null()).map(|c| LiveSpend {
        enabled: c
            .get("hasCredits")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        // Codex sends the balance as a string. Parsing it here rather than in
        // the surface keeps the "one parse boundary" rule this module opens with.
        used: 0.0,
        limit: c
            .get("balance")
            .and_then(Value::as_str)
            .and_then(|b| b.parse::<f64>().ok())
            .unwrap_or(0.0),
        currency: "credits".to_string(),
        decimal_places: 0,
        percent_used: None,
        disabled_reason: None,
        limit_reached: limits
            .get("spendControlReached")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    });

    LiveStatus::Ok {
        reading: LiveReading {
            read_at_ms,
            source: "codexAppServer".to_string(),
            plan: limits
                .get("planType")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            windows,
            spend,
            reset_credits: result
                .get("rateLimitResetCredits")
                .and_then(|c| c.get("availableCount"))
                .and_then(Value::as_i64)
                .unwrap_or(0),
            // Filled in by the probe from the `account/read` reply that shares
            // this app-server session.
            identity: None,
        },
    }
}

/// Ask Codex, in one account's configuration directory, for its rate limits.
///
/// The `initialize` result echoes the `codexHome` it actually resolved, and
/// this asserts it against the account's own directory before trusting a
/// number. Claude Code's probe echoes no identity at all, which is why that one
/// leans on the identity read instead — but where a provider does hand back
/// which account it opened, not checking would be careless.
pub fn probe_codex(account: &Account) -> LiveStatus {
    let read_at_ms = now_ms();
    let command = match account_command("codex", &["app-server"], account) {
        Ok(command) => command,
        Err(reason) => return LiveStatus::Failed { reason, read_at_ms },
    };

    let requests = vec![
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "clientInfo": {
                "name": "jarvis", "title": "J.A.R.V.I.S.",
                "version": env!("CARGO_PKG_VERSION")
            }}
        })
        .to_string(),
        serde_json::json!({ "jsonrpc": "2.0", "method": "initialized", "params": null })
            .to_string(),
        // `account/read` needs an object rather than `null`, unlike its
        // neighbours — Codex answers a bare null with "missing field `params`".
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": "account/read", "params": {}
        })
        .to_string(),
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3, "method": "account/rateLimits/read", "params": null
        })
        .to_string(),
    ];

    // Three answers arrive in order on one stream, and each does a different
    // job: `initialize` echoes the home that decides whether anything may be
    // trusted, `account/read` says who that home belongs to, and only the
    // limits end the exchange. Identity is set aside as it passes rather than
    // costing a second app-server startup.
    let expected_home = account.config_dir.clone();
    let identity = std::sync::Arc::new(std::sync::Mutex::new(None::<ProbedIdentity>));
    let seen = std::sync::Arc::clone(&identity);

    let found = drive(command, &requests, move |line| {
        let value: Value = serde_json::from_str(line).ok()?;
        match value.get("id").and_then(Value::as_i64)? {
            1 => {
                let home = value
                    .get("result")?
                    .get("codexHome")
                    .and_then(Value::as_str)?;
                // A mismatch means the CLI ignored the variable and opened
                // somebody else's account. Stopping here is the whole point:
                // plausible numbers under the wrong name are worse than none.
                if same_path(home, &expected_home) {
                    None
                } else {
                    Some(Err("wrongDirectory".to_string()))
                }
            }
            2 => {
                if let Some(result) = value.get("result") {
                    if let Ok(mut slot) = seen.lock() {
                        *slot = Some(parse_codex_account(result));
                    }
                }
                // Never ends the exchange: an older Codex that does not know
                // this method must still get as far as its limits.
                None
            }
            3 => Some(match value.get("result") {
                Some(result) => Ok(result.clone()),
                None => Err(match value.get("error").and_then(|e| e.get("message")) {
                    // Codex says plainly when the directory has no account, and
                    // that is a definite answer rather than a failed probe.
                    Some(Value::String(message))
                        if message.to_lowercase().contains("authentication") =>
                    {
                        "signedOut".to_string()
                    }
                    _ => "providerError".to_string(),
                }),
            }),
            _ => None,
        }
    });

    match found {
        Ok(Ok(result)) => {
            let mut status = parse_codex_limits(&result);
            if let LiveStatus::Ok { reading } = &mut status {
                reading.identity = identity.lock().ok().and_then(|slot| slot.clone());
                // The account endpoint's plan is the more direct statement of
                // the two; the limits block repeats it and can lag.
                if let Some(plan) = reading.identity.as_ref().and_then(|i| i.plan.clone()) {
                    reading.plan = Some(plan);
                }
            }
            status
        }
        Ok(Err(reason)) if reason == "signedOut" => LiveStatus::Unavailable {
            reason,
            read_at_ms,
        },
        Ok(Err(reason)) | Err(reason) => LiveStatus::Failed { reason, read_at_ms },
    }
}

/// Whether two paths name the same directory.
///
/// This check exists to catch a CLI that ignored `CODEX_HOME` and opened
/// somebody else's account, so it must be strict about *that* and forgiving
/// about everything else. Two spellings of the same directory are common and
/// mean nothing:
///
/// * separators and case, which Windows does not distinguish;
/// * **8.3 short names**, which is not hypothetical — the very first run of
///   this against the real CLI failed here, because `std::env::temp_dir()`
///   returns `C:\Users\ALANAR~1\...` while Codex echoed back the long form. A
///   user whose data directory sits under a path like that would have seen a
///   signed-in account reported as broken;
/// * junctions and symlinks, which the same resolution handles.
///
/// So the comparison is made on the resolved paths where the filesystem can
/// resolve them, and falls back to normalised text where it cannot — a
/// directory that does not exist yet still has to compare sensibly.
fn same_path(a: &str, b: &str) -> bool {
    let normalise = |p: &str| {
        p.trim_end_matches(['/', '\\'])
            .replace('\\', "/")
            .to_lowercase()
    };
    if normalise(a) == normalise(b) {
        return true;
    }
    match (
        std::fs::canonicalize(Path::new(a)),
        std::fs::canonicalize(Path::new(b)),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Probing, storing, and feeding the rest of the model
// ---------------------------------------------------------------------------

/// Probe one account, whatever provider it is on.
pub fn probe(account: &Account) -> LiveStatus {
    // An account whose directory has never been signed into cannot answer, and
    // spawning a CLI to be told so costs a second per refresh for nothing.
    if !account.adopted && !Path::new(&account.config_dir).exists() {
        return LiveStatus::Unavailable {
            reason: "signedOut".to_string(),
            read_at_ms: now_ms(),
        };
    }
    match account.provider.as_str() {
        "claude-code" => probe_claude(account),
        "codex" => probe_codex(account),
        _ => LiveStatus::Failed {
            reason: "unsupported".to_string(),
            read_at_ms: now_ms(),
        },
    }
}

/// Store the newest reading for an account, replacing the one before it.
///
/// One row per account, not a log: `account_limit_events` is already the
/// append-only history and duplicating it here would give two records that can
/// disagree. This table exists so the panel has something to draw the instant
/// it opens, instead of a spinner over an empty card while a CLI starts up.
pub fn store(db: &Database, account_id: &str, status: &LiveStatus) {
    let Ok(payload) = serde_json::to_string(status) else {
        return;
    };
    let _ = db.with(|conn| {
        conn.execute(
            "INSERT INTO account_live_readings (account_id, read_at_ms, payload)
             VALUES (?1, ?2, ?3)
             ON CONFLICT (account_id) DO UPDATE SET
                 read_at_ms = excluded.read_at_ms,
                 payload    = excluded.payload",
            params![account_id, status.read_at_ms(), payload],
        )?;
        Ok(())
    });
}

/// The stored reading for an account, if there is one.
pub fn stored(db: &Database, account_id: &str) -> Option<LiveStatus> {
    let payload: Option<String> = db
        .with(|conn| {
            Ok(conn
                .query_row(
                    "SELECT payload FROM account_live_readings WHERE account_id = ?1",
                    [account_id],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .ok()
        .flatten();
    // A payload written by a build with a different shape is dropped rather
    // than failing the panel: a stale cache is a convenience, never a contract.
    payload.and_then(|text| serde_json::from_str(&text).ok())
}

/// Whether a stored reading is old enough to be worth replacing.
pub fn is_stale(status: Option<&LiveStatus>, now: i64) -> bool {
    match status {
        None => true,
        Some(status) => now - status.read_at_ms() >= STALE_AFTER_MS,
    }
}

/// Fold a live reading into the append-only limit history.
///
/// This is what makes the live source *additive* rather than a second system:
/// everything downstream — `quota::build_window`, the automatic switch, the
/// calibration that keeps working when a probe cannot run — already reads
/// `account_limit_events` and needs no knowledge that a probe exists.
///
/// Window names are mapped onto the ones that table already uses, so an
/// official percentage lands on the same row a refusal would have.
pub fn record_reading(db: &Database, account_id: &str, reading: &LiveReading) {
    for window in &reading.windows {
        let stored_window = match window.kind.as_str() {
            "session" => "five_hour",
            "weekly" => "weekly",
            other => other,
        };
        super::quota::record(
            db,
            account_id,
            None,
            &super::quota::LimitObservation {
                window: stored_window.to_string(),
                status: match window.severity.as_str() {
                    "exhausted" => "rejected",
                    "critical" | "warning" => "warning",
                    _ => "ok",
                }
                .to_string(),
                resets_at_ms: window.resets_at_ms,
                percent: Some(window.percent_used),
                detail: None,
            },
        );
    }
}

/// Save an identity a provider volunteered during a probe.
///
/// Only fills gaps. `refresh_identity` remains the primary path and a label the
/// person typed is never touched — the same precedence M13 settled on, applied
/// to a second source rather than replaced by it.
///
/// This exists because Codex 0.149.1 stopped writing `id_token_claims` into
/// `auth.json`, which left every Codex card nameless with nothing logged. A
/// second, independent route to the same fact is what keeps a silent format
/// change from being a silent regression.
fn apply_identity(db: &Database, account_id: &str, identity: &ProbedIdentity) {
    let _ = db.with(|conn| {
        if let Some(email) = identity.email.as_deref() {
            conn.execute(
                "UPDATE provider_accounts
                    SET email = ?2, signed_in = 1
                  WHERE id = ?1 AND (email IS NULL OR email = '')",
                params![account_id, email],
            )?;
            conn.execute(
                "UPDATE provider_accounts SET label = ?2 WHERE id = ?1 AND label = ''",
                params![account_id, email],
            )?;
        }
        if let Some(plan) = identity.plan.as_deref() {
            conn.execute(
                "UPDATE provider_accounts
                    SET plan = ?2 WHERE id = ?1 AND (plan IS NULL OR plan = '')",
                params![account_id, plan],
            )?;
        }
        Ok(())
    });
}

/// Probe every account of every provider, store what comes back, and fold it in.
///
/// Each account gets its own thread because a probe is dominated by CLI startup
/// rather than by anything this process does, and four accounts refreshed one
/// after another is four times a wait somebody is watching.
pub fn refresh_all(db: &std::sync::Arc<Database>, accounts: &[Account]) {
    let handles: Vec<_> = accounts
        .iter()
        .cloned()
        .map(|account| {
            let db = std::sync::Arc::clone(db);
            std::thread::Builder::new()
                .name(format!("quota-{}", account.id))
                .spawn(move || {
                    let status = probe(&account);
                    if let Some(reading) = status.reading() {
                        record_reading(&db, &account.id, reading);
                        if let Some(identity) = &reading.identity {
                            apply_identity(&db, &account.id, identity);
                        }
                    }
                    store(&db, &account.id, &status);
                })
        })
        .filter_map(std::result::Result::ok)
        .collect();
    for handle in handles {
        let _ = handle.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from Claude Code 2.1.241 on this machine, trimmed to
    /// the fields the parser reads. The rotating codename is kept on purpose —
    /// it is the case that must not crash the panel.
    const CLAUDE_RESPONSE: &str = r#"{
      "session": {"total_cost_usd": 0},
      "subscription_type": "pro",
      "rate_limits_available": true,
      "rate_limits": {
        "five_hour": {"utilization": 5, "resets_at": "2026-08-24T23:20:00.094226+00:00"},
        "seven_day": {"utilization": 99, "resets_at": "2026-08-26T04:00:00.094247+00:00"},
        "nimbus_quill": {"utilization": 0, "resets_at": null},
        "extra_usage": {"is_enabled": false, "monthly_limit": 0, "used_credits": 0,
                        "utilization": null, "currency": "BRL", "decimal_places": 2,
                        "disabled_reason": "org_level_disabled_until",
                        "spend_limit_reached": false},
        "limits": [
          {"kind": "session", "group": "session", "percent": 5, "severity": "normal",
           "resets_at": "2026-08-24T23:20:00.094226+00:00", "scope": null, "is_active": false},
          {"kind": "weekly_all", "group": "weekly", "percent": 99, "severity": "critical",
           "resets_at": "2026-08-26T04:00:00.094247+00:00", "scope": null, "is_active": true},
          {"kind": "cinder_cove", "group": "other", "percent": 12, "severity": "normal",
           "resets_at": null, "scope": {"model": {"display_name": "Fable"}}, "is_active": false}
        ]
      }
    }"#;

    fn claude() -> LiveReading {
        match parse_claude_usage(&serde_json::from_str(CLAUDE_RESPONSE).unwrap()) {
            LiveStatus::Ok { reading } => reading,
            other => panic!("expected a reading, got {other:?}"),
        }
    }

    #[test]
    fn the_binding_window_is_the_one_the_provider_named() {
        let reading = claude();
        let binding: Vec<&LiveWindow> = reading.windows.iter().filter(|w| w.binding).collect();
        assert_eq!(binding.len(), 1, "exactly one window can be binding");
        assert_eq!(binding[0].kind, "weekly");
        assert_eq!(binding[0].percent_used, 99.0);
        assert_eq!(
            binding[0].binding_source, "provider",
            "Claude states which window binds; presenting that as our inference \
             would understate a fact, which §28 forbids in both directions"
        );
    }

    #[test]
    fn an_unknown_codename_is_rendered_rather_than_dropped() {
        let reading = claude();
        let odd = reading
            .windows
            .iter()
            .find(|w| w.raw_kind == "cinder_cove")
            .expect("an unrecognised window must survive parsing");
        assert_eq!(odd.kind, "cinder_cove", "it keeps the provider's own name");
        assert_eq!(odd.scope_label.as_deref(), Some("Fable"));
    }

    #[test]
    fn an_iso_reset_becomes_milliseconds_and_a_unix_second_stamp_does_too() {
        let reading = claude();
        let session = reading
            .windows
            .iter()
            .find(|w| w.kind == "session")
            .unwrap();
        // 2026-08-24T23:20:00.094226Z — the sub-second part is kept rather than
        // truncated, because a countdown that is a fraction of a second wrong
        // is right and one that silently rounds is a habit that gets applied to
        // a field where it matters.
        assert_eq!(session.resets_at_ms, Some(1_787_613_600_094));

        // The other two shapes the same value arrives in elsewhere.
        assert_eq!(
            reset_to_ms(&serde_json::json!(1_787_613_600i64)),
            Some(1_787_613_600_000)
        );
        assert_eq!(
            reset_to_ms(&serde_json::json!(1_787_613_600_000i64)),
            Some(1_787_613_600_000)
        );
    }

    #[test]
    fn the_monthly_spend_block_is_read_with_its_own_currency() {
        let spend = claude().spend.expect("extra_usage must be read");
        assert!(!spend.enabled);
        assert_eq!(spend.currency, "BRL");
        assert_eq!(
            spend.disabled_reason.as_deref(),
            Some("org_level_disabled_until"),
            "a stable id, so the surface can translate it (§65)"
        );
    }

    #[test]
    fn an_empty_config_directory_reads_as_signed_out_not_as_a_failure() {
        // Exactly what an empty CLAUDE_CONFIG_DIR returned on this machine.
        let response = serde_json::json!({
            "session": {"total_cost_usd": 0},
            "subscription_type": null,
            "rate_limits_available": false,
            "rate_limits": null
        });
        match parse_claude_usage(&response) {
            LiveStatus::Unavailable { reason, .. } => assert_eq!(reason, "signedOut"),
            other => panic!(
                "an empty directory is a definite 'no account here', not a broken \
                 probe — a Failed here would put a retry spinner where a sign-in \
                 button belongs. Got {other:?}"
            ),
        }
    }

    /// Captured verbatim from `codex app-server` 0.149.1 on this machine.
    const CODEX_RESULT: &str = r#"{
      "rateLimits": {
        "limitId": "codex", "planType": "plus",
        "primary": {"usedPercent": 80, "windowDurationMins": 10080, "resetsAt": 1788147529},
        "secondary": null,
        "credits": {"hasCredits": false, "unlimited": false, "balance": "0"},
        "spendControlReached": false, "rateLimitReachedType": null
      },
      "rateLimitResetCredits": {"availableCount": 1}
    }"#;

    #[test]
    fn codex_windows_are_named_by_their_length_and_binding_is_marked_derived() {
        let reading = match parse_codex_limits(&serde_json::from_str(CODEX_RESULT).unwrap()) {
            LiveStatus::Ok { reading } => reading,
            other => panic!("expected a reading, got {other:?}"),
        };
        assert_eq!(reading.plan.as_deref(), Some("plus"));
        assert_eq!(reading.reset_credits, 1, "shown, never spent");

        let window = &reading.windows[0];
        assert_eq!(window.kind, "weekly", "10080 minutes is a week");
        assert_eq!(window.percent_used, 80.0);
        assert_eq!(window.resets_at_ms, Some(1_788_147_529_000));
        assert!(window.binding);
        assert_eq!(
            window.binding_source, "derived",
            "Codex does not say which window binds, so choosing the fullest one \
             is our inference and has to be labelled as one"
        );
    }

    /// Codex 0.149.1's `account/read`, captured verbatim.
    ///
    /// This test is the guard on a regression that shipped silently once
    /// already: `accounts::parse_codex_identity` reads `id_token_claims` out of
    /// `auth.json`, and this Codex build stopped writing that key, so every
    /// Codex account rendered as "Unnamed account". Nothing errored. It was
    /// found by opening the panel and reading it.
    #[test]
    fn codex_identity_comes_from_the_account_endpoint_not_from_a_token() {
        let result: Value = serde_json::from_str(
            r#"{"account":{"type":"chatgpt","email":"someone@example.test","planType":"plus"},
                "requiresOpenaiAuth":true}"#,
        )
        .unwrap();
        let identity = parse_codex_account(&result);
        assert_eq!(identity.email.as_deref(), Some("someone@example.test"));
        assert_eq!(identity.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn an_api_key_account_has_no_email_and_that_is_not_an_error() {
        let result: Value =
            serde_json::from_str(r#"{"account":{"type":"apiKey"},"requiresOpenaiAuth":false}"#)
                .unwrap();
        assert_eq!(parse_codex_account(&result), ProbedIdentity {
            email: None,
            plan: None
        });
    }

    #[test]
    fn a_full_window_is_exhausted_rather_than_merely_critical() {
        assert_eq!(severity_from_percent(100.0), "exhausted");
        assert_eq!(severity_from_percent(99.0), "critical");
        assert_eq!(severity_from_percent(86.0), "warning");
        assert_eq!(severity_from_percent(20.0), "normal");
    }

    #[test]
    fn a_codex_home_echo_that_does_not_match_is_caught() {
        assert!(same_path("C:\\Users\\A\\.codex", "C:/Users/a/.codex/"));
        assert!(!same_path("C:/Users/A/.codex", "C:/Users/A/accounts/2"));
    }

    /// The 8.3 short-name case, which a real run against Codex actually hit.
    /// Skipped where the platform has no short names to produce.
    #[test]
    fn two_spellings_of_one_real_directory_are_the_same_directory() {
        let short = std::env::temp_dir();
        let Ok(long) = std::fs::canonicalize(&short) else {
            return;
        };
        assert!(
            same_path(&short.to_string_lossy(), &long.to_string_lossy()),
            "a directory must not stop matching itself because the OS spelled \
             it differently — that reads as 'the CLI opened the wrong account'"
        );
    }
}
