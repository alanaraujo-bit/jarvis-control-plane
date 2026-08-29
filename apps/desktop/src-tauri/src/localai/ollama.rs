//! The Ollama HTTP API, as much of it as this product actually needs.
//!
//! Everything here is a **read of the running server**, not of a configuration
//! file we hope it obeyed. That distinction is the whole point of the module:
//! a local model has no billing page and no rate-limit header, so the only
//! honest answers about what it will do come from asking the process that is
//! about to do it.
//!
//! Two endpoints carry almost all the value:
//!
//! * `/api/tags` — every model on disk, with its parameter count, quantisation
//!   and the context length its metadata declares.
//! * `/api/ps` — what is **resident right now**, and the two numbers that
//!   decide whether this machine is fast or slow: `size` against `size_vram`.
//!   When they are equal the model is entirely on the GPU; when `size_vram` is
//!   smaller the remainder is being run on the CPU and throughput collapses.
//!   Measured on this machine, `/api/ps` also reports the **effective**
//!   `context_length` the runner was loaded with — 65536 — while the agent CLI
//!   in front of it advertised 258400 from fallback metadata. Where the two
//!   disagree the resident runner is right, and the surface says so.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Ollama answers locally, so a request that has not completed in this long is
/// not slow — the server is gone, or a firewall swallowed the connection.
/// Short on purpose: this runs on a poll behind a live HUD.
const TIMEOUT: Duration = Duration::from_secs(4);

/// Pulling a model into VRAM is not a poll. A cold 27B Q4 load off an NVMe was
/// measured at roughly ten seconds on this machine, and a first load after a
/// reboot is slower still.
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// A model installed on this machine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledModel {
    pub name: String,
    /// Bytes on disk.
    pub size_bytes: u64,
    /// `27.3B`, exactly as Ollama spells it.
    pub parameter_size: Option<String>,
    /// `Q4_K_M`.
    pub quantization: Option<String>,
    /// The maximum this model's own metadata declares. Not what it is loaded
    /// with — see `ResidentModel::context_length` for that.
    pub max_context: Option<u64>,
    /// `tools`, `thinking`, `vision`, `completion`.
    ///
    /// Reported rather than assumed because it decides whether the model can
    /// be an agent at all: a model without `tools` cannot call one, and
    /// launching a coding agent on it would produce a session that talks about
    /// editing files and never edits one.
    pub capabilities: Vec<String>,
    pub modified_at: Option<String>,
}

impl InstalledModel {
    /// Whether this model can drive an agent session.
    pub fn supports_tools(&self) -> bool {
        self.capabilities.iter().any(|c| c == "tools")
    }
}

/// A model that is loaded right now.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResidentModel {
    pub name: String,
    /// Total bytes the runner occupies.
    pub size_bytes: u64,
    /// How many of those bytes are on the GPU. Equal to `size_bytes` means
    /// every layer is offloaded; anything less is running partly on the CPU.
    pub size_vram_bytes: u64,
    /// The context window this runner was **actually** loaded with.
    pub context_length: Option<u64>,
    /// When Ollama will evict it, unless something touches it first.
    pub expires_at: Option<String>,
}

impl ResidentModel {
    /// Share of the model held on the GPU, 0.0–1.0.
    pub fn gpu_fraction(&self) -> f64 {
        if self.size_bytes == 0 {
            return 0.0;
        }
        (self.size_vram_bytes as f64 / self.size_bytes as f64).clamp(0.0, 1.0)
    }

    /// Whether any part of this model is being run on the CPU.
    ///
    /// The single most useful predictor of a bad session: a 27B Q4 that fits
    /// entirely in 24 GB runs at ~72 tok/s, and the same model with a few
    /// layers spilled runs at a small fraction of that. A person watching
    /// throughput drop deserves the cause, not just the symptom.
    pub fn spilled_to_cpu(&self) -> bool {
        self.size_vram_bytes < self.size_bytes
    }
}

fn base(endpoint: &str) -> String {
    endpoint.trim_end_matches('/').to_string()
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(2))
        .build()
}

/// The server's version, or `None` when nothing is listening.
///
/// This is the reachability check the whole surface hangs off, and it is
/// deliberately the cheapest endpoint: asking `/api/tags` to find out whether
/// the server exists would read a model index to answer a yes/no question.
pub fn version(endpoint: &str) -> Option<String> {
    let response = agent()
        .get(&format!("{}/api/version", base(endpoint)))
        .timeout(TIMEOUT)
        .call()
        .ok()?;
    let value: serde_json::Value = serde_json::from_reader(response.into_reader()).ok()?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Every model installed on this machine.
pub fn installed(endpoint: &str) -> Result<Vec<InstalledModel>, String> {
    let response = agent()
        .get(&format!("{}/api/tags", base(endpoint)))
        .timeout(TIMEOUT)
        .call()
        .map_err(|e| e.to_string())?;
    let value: serde_json::Value =
        serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string())?;
    Ok(parse_installed(&value))
}

pub fn parse_installed(value: &serde_json::Value) -> Vec<InstalledModel> {
    let Some(models) = value.get("models").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|model| {
            let name = model.get("name").and_then(serde_json::Value::as_str)?;
            let details = model.get("details");
            Some(InstalledModel {
                name: name.to_string(),
                size_bytes: model
                    .get("size")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                parameter_size: details
                    .and_then(|d| d.get("parameter_size"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                quantization: details
                    .and_then(|d| d.get("quantization_level"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
                max_context: details
                    .and_then(|d| d.get("context_length"))
                    .and_then(serde_json::Value::as_u64),
                capabilities: model
                    .get("capabilities")
                    .and_then(serde_json::Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
                modified_at: model
                    .get("modified_at")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// What is loaded into memory right now.
pub fn resident(endpoint: &str) -> Result<Vec<ResidentModel>, String> {
    let response = agent()
        .get(&format!("{}/api/ps", base(endpoint)))
        .timeout(TIMEOUT)
        .call()
        .map_err(|e| e.to_string())?;
    let value: serde_json::Value =
        serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string())?;
    Ok(parse_resident(&value))
}

pub fn parse_resident(value: &serde_json::Value) -> Vec<ResidentModel> {
    let Some(models) = value.get("models").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|model| {
            let name = model.get("name").and_then(serde_json::Value::as_str)?;
            let size = model
                .get("size")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            Some(ResidentModel {
                name: name.to_string(),
                size_bytes: size,
                // Absent means we do not know how much is on the GPU, and
                // guessing `size` would report a perfect offload for a model
                // that may be running half on the CPU. Zero is the value that
                // makes the surface say "unknown" rather than "excellent".
                size_vram_bytes: model
                    .get("size_vram")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0),
                context_length: model
                    .get("context_length")
                    .and_then(serde_json::Value::as_u64),
                expires_at: model
                    .get("expires_at")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect()
}

/// Load a model into memory without generating anything.
///
/// An empty prompt is Ollama's documented preload: the runner starts, the
/// weights land in VRAM and nothing is sampled. `keep_alive` is passed in the
/// same call because eviction is a property of the load, not a separate
/// setting — see `crate::localai::RuntimeConfig::keep_alive`.
///
/// Deliberately **no** `options` are sent. Passing `num_ctx` here would load a
/// runner the agent CLI then fails to match — its own requests carry no such
/// option, so Ollama would tear the runner down and rebuild it at the server
/// default on the first real turn, costing a full reload and quietly
/// contradicting the number this app had just displayed. The context window is
/// a server-level setting and is managed as one.
pub fn load(endpoint: &str, model: &str, keep_alive: &str) -> Result<(), String> {
    agent()
        .post(&format!("{}/api/generate", base(endpoint)))
        .timeout(LOAD_TIMEOUT)
        .set("content-type", "application/json")
        .send_string(
            &serde_json::json!({ "model": model, "prompt": "", "keep_alive": keep_alive })
                .to_string(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Evict a model from memory now, freeing its VRAM.
///
/// `keep_alive: 0` is Ollama's own spelling for "unload immediately".
pub fn unload(endpoint: &str, model: &str) -> Result<(), String> {
    agent()
        .post(&format!("{}/api/generate", base(endpoint)))
        .timeout(TIMEOUT)
        .set("content-type", "application/json")
        .send_string(
            &serde_json::json!({ "model": model, "prompt": "", "keep_alive": 0 }).to_string(),
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `/api/tags` shape this machine actually returns, trimmed.
    #[test]
    fn an_installed_model_is_read_with_what_decides_whether_it_can_be_an_agent() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"models":[{"name":"qwen3.8:latest","size":17741872154,
                "details":{"parameter_size":"27.3B","quantization_level":"Q4_K_M",
                "context_length":262144},
                "capabilities":["completion","tools","thinking","vision"]}]}"#,
        )
        .unwrap();
        let models = parse_installed(&value);
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].parameter_size.as_deref(), Some("27.3B"));
        assert_eq!(models[0].quantization.as_deref(), Some("Q4_K_M"));
        assert_eq!(models[0].max_context, Some(262144));
        assert!(
            models[0].supports_tools(),
            "a model that lists `tools` can drive an agent session"
        );
    }

    /// A model with no `tools` capability must not be offered as an agent, and
    /// the only thing standing between that and a session which narrates edits
    /// it never makes is this flag being read rather than assumed.
    #[test]
    fn a_model_without_tools_cannot_drive_an_agent() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"models":[{"name":"embed:latest","size":1,
                "capabilities":["completion"]}]}"#,
        )
        .unwrap();
        assert!(!parse_installed(&value)[0].supports_tools());
    }

    /// The measurement the whole predictability story rests on.
    #[test]
    fn a_fully_offloaded_model_is_distinguished_from_a_spilled_one() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"models":[{"name":"qwen3.8:latest","size":17536059963,
                "size_vram":17536059963,"context_length":65536,
                "expires_at":"2026-08-29T04:38:24.1444209-03:00"}]}"#,
        )
        .unwrap();
        let resident = parse_resident(&value);
        assert_eq!(resident[0].context_length, Some(65536));
        assert!(!resident[0].spilled_to_cpu());
        assert_eq!(resident[0].gpu_fraction(), 1.0);

        let spilled = ResidentModel {
            size_vram_bytes: 8_000_000_000,
            ..resident[0].clone()
        };
        assert!(
            spilled.spilled_to_cpu(),
            "layers on the CPU are the difference between 72 tok/s and a crawl"
        );
        assert!(spilled.gpu_fraction() < 0.5);
    }

    /// `size_vram` missing must not read as a perfect offload.
    #[test]
    fn an_unreported_vram_split_is_not_reported_as_success() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"models":[{"name":"m","size":100}]}"#).unwrap();
        let resident = parse_resident(&value);
        assert_eq!(resident[0].size_vram_bytes, 0);
        assert!(resident[0].spilled_to_cpu());
    }

    #[test]
    fn nothing_resident_is_an_empty_list_not_an_error() {
        let value: serde_json::Value = serde_json::from_str(r#"{"models":[]}"#).unwrap();
        assert!(parse_resident(&value).is_empty());
    }

    #[test]
    fn a_trailing_slash_in_the_endpoint_does_not_double_up() {
        assert_eq!(base("http://127.0.0.1:11434/"), "http://127.0.0.1:11434");
        assert_eq!(base("http://127.0.0.1:11434"), "http://127.0.0.1:11434");
    }
}
