//! The local model runtime (§92).
//!
//! A third execution provider that is not a service. Claude Code and Codex are
//! programs that talk to somebody else's datacentre; this one talks to a model
//! sitting in the GPU in this machine, and almost everything that follows is a
//! consequence of that single difference.
//!
//! ## Why this is Codex CLI, and not a chat client we wrote
//!
//! The obvious build is an Ollama chat window. It was rejected, because a chat
//! window is not what the rest of this product is for: sessions here edit
//! files, run commands, are governed by guardrails, are correlated to a
//! transcript, are searched, and are driven by autopilots. Reaching parity with
//! a hand-written client would mean rewriting all of that against a second
//! agent loop.
//!
//! Codex CLI 0.150.1 already runs an agent against a local OpenAI-compatible
//! server, and its rollout is byte-identical in shape to the one the Codex
//! provider already follows. Verified by experiment on this machine rather than
//! read off a help page: a local run wrote `session_meta` with `cwd`, its model
//! provider and a start timestamp, then a `token_count` event carrying real
//! input and output counts. So correlation, the conversation view, guardrail
//! hooks, worktrees, Analytics and Global Search all work on day one, and the
//! honest description of this provider is "the same agent, a different brain".
//!
//! It is pointed at the local server by a provider **written into this
//! runtime's configuration**, not by the CLI's own `--oss` flag. That flag
//! exists, works, and was the first build here; it always talks to the runner's
//! default address, which would have made the endpoint on the Local model
//! screen a setting that changed what this app reads and not what its sessions
//! do. See `config_toml` for the one non-obvious value that configuration needs.
//!
//! ## Why it is not an account
//!
//! `accounts` exists for a credentialed configuration directory with a
//! five-hour allowance behind it. The same experiment showed `rate_limits`
//! present and **entirely null** for a local run — no limit id, no plan, no
//! window — because there is nothing to meter. Modelling this as an account
//! would put a quota dial on a card that can never move.
//!
//! What actually constrains a local model is physical: how much of it fits in
//! VRAM, how much context the runner was loaded with, and whether the card is
//! being held back by its power limit. That is a *runtime*, not an account, and
//! it gets its own surface reporting its own real numbers (§81).
//!
//! ## Its own CODEX_HOME
//!
//! Local sessions run with a configuration root this app owns, under its data
//! directory. Two reasons, both concrete:
//!
//! 1. **Correlation.** `codex::correlate` matches a rollout by working
//!    directory and start time. A local session and a cloud Codex session
//!    started seconds apart in the same project would write into one rollout
//!    tree and could match each other's file. Separate roots make that
//!    impossible rather than unlikely.
//! 2. **Configuration.** `--oss` needs the model, the sandbox policy and the
//!    real context window stated somewhere. Writing that into the user's own
//!    `~/.codex/config.toml` would change how *their* Codex behaves outside
//!    this app, which it must not.

pub mod commands;
pub mod ollama;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::db::Database;

/// The provider id this runtime launches sessions under.
pub const PROVIDER_ID: &str = "local";

/// Where the runtime configuration is stored (§64).
const SETTINGS_KEY: &str = "localai.runtime";

/// The default Ollama endpoint. Loopback, never a LAN address: a local model is
/// local, and defaulting to anything reachable from another machine would make
/// this product's most private surface its least.
pub const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";

/// Server-level settings that are **not** ours to set per session.
///
/// Ollama reads these from its own process environment at startup, so they are
/// fixed for every client of the server, including this one. They are listed
/// here because they are the three knobs that decide whether a 27B Q4 model
/// fits in 24 GB with a long context — and because a surface that appeared to
/// set them per session would be lying about where the control lives.
pub const SERVER_ENV: &[ServerSetting] = &[
    ServerSetting {
        key: "OLLAMA_CONTEXT_LENGTH",
        // The default is 4096, which is not a detail: an agent session on a
        // 4096-token window truncates its own instructions.
        default: Some("4096"),
    },
    ServerSetting {
        key: "OLLAMA_FLASH_ATTENTION",
        default: Some("0"),
    },
    ServerSetting {
        key: "OLLAMA_KV_CACHE_TYPE",
        default: Some("f16"),
    },
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerSetting {
    pub key: &'static str,
    /// What Ollama does when the variable is unset.
    pub default: Option<&'static str>,
}

/// How far a local session may reach into the machine.
///
/// Spelled exactly as Codex spells it, because these values are written into
/// its configuration verbatim and a translation layer would be one more place
/// for the two to disagree.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl SandboxMode {
    fn as_config(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

/// When a local session stops to ask.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    Untrusted,
    OnFailure,
    OnRequest,
    Never,
}

impl ApprovalPolicy {
    fn as_config(self) -> &'static str {
        match self {
            Self::Untrusted => "untrusted",
            Self::OnFailure => "on-failure",
            Self::OnRequest => "on-request",
            Self::Never => "never",
        }
    }
}

/// Everything the person controls about the local brain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeConfig {
    pub endpoint: String,
    /// The model local sessions start on. `None` until one is chosen — an
    /// unset model is a supported state and the launcher refuses rather than
    /// guessing which of the installed models the person meant.
    pub model: Option<String>,
    /// How long Ollama keeps the model in VRAM after the last request.
    ///
    /// `-1` keeps it resident indefinitely, which is what makes a second
    /// prompt answer immediately instead of paying a ten-second reload. `0`
    /// evicts it the moment a turn ends, which is what you want when the card
    /// is also driving something else.
    pub keep_alive_minutes: i64,
    /// Load the model before the first prompt, so the first turn is not the
    /// one that pays for the load.
    pub preload_on_start: bool,
    pub sandbox: SandboxMode,
    pub approval: ApprovalPolicy,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_string(),
            model: None,
            // Long enough that a working session never reloads, short enough
            // that a forgotten one gives 17 GB back within the hour.
            keep_alive_minutes: 30,
            preload_on_start: true,
            // The agent is here to change code, and a read-only default would
            // make every session start by failing to do the thing it is for.
            // Still bounded to the workspace: `danger-full-access` is a choice
            // a person makes deliberately, never one they inherit.
            sandbox: SandboxMode::WorkspaceWrite,
            approval: ApprovalPolicy::OnRequest,
        }
    }
}

impl RuntimeConfig {
    /// `keep_alive` in the spelling Ollama's API expects.
    pub fn keep_alive(&self) -> String {
        match self.keep_alive_minutes {
            minutes if minutes < 0 => "-1".to_string(),
            0 => "0".to_string(),
            minutes => format!("{minutes}m"),
        }
    }
}

/// Read the runtime configuration, falling back to the defaults.
pub fn config(db: &Database) -> RuntimeConfig {
    crate::settings::get_or(db, SETTINGS_KEY, RuntimeConfig::default())
}

/// Store the runtime configuration.
pub fn set_config(db: &Database, config: &RuntimeConfig) -> Result<(), String> {
    crate::settings::set(db, SETTINGS_KEY, config)
}

/// The configuration root local sessions run with. See the module docs.
pub fn codex_home(data_dir: &Path) -> PathBuf {
    data_dir.join("local-runtime").join("codex-home")
}

/// Where this runtime's rollouts land, for `codex::correlate_in`.
pub fn transcript_root(data_dir: &Path) -> PathBuf {
    codex_home(data_dir).join("sessions")
}

/// The context window the configured model's runner is loaded with, if it is.
///
/// One function rather than the same three lines at each call site, because
/// both of them — saving the configuration, and starting a session — must
/// arrive at the same number or the file a session reads disagrees with the
/// screen that wrote it.
pub fn measured_context(config: &RuntimeConfig, resident: &[ollama::ResidentModel]) -> Option<u64> {
    let model = config.model.as_deref()?;
    resident
        .iter()
        .find(|runner| runner.name == model)
        .and_then(|runner| runner.context_length)
}

/// Prepare the configuration root and return the environment a local session
/// launches with.
///
/// `context_window` is the **measured** window of the resident runner where one
/// is loaded. This matters more than it looks: without it Codex warns that the
/// model's metadata is unknown and falls back to an invented 258400-token
/// window — measured on this machine against a runner actually loaded at 65536.
/// An agent that believes it has four times the context it has does not degrade
/// gracefully; it fills the window and the server truncates the front of the
/// conversation out from under it.
pub fn prepare(
    data_dir: &Path,
    config: &RuntimeConfig,
    context_window: Option<u64>,
) -> Result<PathBuf, String> {
    let home = codex_home(data_dir);
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    std::fs::write(home.join("config.toml"), config_toml(config, context_window))
        .map_err(|e| e.to_string())?;
    Ok(home)
}

/// The provider id written into this runtime's configuration.
///
/// Deliberately **not** `ollama`, which is a provider the runner already ships.
/// Defining our own under a distinct name means this configuration adds a
/// provider rather than redefining one, and the two cannot disagree.
const PROVIDER_KEY: &str = "jarvis-ollama";

/// The configuration written into this runtime's own root.
pub fn config_toml(config: &RuntimeConfig, context_window: Option<u64>) -> String {
    let mut toml = String::new();
    toml.push_str("# Written by J.A.R.V.I.S. for local model sessions.\n");
    toml.push_str("# This is the app's own Codex configuration root; the user's\n");
    toml.push_str("# ~/.codex/config.toml is never read or modified from here.\n\n");

    if let Some(model) = &config.model {
        toml.push_str(&format!("model = {}\n", toml_string(model)));
    }
    toml.push_str(&format!("model_provider = \"{PROVIDER_KEY}\"\n"));
    toml.push_str(&format!(
        "approval_policy = \"{}\"\n",
        config.approval.as_config()
    ));
    toml.push_str(&format!(
        "sandbox_mode = \"{}\"\n",
        config.sandbox.as_config()
    ));
    // Only stated when it was measured. A guessed context window is the exact
    // failure this key exists to prevent.
    if let Some(window) = context_window {
        toml.push_str(&format!("model_context_window = {window}\n"));
    }

    toml.push_str(&format!("\n[model_providers.{PROVIDER_KEY}]\n"));
    toml.push_str("name = \"Ollama\"\n");
    toml.push_str(&format!(
        "base_url = {}\n",
        toml_string(&format!("{}/v1", config.endpoint.trim_end_matches('/')))
    ));
    // Measured, and the second value tried.
    //
    // `wire_api = "chat"` is the obvious one and it is **rejected outright** by
    // Codex 0.150.1: the session dies on startup with "no longer supported".
    // `"responses"` works, and Ollama 0.33.2 answers `POST /v1/responses` — a
    // full agent turn was run through this exact configuration before it was
    // written here. Naming the wire protocol at all is what lets the endpoint
    // above be a real setting: the runner's own `--oss` mode always talks to
    // its default address, so a person who moved their server would otherwise
    // see this screen read one machine while sessions talked to another.
    toml.push_str("wire_api = \"responses\"\n");
    toml
}

/// A TOML basic string. Endpoints and model tags are user-supplied text and go
/// into a file a program parses, so they are escaped rather than interpolated.
fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// Command-line arguments for a local session.
///
/// Only the model, and only because it is worth having on the command line
/// where a person reading their own terminal can see it. Everything else —
/// which server, which wire protocol, what the sandbox may touch, how large the
/// context really is — comes from the configuration root.
///
/// **Not** `--oss`. That flag exists and works, and it was the first build:
/// `codex --oss --local-provider ollama` runs an agent against a local model
/// with no configuration at all. It was dropped for one reason — it always
/// talks to the runner's own default address, so the endpoint on the Local
/// model screen would have been a setting that changed what this app reads and
/// not what its sessions do.
pub fn launch_args(config: &RuntimeConfig) -> Vec<String> {
    match &config.model {
        Some(model) => vec!["-m".to_string(), model.clone()],
        // Unreachable: the launcher refuses without a model. An empty argument
        // list is still the right answer, because a dangling `-m` would not
        // start at all.
        None => Vec::new(),
    }
}
