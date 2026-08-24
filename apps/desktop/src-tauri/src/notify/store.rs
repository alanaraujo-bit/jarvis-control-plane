//! Raising, storing and reading notifications (§49).

use rusqlite::{params, OptionalExtension};
use serde::Serialize;

use crate::db::Database;
use crate::session::event::Confidence;
use crate::session::log::now_ms;

use super::{Attention, Kind, Raise, Reason, PREVIEW_CHARS};

/// The name of the event the surface listens for.
///
/// One event for every notification, carrying the whole row. The surface never
/// has to go and fetch what just happened, which matters because it may be
/// about to fire a toast from a window nobody is looking at.
pub const EVENT: &str = "jarvis://notification";

/// How long the same thing has to wait before it can be raised again.
///
/// An agent redraws its question every time the selection moves, and a
/// transcript can be re-read after a reconnect. Without a window here, one
/// question could arrive as a dozen notifications.
///
/// **Keyed on the preview, not only on the session and the reason.** An agent
/// that asks about one file, is approved, and asks about a different file
/// twenty seconds later has asked two questions — and swallowing the second is
/// exactly the failure this feature exists to prevent.
const COOLDOWN_MS: i64 = 45_000;

/// One notification, as the surface receives it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notification {
    pub id: i64,
    pub ts_ms: i64,
    pub kind: Kind,
    pub reason: Reason,
    /// Official when a provider stated it, Observed when we read it (§28).
    pub confidence: Confidence,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub session_id: Option<String>,
    pub mission_id: Option<String>,
    pub mission_title: Option<String>,
    pub provider: Option<String>,
    /// The agent's own words. Untranslated, deliberately — see the migration.
    pub preview: Option<String>,
    /// A stable identifier for a cause more specific than `reason`.
    pub detail_code: Option<String>,
    pub seen_at: Option<i64>,
    pub acted_at: Option<i64>,
}


/// Raise a notification, unless the person is already looking at it.
///
/// Returns the stored notification when one was raised, and `None` when it was
/// suppressed or is a duplicate. Never fails loudly: a notification that cannot
/// be stored must not take down the agent it is about.
pub fn raise(
    db: &Database,
    attention: &Attention,
    reason: Reason,
    confidence: Confidence,
    raised: Raise,
) -> Option<Notification> {
    if !attention.is_enabled() {
        return None;
    }
    // The rule the whole feature turns on. See the module header.
    if attention.is_watching(raised.session_id.as_deref()) {
        return None;
    }

    // A driven session's finished turns are not news (§32).
    //
    // An unattended run finishes a turn every minute or two for as long as it
    // lasts, and nobody asked to hear about any of them — setting a mission to
    // Unattended is asking *not* to watch. The run announces itself once, when
    // it stops, and that is the notification that was promised.
    //
    // Only `TurnEnded` is dropped. A driven agent that stops to ask a question
    // is the one thing about a driven run worth interrupting somebody for: the
    // run cannot continue until they answer.
    if reason == Reason::TurnEnded && attention.is_driven(raised.session_id.as_deref()) {
        return None;
    }

    let preview = raised
        .preview
        .as_deref()
        .map(|text| crate::providers::conversation::truncate(text, PREVIEW_CHARS))
        .filter(|text| !text.is_empty());

    let now = now_ms();
    if is_a_repeat(db, &raised, reason, preview.as_deref(), now) {
        return None;
    }

    let kind = reason.kind();
    let id = db
        .with(|conn| {
            conn.execute(
                "INSERT INTO notifications
                     (ts_ms, kind, reason, confidence, project_id, session_id,
                      mission_id, provider, preview, seen_at, acted_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL)",
                params![
                    now,
                    kind.as_str(),
                    reason.as_str(),
                    confidence_str(confidence),
                    raised.project_id,
                    raised.session_id,
                    raised.mission_id,
                    raised.provider,
                    preview,
                ],
            )?;
            Ok(conn.last_insert_rowid())
        })
        .map_err(|error| tracing::warn!(%error, "could not record a notification"))
        .ok()?;

    // `detail_code` is not stored. It is only ever set alongside a reason that
    // already implies it — an autopilot stop carries its own §34 cause — and a
    // column that is NULL for nine kinds out of ten earns its place by being
    // read, which nothing does after the toast has been shown.
    let mut notification = one(db, id)?;
    notification.detail_code = raised.detail_code;
    Some(notification)
}

fn confidence_str(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Official => "official",
        Confidence::Observed => "observed",
        Confidence::Estimated => "estimated",
        Confidence::Unknown => "unknown",
    }
}

fn parse_confidence(text: &str) -> Confidence {
    match text {
        "official" => Confidence::Official,
        "observed" => Confidence::Observed,
        "estimated" => Confidence::Estimated,
        _ => Confidence::Unknown,
    }
}

/// Whether this is the same thing, said again, too soon.
fn is_a_repeat(
    db: &Database,
    raised: &Raise,
    reason: Reason,
    preview: Option<&str>,
    now: i64,
) -> bool {
    db.with(|conn| {
        let found: Option<i64> = conn
            .query_row(
                "SELECT id FROM notifications
                  WHERE reason = ?1
                    AND ts_ms > ?2
                    AND session_id IS ?3
                    AND preview IS ?4
                  LIMIT 1",
                params![reason.as_str(), now - COOLDOWN_MS, raised.session_id, preview],
                |row| row.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    })
    .unwrap_or(false)
}

const SELECT: &str = "SELECT n.id, n.ts_ms, n.kind, n.reason, n.confidence,
            n.project_id, p.name, n.session_id, n.mission_id, m.title,
            n.provider, n.preview, n.seen_at, n.acted_at
       FROM notifications n
       LEFT JOIN projects p ON p.id = n.project_id
       LEFT JOIN missions m ON m.id = n.mission_id";

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Notification> {
    Ok(Notification {
        id: row.get(0)?,
        ts_ms: row.get(1)?,
        kind: Kind::parse(&row.get::<_, String>(2)?),
        reason: Reason::parse(&row.get::<_, String>(3)?),
        confidence: parse_confidence(&row.get::<_, String>(4)?),
        project_id: row.get(5)?,
        project_name: row.get(6)?,
        session_id: row.get(7)?,
        mission_id: row.get(8)?,
        mission_title: row.get(9)?,
        provider: row.get(10)?,
        preview: row.get(11)?,
        detail_code: None,
        seen_at: row.get(12)?,
        acted_at: row.get(13)?,
    })
}

pub fn one(db: &Database, id: i64) -> Option<Notification> {
    db.with(|conn| {
        conn.query_row(&format!("{SELECT} WHERE n.id = ?1"), params![id], read)
            .optional()
    })
    .ok()
    .flatten()
}

/// The most recent notifications, newest first.
///
/// Ordered by id as well as time. Two agents finishing in the same millisecond
/// is not exotic — several run at once, and this is a feature about several
/// things stopping — and `ts_ms` alone leaves their order to whatever the
/// query planner feels like, which is how a list quietly reshuffles itself
/// between two refreshes that changed nothing.
pub fn recent(db: &Database, limit: u32) -> crate::db::Result<Vec<Notification>> {
    let limit = limit.clamp(1, 500) as i64;
    db.with(|conn| {
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY n.ts_ms DESC, n.id DESC LIMIT ?1"))?;
        let rows = stmt.query_map(params![limit], read)?;
        rows.collect()
    })
}

/// How many notifications have not been seen.
///
/// Counted rather than derived in the surface, because the badge is drawn
/// before the list has been fetched and must not flicker from 0 to n.
///
/// **Unseen, not unanswered.** A badge that only cleared once every question
/// had been *acted on* would keep a number on the titlebar for a question the
/// person read and decided to leave — which is a nag, not a notification.
pub fn outstanding(db: &Database) -> crate::db::Result<u32> {
    db.with(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM notifications WHERE seen_at IS NULL",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|n| n as u32)
    })
}

/// Mark specific notifications as seen.
pub fn mark_seen(db: &Database, ids: &[i64]) -> crate::db::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    db.with(|conn| {
        let tx = conn.unchecked_transaction()?;
        {
            let mut stmt =
                tx.prepare("UPDATE notifications SET seen_at = ?1 WHERE id = ?2 AND seen_at IS NULL")?;
            let now = now_ms();
            for id in ids {
                stmt.execute(params![now, id])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

/// Mark everything as seen.
pub fn mark_all_seen(db: &Database) -> crate::db::Result<()> {
    db.with(|conn| {
        conn.execute(
            "UPDATE notifications SET seen_at = ?1 WHERE seen_at IS NULL",
            params![now_ms()],
        )?;
        Ok(())
    })
}

/// Record that the person went to the thing, not merely saw it existed.
pub fn mark_acted(db: &Database, id: i64) -> crate::db::Result<()> {
    db.with(|conn| {
        let now = now_ms();
        conn.execute(
            "UPDATE notifications
                SET acted_at = ?1, seen_at = COALESCE(seen_at, ?1)
              WHERE id = ?2",
            params![now, id],
        )?;
        Ok(())
    })
}

/// Forget everything.
pub fn clear(db: &Database) -> crate::db::Result<()> {
    db.with(|conn| {
        conn.execute("DELETE FROM notifications", [])?;
        Ok(())
    })
}

/// Drop rows nobody will ever read again.
///
/// Called once at startup rather than on a timer: the list is capped by what a
/// person can plausibly care about, and a table that grows without bound is a
/// slow leak in a product meant to run for months.
pub fn prune(db: &Database, keep: u32) -> crate::db::Result<usize> {
    db.with(|conn| {
        conn.execute(
            "DELETE FROM notifications
              WHERE id NOT IN (
                    SELECT id FROM notifications ORDER BY ts_ms DESC, id DESC LIMIT ?1)",
            params![keep],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Database {
        Database::open_in_memory().expect("open a database")
    }

    fn raise_one(db: &Database, attention: &Attention, session: &str, preview: &str) -> Option<Notification> {
        raise(
            db,
            attention,
            Reason::ProviderPrompt,
            Confidence::Observed,
            Raise {
                session_id: Some(session.into()),
                preview: Some(preview.into()),
                ..Default::default()
            },
        )
    }

    #[test]
    fn a_question_from_a_session_nobody_is_watching_is_raised() {
        let db = db();
        let attention = Attention::default();
        let raised = raise_one(&db, &attention, "s1", "Do you want to create hello.txt?")
            .expect("raised");
        assert_eq!(raised.kind, Kind::NeedsApproval);
        assert_eq!(raised.confidence, Confidence::Observed);
        assert_eq!(outstanding(&db).unwrap(), 1);
    }

    /// The rule the whole feature turns on.
    #[test]
    fn nothing_is_raised_about_a_session_the_person_is_looking_at() {
        let db = db();
        let attention = Attention::default();
        attention.set_focused(true);
        attention.set_visible_sessions(vec!["s1".into()]);

        assert!(raise_one(&db, &attention, "s1", "Do you want to proceed?").is_none());
        // And it is dropped, not stored-and-marked-read: the centre is a list
        // of what you missed, not a transcript of what you watched.
        assert_eq!(recent(&db, 10).unwrap().len(), 0);
    }

    #[test]
    fn a_different_session_is_still_raised_while_one_is_watched() {
        let db = db();
        let attention = Attention::default();
        attention.set_visible_sessions(vec!["s1".into()]);
        assert!(raise_one(&db, &attention, "s2", "Do you want to proceed?").is_some());
    }

    #[test]
    fn turning_notifications_off_stops_them_being_recorded_at_all() {
        let db = db();
        let attention = Attention::default();
        attention.set_enabled(false);
        assert!(raise_one(&db, &attention, "s1", "anything").is_none());
        assert_eq!(recent(&db, 10).unwrap().len(), 0);
    }

    #[test]
    fn the_same_question_twice_in_a_row_is_one_notification() {
        let db = db();
        let attention = Attention::default();
        assert!(raise_one(&db, &attention, "s1", "Do you want to create hello.txt?").is_some());
        assert!(raise_one(&db, &attention, "s1", "Do you want to create hello.txt?").is_none());
    }

    /// The case a cooldown keyed only on the session would get wrong, and it is
    /// the ordinary shape of an agent working through a task.
    #[test]
    fn a_second_different_question_is_not_swallowed_by_the_first() {
        let db = db();
        let attention = Attention::default();
        assert!(raise_one(&db, &attention, "s1", "Do you want to create a.txt?").is_some());
        assert!(raise_one(&db, &attention, "s1", "Do you want to create b.txt?").is_some());
        assert_eq!(outstanding(&db).unwrap(), 2);
    }

    /// The noise an unattended run would otherwise produce.
    ///
    /// A driven agent finishes a turn every minute or two for as long as the
    /// run lasts. Setting a mission to Unattended is asking *not* to watch, so
    /// being told about each of those turns is the opposite of what was asked
    /// for — and the run already announces itself once, when it stops.
    #[test]
    fn a_driven_sessions_finished_turns_are_not_raised() {
        let db = db();
        let attention = Attention::default();
        attention.set_driven("s1", true);

        let turn = |session: &str| {
            raise(
                &db,
                &attention,
                Reason::TurnEnded,
                Confidence::Official,
                Raise {
                    session_id: Some(session.into()),
                    preview: Some("did a thing".into()),
                    ..Default::default()
                },
            )
        };

        assert!(turn("s1").is_none());
        // A session nobody is driving is unaffected.
        assert!(turn("s2").is_some());

        // And when the run gives the seat back, its turns are news again.
        attention.set_driven("s1", false);
        assert!(turn("s1").is_some());
    }

    /// The one thing about a driven run that *is* worth interrupting somebody
    /// for: it cannot continue until they answer.
    #[test]
    fn a_driven_session_that_stops_to_ask_is_still_raised() {
        let db = db();
        let attention = Attention::default();
        attention.set_driven("s1", true);

        let asked = raise(
            &db,
            &attention,
            Reason::ProviderPrompt,
            Confidence::Observed,
            Raise {
                session_id: Some("s1".into()),
                preview: Some("Do you want to proceed?".into()),
                ..Default::default()
            },
        );
        assert!(asked.is_some());
    }

    #[test]
    fn seeing_a_notification_takes_it_off_the_badge() {
        let db = db();
        let attention = Attention::default();
        let first = raise_one(&db, &attention, "s1", "one").unwrap();
        raise_one(&db, &attention, "s2", "two").unwrap();
        assert_eq!(outstanding(&db).unwrap(), 2);

        mark_seen(&db, &[first.id]).unwrap();
        assert_eq!(outstanding(&db).unwrap(), 1);

        mark_all_seen(&db).unwrap();
        assert_eq!(outstanding(&db).unwrap(), 0);
    }

    /// Being seen and being acted on are different facts, and the second one
    /// implies the first.
    #[test]
    fn acting_on_a_notification_also_counts_as_seeing_it() {
        let db = db();
        let attention = Attention::default();
        let raised = raise_one(&db, &attention, "s1", "one").unwrap();
        mark_acted(&db, raised.id).unwrap();
        let stored = one(&db, raised.id).unwrap();
        assert!(stored.acted_at.is_some());
        assert!(stored.seen_at.is_some());
    }

    #[test]
    fn a_preview_longer_than_a_toast_can_show_is_cut_on_a_character() {
        let db = db();
        let attention = Attention::default();
        let long = "ação ".repeat(200);
        let raised = raise_one(&db, &attention, "s1", &long).unwrap();
        let preview = raised.preview.unwrap();
        assert!(preview.chars().count() <= PREVIEW_CHARS + 1);
        assert!(preview.ends_with('…'));
    }

    #[test]
    fn pruning_keeps_the_newest_and_drops_the_rest() {
        let db = db();
        let attention = Attention::default();
        for i in 0..12 {
            raise_one(&db, &attention, &format!("s{i}"), &format!("question {i}"));
        }
        assert_eq!(recent(&db, 100).unwrap().len(), 12);
        prune(&db, 5).unwrap();
        let left = recent(&db, 100).unwrap();
        assert_eq!(left.len(), 5);
        assert_eq!(left[0].preview.as_deref(), Some("question 11"));
    }

    #[test]
    fn clearing_empties_the_list() {
        let db = db();
        let attention = Attention::default();
        raise_one(&db, &attention, "s1", "one");
        clear(&db).unwrap();
        assert_eq!(recent(&db, 10).unwrap().len(), 0);
        assert_eq!(outstanding(&db).unwrap(), 0);
    }
}
