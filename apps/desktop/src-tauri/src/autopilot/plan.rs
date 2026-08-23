//! Deciding what to say next, and when to stop (§32).
//!
//! Deliberately a pure function of mission state. Everything that talks to a
//! process, a database or a clock lives in `driver`; this module can be
//! reasoned about — and tested — by handing it a mission and reading what it
//! decides.
//!
//! That split is not tidiness. The decision to keep going or stop is the part
//! that must never be subtly wrong, and it is the part that would otherwise be
//! buried inside a thread that is hard to test.

use crate::mission::model::{
    completion_blockers, AcceptanceCriterion, CriterionStatus, MissionDetail, MissionStatus,
    Verification,
};

/// What the autopilot should do after a turn ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Send this to the agent and keep going.
    Say(String),
    /// Everything required is verified. Complete the mission (§30).
    Complete,
    /// Stop and put it in front of a person, with a reason.
    ///
    /// The reason is a code the UI localises, never prose (§65).
    NeedsHuman { code: &'static str },
    /// Stop and mark the mission failed, with a reason code.
    Fail { code: &'static str },
}

/// Reason codes, so the UI words them (§65).
pub mod reason {
    /// The turn budget ran out.
    pub const OUT_OF_TURNS: &str = "autopilot.outOfTurns";
    /// A required criterion can only be judged by a person.
    pub const NEEDS_MANUAL_CHECK: &str = "autopilot.needsManualCheck";
    /// A guardrail is holding an approval.
    pub const AWAITING_APPROVAL: &str = "autopilot.awaitingApproval";
    /// The same criteria failed repeatedly with no progress between attempts.
    pub const NOT_CONVERGING: &str = "autopilot.notConverging";
    /// The mission was moved to Blocked, by the agent or by a person.
    pub const MISSION_BLOCKED: &str = "autopilot.missionBlocked";
}

/// How many turns a run may take before it is called failed.
///
/// Not a safety net bolted on afterwards: an agent that has taken this many
/// turns without verifying anything is not about to succeed on the next one,
/// and §34 forbids consuming resources indefinitely. Generous enough that a
/// real multi-step mission finishes well inside it.
pub const DEFAULT_TURN_BUDGET: u32 = 24;

/// How many consecutive verification passes may fail without anything changing
/// before the run is called stuck.
///
/// Three, because two is a coincidence — a fix can legitimately fail once while
/// the agent is still narrowing down the cause. Three identical failures in a
/// row means the agent is going round in a circle, and continuing would just
/// spend tokens to arrive at the same place.
pub const STALL_LIMIT: u32 = 3;

/// What the autopilot knows about the run so far.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    /// Turns the agent has taken in this run.
    pub turns: u32,
    pub budget: u32,
    /// Consecutive verification passes with the same failing criteria.
    pub stalled_rounds: u32,
    /// True when a guardrail is holding an approval (§35).
    pub approval_pending: bool,
}

/// Decide the next step.
///
/// The order of these checks is the design. Blocked and pending approvals come
/// before anything else, because a run that cannot proceed must stop *now*
/// rather than take one more turn first (§34). Completion is checked before the
/// budget, so a mission that finished on its last allowed turn completes rather
/// than being failed on a technicality.
pub fn next_instruction(mission: &MissionDetail, progress: &Progress) -> Step {
    // Somebody — the agent itself, or a person — said this cannot proceed.
    if mission.mission.status == MissionStatus::Blocked {
        return Step::NeedsHuman {
            code: reason::MISSION_BLOCKED,
        };
    }

    // A guardrail is holding something. Continuing would have the agent work
    // around a decision that has not been made yet (§35).
    if progress.approval_pending {
        return Step::NeedsHuman {
            code: reason::AWAITING_APPROVAL,
        };
    }

    let active: Vec<&AcceptanceCriterion> =
        mission.criteria.iter().filter(|c| c.is_active()).collect();
    let blockers = completion_blockers(&mission.criteria);

    // Done: every required, active criterion has evidence (§30).
    if blockers.is_empty() {
        return Step::Complete;
    }

    // What remains can only be judged by a person, so no number of further
    // turns will finish it. Saying so beats burning the budget to discover it.
    let all_manual = blockers
        .iter()
        .all(|c| matches!(c.verification, Verification::Manual));
    if all_manual {
        return Step::NeedsHuman {
            code: reason::NEEDS_MANUAL_CHECK,
        };
    }

    if progress.stalled_rounds >= STALL_LIMIT {
        return Step::Fail {
            code: reason::NOT_CONVERGING,
        };
    }

    if progress.turns >= progress.budget {
        return Step::Fail {
            code: reason::OUT_OF_TURNS,
        };
    }

    Step::Say(instruction_for(&blockers, &active))
}

/// The first thing said to a freshly started agent.
///
/// Separate from the follow-up instruction because the situations differ: this
/// one has to introduce the work, where the later ones report on progress
/// against it. Both share `instruction_for`, so what "done" means is stated the
/// same way throughout the run.
pub fn opening_instruction(mission: &MissionDetail) -> String {
    let mut text = format!("Work on this: {}.", mission.mission.title);
    if let Some(goal) = mission.mission.goal.as_deref().filter(|g| !g.trim().is_empty()) {
        text.push_str(&format!(" {goal}"));
    }
    text.push_str("\n\n");

    let active: Vec<&AcceptanceCriterion> =
        mission.criteria.iter().filter(|c| c.is_active()).collect();
    let blockers = completion_blockers(&mission.criteria);
    if blockers.is_empty() {
        // Nothing to do, which the caller will notice on the first pass. Say
        // something harmless rather than an empty line.
        text.push_str("Everything it asks for already passes.");
        return text;
    }
    text.push_str(&instruction_for(&blockers, &active));
    text
}

/// The message sent to the agent.
///
/// Written as a brief addressed to a competent colleague, not as a prompt full
/// of instructions about how to behave. It says what is not yet true and how
/// each of those things is checked — which is exactly the information a person
/// would give, and nothing more.
///
/// It deliberately does **not** tell the agent to mark anything complete. The
/// agent does not get to declare success; verification does (§30).
fn instruction_for(
    blockers: &[&AcceptanceCriterion],
    active: &[&AcceptanceCriterion],
) -> String {
    let mut text = String::new();

    let verified = active
        .iter()
        .filter(|c| c.status == CriterionStatus::Verified)
        .count();
    if verified > 0 {
        text.push_str(&format!(
            "{verified} of {} acceptance criteria now pass.\n\n",
            active.len()
        ));
    }

    text.push_str("These do not pass yet:\n\n");
    for criterion in blockers {
        text.push_str(&format!("- {}", criterion.description));
        match &criterion.verification {
            Verification::Command { command, .. } => {
                text.push_str(&format!("\n  Checked by running: {command}"));
            }
            Verification::FileExists { path } => {
                text.push_str(&format!("\n  Checked by: {path} must exist"));
            }
            Verification::FileContains { path, text: needle } => {
                text.push_str(&format!("\n  Checked by: {path} must contain {needle:?}"));
            }
            Verification::Manual => {
                text.push_str("\n  Only a person can confirm this one.");
            }
        }
        // The failure output is the most useful thing we have; an agent that
        // can see why a check failed does not have to rediscover it.
        if criterion.status == CriterionStatus::Failed {
            text.push_str("  (it was tried and failed)");
        }
        text.push('\n');
    }

    text.push_str(
        "\nKeep working until they do. J.A.R.V.I.S. runs these checks itself, so \
         there is no need to claim anything is done — just make it true. If you \
         cannot proceed, say what you need and stop.",
    );
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::model::{Autonomy, Mission};

    fn criterion(
        required: bool,
        status: CriterionStatus,
        verification: Verification,
    ) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id: "c1".into(),
            mission_id: "m1".into(),
            description: "the tests pass".into(),
            required,
            verification,
            status,
            position: 0,
            removed_at: None,
            removed_reason: None,
            removed_by: None,
        }
    }

    fn command_check() -> Verification {
        Verification::Command {
            command: "cargo test".into(),
            cwd: None,
            expect_exit: 0,
        }
    }

    fn mission(status: MissionStatus, criteria: Vec<AcceptanceCriterion>) -> MissionDetail {
        MissionDetail {
            mission: Mission {
                id: "m1".into(),
                project_id: "p1".into(),
                title: "ship it".into(),
                goal: None,
                description: None,
                status,
                autonomy: Some(Autonomy::Unattended),
                blocked_reason: None,
                created_at: 0,
                updated_at: 0,
                started_at: None,
                completed_at: None,
            },
            tasks: vec![],
            criteria,
            evidence: vec![],
            effective_autonomy: Autonomy::Unattended,
        }
    }

    fn progress(turns: u32) -> Progress {
        Progress {
            turns,
            budget: DEFAULT_TURN_BUDGET,
            stalled_rounds: 0,
            approval_pending: false,
        }
    }

    /// A freshly started agent is at an empty prompt with nothing to react to.
    ///
    /// The loop waits for a turn to end; if nothing opens the conversation,
    /// no turn ever ends and the run sits at turn 0 forever. That is exactly
    /// what happened on the first real run.
    #[test]
    fn an_opening_instruction_states_the_work_and_how_it_is_judged() {
        let mut m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Pending, command_check())],
        );
        m.mission.goal = Some("so the suite is green".into());

        let text = opening_instruction(&m);
        assert!(text.contains("ship it"), "the mission has to be named");
        assert!(text.contains("so the suite is green"));
        assert!(text.contains("cargo test"), "and how it will be judged");
        assert!(!text.trim().is_empty());
    }

    #[test]
    fn an_opening_instruction_on_an_already_passing_mission_is_still_sensible() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Verified, command_check())],
        );
        let text = opening_instruction(&m);
        assert!(!text.trim().is_empty());
        assert!(text.contains("ship it"));
    }

    #[test]
    fn a_mission_whose_criteria_all_pass_is_completed() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Verified, command_check())],
        );
        assert_eq!(next_instruction(&m, &progress(1)), Step::Complete);
    }

    #[test]
    fn a_mission_with_no_criteria_completes_rather_than_looping() {
        // Nothing to verify means nothing to work towards; continuing would be
        // an agent talking to itself forever.
        let m = mission(MissionStatus::Running, vec![]);
        assert_eq!(next_instruction(&m, &progress(1)), Step::Complete);
    }

    #[test]
    fn an_unverified_criterion_produces_an_instruction_naming_it() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Pending, command_check())],
        );
        let Step::Say(text) = next_instruction(&m, &progress(1)) else {
            panic!("expected an instruction");
        };
        assert!(text.contains("the tests pass"));
        // The agent is told *how* it will be judged. Hiding the check would
        // make the loop guesswork.
        assert!(text.contains("cargo test"));
        // And is never invited to declare success (§30).
        assert!(!text.to_lowercase().contains("mark it complete"));
    }

    #[test]
    fn a_failed_criterion_is_reported_as_already_tried() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Failed, command_check())],
        );
        let Step::Say(text) = next_instruction(&m, &progress(1)) else {
            panic!("expected an instruction");
        };
        assert!(text.contains("tried and failed"));
    }

    /// §34: a run that cannot proceed must stop, not keep spending.
    #[test]
    fn a_blocked_mission_stops_immediately() {
        let m = mission(
            MissionStatus::Blocked,
            vec![criterion(true, CriterionStatus::Pending, command_check())],
        );
        assert_eq!(
            next_instruction(&m, &progress(1)),
            Step::NeedsHuman {
                code: reason::MISSION_BLOCKED
            }
        );
    }

    /// §35: continuing here would have the agent work around a decision nobody
    /// has made yet.
    #[test]
    fn a_pending_approval_stops_the_run_before_anything_else() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Pending, command_check())],
        );
        let mut p = progress(1);
        p.approval_pending = true;
        assert_eq!(
            next_instruction(&m, &p),
            Step::NeedsHuman {
                code: reason::AWAITING_APPROVAL
            }
        );
    }

    #[test]
    fn work_only_a_person_can_judge_asks_for_that_person() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Pending, Verification::Manual)],
        );
        assert_eq!(
            next_instruction(&m, &progress(1)),
            Step::NeedsHuman {
                code: reason::NEEDS_MANUAL_CHECK
            }
        );
    }

    #[test]
    fn a_manual_criterion_alongside_machine_ones_still_lets_the_agent_work() {
        // Only *all* remaining work being manual means there is nothing to do.
        let mut manual = criterion(true, CriterionStatus::Pending, Verification::Manual);
        manual.id = "c2".into();
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Pending, command_check()), manual],
        );
        assert!(matches!(next_instruction(&m, &progress(1)), Step::Say(_)));
    }

    #[test]
    fn the_turn_budget_ends_a_run_that_is_going_nowhere() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Pending, command_check())],
        );
        assert_eq!(
            next_instruction(&m, &progress(DEFAULT_TURN_BUDGET)),
            Step::Fail {
                code: reason::OUT_OF_TURNS
            }
        );
    }

    /// The budget must never turn a success into a failure.
    #[test]
    fn a_mission_that_finished_on_its_last_turn_completes() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Verified, command_check())],
        );
        assert_eq!(
            next_instruction(&m, &progress(DEFAULT_TURN_BUDGET + 5)),
            Step::Complete
        );
    }

    #[test]
    fn repeated_identical_failures_stop_the_run() {
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Failed, command_check())],
        );
        let mut p = progress(2);
        p.stalled_rounds = STALL_LIMIT;
        assert_eq!(
            next_instruction(&m, &p),
            Step::Fail {
                code: reason::NOT_CONVERGING
            }
        );
    }

    #[test]
    fn one_failure_is_not_a_stall() {
        // An agent is allowed to be wrong once while narrowing down a cause.
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Failed, command_check())],
        );
        let mut p = progress(2);
        p.stalled_rounds = 1;
        assert!(matches!(next_instruction(&m, &p), Step::Say(_)));
    }

    #[test]
    fn an_optional_criterion_never_holds_a_run_open() {
        let mut optional = criterion(false, CriterionStatus::Pending, command_check());
        optional.id = "c2".into();
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Verified, command_check()), optional],
        );
        assert_eq!(next_instruction(&m, &progress(1)), Step::Complete);
    }

    /// §31 — a withdrawn criterion must not be worked on or waited for.
    #[test]
    fn a_withdrawn_criterion_is_not_worked_on() {
        let mut withdrawn = criterion(true, CriterionStatus::Pending, command_check());
        withdrawn.id = "c2".into();
        withdrawn.removed_at = Some(1);
        let m = mission(
            MissionStatus::Running,
            vec![criterion(true, CriterionStatus::Verified, command_check()), withdrawn],
        );
        assert_eq!(next_instruction(&m, &progress(1)), Step::Complete);
    }

    #[test]
    fn progress_is_reported_so_the_agent_knows_it_is_getting_somewhere() {
        let mut done = criterion(true, CriterionStatus::Verified, command_check());
        done.id = "c2".into();
        done.description = "it builds".into();
        let m = mission(
            MissionStatus::Running,
            vec![done, criterion(true, CriterionStatus::Pending, command_check())],
        );
        let Step::Say(text) = next_instruction(&m, &progress(3)) else {
            panic!("expected an instruction");
        };
        assert!(text.contains("1 of 2"));
    }
}
