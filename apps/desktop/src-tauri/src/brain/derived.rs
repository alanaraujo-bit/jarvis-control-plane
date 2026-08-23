//! What the log already knows about a project (§37/§39).
//!
//! Nothing here is stored. Every fact is recomputed from the append-only log
//! and the tables mirrored out of it, which is the same choice Review made and
//! for the same reason: a **stored** derived fact is one that can go stale
//! without anybody noticing, and a memory layer that quietly lies is worse than
//! one that is empty.
//!
//! ## What earns a place here
//!
//! Only facts that would change what somebody *does*. Analytics (§52) already
//! counts tokens and runtime; repeating that would be a second dashboard
//! wearing a different hat. These answer a narrower question:
//!
//! > *"What should I know before I touch this project?"*
//!
//! Which files everyone keeps editing. What has been refused here. Whether
//! missions in this project tend to hold up or get revoked. A number that does
//! not change a decision is noise, and noise in a briefing is expensive twice —
//! once in tokens and once in attention.
//!
//! Each fact carries a `code` the interface localises rather than a sentence
//! (§65), because these are read by people in two languages and shipped into a
//! briefing in one.

use serde::{Deserialize, Serialize};

use crate::db::Database;

use super::Result;

/// Something computed from the record, with the numbers that produced it.
///
/// `code` names the sentence; `args` fills it. Prose is never assembled here —
/// the guardrail refusal learned that lesson first (§65).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Fact {
    pub code: String,
    /// Values for the message, in the order the catalogue names them.
    pub args: Vec<String>,
    /// The single most important thing about a derived fact: that it is
    /// derived. Carried so a surface cannot render it as something stated.
    pub derived: bool,
    /// The number the sentence has to agree with.
    ///
    /// Carried separately from `args` because it is not the interface's job to
    /// guess which argument decides the plural, and getting that wrong renders
    /// **"1 sessões"** — which is exactly what it did until somebody looked at
    /// the screen. pt-BR and English happen to agree on the rule here, and both
    /// disagree with a sentence that ignores it.
    pub count: i64,
}

impl Fact {
    fn new(code: &str, args: Vec<String>, count: i64) -> Self {
        Self {
            code: code.into(),
            args,
            derived: true,
            count,
        }
    }
}

/// One thing that happened, for the project's own history (§39).
///
/// Activity (§48) already records the moments worth knowing and the core can
/// already filter it by project — but until now no surface asked it to, so a
/// project had a global feed and no story of its own. This is that story.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Moment {
    pub ts_ms: i64,
    /// The activity kind, localised through `activity.kind.<kind>`.
    pub kind: String,
    pub title: String,
    pub severity: String,
}

/// How far back the derived facts look.
///
/// A project worked on for a year would otherwise report the hot files of last
/// spring as though they were today's. Ninety days is long enough to survive a
/// quiet fortnight and short enough to still describe the present.
const WINDOW_DAYS: i64 = 90;

fn window_start() -> i64 {
    crate::session::log::now_ms() - WINDOW_DAYS * 24 * 60 * 60 * 1000
}

/// Everything worth saying about a project, computed now.
pub fn facts(db: &Database, project_id: &str) -> Result<Vec<Fact>> {
    let since = window_start();
    let mut out = Vec::new();

    db.with(|conn| {
        // ---- Who has worked here ------------------------------------------
        let mut stmt = conn.prepare(
            "SELECT provider, COUNT(*) FROM sessions
              WHERE project_id = ?1 AND provider <> 'shell'
              GROUP BY provider
              ORDER BY COUNT(*) DESC",
        )?;
        let providers: Vec<(String, i64)> = stmt
            .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (provider, count) in providers {
            out.push(Fact::new(
                "brain.fact.sessions",
                vec![provider_name(&provider), count.to_string()],
                count,
            ));
        }

        // ---- Missions, and whether they held -------------------------------
        //
        // Completed and revoked are reported together on purpose. A project
        // with nine completions and six revocations is telling you something
        // that "nine completed" on its own actively hides (§30).
        let completed: i64 = conn.query_row(
            "SELECT COUNT(*) FROM missions WHERE project_id = ?1 AND status = 'completed'",
            [project_id],
            |r| r.get(0),
        )?;
        let blocked: i64 = conn.query_row(
            "SELECT COUNT(*) FROM missions
              WHERE project_id = ?1 AND status IN ('blocked', 'failed')",
            [project_id],
            |r| r.get(0),
        )?;
        if completed > 0 {
            out.push(Fact::new(
                "brain.fact.completed",
                vec![completed.to_string()],
                completed,
            ));
        }
        if blocked > 0 {
            out.push(Fact::new("brain.fact.blocked", vec![blocked.to_string()], blocked));
        }

        // Completion being taken back is the most informative thing this
        // product records about a project, because it means a criterion that
        // once held stopped holding (§30).
        let revoked: i64 = conn.query_row(
            "SELECT COUNT(*) FROM activity
              WHERE project_id = ?1 AND kind = 'mission.completionRevoked'",
            [project_id],
            |r| r.get(0),
        )?;
        if revoked > 0 {
            out.push(Fact::new("brain.fact.revoked", vec![revoked.to_string()], revoked));
        }

        // ---- What keeps being edited ---------------------------------------
        //
        // The files an agent is most likely to touch next, and the ones most
        // likely to be delicate.
        let mut stmt = conn.prepare(
            "SELECT path, COUNT(*) AS touches FROM file_changes
              WHERE project_id = ?1 AND ts_ms >= ?2
              GROUP BY path
              ORDER BY touches DESC, path ASC
              LIMIT 5",
        )?;
        let hot: Vec<(String, i64)> = stmt
            .query_map(rusqlite::params![project_id, since], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        // One edit is not a pattern; saying "changed 1 time" about five files
        // would fill the panel with nothing.
        for (path, touches) in hot.into_iter().filter(|(_, n)| *n > 1) {
            out.push(Fact::new(
                "brain.fact.hotFile",
                vec![tidy_path(&path), touches.to_string()],
                touches,
            ));
        }

        // ---- What has been stopped here ------------------------------------
        //
        // The most directly useful thing an agent can be told: this is what
        // gets refused in this project, so do not plan around it.
        let mut stmt = conn.prepare(
            "SELECT operation, COUNT(*) FROM guardrail_events
              WHERE project_id = ?1 AND status = 'denied'
              GROUP BY operation
              ORDER BY COUNT(*) DESC
              LIMIT 4",
        )?;
        let refused: Vec<(String, i64)> = stmt
            .query_map([project_id], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (operation, count) in refused {
            out.push(Fact::new(
                "brain.fact.refused",
                vec![operation, count.to_string()],
                count,
            ));
        }

        Ok(())
    })
    .map_err(|e| e.to_string())?;

    Ok(out)
}

/// This project's own history, newest first (§39).
pub fn timeline(db: &Database, project_id: &str) -> Result<Vec<Moment>> {
    db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT ts_ms, kind, title, severity FROM activity
              WHERE project_id = ?1
              ORDER BY ts_ms DESC
              LIMIT 60",
        )?;
        let rows: rusqlite::Result<Vec<_>> = stmt
            .query_map([project_id], |r| {
                Ok(Moment {
                    ts_ms: r.get(0)?,
                    kind: r.get(1)?,
                    title: r.get(2)?,
                    severity: r.get(3)?,
                })
            })?
            .collect();
        rows
    })
    .map_err(|e| e.to_string())
}

fn provider_name(id: &str) -> String {
    match id {
        "claude-code" => "Claude Code".into(),
        "codex" => "Codex".into(),
        other => other.into(),
    }
}

/// Recorded paths are relative to the session's working directory and spelled
/// with backslashes on Windows (see `review::attributions`). Neither is how a
/// person reads a path.
fn tidy_path(path: &str) -> String {
    path.replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_shown_the_way_a_person_reads_one() {
        assert_eq!(tidy_path(r"src\game\types.ts"), "src/game/types.ts");
        assert_eq!(tidy_path("src/game/types.ts"), "src/game/types.ts");
    }

    #[test]
    fn a_fact_says_that_it_is_derived() {
        // The one property a derived fact must never lose on the way to a
        // surface: that it was computed rather than stated (§28).
        let fact = Fact::new("brain.fact.completed", vec!["3".into()], 3);
        assert!(fact.derived);
        let json = serde_json::to_string(&fact).unwrap();
        assert!(
            json.contains(r#""derived":true"#),
            "the flag has to survive serialisation, or the UI cannot tell: {json}"
        );
    }

    #[test]
    fn a_fact_carries_a_code_and_arguments_rather_than_a_sentence() {
        // Prose assembled in Rust cannot be translated (§65), and this panel is
        // read in two languages.
        let fact = Fact::new("brain.fact.hotFile", vec!["src/app.ts".into(), "9".into()], 9);
        assert_eq!(fact.code, "brain.fact.hotFile");
        assert_eq!(fact.args, vec!["src/app.ts", "9"]);
        assert!(
            !fact.code.contains(' '),
            "a code is an identifier, never a sentence"
        );
    }

    #[test]
    fn the_window_is_a_recent_one() {
        let start = window_start();
        let now = crate::session::log::now_ms();
        assert!(start < now);
        let days = (now - start) / (24 * 60 * 60 * 1000);
        assert_eq!(days, WINDOW_DAYS);
    }
}
