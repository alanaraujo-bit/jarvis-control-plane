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

/// How far guardrails can actually reach into this provider (§35).
///
/// The whole point of the capability model is that providers are not equal, and
/// this is the sharpest instance of it: one of these values means a command can
/// be stopped before it runs, and the other means it cannot. Presenting them
/// the same way would be the product claiming a protection it does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuardrailSupport {
    /// The provider consults us before running a tool and honours a refusal.
    /// Claude Code does this through a pre-tool hook, installed per session.
    PreExecution,
    /// The same mechanism exists, but the provider will not run our hook until
    /// the person has reviewed and trusted it themselves.
    ///
    /// Codex 0.149.0 is here: it has PreToolUse hooks with the same wire shape,
    /// and deliberately refuses to run a hook that arrived from outside until
    /// it has been trusted in its own interface. That is a sound decision on
    /// their part — a tool that silently ran hook programs dropped into a
    /// project directory would be a hazard — and it means enforcement is real
    /// but not automatic. Until then such a session is observed, not guarded,
    /// and the UI must say which of the two it is.
    PreExecutionWhenTrusted,
    /// No callback exists, so a matched operation is recorded after the fact
    /// and never prevented.
    Observed,
    /// Nothing to govern — a plain shell is the user typing on their own
    /// machine, and guardrails govern agents.
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
    /// Whether a sensitive operation can be stopped before it runs (§35).
    pub guardrails: GuardrailSupport,
    /// Sessions can be started in a Git worktree (§45).
    pub worktrees: bool,
    /// How a project brief can reach this provider (§38).
    pub briefing: BriefingSupport,
    /// The signed-in account can be switched.
    pub account_switching: bool,
}

/// How a project brief can be handed to a session before it starts (§38).
///
/// This is a capability, not a preference, and it is the reason the Brain has
/// to be told what a provider can do rather than assuming (§26).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BriefingSupport {
    /// The brief is passed out of band, as an appended system prompt read from
    /// a file **we** own. Nothing is written into the user's repository and
    /// nothing appears in their terminal.
    ///
    /// Claude Code takes `--append-system-prompt-file`. Verified rather than
    /// read off a help page: with the flag a fresh session answers a question
    /// only the brief could have taught it, and without the flag the same
    /// session does not — see `claude::briefing_capability`.
    SystemPrompt,
    /// No out-of-band route was found, so a brief could only arrive as the
    /// session's opening message — visible in the terminal and spending a turn.
    ///
    /// Codex 0.147.0 is here: its `--help` lists no equivalent flag, and its
    /// instructions arrive as the prompt argument or on stdin. Reported rather
    /// than papered over, because a person choosing a provider for an
    /// unattended run should know which of the two they are getting.
    OpeningMessage,
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

    /// The difference guardrails turn on (§35).
    ///
    /// Claude Code can be stopped before a tool runs; Codex 0.149.0 cannot. If
    /// these ever became equal the UI would be free to promise enforcement it
    /// does not have for one of them.
    #[test]
    fn only_a_provider_that_can_be_stopped_claims_pre_execution_guardrails() {
        assert_eq!(
            claude::ClaudeCode.capabilities().guardrails,
            GuardrailSupport::PreExecution
        );
        assert_eq!(
            codex::Codex.capabilities().guardrails,
            GuardrailSupport::PreExecutionWhenTrusted
        );
        assert_ne!(
            claude::ClaudeCode.capabilities().guardrails,
            codex::Codex.capabilities().guardrails,
            "one of these enforces on install and the other only after the user \
             trusts the hook; collapsing them would promise protection that is \
             not switched on yet"
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
