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

/// Where a session's name comes from (§88, D36).
///
/// The third genuine difference between these two providers, and it is the one
/// Session History is built on. Claude Code names its own sessions: it writes
/// an `ai-title` line into its transcript, and 89 of the 124 transcripts on
/// this machine carry one. Codex 0.147.0 does not — `set_thread_title` appears
/// in its rollouts only as a *tool definition inside the instructions*, never
/// as an event it emitted, and enumerating every event type across a day of
/// real rollouts finds no title at all.
///
/// Reported rather than smoothed over, for the same reason `UsageReporting` is:
/// a title a provider chose and a title we cut out of the first sentence
/// somebody typed are not the same claim, and a list that renders them
/// identically is asserting something the product does not know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TitleSupport {
    /// The provider names the session itself, and we take that name.
    Provider,
    /// No title is ever stated, so one is derived from what was typed first.
    Derived,
    /// Nothing to name — a plain shell is not a conversation.
    None,
}

/// How a past conversation is handed back to a provider (§88, D41).
///
/// Both CLIs resume, and they do it by **opposite mechanisms** — which matters
/// far more than the fact that both say yes, because the difference decides
/// whether this product can correlate the result at all. Measured on this
/// machine, not read off a help page:
///
/// * **Claude Code 2.1.241** forks. `--resume <id> --fork-session` honours our
///   `--session-id` and writes a *new* transcript named for it, opening with a
///   full copy of the prior conversation. Correlation survives.
/// * **Codex 0.147.0** appends. `codex resume <id>` writes into the rollout it
///   was given — 14 lines became 25, no new file. That rollout was created
///   before we launched, so `codex::correlate`, which matches on start time,
///   cannot find it by design.
///
/// So a single `resume: bool` would be true for both and useful for neither.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResumeSupport {
    /// Resuming produces a new transcript this product can own and follow.
    Fork,
    /// Resuming appends to the original transcript. Real, and **not wired up
    /// here**: following it needs locating by id instead of by correlation, and
    /// a boundary so the prior conversation is not mirrored twice. The pieces
    /// exist (`session_id_from_path`, `transcript::is_replayed_line`); the arm
    /// does not, and §81 says an unbuilt thing is absent rather than pretended.
    AppendInPlace,
    /// Nothing to resume — a shell has no conversation.
    None,
}

impl ResumeSupport {
    /// Whether **this build** can actually continue such a session.
    ///
    /// Deliberately narrower than "the provider can resume": offering a button
    /// that starts a fresh agent while claiming to continue a conversation
    /// would be worse than not offering it.
    pub fn is_available(self) -> bool {
        matches!(self, Self::Fork)
    }
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
    /// A previous session can be resumed. A statement about the *provider*;
    /// `resume_support` says whether this build can act on it.
    pub resume: bool,
    /// How a past conversation is handed back, and whether we can follow the
    /// result (§88, D41).
    pub resume_support: ResumeSupport,
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
    /// Where this provider's sessions get their names (§88).
    pub titles: TitleSupport,
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

    /// The difference Session History's titles turn on (§88, D36).
    ///
    /// Claude Code names its own sessions and Codex 0.147.0 does not. If these
    /// ever became equal, the surface would be free to present a title we cut
    /// out of somebody's first sentence as one the provider chose.
    #[test]
    fn only_a_provider_that_names_its_own_sessions_claims_to() {
        assert_eq!(
            claude::ClaudeCode.capabilities().titles,
            TitleSupport::Provider
        );
        assert_eq!(codex::Codex.capabilities().titles, TitleSupport::Derived);
        assert_ne!(
            claude::ClaudeCode.capabilities().titles,
            codex::Codex.capabilities().titles,
            "one of these writes its own title and the other never does; \
             collapsing them would let a derived title be shown as a stated one"
        );
    }

    /// Both providers resume, by opposite mechanisms, and only one of them can
    /// be followed by this build (§88, D41). Collapsing these would let the
    /// surface offer a Continue button that silently starts a fresh agent.
    #[test]
    fn resuming_is_only_offered_where_the_result_can_be_followed() {
        assert_eq!(
            claude::ClaudeCode.capabilities().resume_support,
            ResumeSupport::Fork
        );
        assert_eq!(
            codex::Codex.capabilities().resume_support,
            ResumeSupport::AppendInPlace
        );
        assert!(ResumeSupport::Fork.is_available());
        assert!(
            !ResumeSupport::AppendInPlace.is_available(),
            "appending to the original rollout is real and is not wired up; \n             claiming otherwise would offer a button that does not continue \n             anything"
        );
        assert!(!ResumeSupport::None.is_available());
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
