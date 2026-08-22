//! Provider adapters (§26).
//!
//! Claude Code and Codex are execution providers inside J.A.R.V.I.S., not the
//! product. The architecture must let a third arrive without reshaping anything
//! above it, so:
//!
//! * capabilities are **data**, never booleans hardcoded in a component;
//! * providers are not assumed to be equal — where one can do something the
//!   other cannot, the capability model says so rather than pretending.
//!
//! Concretely, Claude Code accepts `--session-id` and therefore has
//! deterministic transcript correlation. Codex 0.147.0 does not, and needs its
//! rollout directory watched instead. That is a genuine difference in kind and
//! is expressed as one.

pub mod claude;
pub mod codex;
pub mod conversation;
pub mod tail;

use serde::{Deserialize, Serialize};

/// How a provider's structured stream can be tied to a session we started.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Correlation {
    /// We assign the id, so the transcript path is known in advance.
    Deterministic,
    /// The provider assigns its own id; we identify the session by watching
    /// where it writes and matching working directory and start time.
    FileWatch,
    /// No structured stream — the terminal is all there is.
    None,
}

/// Where a provider's structured conversation comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConversationSource {
    /// A transcript file the provider writes while it runs.
    Transcript,
    /// A protocol we speak directly.
    Protocol,
    None,
}

/// Quality of usage reporting (§28).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageReporting {
    /// The provider states token counts itself.
    Official,
    /// We derive them from what the provider emits.
    Observed,
    None,
}

/// What a provider can do. Rendered from, never assumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCapabilities {
    pub id: String,
    pub name: String,
    /// Executable that must be on PATH.
    pub executable: String,

    pub correlation: Correlation,
    pub conversation: ConversationSource,
    pub usage: UsageReporting,

    /// Images can be attached to a prompt (§22).
    pub images: bool,
    /// A previous session can be resumed.
    pub resume: bool,
    /// The provider surfaces approval requests we can represent (§35).
    pub approvals: bool,
    /// Sessions can be started in a Git worktree (§45).
    pub worktrees: bool,
    /// The signed-in account can be switched.
    pub account_switching: bool,
}

/// A provider adapter.
pub trait Provider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;

    /// Arguments for starting a session, given the id J.A.R.V.I.S. assigned.
    ///
    /// A provider that cannot accept an externally supplied id simply ignores
    /// it — that is exactly what `Correlation::FileWatch` means.
    fn launch_args(&self, session_id: &str) -> Vec<String>;
}

/// Every provider this build knows about.
pub fn all() -> Vec<Box<dyn Provider>> {
    vec![
        Box::new(claude::ClaudeCode),
        Box::new(codex::Codex),
    ]
}

pub fn by_id(id: &str) -> Option<Box<dyn Provider>> {
    all().into_iter().find(|p| p.capabilities().id == id)
}

#[tauri::command]
pub fn list_providers() -> Vec<ProviderCapabilities> {
    all().iter().map(|p| p.capabilities()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_have_distinct_ids() {
        let ids: Vec<_> = all().iter().map(|p| p.capabilities().id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "provider ids must be unique");
    }

    /// The capability model exists to record real differences. If both
    /// providers ever describe themselves identically, it has stopped doing its
    /// job and the UI would be free to assume they are interchangeable.
    #[test]
    fn the_capability_model_distinguishes_the_two_providers() {
        let claude = claude::ClaudeCode.capabilities();
        let codex = codex::Codex.capabilities();

        assert_eq!(claude.correlation, Correlation::Deterministic);
        assert_eq!(codex.correlation, Correlation::FileWatch);
        assert_ne!(
            claude.correlation, codex.correlation,
            "these providers correlate sessions differently and must say so"
        );
    }

    #[test]
    fn only_claude_receives_our_session_id() {
        assert_eq!(
            claude::ClaudeCode.launch_args("SID"),
            vec!["--session-id".to_string(), "SID".to_string()]
        );
        assert!(
            !codex::Codex.launch_args("SID").iter().any(|a| a == "SID"),
            "Codex cannot be told our session id; claiming otherwise would break correlation"
        );
    }
}
