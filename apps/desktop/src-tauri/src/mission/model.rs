//! Mission types (§29).
//!
//! A Mission is work that needs to be finished. The important idea is §30: a
//! Mission is not complete because an agent says so, but because its required
//! acceptance criteria have **evidence** that they hold.

use serde::{Deserialize, Serialize};

/// Where a mission is, honestly.
///
/// `Blocked` and `Waiting` exist so that a mission which cannot proceed says so
/// instead of quietly declaring success or burning resources forever (§34).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MissionStatus {
    /// Defined, not started.
    Ready,
    /// An agent is working on it.
    Running,
    /// Work is done being claimed; criteria are being checked.
    Verifying,
    /// Needs a human decision — an approval, an answer.
    Waiting,
    /// Cannot proceed without something the agent cannot do (§34).
    Blocked,
    /// Finished and failed.
    Failed,
    /// Finished, and every required criterion is verified (§30).
    Completed,
}

impl MissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Completed => "completed",
        }
    }

    pub fn parse(text: &str) -> Self {
        match text {
            "running" => Self::Running,
            "verifying" => Self::Verifying,
            "waiting" => Self::Waiting,
            "blocked" => Self::Blocked,
            "failed" => Self::Failed,
            "completed" => Self::Completed,
            _ => Self::Ready,
        }
    }

    /// Whether this state needs a human before anything else can happen (§18).
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::Waiting | Self::Blocked | Self::Failed)
    }

    pub fn is_finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// How much the agent may decide on its own (§32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Autonomy {
    /// The human stays close; the agent checks in often.
    Guided,
    /// The agent handles what falls within its remit and asks when it does not.
    Autonomous,
    /// Designed to be left alone: keep implementing, testing, fixing and
    /// verifying until done, genuinely blocked, or failed.
    Unattended,
}

impl Autonomy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guided => "guided",
            Self::Autonomous => "autonomous",
            Self::Unattended => "unattended",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "guided" => Self::Guided,
            "autonomous" => Self::Autonomous,
            "unattended" => Self::Unattended,
            _ => return None,
        })
    }
}

/// Resolve autonomy through the §33 chain: mission, then project, then global.
///
/// The most specific setting wins. Guided is the default because the safe
/// assumption about an unconfigured mission is that a human wants to be
/// involved, not that they do not.
pub fn resolve_autonomy(
    mission: Option<Autonomy>,
    project: Option<Autonomy>,
    global: Option<Autonomy>,
) -> Autonomy {
    mission.or(project).or(global).unwrap_or(Autonomy::Guided)
}

/// How a criterion is checked.
///
/// Every variant except `Manual` is machine-checkable, which is the whole
/// point: a criterion that cannot be checked can only ever be claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Verification {
    /// Run a command; the criterion holds if it exits with `expect_exit`.
    #[serde(rename_all = "camelCase")]
    Command {
        command: String,
        /// Relative to the project directory when absent.
        cwd: Option<String>,
        #[serde(default)]
        expect_exit: i32,
    },
    /// A path must exist, relative to the project directory.
    #[serde(rename_all = "camelCase")]
    FileExists { path: String },
    /// A path must contain the given text.
    #[serde(rename_all = "camelCase")]
    FileContains { path: String, text: String },
    /// Only a human can judge it. Recorded as such so it is never mistaken for
    /// something that was actually checked.
    Manual,
}

impl Verification {
    /// Whether J.A.R.V.I.S. can check this itself.
    pub fn is_automatic(&self) -> bool {
        !matches!(self, Self::Manual)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CriterionStatus {
    Pending,
    Verified,
    Failed,
}

impl CriterionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Verified => "verified",
            Self::Failed => "failed",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "verified" => Self::Verified,
            "failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptanceCriterion {
    pub id: String,
    pub mission_id: String,
    pub description: String,
    pub required: bool,
    pub verification: Verification,
    pub status: CriterionStatus,
    pub position: i64,
    /// Set when a criterion was withdrawn. Withdrawal is recorded rather than
    /// deleted, so a Mission's requirements can never quietly shrink (§31).
    pub removed_at: Option<i64>,
    pub removed_reason: Option<String>,
    pub removed_by: Option<String>,
}

impl AcceptanceCriterion {
    pub fn is_active(&self) -> bool {
        self.removed_at.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EvidenceKind {
    Command,
    File,
    Commit,
    Screenshot,
    Url,
    Manual,
}

impl EvidenceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::File => "file",
            Self::Commit => "commit",
            Self::Screenshot => "screenshot",
            Self::Url => "url",
            Self::Manual => "manual",
        }
    }
    pub fn parse(text: &str) -> Self {
        match text {
            "command" => Self::Command,
            "file" => Self::File,
            "commit" => Self::Commit,
            "screenshot" => Self::Screenshot,
            "url" => Self::Url,
            _ => Self::Manual,
        }
    }
}

/// A recorded fact about whether something holds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub id: String,
    pub mission_id: String,
    pub criterion_id: Option<String>,
    pub session_id: Option<String>,
    pub kind: EvidenceKind,
    pub ok: bool,
    /// English text, always present.
    ///
    /// Kept as the fallback so a build that does not recognise `code` still
    /// shows something true, and so existing evidence keeps rendering.
    pub summary: String,
    /// A stable code the UI translates, when one applies (§65).
    ///
    /// Evidence is generated in Rust, which has no business choosing the
    /// reader's language. Where the sentence is ours to write, the code says
    /// *which* sentence and the catalogue says how to word it.
    pub code: Option<String>,
    /// Arguments for that message, as JSON. Lets one code serve many subjects.
    pub code_args: Option<String>,
    pub detail: Option<String>,
    pub ts_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionTask {
    pub id: String,
    pub mission_id: String,
    pub description: String,
    pub done: bool,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub goal: Option<String>,
    pub description: Option<String>,
    pub status: MissionStatus,
    pub autonomy: Option<Autonomy>,
    pub blocked_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

/// A mission with everything needed to render or judge it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionDetail {
    #[serde(flatten)]
    pub mission: Mission,
    pub tasks: Vec<MissionTask>,
    pub criteria: Vec<AcceptanceCriterion>,
    pub evidence: Vec<Evidence>,
    /// Autonomy after the §33 chain is applied.
    pub effective_autonomy: Autonomy,
}

/// Whether a mission may be called complete (§30).
///
/// Every **active, required** criterion must be verified. Withdrawn criteria do
/// not count — but they were recorded on the way out, so a shrinking Mission is
/// visible rather than silent (§31).
pub fn completion_blockers(criteria: &[AcceptanceCriterion]) -> Vec<&AcceptanceCriterion> {
    criteria
        .iter()
        .filter(|c| c.is_active() && c.required && c.status != CriterionStatus::Verified)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn criterion(required: bool, status: CriterionStatus) -> AcceptanceCriterion {
        AcceptanceCriterion {
            id: "c".into(),
            mission_id: "m".into(),
            description: "d".into(),
            required,
            verification: Verification::Manual,
            status,
            position: 0,
            removed_at: None,
            removed_reason: None,
            removed_by: None,
        }
    }

    #[test]
    fn autonomy_resolves_most_specific_first() {
        use Autonomy::*;
        assert_eq!(resolve_autonomy(Some(Unattended), Some(Guided), Some(Autonomous)), Unattended);
        assert_eq!(resolve_autonomy(None, Some(Guided), Some(Autonomous)), Guided);
        assert_eq!(resolve_autonomy(None, None, Some(Autonomous)), Autonomous);
    }

    #[test]
    fn autonomy_defaults_to_guided_when_nothing_is_configured() {
        // The safe assumption about an unconfigured mission is that a human
        // wants to be involved.
        assert_eq!(resolve_autonomy(None, None, None), Autonomy::Guided);
    }

    #[test]
    fn a_mission_with_an_unverified_required_criterion_cannot_complete() {
        let criteria = vec![
            criterion(true, CriterionStatus::Verified),
            criterion(true, CriterionStatus::Pending),
        ];
        assert_eq!(completion_blockers(&criteria).len(), 1);
    }

    #[test]
    fn a_failed_criterion_blocks_completion() {
        let criteria = vec![criterion(true, CriterionStatus::Failed)];
        assert_eq!(completion_blockers(&criteria).len(), 1);
    }

    #[test]
    fn optional_criteria_do_not_block_completion() {
        let criteria = vec![
            criterion(true, CriterionStatus::Verified),
            criterion(false, CriterionStatus::Pending),
        ];
        assert!(completion_blockers(&criteria).is_empty());
    }

    /// §31 — the failure mode this guards against is an agent "simplifying" a
    /// Mission until it can declare success.
    #[test]
    fn a_withdrawn_criterion_stops_blocking_but_leaves_a_record() {
        let mut withdrawn = criterion(true, CriterionStatus::Pending);
        withdrawn.removed_at = Some(1);
        withdrawn.removed_reason = Some("out of scope, agreed with the user".into());
        withdrawn.removed_by = Some("claude-code".into());

        let criteria = vec![criterion(true, CriterionStatus::Verified), withdrawn];
        assert!(completion_blockers(&criteria).is_empty());

        // It is still there, with its reason, rather than deleted.
        assert!(!criteria[1].is_active());
        assert!(criteria[1].removed_reason.is_some());
    }

    #[test]
    fn statuses_round_trip_through_storage() {
        for status in [
            MissionStatus::Ready,
            MissionStatus::Running,
            MissionStatus::Verifying,
            MissionStatus::Waiting,
            MissionStatus::Blocked,
            MissionStatus::Failed,
            MissionStatus::Completed,
        ] {
            assert_eq!(MissionStatus::parse(status.as_str()), status);
        }
        assert_eq!(MissionStatus::parse("nonsense"), MissionStatus::Ready);
    }

    #[test]
    fn blocked_and_waiting_ask_for_a_human() {
        assert!(MissionStatus::Blocked.needs_attention());
        assert!(MissionStatus::Waiting.needs_attention());
        assert!(MissionStatus::Failed.needs_attention());
        assert!(!MissionStatus::Running.needs_attention());
        // Completed emphatically does not — that is the §34 confusion.
        assert!(!MissionStatus::Completed.needs_attention());
    }

    #[test]
    fn manual_verification_is_not_automatic() {
        assert!(!Verification::Manual.is_automatic());
        assert!(Verification::Command {
            command: "cargo test".into(),
            cwd: None,
            expect_exit: 0
        }
        .is_automatic());
    }

    #[test]
    fn verification_round_trips_as_json() {
        // Stored as JSON in the criteria table, so this must survive exactly.
        let original = Verification::Command {
            command: "pnpm test".into(),
            cwd: Some("apps/desktop".into()),
            expect_exit: 0,
        };
        let text = serde_json::to_string(&original).unwrap();
        assert_eq!(serde_json::from_str::<Verification>(&text).unwrap(), original);
    }
}
