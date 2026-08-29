//! Wiring a live agent session to the guard (§35).
//!
//! Three moving parts, and the direction of each matters:
//!
//! 1. **The application writes** a resolved policy snapshot into the session's
//!    log directory, and a provider settings file pointing the provider's
//!    pre-tool hook at our own executable in guard mode.
//! 2. **The guard writes** what it decided to its own append-only JSONL, in a
//!    separate process, once per tool call.
//! 3. **The session runtime reads** that JSONL and appends `Approval` frames to
//!    the session log.
//!
//! The third step is the one that has to be this way. A session log has exactly
//! one writer (D2) — that is the invariant the whole architecture is built to
//! protect. The guard is a different process and can never be that writer, so
//! it hands its decisions over the same way a provider hands over its
//! transcript: by appending to a file we follow.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::db::Database;
use crate::providers::tail::JsonlTailer;
use crate::session::event::EventKind;
use crate::session::manager::LiveSession;

use super::policy::{self, DecisionRecord, Snapshot, DECISIONS_FILE, SNAPSHOT_FILE};
use super::{NewEvent, Operation, Origin, Status};

/// How often the guard's decision log is polled.
///
/// Slower than the transcript tailer: these are rare events, and a second's
/// delay before a refusal appears in the timeline costs nothing. The refusal
/// itself already happened — the guard did not wait for us.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Whether a guardrail hook is worth installing for this provider at all.
///
/// True for both agent providers, for different reasons. Claude Code runs the
/// hook as soon as it is pointed at one. Codex 0.149.0 has the same mechanism
/// but will not run a hook until the person has trusted it in Codex's own
/// interface — so the file is written and waits there to be trusted, rather
/// than being withheld until somebody thinks to ask for it.
///
/// What each one actually delivers is reported by the capability model, never
/// assumed in the UI (§26).
pub fn installs_hook(provider: &str) -> bool {
    matches!(provider, "claude-code" | "codex" | "local")
}

/// Whether a session of this provider is guarded from the moment it starts.
pub fn enforces_before_execution(provider: &str) -> bool {
    provider == "claude-code"
}

/// Write the policy snapshot for a session and return its path.
pub fn write_snapshot(
    db: &Database,
    log_dir: &Path,
    session_id: &str,
    project_id: &str,
    mission_id: Option<&str>,
    provider: &str,
    attended: bool,
    driven: bool,
) -> std::io::Result<PathBuf> {
    let decisions = db
        .with(|conn| Ok(policy::resolve_all(conn, Some(project_id))?))
        .unwrap_or_default()
        .into_iter()
        .map(|(op, resolved)| (op.as_str().to_string(), resolved.decision.as_str().to_string()))
        .collect();

    let snapshot = Snapshot {
        version: policy::SNAPSHOT_VERSION,
        session_id: session_id.to_string(),
        project_id: project_id.to_string(),
        mission_id: mission_id.map(str::to_string),
        provider: provider.to_string(),
        attended,
        driven,
        decisions,
        log_path: log_dir.join(DECISIONS_FILE).to_string_lossy().to_string(),
    };

    std::fs::create_dir_all(log_dir)?;
    let path = log_dir.join(SNAPSHOT_FILE);
    std::fs::write(&path, serde_json::to_vec_pretty(&snapshot)?)?;
    Ok(path)
}

/// Record whether an autopilot is occupying the human's seat (§32).
///
/// Separate from `set_attended` because they answer different questions: one is
/// "is anyone looking", the other is "is the seat free". A driven session very
/// often has both a viewer and no one able to answer.
pub fn set_driven(log_dir: &Path, driven: bool) {
    let path = log_dir.join(SNAPSHOT_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut snapshot) = serde_json::from_str::<Snapshot>(&text) else {
        return;
    };
    if snapshot.driven == driven {
        return;
    }
    snapshot.driven = driven;
    if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
        let _ = std::fs::write(&path, bytes);
    }
}

/// Rewrite a snapshot in place, changing only whether a view is attached.
///
/// Called on attach and detach. The difference between "ask the person at the
/// terminal" and "there is nobody to ask" is a property of *now*, not of when
/// the session started, so it cannot be decided once at spawn.
pub fn set_attended(log_dir: &Path, attended: bool) {
    let path = log_dir.join(SNAPSHOT_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut snapshot) = serde_json::from_str::<Snapshot>(&text) else {
        return;
    };
    if snapshot.attended == attended {
        return;
    }
    snapshot.attended = attended;
    if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
        let _ = std::fs::write(&path, bytes);
    }
}

/// Re-resolve the rules in an existing snapshot.
///
/// Called when policy changes, so a setting takes effect on the next tool call
/// rather than on the next session. A guardrail you have to restart the agent
/// to apply is one people stop trusting.
pub fn refresh(db: &Database, log_dir: &Path) {
    let path = log_dir.join(SNAPSHOT_FILE);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return;
    };
    let Ok(mut snapshot) = serde_json::from_str::<Snapshot>(&text) else {
        return;
    };

    let Ok(resolved) = db.with(|conn| Ok(policy::resolve_all(conn, Some(&snapshot.project_id))?))
    else {
        return;
    };
    snapshot.decisions = resolved
        .into_iter()
        .map(|(op, r)| (op.as_str().to_string(), r.decision.as_str().to_string()))
        .collect();

    if let Ok(bytes) = serde_json::to_vec_pretty(&snapshot) {
        let _ = std::fs::write(&path, bytes);
    }
}

/// Write the provider settings file that points at the guard, if we can.
///
/// Returns `None` when the executable cannot be located or the file cannot be
/// written. The caller then launches the session **without** the flag: a
/// guardrail that cannot be installed must not stop the agent from running, and
/// must not be claimed either.
pub fn write_hook_settings(log_dir: &Path, snapshot_path: &Path) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;

    // Quoted because both the executable path and the snapshot path routinely
    // contain spaces on Windows — `C:\Users\Alan Araujo\…` is the normal case
    // here, not an edge one.
    let command = format!(
        "\"{}\" {} \"{}\"",
        exe.display(),
        super::guard::HOOK_FLAG,
        snapshot_path.display()
    );

    let settings = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                // Only shell commands. The classifier reads command lines; it
                // would be dishonest to claim coverage of tools whose shape it
                // cannot read.
                "matcher": "Bash",
                "hooks": [{ "type": "command", "command": command }]
            }]
        }
    });

    let path = log_dir.join("guardrail-settings.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&settings).ok()?).ok()?;
    Some(path)
}

/// Install the guardrail hook for a Codex session.
///
/// Codex discovers hooks at `.codex/hooks/hooks.json` inside the project rather
/// than through a command-line flag, and will not run one until the person has
/// reviewed and trusted it in Codex's own interface. Writing it is therefore an
/// offer, not an installation — which is exactly what the capability
/// `PreExecutionWhenTrusted` reports.
///
/// This writes into the **user's project directory**, so it is deliberately
/// conservative: it never overwrites a hooks file that is already there. That
/// file is theirs, and a guardrail that quietly replaced someone's own
/// configuration would be doing the very thing it exists to prevent.
pub fn write_codex_hook(project_dir: &Path, snapshot_path: &Path) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = project_dir.join(".codex").join("hooks").join("hooks.json");

    if path.exists() {
        // Something is already there. Ours or theirs, it is not ours to
        // replace; we only report a hook when the one present is ours.
        return std::fs::read_to_string(&path)
            .ok()
            .filter(|existing| existing.contains(super::guard::HOOK_FLAG))
            .map(|_| path);
    }

    let hooks = serde_json::json!({
        "hooks": {
            "PreToolUse": [{
                "hooks": [{
                    "type": "command",
                    "command": format!(
                        "\"{}\" {} \"{}\"",
                        exe.display(),
                        super::guard::HOOK_FLAG,
                        snapshot_path.display()
                    )
                }]
            }]
        }
    });

    std::fs::create_dir_all(path.parent()?).ok()?;
    std::fs::write(&path, serde_json::to_vec_pretty(&hooks).ok()?).ok()?;
    Some(path)
}

/// Follow the guard's decision log for the lifetime of a session.
pub fn spawn_watcher(
    session: Arc<LiveSession>,
    db: Arc<Database>,
    log_dir: PathBuf,
    project_id: String,
    mission_id: Option<String>,
    stop: Arc<AtomicBool>,
) {
    let session_id = session.id.clone();
    std::thread::Builder::new()
        .name(format!("guardrail-{session_id}"))
        .spawn(move || {
            let mut tailer = JsonlTailer::new(log_dir.join(DECISIONS_FILE));
            while !stop.load(Ordering::Relaxed) {
                if let Ok(lines) = tailer.poll() {
                    for line in lines {
                        if let Ok(record) = serde_json::from_str::<DecisionRecord>(&line) {
                            absorb(&session, &db, &project_id, mission_id.as_deref(), &record);
                        }
                    }
                }
                std::thread::sleep(POLL_INTERVAL);
            }
        })
        .expect("spawn guardrail watcher");
}

/// Whether a guard decision left the agent unable to go on (§49).
///
/// Extracted so it can be tested: the difference between this and
/// `status == Denied` is one notification per policy refusal, which for
/// somebody who has set an operation to Never allow is a notification every
/// time the guardrail does exactly what they asked for.
fn stops_the_agent(status: Status, reason: &str) -> bool {
    status == Status::Denied && reason == policy::reason::NOBODY_TO_ASK
}

/// Take one decision the guard made into the session log and the database.
fn absorb(
    session: &LiveSession,
    db: &Database,
    project_id: &str,
    mission_id: Option<&str>,
    record: &DecisionRecord,
) {
    // Into the log first, through the session's single writer. The frame kind
    // was reserved for this from the beginning (event kind 50).
    if let Ok(payload) = serde_json::to_vec(record) {
        session.log(EventKind::Approval, payload);
    }

    let Some(operation) = Operation::parse(&record.operation) else {
        return;
    };
    let status = match record.outcome.as_str() {
        "denied" => Status::Denied,
        "allowed" => Status::Allowed,
        _ => Status::Asked,
    };

    let _ = super::record(
        db,
        NewEvent {
            project_id: Some(project_id),
            session_id: Some(&session.id),
            mission_id,
            criterion_id: None,
            origin: Origin::Agent,
            operation,
            fragment: &record.fragment,
            command: &record.command,
            status,
            reason: &record.reason,
        },
    );

    // An allowed operation is not news. A refusal is — it changed what the
    // agent could do, and the §48/§49 bar for a row is "would a person want to
    // know this?".
    if status == Status::Denied {
        crate::activity::record(
            db,
            "guardrail.denied",
            crate::activity::Severity::Attention,
            operation.as_str(),
            Some(record.command.clone()),
            Some(project_id),
            Some(&session.id),
            mission_id,
        );
    }

    // The one refusal worth interrupting somebody for (§49).
    //
    // Not every denial. A policy that says *never* is the guardrail doing its
    // job: the agent is told no, reports it, and carries on, and that is
    // already in Activity. `NOBODY_TO_ASK` is the different case — the rule
    // said *ask*, there was nobody who could answer, and the agent is now
    // stopped on something only a person can unstick.
    //
    // A guardrail set to *ask* with somebody available is deliberately not
    // raised either: Claude Code then draws its own question on the terminal,
    // which `notify`'s watcher already sees with a better preview, because it
    // reads what the provider actually wrote. Raising both would notify twice
    // for one question, which is precisely the noise that makes people stop
    // reading.
    if stops_the_agent(status, &record.reason) {
        crate::notify::bus::raise(
            crate::notify::Reason::GuardrailBlocked,
            // The guard reported its own decision (§28).
            crate::session::event::Confidence::Official,
            crate::notify::Raise {
                session_id: Some(session.id.clone()),
                project_id: Some(project_id.to_string()),
                mission_id: mission_id.map(str::to_string),
                // No detail code: the preview already names the operation and
                // the command, which says more than the refusal's own reason.
                preview: Some(format!("{}: {}", operation.as_str(), record.command)),
                ..Default::default()
            },
        );
    }

    // Nobody was attached to answer, so the agent has been stopped rather than
    // left waiting on a prompt that could never be answered (§34). The mission
    // has to say so, or the run is silently going nowhere.
    if status == Status::Denied && record.reason == policy::reason::NOBODY_TO_ASK {
        if let Some(mission_id) = mission_id {
            let reason = format!(
                "A guardrail needs approval for {} and no one was attached to the session. \
                 Decide the policy for this operation, then start the mission again.",
                operation.as_str()
            );
            let _ = crate::mission::store::set_status(
                db,
                mission_id,
                crate::mission::model::MissionStatus::Waiting,
                Some(reason),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::policy::Decision;

    /// The line between "the guardrail did its job" and "somebody has to come
    /// and unstick this" (§49).
    ///
    /// A policy set to Never allow refuses, the agent is told, and it carries
    /// on — notifying there would interrupt somebody every time the setting
    /// they chose is honoured. `NOBODY_TO_ASK` is the other case: the rule
    /// said *ask*, there was no one to ask, and nothing moves until a person
    /// changes something.
    #[test]
    fn only_a_refusal_nobody_could_answer_is_worth_interrupting_somebody_for() {
        assert!(stops_the_agent(Status::Denied, policy::reason::NOBODY_TO_ASK));
        assert!(!stops_the_agent(Status::Denied, policy::reason::POLICY_DENIES));
        assert!(!stops_the_agent(Status::Asked, policy::reason::ASKED_HUMAN));
        assert!(!stops_the_agent(Status::Allowed, policy::reason::POLICY_ALLOWS));
    }

    fn db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'demo', 'C:/demo', 1, 1)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    #[test]
    fn a_snapshot_carries_every_operation_already_resolved() {
        let db = db();
        db.with(|conn| {
            Ok(policy::set(
                conn,
                Some("p1"),
                Operation::GitForcePush,
                Some(Decision::Deny),
                1,
            )?)
        })
        .unwrap();

        let dir = tempfile::tempdir().unwrap();
        let path =
            write_snapshot(&db, dir.path(), "s1", "p1", None, "claude-code", true, false).unwrap();

        let snapshot: Snapshot =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(snapshot.decisions.len(), super::super::classify::ALL.len());
        assert_eq!(
            snapshot.decision_for(Operation::GitForcePush),
            Decision::Deny
        );
        // Everything else falls through to the default.
        assert_eq!(
            snapshot.decision_for(Operation::PackagePublish),
            Decision::Ask
        );
        assert!(snapshot.attended);
    }

    #[test]
    fn attaching_and_detaching_a_view_changes_the_snapshot() {
        // The guard's whole Unattended branch turns on this flag being current.
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(&db, dir.path(), "s1", "p1", None, "claude-code", false, false).unwrap();

        let read = || -> Snapshot {
            serde_json::from_str(
                &std::fs::read_to_string(dir.path().join(SNAPSHOT_FILE)).unwrap(),
            )
            .unwrap()
        };
        assert!(!read().attended);

        set_attended(dir.path(), true);
        assert!(read().attended);

        set_attended(dir.path(), false);
        assert!(!read().attended);
    }

    /// Watching an autopilot is not the same as being able to answer it (§32).
    #[test]
    fn a_driven_session_reports_that_nobody_can_answer() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(&db, dir.path(), "s1", "p1", None, "claude-code", true, true).unwrap();

        let read = || -> Snapshot {
            serde_json::from_str(
                &std::fs::read_to_string(dir.path().join(SNAPSHOT_FILE)).unwrap(),
            )
            .unwrap()
        };
        // A person is watching, and still cannot answer: the seat is taken.
        assert!(read().attended);
        assert!(read().driven);
        assert!(!read().can_ask_a_person());

        // Handing the seat back makes the question answerable again.
        set_driven(dir.path(), false);
        assert!(read().can_ask_a_person());
    }

    #[test]
    fn changing_policy_reaches_a_session_that_is_already_running() {
        let db = db();
        let dir = tempfile::tempdir().unwrap();
        write_snapshot(&db, dir.path(), "s1", "p1", None, "claude-code", true, false).unwrap();

        db.with(|conn| {
            Ok(policy::set(
                conn,
                Some("p1"),
                Operation::RecursiveDelete,
                Some(Decision::Deny),
                2,
            )?)
        })
        .unwrap();
        refresh(&db, dir.path());

        let snapshot: Snapshot = serde_json::from_str(
            &std::fs::read_to_string(dir.path().join(SNAPSHOT_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            snapshot.decision_for(Operation::RecursiveDelete),
            Decision::Deny
        );
    }

    #[test]
    fn the_hook_settings_quote_paths_that_contain_spaces() {
        // Not hypothetical: this machine's own paths contain a space, and an
        // unquoted command would invoke a program that does not exist.
        let dir = tempfile::tempdir().unwrap();
        let snapshot = dir.path().join("a b").join(SNAPSHOT_FILE);
        let path = write_hook_settings(dir.path(), &snapshot).unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let command = value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();

        assert!(command.starts_with('"'), "the executable must be quoted");
        assert!(command.contains(super::super::guard::HOOK_FLAG));
        assert!(command.ends_with(&format!("\"{}\"", snapshot.display())));
        assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    }

    #[test]
    fn only_claude_code_claims_to_enforce_from_the_start() {
        // Both providers get a hook written; only one runs it without the user
        // having to trust it first. Conflating the two would promise Codex
        // sessions a protection that is not switched on yet.
        assert!(installs_hook("claude-code"));
        assert!(installs_hook("codex"));
        assert!(!installs_hook("shell"));

        assert!(enforces_before_execution("claude-code"));
        assert!(!enforces_before_execution("codex"));
        assert!(!enforces_before_execution("shell"));
    }

    #[test]
    fn a_codex_hook_never_overwrites_one_the_user_already_had() {
        let project = tempfile::tempdir().unwrap();
        let hooks = project.path().join(".codex").join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let existing = hooks.join("hooks.json");
        let theirs = r#"{"hooks":{"theirs":true}}"#;
        std::fs::write(&existing, theirs).unwrap();

        // Their configuration is not ours to replace, so we decline — and say
        // so by reporting no hook rather than by clobbering it.
        assert!(write_codex_hook(project.path(), Path::new("snap.json")).is_none());
        assert_eq!(std::fs::read_to_string(&existing).unwrap(), theirs);
    }

    #[test]
    fn a_codex_hook_is_written_where_codex_looks_for_it() {
        let project = tempfile::tempdir().unwrap();
        let snapshot = project.path().join(SNAPSHOT_FILE);
        let path = write_codex_hook(project.path(), &snapshot).unwrap();

        assert!(path.ends_with("hooks.json"));
        assert!(path.parent().unwrap().ends_with("hooks"));

        let value: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let command = value["hooks"]["PreToolUse"][0]["hooks"][0]["command"]
            .as_str()
            .unwrap();
        assert!(command.contains(super::super::guard::HOOK_FLAG));
        assert!(command.starts_with('"'), "paths here contain spaces");

        // Running again finds our own file and leaves it as it is.
        assert_eq!(write_codex_hook(project.path(), &snapshot), Some(path));
    }
}
