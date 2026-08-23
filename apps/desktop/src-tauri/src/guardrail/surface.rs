//! The guardrail in front of an operation **J.A.R.V.I.S. itself** performs (§35).
//!
//! ## Why this is not `classify`
//!
//! `hold_for_guardrail` classifies a mission's verification command because that
//! command is text a person wrote and we have to work out what it does. A button
//! is not text. When the Review surface discards a file, this crate builds the
//! `git restore` line itself and already knows, with certainty, which operation
//! it is about to perform — so it names the `Operation` directly instead of
//! writing a string and asking a heuristic to read it back.
//!
//! That difference is the whole point rather than a tidiness preference. D11
//! says the guard **fails open**: a command that matches no pattern proceeds.
//! Round-tripping a command we constructed through the classifier would import
//! that failure mode into the one place D11 promises enforcement is
//! unconditional — if the matcher ever stopped recognising our own spelling, a
//! destructive operation would run silently unguarded and every test would
//! still pass. Naming the operation cannot fail that way.
//!
//! The command text still travels with the record, verbatim, because a
//! guardrail nobody can interrogate is a guardrail nobody trusts.
//!
//! ## Two phases, and why there is no `Pending` row
//!
//! An agent's tool call is intercepted mid-flight and a verification can be
//! parked and resumed, so both can produce a `Pending` event that waits for
//! somebody to come back. A button cannot: the person is **right here**, having
//! just clicked it.
//!
//! Writing a `Pending` row for them would be actively harmful. `pending()` feeds
//! Mission Control's needs-attention list, and `decide_guardrail` only knows how
//! to resume work through a `criterion_id` — which a Git action does not have.
//! The row would be settled, the action silently dropped, and the queue left
//! asserting that something needs a human forever.
//!
//! So the surface asks first (`check`, which records nothing), shows the §35
//! choices, and calls back with the answer. The core **re-resolves** on that
//! second call: the choice is the human's answer, never the caller's authority.
//! In particular a `Deny` cannot be overridden by anything arriving from the
//! webview — only Settings changes a `Deny`.

use serde::{Deserialize, Serialize};

use crate::db::Database;
use crate::session::log::now_ms;

use super::classify::Operation;
use super::commands::Choice;
use super::policy::{self, Decision, Resolved, Scope};
use super::{record, NewEvent, Origin, Status};

/// What the guardrail says about an operation this product is about to run.
///
/// The scope travels with the decision for the same reason it does in Settings:
/// "allowed for this project" and "allowed everywhere" are different facts, and
/// a confirmation that cannot tell them apart cannot be audited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gate {
    pub operation: Operation,
    pub decision: Decision,
    pub scope: Scope,
}

impl Gate {
    pub fn needs_asking(&self) -> bool {
        self.decision == Decision::Ask
    }
    pub fn refuses(&self) -> bool {
        self.decision == Decision::Deny
    }
}

/// What policy says, without recording anything or acting.
///
/// Deliberately side-effect free: this is called to decide whether to *show* a
/// confirmation, and a surface asking what the rules are must not itself appear
/// in the guardrail history.
pub fn check(
    db: &Database,
    project_id: &str,
    operation: Operation,
) -> crate::db::Result<Gate> {
    let Resolved { decision, scope } =
        db.with(|conn| Ok(policy::resolve(conn, Some(project_id), operation)?))?;
    Ok(Gate {
        operation,
        decision,
        scope,
    })
}

/// The outcome of putting a surface-initiated operation to the guardrail.
pub enum Verdict {
    /// Go ahead. Nothing was recorded unless a person was asked.
    Proceed,
    /// A rule says ask and nobody has answered yet. Nothing was recorded and
    /// nothing has happened.
    NeedsApproval(Gate),
    /// Refused. The event is in the guardrail history with `reason`.
    Refused { gate: Gate, reason: &'static str },
}

/// Put an operation to the guardrail, applying the person's answer if they gave
/// one.
///
/// `choice` is what the human clicked in the confirmation, and it is treated as
/// an *answer* rather than as permission: policy is resolved again here, and a
/// `Deny` still refuses no matter what arrived from the webview.
pub fn gate(
    db: &Database,
    project_id: &str,
    session_id: Option<&str>,
    operation: Operation,
    command: &str,
    choice: Option<Choice>,
) -> crate::db::Result<Verdict> {
    let gate = check(db, project_id, operation)?;

    // Silence is not permission (see `policy`), and it is not an event either.
    // Recording every allowed stage would bury the occasions a guardrail
    // actually spoke, which is what this log is for.
    if gate.decision == Decision::Allow {
        return Ok(Verdict::Proceed);
    }

    if gate.refuses() {
        let reason = policy::reason::POLICY_DENIES;
        write_event(db, project_id, session_id, &gate, command, Status::Denied, reason)?;
        return Ok(Verdict::Refused { gate, reason });
    }

    // Ask, and nobody has been asked yet.
    let Some(choice) = choice else {
        return Ok(Verdict::NeedsApproval(gate));
    };

    // Remember the answer where answers are remembered. `AllowOnce` stores
    // nothing on purpose: approving one discard is not a policy change, and
    // conflating the two is how safety settings drift open one dialog at a time.
    let stored = match choice {
        Choice::AllowOnce => None,
        Choice::AllowForProject => Some((Some(project_id), Decision::Allow)),
        Choice::AlwaysAllow => Some((None, Decision::Allow)),
        // Narrower than "everywhere", for the same reason `decide_guardrail`
        // records it that way: the narrow reading can be widened in Settings,
        // and the wide one cannot be taken back by someone who did not mean it.
        Choice::NeverAllow => Some((Some(project_id), Decision::Deny)),
    };
    if let Some((scope, decision)) = stored {
        db.with(|conn| Ok(policy::set(conn, scope, operation, Some(decision), now_ms())?))?;
    }

    if choice == Choice::NeverAllow {
        let reason = choice.reason();
        write_event(db, project_id, session_id, &gate, command, Status::Denied, reason)?;
        return Ok(Verdict::Refused { gate, reason });
    }

    // A person was asked and said yes. That is worth a row: it is the record of
    // a guardrail having stopped something and a human having released it.
    write_event(
        db,
        project_id,
        session_id,
        &gate,
        command,
        Status::Allowed,
        choice.reason(),
    )?;
    Ok(Verdict::Proceed)
}

fn write_event(
    db: &Database,
    project_id: &str,
    session_id: Option<&str>,
    gate: &Gate,
    command: &str,
    status: Status,
    reason: &str,
) -> crate::db::Result<()> {
    record(
        db,
        NewEvent {
            project_id: Some(project_id),
            session_id,
            mission_id: None,
            criterion_id: None,
            // Not `Agent`: nobody's tool call was intercepted. The product was
            // asked to do this and asked itself first.
            origin: Origin::Surface,
            operation: gate.operation,
            // There is no heuristic here, so there is no matched fragment to
            // quote. The command is the whole of what happened.
            fragment: command,
            command,
            status,
            reason,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::pending;

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

    fn gate_it(db: &Database, choice: Option<Choice>) -> Verdict {
        gate(
            db,
            "p1",
            None,
            Operation::GitDiscardChanges,
            "git restore --source=HEAD --staged --worktree -- a.txt",
            choice,
        )
        .unwrap()
    }

    #[test]
    fn an_unconfigured_operation_asks_before_it_acts() {
        let db = db();
        assert!(matches!(gate_it(&db, None), Verdict::NeedsApproval(_)));
        // Asking must not have done anything or left a trace.
        assert!(super::super::list(&db, Some("p1"), None, 10).unwrap().is_empty());
    }

    /// The hazard this module was shaped around.
    ///
    /// A `Pending` row feeds Mission Control's needs-attention list, and nothing
    /// can resume a Git action from one. If a row like that were ever written
    /// here it would sit there claiming a human is needed, forever.
    #[test]
    fn nothing_here_ever_writes_a_pending_approval() {
        let db = db();
        gate_it(&db, None);
        gate_it(&db, Some(Choice::AllowOnce));
        gate_it(&db, Some(Choice::NeverAllow));
        // And once denied, again.
        gate_it(&db, None);

        assert!(pending(&db, None).unwrap().is_empty());
        assert!(super::super::list(&db, Some("p1"), None, 50)
            .unwrap()
            .iter()
            .all(|e| e.status != Status::Pending));
    }

    #[test]
    fn allowing_once_records_the_release_but_changes_no_rule() {
        let db = db();
        assert!(matches!(
            gate_it(&db, Some(Choice::AllowOnce)),
            Verdict::Proceed
        ));

        let events = super::super::list(&db, Some("p1"), None, 10).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].status, Status::Allowed);
        assert_eq!(events[0].origin, Origin::Surface);
        assert_eq!(events[0].reason, "allowedOnce");

        // The next one still asks.
        assert!(matches!(gate_it(&db, None), Verdict::NeedsApproval(_)));
    }

    #[test]
    fn allowing_for_the_project_stops_the_asking_and_stops_the_recording() {
        let db = db();
        gate_it(&db, Some(Choice::AllowForProject));
        assert!(matches!(gate_it(&db, None), Verdict::Proceed));

        // Only the release is in the history — a rule that now says "allow" is
        // silence, not an event (see `policy`).
        assert_eq!(super::super::list(&db, Some("p1"), None, 10).unwrap().len(), 1);
    }

    /// A `Deny` is not overridable by anything arriving from the webview.
    #[test]
    fn a_refusal_cannot_be_talked_out_of_by_a_choice() {
        let db = db();
        gate_it(&db, Some(Choice::NeverAllow));

        for claimed in [Choice::AllowOnce, Choice::AllowForProject, Choice::AlwaysAllow] {
            match gate_it(&db, Some(claimed)) {
                Verdict::Refused { reason, .. } => {
                    assert_eq!(reason, policy::reason::POLICY_DENIES)
                }
                _ => panic!("a denied operation proceeded with choice {claimed:?}"),
            }
        }
    }

    #[test]
    fn the_command_is_recorded_verbatim_so_the_decision_can_be_reviewed() {
        let db = db();
        gate_it(&db, Some(Choice::AllowOnce));
        let events = super::super::list(&db, Some("p1"), None, 10).unwrap();
        assert_eq!(
            events[0].command,
            "git restore --source=HEAD --staged --worktree -- a.txt"
        );
    }
}
