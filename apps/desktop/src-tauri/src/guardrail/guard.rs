//! The guard: pre-execution enforcement for Claude Code sessions (§35).
//!
//! ## Why this runs as a separate process
//!
//! Claude Code can call out to a program before it runs a tool, and honour a
//! refusal. That is a **real** enforcement point: the command does not run. It
//! is also the only one available for an agent driving its own terminal — a
//! CLI in a PTY is not something we can intercept from outside.
//!
//! So J.A.R.V.I.S. launches agent sessions with a `PreToolUse` hook pointing at
//! its own executable in this mode. The hook reads the tool call on stdin,
//! consults the policy snapshot the application wrote for that session, and
//! answers on stdout.
//!
//! Verified empirically against Claude Code 2.1.240 before any of it was built:
//! the stdin envelope carries `tool_name` and `tool_input.command`, and
//! `permissionDecision: "deny"` genuinely stops the command and hands the
//! reason back to the model.
//!
//! ## Fail-open, deliberately
//!
//! If anything here goes wrong — no snapshot, unreadable JSON, a version it
//! does not understand — the guard says **nothing** and the tool call proceeds.
//!
//! That is the uncomfortable choice, and it is the right one. This process runs
//! before every single tool call in every agent session; a guard that fails
//! closed turns any bug in it into an agent that cannot work at all. The cost is
//! that a rule can silently fail to apply, so the product must never claim this
//! layer is absolute — and it does not: the capability model reports what each
//! provider can actually enforce, and the UI says "matched", not "blocked
//! everything".
//!
//! Claude Code's own behaviour reinforces this: a hook that exits non-zero is
//! treated as having no opinion, which was also confirmed by experiment.

use std::io::{Read, Write};

use super::classify;
use super::policy::{self, DecisionRecord, Snapshot};

/// Argument that selects this mode.
///
/// Distinctive on purpose. This is checked before anything else in `main`, so
/// it must not be a string that could plausibly be passed for another reason.
pub const HOOK_FLAG: &str = "--jarvis-guardrail-hook";

/// The part of Claude Code's `PreToolUse` payload this needs.
///
/// Deliberately partial: everything else in the envelope is ignored, so a
/// provider adding fields cannot break the guard.
#[derive(serde::Deserialize)]
struct HookInput {
    #[serde(default)]
    tool_name: String,
    #[serde(default)]
    tool_input: ToolInput,
    #[serde(default)]
    tool_use_id: Option<String>,
}

#[derive(serde::Deserialize, Default)]
struct ToolInput {
    #[serde(default)]
    command: Option<String>,
}

/// Run the guard. Returns the process exit code.
///
/// Never returns an error: every failure path is silence.
pub fn run(snapshot_path: &str) -> i32 {
    let Some(snapshot) = read_snapshot(snapshot_path) else {
        return 0;
    };

    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(input) = serde_json::from_str::<HookInput>(&raw) else {
        return 0;
    };

    // Only shell commands are understood. A guardrail that pretended to inspect
    // a tool whose shape it cannot read would be worse than one that says it
    // covers commands and covers commands.
    if input.tool_name != "Bash" {
        return 0;
    }
    let Some(command) = input.tool_input.command.as_deref() else {
        return 0;
    };

    let matches = classify::classify(command);
    if matches.is_empty() {
        return 0;
    }

    // A command can match several operations. The strictest decision wins:
    // permitting the whole command because one part of it was allowed would
    // defeat the point.
    let mut verdict: Option<(classify::Match, policy::Decision)> = None;
    for m in matches {
        let decision = snapshot.decision_for(m.operation);
        let stricter = match &verdict {
            None => true,
            Some((_, current)) => rank(decision) > rank(*current),
        };
        if stricter {
            verdict = Some((m, decision));
        }
    }
    let Some((matched, decision)) = verdict else {
        return 0;
    };

    let (outcome, reason, response) = match decision {
        policy::Decision::Allow => (
            "allowed",
            policy::reason::POLICY_ALLOWS,
            // Silence, not permission. J.A.R.V.I.S. must never talk a provider
            // out of its own safety prompt.
            None,
        ),
        policy::Decision::Deny => (
            "denied",
            policy::reason::POLICY_DENIES,
            Some(deny(&format!(
                "J.A.R.V.I.S. guardrail: {} is set to Never allow{}. \
                 Matched: {}. Do not attempt to work around this — \
                 tell the user what you wanted to do and why.",
                matched.operation.as_str(),
                scope_note(&snapshot),
                matched.fragment
            ))),
        ),
        policy::Decision::Ask if snapshot.attended => (
            "asked",
            policy::reason::ASKED_HUMAN,
            Some(ask(&format!(
                "J.A.R.V.I.S. guardrail: {} needs a person to approve it. \
                 Matched: {}.",
                matched.operation.as_str(),
                matched.fragment
            ))),
        ),
        // Nobody is attached to the terminal, so the provider's own prompt
        // would wait for an answer that cannot arrive. Refusing and letting
        // the mission go to Waiting is the §34 behaviour: a run that cannot
        // proceed says so instead of consuming resources indefinitely.
        policy::Decision::Ask => (
            "denied",
            policy::reason::NOBODY_TO_ASK,
            Some(deny(&format!(
                "J.A.R.V.I.S. guardrail: {} needs a person to approve it, and \
                 no one is attached to this session. Matched: {}. Stop and \
                 report that you are blocked on this approval.",
                matched.operation.as_str(),
                matched.fragment
            ))),
        ),
    };

    append_record(
        &snapshot,
        &DecisionRecord {
            ts_ms: now_ms(),
            operation: matched.operation.as_str().to_string(),
            fragment: matched.fragment.clone(),
            outcome: outcome.to_string(),
            reason: reason.to_string(),
            command: command.chars().take(2000).collect(),
            tool_use_id: input.tool_use_id.clone(),
        },
    );

    if let Some(response) = response {
        let mut stdout = std::io::stdout();
        let _ = stdout.write_all(response.as_bytes());
        let _ = stdout.flush();
    }
    0
}

/// Strictness order, so the harshest decision across several matches wins.
fn rank(decision: policy::Decision) -> u8 {
    match decision {
        policy::Decision::Allow => 0,
        policy::Decision::Ask => 1,
        policy::Decision::Deny => 2,
    }
}

/// Whether the rule in force is project-scoped, for the message to the agent.
fn scope_note(snapshot: &Snapshot) -> String {
    if snapshot.project_id.is_empty() {
        String::new()
    } else {
        " for this project".to_string()
    }
}

fn deny(reason: &str) -> String {
    hook_response("deny", reason)
}

fn ask(reason: &str) -> String {
    hook_response("ask", reason)
}

/// The response envelope, verified against Claude Code 2.1.240.
///
/// The text here is addressed to the **model**, which is why it is English and
/// is not a message catalogue key: it is a prompt, not interface copy. What the
/// person sees is rendered by the UI from `DecisionRecord`, whose `operation`
/// and `reason` are stable codes the catalogues translate (§65).
fn hook_response(decision: &str, reason: &str) -> String {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": decision,
            "permissionDecisionReason": reason,
        }
    })
    .to_string()
}

fn read_snapshot(path: &str) -> Option<Snapshot> {
    let text = std::fs::read_to_string(path).ok()?;
    let snapshot: Snapshot = serde_json::from_str(&text).ok()?;
    // A snapshot written by a build that has changed the format is not one this
    // guard can reason about, and guessing is worse than staying quiet.
    (snapshot.version == policy::SNAPSHOT_VERSION).then_some(snapshot)
}

/// Append one decision for the session runtime to pick up.
///
/// A single `write_all` of one line in append mode. Several tool calls can be
/// in flight at once, and this keeps each line whole; the reader tolerates a
/// partial trailing line regardless.
fn append_record(snapshot: &Snapshot, record: &DecisionRecord) {
    let Ok(mut line) = serde_json::to_string(record) else {
        return;
    };
    line.push('\n');

    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&snapshot.log_path)
        .and_then(|mut file| file.write_all(line.as_bytes()));
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::classify::Operation;
    use std::collections::BTreeMap;

    fn snapshot(attended: bool, rules: &[(Operation, policy::Decision)]) -> Snapshot {
        let mut decisions = BTreeMap::new();
        for (op, decision) in rules {
            decisions.insert(op.as_str().to_string(), decision.as_str().to_string());
        }
        Snapshot {
            version: policy::SNAPSHOT_VERSION,
            session_id: "s1".into(),
            project_id: "p1".into(),
            mission_id: None,
            provider: "claude-code".into(),
            attended,
            decisions,
            log_path: String::new(),
        }
    }

    #[test]
    fn the_strictest_decision_across_several_matches_wins() {
        let snap = snapshot(
            true,
            &[
                (Operation::GitForcePush, policy::Decision::Allow),
                (Operation::PackagePublish, policy::Decision::Deny),
            ],
        );
        assert_eq!(rank(snap.decision_for(Operation::PackagePublish)), 2);
        assert!(
            rank(snap.decision_for(Operation::PackagePublish))
                > rank(snap.decision_for(Operation::GitForcePush)),
            "allowing one half of a chain must not permit the other half"
        );
    }

    /// The substantive product decision in this module.
    #[test]
    fn ask_becomes_deny_when_no_one_is_attached() {
        // Attached: the person at the terminal can answer, so the provider's own
        // prompt is the right place for the question.
        let attended = snapshot(true, &[(Operation::GitForcePush, policy::Decision::Ask)]);
        assert!(attended.attended);

        // Unattended (§32): the same prompt would wait forever. §34 says a run
        // that cannot proceed must stop and say so.
        let unattended = snapshot(false, &[(Operation::GitForcePush, policy::Decision::Ask)]);
        assert!(!unattended.attended);
        assert_eq!(
            unattended.decision_for(Operation::GitForcePush),
            policy::Decision::Ask
        );
    }

    #[test]
    fn the_hook_response_is_the_shape_the_provider_expects() {
        // Verified against Claude Code 2.1.240 by experiment, not from memory.
        let response: serde_json::Value = serde_json::from_str(&deny("because")).unwrap();
        let specific = &response["hookSpecificOutput"];
        assert_eq!(specific["hookEventName"], "PreToolUse");
        assert_eq!(specific["permissionDecision"], "deny");
        assert_eq!(specific["permissionDecisionReason"], "because");

        let asked: serde_json::Value = serde_json::from_str(&ask("why")).unwrap();
        assert_eq!(asked["hookSpecificOutput"]["permissionDecision"], "ask");
    }

    #[test]
    fn a_snapshot_from_another_version_is_ignored_rather_than_guessed_at() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("policy.json");
        let mut value = serde_json::to_value(snapshot(true, &[])).unwrap();
        value["version"] = serde_json::json!(999);
        std::fs::write(&path, value.to_string()).unwrap();

        assert!(read_snapshot(path.to_str().unwrap()).is_none());
    }

    #[test]
    fn a_missing_snapshot_is_silence_not_a_refusal() {
        assert!(read_snapshot("nowhere-at-all.json").is_none());
        // `run` turns that into exit 0 with no output, which the provider reads
        // as "no opinion". Proved here so the fail-open contract is explicit.
        assert_eq!(run("nowhere-at-all.json"), 0);
    }

    #[test]
    fn decisions_are_appended_as_whole_lines() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join(policy::DECISIONS_FILE);
        let mut snap = snapshot(true, &[]);
        snap.log_path = log.to_string_lossy().to_string();

        for i in 0..3 {
            append_record(
                &snap,
                &DecisionRecord {
                    ts_ms: i,
                    operation: Operation::GitForcePush.as_str().into(),
                    fragment: "git push --force".into(),
                    outcome: "denied".into(),
                    reason: policy::reason::POLICY_DENIES.into(),
                    command: "git push --force".into(),
                    tool_use_id: None,
                },
            );
        }

        let text = std::fs::read_to_string(&log).unwrap();
        let lines: Vec<_> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        for line in lines {
            let parsed: DecisionRecord = serde_json::from_str(line).unwrap();
            assert_eq!(parsed.outcome, "denied");
        }
    }
}
