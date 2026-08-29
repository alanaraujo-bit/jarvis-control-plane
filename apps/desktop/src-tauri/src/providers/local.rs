//! The local model adapter (§26, §92).
//!
//! Verified against Codex CLI 0.150.1 driving Ollama 0.33.2 with
//! `qwen3.8:27b-q4_K_M` on this machine.
//!
//! This provider runs the same agent binary as `codex`, pointed at a model in
//! the GPU rather than at OpenAI. That makes most of its capabilities identical
//! by construction — the rollout envelope is the same file format, so
//! correlation, the conversation view, guardrail hooks and worktrees all behave
//! as they do for Codex — and it makes three of them genuinely different. The
//! point of this module is those three.
//!
//! ## What is really different
//!
//! **There is no account.** A local run's `token_count` event carries a
//! `rate_limits` object with every field null: no limit id, no plan, no window.
//! Nothing meters a model you own, so `account_switching` is false and the
//! runtime surface reports VRAM headroom where the others report an allowance.
//!
//! **Usage is reported, but a rate limit is not.** The same event carries real
//! `input_tokens` and `output_tokens`, so `UsageReporting::Official` is the
//! honest value — the counts come from the runner, not from us estimating.
//!
//! **The transcript root is ours.** Local sessions run with a `CODEX_HOME`
//! this app owns, so their rollouts cannot be confused with a cloud Codex
//! session's. See `crate::localai` for why that is a correctness requirement
//! and not tidiness.

use super::{
    BriefingSupport, ConversationSource, Correlation, GuardrailSupport, Provider,
    ProviderCapabilities, ResumeSupport, TitleSupport, UsageReporting,
};

pub struct LocalModel;

impl Provider for LocalModel {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            id: crate::localai::PROVIDER_ID.into(),
            name: "Local".into(),
            // The same binary as Codex, configured differently. Naming the
            // real executable is what lets the environment scan and the launch
            // failure message point at something a person can install.
            executable: "codex".into(),
            // Same rollout tree shape, same lack of an id flag — but into a
            // directory only this provider writes, which makes the match
            // stronger in practice than Codex's even though it is the same
            // mechanism.
            correlation: Correlation::FileWatch,
            conversation: ConversationSource::Transcript,
            // Measured: `token_count` carries real input and output counts
            // from the local runner.
            usage: UsageReporting::Official,
            // Whether the *model* can see an image is a property of the model,
            // not of this adapter — `qwen3.8` lists `vision` and a 7B code
            // model will not. The runtime surface reports each model's
            // capabilities from Ollama; claiming it here for every one of them
            // would be a promise this provider cannot keep.
            images: false,
            resume: true,
            // Same append-in-place behaviour as Codex, and equally not wired
            // up. §81: an unbuilt thing is absent, not pretended.
            resume_support: ResumeSupport::AppendInPlace,
            approvals: true,
            // Identical mechanism to Codex — the hook file is written into the
            // project and waits to be trusted.
            guardrails: GuardrailSupport::PreExecutionWhenTrusted,
            worktrees: true,
            briefing: BriefingSupport::OpeningMessage,
            // Nothing to switch. There is no account behind a local model.
            account_switching: false,
            titles: TitleSupport::Derived,
        }
    }

    fn launch_args(&self, _session_id: &str) -> Vec<String> {
        // The real arguments depend on the runtime configuration — which model,
        // which endpoint — and are built in `crate::localai::launch_args`. This
        // trait method takes only a session id, and this provider cannot be
        // told one, so there is nothing honest to return here.
        Vec::new()
    }
}
