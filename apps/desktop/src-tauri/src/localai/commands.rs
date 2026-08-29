//! Commands the local-runtime surface calls.
//!
//! Every field returned here is measured or read; none is inferred. Where a
//! number could not be obtained it is `None` and the surface shows an em dash,
//! because the request this whole area answers was for *precision*, and a
//! plausible stand-in is the one thing that would defeat it.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::ollama::{InstalledModel, ResidentModel};
use super::{RuntimeConfig, SERVER_ENV};
use crate::AppState;

/// One server-level setting, with where its value came from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ServerEnvValue {
    pub key: String,
    /// What is set for this user, or `None` when nothing is.
    pub value: Option<String>,
    /// What Ollama does when it is unset.
    pub fallback: Option<String>,
    /// True when the value was read from the **persisted** user environment,
    /// rather than only from the environment this process happened to inherit.
    ///
    /// Deliberately not a claim about whether the running server has it: this
    /// app cannot see the environment a process started before it was launched.
    /// What it can say is which of the two places the number came from, and the
    /// surface reports the weaker case rather than presenting an inherited
    /// value as a saved setting.
    pub persisted: bool,
}

impl ServerEnvValue {
    /// The value in force, as far as can be known.
    pub fn effective(&self) -> Option<String> {
        self.value
            .clone()
            .or_else(|| self.fallback.clone())
    }
}

/// The whole local runtime, as one read.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRuntimeReport {
    pub config: RuntimeConfig,
    /// The Ollama server version, and therefore whether anything is listening.
    pub server_version: Option<String>,
    pub reachable: bool,
    /// Why the server could not be reached, verbatim. Shown because "it does
    /// not work" is not something a person can act on and "connection refused
    /// on 127.0.0.1:11434" is.
    pub error: Option<String>,
    pub models: Vec<InstalledModel>,
    pub resident: Vec<ResidentModel>,
    pub server_env: Vec<ServerEnvValue>,
    /// This runtime's own configuration root.
    pub codex_home: String,
    /// Whether the agent runner this provider is built on is installed.
    pub runner_installed: bool,
}

/// Read everything about the local runtime.
#[tauri::command]
pub fn local_runtime_report(state: State<'_, AppState>) -> LocalRuntimeReport {
    let config = super::config(&state.db);
    let server_version = super::ollama::version(&config.endpoint);
    let reachable = server_version.is_some();

    let (models, resident, error) = if reachable {
        let models = super::ollama::installed(&config.endpoint);
        let resident = super::ollama::resident(&config.endpoint);
        let error = models
            .as_ref()
            .err()
            .or(resident.as_ref().err())
            .cloned();
        (
            models.unwrap_or_default(),
            resident.unwrap_or_default(),
            error,
        )
    } else {
        (
            Vec::new(),
            Vec::new(),
            Some(format!("no response from {}", config.endpoint)),
        )
    };

    LocalRuntimeReport {
        server_env: read_server_env(),
        codex_home: super::codex_home(&state.data_dir).to_string_lossy().to_string(),
        runner_installed: runner_installed(),
        config,
        server_version,
        reachable,
        error,
        models,
        resident,
    }
}

/// Store the runtime configuration.
///
/// Writing the Codex configuration root here, rather than only at launch,
/// means the settings a person just saved are on disk before the next session
/// starts even if this app is closed in between.
#[tauri::command]
pub fn local_runtime_save(
    state: State<'_, AppState>,
    config: RuntimeConfig,
) -> Result<(), String> {
    super::set_config(&state.db, &config)?;
    let resident = super::ollama::resident(&config.endpoint).unwrap_or_default();
    let context = super::measured_context(&config, &resident);
    super::prepare(&state.data_dir, &config, context).map(|_| ())
}

/// Pull a model into VRAM now.
#[tauri::command]
pub fn local_runtime_load(state: State<'_, AppState>, model: String) -> Result<(), String> {
    let config = super::config(&state.db);
    super::ollama::load(&config.endpoint, &model, &config.keep_alive())
}

/// Evict a model from VRAM now.
#[tauri::command]
pub fn local_runtime_unload(state: State<'_, AppState>, model: String) -> Result<(), String> {
    let config = super::config(&state.db);
    super::ollama::unload(&config.endpoint, &model)
}

/// Set a server-level variable for this user.
///
/// It takes effect when Ollama is next started, and **not before** — the
/// running server read its environment once, at launch. The command reports
/// that rather than letting the surface imply the change is live, which would
/// be the same class of lie as an invented context window.
#[tauri::command]
pub fn local_runtime_set_server_env(key: String, value: String) -> Result<(), String> {
    if !SERVER_ENV.iter().any(|setting| setting.key == key) {
        // Only the three documented knobs. This command writes to the user's
        // persistent environment, and a surface bug that let it write an
        // arbitrary name would be a much larger thing than a wrong setting.
        return Err("localAi.unknownSetting".into());
    }
    write_user_env(&key, &value)?;
    // So the surface shows the value it just saved, rather than the cached
    // reading from before it.
    forget_server_env();
    Ok(())
}

/// How long a reading of the user's environment is reused.
///
/// This report is polled every couple of seconds by two surfaces, and on
/// Windows each variable costs a `reg query` process. These values change when
/// a person edits them — which is rare, and which goes through this module and
/// invalidates the cache itself — so re-reading the registry six times a second
/// would buy nothing.
#[cfg(windows)]
const ENV_FRESH_FOR: std::time::Duration = std::time::Duration::from_secs(10);

#[cfg(windows)]
static ENV_CACHE: std::sync::Mutex<Option<(std::time::Instant, Vec<ServerEnvValue>)>> =
    std::sync::Mutex::new(None);

/// Read the three server-level settings from the user's environment.
#[cfg(windows)]
fn read_server_env() -> Vec<ServerEnvValue> {
    if let Ok(cache) = ENV_CACHE.lock() {
        if let Some((taken, values)) = cache.as_ref() {
            if taken.elapsed() < ENV_FRESH_FOR {
                return values.clone();
            }
        }
    }
    let values = query_server_env();
    if let Ok(mut cache) = ENV_CACHE.lock() {
        *cache = Some((std::time::Instant::now(), values.clone()));
    }
    values
}

/// Drop the cached reading, so a value this app just wrote is visible at once.
#[cfg(windows)]
fn forget_server_env() {
    if let Ok(mut cache) = ENV_CACHE.lock() {
        *cache = None;
    }
}

#[cfg(not(windows))]
fn forget_server_env() {}

#[cfg(windows)]
fn query_server_env() -> Vec<ServerEnvValue> {
    SERVER_ENV
        .iter()
        .map(|setting| {
            let persisted = read_user_env(setting.key);
            let value = persisted
                .clone()
                .or_else(|| std::env::var(setting.key).ok())
                .filter(|value| !value.trim().is_empty());
            ServerEnvValue {
                key: setting.key.to_string(),
                persisted: persisted.is_some(),
                value,
                fallback: setting.default.map(str::to_string),
            }
        })
        .collect()
}

#[cfg(not(windows))]
fn read_server_env() -> Vec<ServerEnvValue> {
    SERVER_ENV
        .iter()
        .map(|setting| {
            let value = std::env::var(setting.key)
                .ok()
                .filter(|value| !value.trim().is_empty());
            ServerEnvValue {
                key: setting.key.to_string(),
                // Nothing on this platform distinguishes an inherited value
                // from a persisted one, and claiming the stronger of the two
                // would be a guess.
                persisted: false,
                value,
                fallback: setting.default.map(str::to_string),
            }
        })
        .collect()
}

/// The user-scope value of an environment variable, as it will be inherited by
/// the *next* process — which is the one that matters, since Ollama is started
/// at login and this app cannot see the environment it already has.
#[cfg(windows)]
fn read_user_env(key: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("reg")
        .args(["query", "HKCU\\Environment", "/v", key])
        // CREATE_NO_WINDOW. Without it every poll of this surface flashes a
        // console window on the user's desktop.
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    parse_reg_query(&text, key)
}

/// `reg query` prints `    NAME    REG_SZ    value`, and the value may itself
/// contain spaces.
fn parse_reg_query(output: &str, key: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with(key) {
            continue;
        }
        let rest = trimmed.strip_prefix(key)?.trim_start();
        // The type token, then the value.
        let mut parts = rest.splitn(2, char::is_whitespace);
        let _kind = parts.next()?;
        let value = parts.next()?.trim();
        if value.is_empty() {
            return None;
        }
        return Some(value.to_string());
    }
    None
}

#[cfg(windows)]
fn write_user_env(key: &str, value: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("setx")
        .args([key, value])
        .creation_flags(0x0800_0000)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

#[cfg(not(windows))]
fn write_user_env(_key: &str, _value: &str) -> Result<(), String> {
    // Nothing here can persist a variable for a login session the way `setx`
    // does, and writing a shell profile on the user's behalf is a larger
    // decision than this command is allowed to make (§81).
    Err("localAi.serverEnvUnsupported".into())
}

/// Whether the agent runner is on PATH.
///
/// A PATH walk rather than `where`/`which`: this is read on every poll of the
/// runtime surface, and spawning a process to answer a question about the
/// filesystem would cost more than the answer.
fn runner_installed() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let names: &[&str] = if cfg!(windows) {
        &["codex.exe", "codex.cmd", "codex.bat", "codex"]
    } else {
        &["codex"]
    };
    std::env::split_paths(&path).any(|dir| names.iter().any(|name| dir.join(name).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registry_value_is_read_out_of_the_line_it_sits_on() {
        let output = "\r\nHKEY_CURRENT_USER\\Environment\r\n    OLLAMA_CONTEXT_LENGTH    REG_SZ    65536\r\n\r\n";
        assert_eq!(
            parse_reg_query(output, "OLLAMA_CONTEXT_LENGTH").as_deref(),
            Some("65536")
        );
        assert_eq!(parse_reg_query(output, "OLLAMA_KV_CACHE_TYPE"), None);
    }

    /// Values with spaces are not truncated at the first one.
    #[test]
    fn a_value_containing_spaces_survives() {
        let output = "    OLLAMA_KV_CACHE_TYPE    REG_SZ    q8 0\r\n";
        assert_eq!(
            parse_reg_query(output, "OLLAMA_KV_CACHE_TYPE").as_deref(),
            Some("q8 0")
        );
    }

    #[test]
    fn an_unset_variable_falls_back_to_what_ollama_would_do() {
        let value = ServerEnvValue {
            key: "OLLAMA_CONTEXT_LENGTH".into(),
            value: None,
            fallback: Some("4096".into()),
            persisted: false,
        };
        assert_eq!(value.effective().as_deref(), Some("4096"));
    }
}
