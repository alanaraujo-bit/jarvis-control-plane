//! Guardrail policy: what should happen when an operation is recognised (§35).
//!
//! ## The resolution chain
//!
//! Project rule, then global rule, then the built-in default — the same shape
//! as autonomy (§33), and for the same reason: the most specific setting wins,
//! and an unconfigured operation errs towards involving the human.
//!
//! ## Why `Allow` is not the same as "permit"
//!
//! When a policy resolves to `Allow`, J.A.R.V.I.S. states **no opinion**. It
//! does not tell the provider to skip its own permission prompt. A guardrail
//! that can loosen another tool's safety model is not a guardrail — this layer
//! can only ever add restriction, never remove it.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::classify::Operation;

/// What policy says about one operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    /// Stop and involve the human.
    Ask,
    /// Do not object. See the module note — this is silence, not permission.
    Allow,
    /// Refuse, every time, without asking.
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "ask" => Self::Ask,
            "allow" => Self::Allow,
            "deny" => Self::Deny,
            _ => return None,
        })
    }
}

/// What every operation does when nothing has been configured.
///
/// `Ask` throughout, deliberately. These operations are rare in a day's work,
/// so asking costs little; and the alternative — guessing that the user would
/// have allowed it — is the guess this whole module exists to avoid making.
pub const DEFAULT: Decision = Decision::Ask;

/// Where a rule was set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    Global,
    Project,
    /// Nothing is configured; `DEFAULT` applies.
    Default,
}

/// A resolved policy, with the scope that decided it.
///
/// The scope travels with the decision so the UI can say *why* an operation
/// behaves as it does — "allowed for this project" reads very differently from
/// "allowed everywhere", and a user who cannot tell them apart cannot audit
/// their own settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved {
    pub decision: Decision,
    pub scope: Scope,
}

/// Resolve one operation for a project.
pub fn resolve(
    conn: &Connection,
    project_id: Option<&str>,
    operation: Operation,
) -> rusqlite::Result<Resolved> {
    if let Some(project_id) = project_id {
        let row: Option<String> = conn
            .query_row(
                "SELECT decision FROM guardrail_policies
                  WHERE operation = ?1 AND project_id = ?2",
                rusqlite::params![operation.as_str(), project_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(decision) = row.as_deref().and_then(Decision::parse) {
            return Ok(Resolved {
                decision,
                scope: Scope::Project,
            });
        }
    }

    let row: Option<String> = conn
        .query_row(
            "SELECT decision FROM guardrail_policies
              WHERE operation = ?1 AND project_id IS NULL",
            [operation.as_str()],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(decision) = row.as_deref().and_then(Decision::parse) {
        return Ok(Resolved {
            decision,
            scope: Scope::Global,
        });
    }

    Ok(Resolved {
        decision: DEFAULT,
        scope: Scope::Default,
    })
}

/// Resolve every operation for a project, in display order.
pub fn resolve_all(
    conn: &Connection,
    project_id: Option<&str>,
) -> rusqlite::Result<Vec<(Operation, Resolved)>> {
    super::classify::ALL
        .iter()
        .map(|op| resolve(conn, project_id, *op).map(|r| (*op, r)))
        .collect()
}

/// Set a rule, or clear it so the next scope up decides again.
///
/// Clearing is `None` rather than writing `Ask`: a project row saying "ask"
/// pins the project to asking even if the global rule later changes, which is
/// a different intention from "follow whatever the global rule is".
pub fn set(
    conn: &Connection,
    project_id: Option<&str>,
    operation: Operation,
    decision: Option<Decision>,
    now_ms: i64,
) -> rusqlite::Result<()> {
    match decision {
        None => {
            conn.execute(
                "DELETE FROM guardrail_policies
                  WHERE operation = ?1
                    AND ((?2 IS NULL AND project_id IS NULL) OR project_id = ?2)",
                rusqlite::params![operation.as_str(), project_id],
            )?;
        }
        Some(decision) => {
            // The unique index is on (operation, project_id) with NULL folded
            // to '', so an upsert needs the same expression it was built with.
            conn.execute(
                "INSERT INTO guardrail_policies
                     (id, project_id, operation, decision, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)
                 ON CONFLICT (operation, IFNULL(project_id, '')) DO UPDATE SET
                     decision = excluded.decision,
                     updated_at = excluded.updated_at",
                rusqlite::params![
                    uuid::Uuid::now_v7().to_string(),
                    project_id,
                    operation.as_str(),
                    decision.as_str(),
                    now_ms
                ],
            )?;
        }
    }
    Ok(())
}

// ---- The snapshot the out-of-process guard reads ------------------------------

/// Resolved policy for one live session, written to its log directory.
///
/// The guard runs as a **separate process**, once per tool call. It reads this
/// file and nothing else — not SQLite. Three reasons, in order of importance:
///
/// 1. The session log has one writer (D2). A second process writing frames is
///    the thing that architecture exists to prevent, so the guard writes its own
///    append-only file and the session runtime projects it into the log.
/// 2. A guard that opens the database on every `Bash` call would contend with
///    the application for it, on the hot path of every agent session.
/// 3. A file the application owns has no failure mode where the guard reads a
///    half-applied migration.
///
/// It is rewritten whenever policy changes or a view attaches or detaches, so a
/// setting takes effect on the next tool call rather than the next session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Bumped if the shape changes. A guard from an older build must be able to
    /// recognise a file it does not understand and stay silent.
    pub version: u32,
    pub session_id: String,
    pub project_id: String,
    pub mission_id: Option<String>,
    pub provider: String,
    /// Whether a terminal view is attached right now.
    ///
    /// This is the difference between "ask the human" and "there is no human to
    /// ask". Under Unattended (§32) nobody is looking at the terminal, so a
    /// provider's own permission prompt would wait forever — which is exactly
    /// the indefinite consumption §34 forbids.
    pub attended: bool,
    /// Operation id to decision, already resolved through the chain.
    pub decisions: std::collections::BTreeMap<String, String>,
    /// Where the guard appends what it decided.
    pub log_path: String,
}

pub const SNAPSHOT_VERSION: u32 = 1;

impl Snapshot {
    pub fn decision_for(&self, operation: Operation) -> Decision {
        self.decisions
            .get(operation.as_str())
            .and_then(|d| Decision::parse(d))
            .unwrap_or(DEFAULT)
    }
}

/// Filename of the policy snapshot inside a session's log directory.
pub const SNAPSHOT_FILE: &str = "guardrail-policy.json";
/// Filename of the guard's own append-only decision log.
pub const DECISIONS_FILE: &str = "guardrail-decisions.jsonl";

/// What the guard decided about one tool call.
///
/// Written by the guard process, read by the session runtime, and projected
/// into the session log as an `Approval` frame (§35, event kind 50).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionRecord {
    pub ts_ms: i64,
    pub operation: String,
    pub fragment: String,
    /// `denied` | `asked` | `allowed`
    pub outcome: String,
    /// A stable code the UI localises, never prose (§65).
    pub reason: String,
    pub command: String,
    pub tool_use_id: Option<String>,
}

/// Reason codes for `DecisionRecord::reason`.
pub mod reason {
    /// A rule says never.
    pub const POLICY_DENIES: &str = "policyDenies";
    /// A rule says ask, and there is nobody attached to answer.
    pub const NOBODY_TO_ASK: &str = "nobodyToAsk";
    /// A rule says ask, and the question was put to the person at the terminal.
    pub const ASKED_HUMAN: &str = "askedHuman";
    /// A rule allows it; J.A.R.V.I.S. stated no opinion.
    pub const POLICY_ALLOWS: &str = "policyAllows";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guardrail::classify::Operation::*;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p1', 'demo', 'C:/demo', 1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, created_at, last_opened_at)
             VALUES ('p2', 'other', 'C:/other', 1, 1)",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn an_unconfigured_operation_asks() {
        let conn = db();
        let resolved = resolve(&conn, Some("p1"), GitForcePush).unwrap();
        assert_eq!(resolved.decision, Decision::Ask);
        assert_eq!(resolved.scope, Scope::Default);
    }

    #[test]
    fn a_project_rule_beats_a_global_one() {
        let conn = db();
        set(&conn, None, GitForcePush, Some(Decision::Deny), 1).unwrap();
        set(&conn, Some("p1"), GitForcePush, Some(Decision::Allow), 2).unwrap();

        let in_project = resolve(&conn, Some("p1"), GitForcePush).unwrap();
        assert_eq!(in_project.decision, Decision::Allow);
        assert_eq!(in_project.scope, Scope::Project);

        // Another project still follows the global rule.
        let elsewhere = resolve(&conn, Some("p2"), GitForcePush).unwrap();
        assert_eq!(elsewhere.decision, Decision::Deny);
        assert_eq!(elsewhere.scope, Scope::Global);
    }

    #[test]
    fn clearing_a_project_rule_returns_it_to_the_global_one() {
        let conn = db();
        set(&conn, None, RecursiveDelete, Some(Decision::Deny), 1).unwrap();
        set(&conn, Some("p1"), RecursiveDelete, Some(Decision::Allow), 2).unwrap();
        set(&conn, Some("p1"), RecursiveDelete, None, 3).unwrap();

        let resolved = resolve(&conn, Some("p1"), RecursiveDelete).unwrap();
        assert_eq!(resolved.decision, Decision::Deny);
        assert_eq!(resolved.scope, Scope::Global);
    }

    #[test]
    fn setting_the_same_rule_twice_updates_it_rather_than_duplicating() {
        let conn = db();
        set(&conn, None, PackagePublish, Some(Decision::Allow), 1).unwrap();
        set(&conn, None, PackagePublish, Some(Decision::Deny), 2).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM guardrail_policies WHERE operation = ?1",
                [PackagePublish.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            resolve(&conn, None, PackagePublish).unwrap().decision,
            Decision::Deny
        );
    }

    #[test]
    fn a_global_rule_and_a_project_rule_are_separate_rows() {
        // The unique index folds NULL to '', so this is worth pinning down: if
        // they collided, setting a project rule would silently overwrite the
        // global one for every other project.
        let conn = db();
        set(&conn, None, SecretAccess, Some(Decision::Deny), 1).unwrap();
        set(&conn, Some("p1"), SecretAccess, Some(Decision::Allow), 2).unwrap();

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM guardrail_policies WHERE operation = ?1",
                [SecretAccess.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(
            resolve(&conn, None, SecretAccess).unwrap().decision,
            Decision::Deny
        );
    }

    #[test]
    fn every_operation_resolves() {
        let conn = db();
        let all = resolve_all(&conn, Some("p1")).unwrap();
        assert_eq!(all.len(), super::super::classify::ALL.len());
        assert!(all.iter().all(|(_, r)| r.decision == Decision::Ask));
    }

    #[test]
    fn a_snapshot_falls_back_to_asking_for_an_operation_it_does_not_carry() {
        // A guard from a newer build may recognise an operation this snapshot
        // predates. Asking is the safe reading of an unknown entry.
        let snapshot = Snapshot {
            version: SNAPSHOT_VERSION,
            session_id: "s".into(),
            project_id: "p".into(),
            mission_id: None,
            provider: "claude-code".into(),
            attended: true,
            decisions: Default::default(),
            log_path: "x".into(),
        };
        assert_eq!(snapshot.decision_for(GitForcePush), Decision::Ask);
    }

    #[test]
    fn decisions_round_trip_through_storage() {
        for decision in [Decision::Ask, Decision::Allow, Decision::Deny] {
            assert_eq!(Decision::parse(decision.as_str()), Some(decision));
        }
        assert_eq!(Decision::parse("maybe"), None);
    }
}
