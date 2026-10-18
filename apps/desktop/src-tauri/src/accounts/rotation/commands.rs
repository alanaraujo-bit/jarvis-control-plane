//! What the surface asks the rotation engine, and the per-session numbers.

use serde::Serialize;
use tauri::State;

use crate::accounts::quota::{AccountQuota, Eta};
use crate::accounts::Account;
use crate::db::Database;
use crate::AppState;

/// One stretch of a session on one account.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub account_id: String,
    pub from_ms: i64,
    pub to_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowEta {
    /// `five_hour` or `weekly`.
    pub window: String,
    pub eta: Eta,
}

/// Everything the session's quota strip and popover draw.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionQuotaSummary {
    pub account: Option<Account>,
    /// Windows with their confidence, the live reading included.
    pub quota: Option<AccountQuota>,
    pub etas: Vec<WindowEta>,
    /// Token sums the provider reported for this session's turns (Observed).
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// Cache reads plus cache writes.
    pub tokens_cache: i64,
    pub started_at_ms: i64,
    pub segments: Vec<Segment>,
    /// Where the next move would go, best first.
    pub queue: Vec<super::rank::Ranked>,
}

pub fn summary(db: &Database, session_id: &str, now_ms: i64) -> Result<SessionQuotaSummary, String> {
    let (account_id, started_at_ms): (Option<String>, i64) = db
        .with(|conn| {
            conn.query_row(
                "SELECT account_id, created_at FROM sessions WHERE id = ?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .map_err(|e| e.to_string())?;
    let account = match account_id.as_deref() {
        Some(id) => crate::accounts::get(db, id)?,
        None => None,
    };
    let (tokens_in, tokens_out, tokens_cache): (i64, i64, i64) = db
        .with(|conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0) + COALESCE(SUM(cache_write_tokens), 0)
                   FROM usage_samples WHERE session_id = ?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .map_err(|e| e.to_string())?;
    let relays: Vec<(i64, String)> = db
        .with(|conn| {
            let mut stmt = conn.prepare(
                "SELECT ts_ms, detail FROM activity
                  WHERE session_id = ?1 AND kind = 'account.relayed' ORDER BY ts_ms",
            )?;
            let rows = stmt
                .query_map([session_id], |row| Ok((row.get(0)?, row.get::<_, Option<String>>(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.into_iter().map(|(ts, d)| (ts, d.unwrap_or_default())).collect())
        })
        .map_err(|e| e.to_string())?;

    let mut segments = Vec::new();
    let mut from_ms = started_at_ms;
    for (ts, detail) in &relays {
        let value: serde_json::Value = serde_json::from_str(detail).unwrap_or_default();
        if let Some(from) = value.get("fromAccountId").and_then(|v| v.as_str()) {
            segments.push(Segment { account_id: from.into(), from_ms, to_ms: Some(*ts) });
            from_ms = *ts;
        }
    }
    if let Some(id) = account_id.clone() {
        segments.push(Segment { account_id: id, from_ms, to_ms: None });
    }

    let quota = account.as_ref().map(|a| crate::accounts::quota::for_account(db, a));
    let etas = account
        .as_ref()
        .map(|a| {
            ["five_hour", "weekly"]
                .into_iter()
                .filter_map(|window| {
                    crate::accounts::quota::eta(db, &a.id, window, now_ms)
                        .map(|eta| WindowEta { window: window.into(), eta })
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(SessionQuotaSummary {
        account,
        quota,
        etas,
        tokens_in,
        tokens_out,
        tokens_cache,
        started_at_ms,
        segments,
        queue: Vec::new(),
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[tauri::command]
pub fn session_quota_summary(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<SessionQuotaSummary, String> {
    let mut summary = summary(&state.db, &session_id, now_ms())?;
    summary.queue = super::engine::queue(&session_id);
    Ok(summary)
}

#[tauri::command]
pub fn session_account_queue(session_id: String) -> Vec<super::rank::Ranked> {
    super::engine::queue(&session_id)
}

#[tauri::command]
pub async fn session_switch_account(
    state: State<'_, AppState>,
    session_id: String,
    account_id: String,
) -> Result<(), String> {
    let to = crate::accounts::get(&state.db, &account_id)?
        .ok_or_else(|| "accounts.notFound".to_string())?;
    // Off the async runtime: the relay waits out the refusal window.
    tauri::async_runtime::spawn_blocking(move || super::engine::switch_now(&session_id, &to))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn rotation_answer(decision_id: String, accept: bool) -> Result<(), String> {
    super::engine::answer(&decision_id, accept)
}

#[tauri::command]
pub fn rotation_accept_wait(decision_id: String, accept: bool) -> Result<(), String> {
    super::engine::accept_wait(&decision_id, accept)
}

#[tauri::command]
pub fn rotation_cancel_wait(decision_id: String) -> Result<(), String> {
    super::engine::cancel_wait(&decision_id)
}

/// Open questions and waits, for a screen that loads after they were asked.
#[tauri::command]
pub fn rotation_pending() -> Vec<super::engine::RotationEvent> {
    super::engine::pending()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_splits_segments_by_switch() {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute_batch(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                     VALUES ('p1', 'demo', 'C:/demo', 0, 0);
                 INSERT INTO provider_accounts (id, provider, label, config_dir, adopted, position, created_at)
                     VALUES ('a', 'claude-code', '', 'C:/a', 0, 0, 0),
                            ('b', 'claude-code', '', 'C:/b', 0, 1, 0);
                 INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at, account_id)
                     VALUES ('s1', 'p1', 'claude-code', 'C:/demo', 'idle', 'C:/logs', 1000, 'b');
                 INSERT INTO activity (ts_ms, project_id, session_id, kind, severity, title, detail)
                     VALUES (5000, 'p1', 's1', 'account.relayed', 'info', 'claude-code',
                             '{\"fromAccountId\":\"a\",\"toAccountId\":\"b\",\"reason\":\"exhausted\"}');
                 INSERT INTO usage_samples (session_id, project_id, provider, ts_ms, input_tokens,
                         output_tokens, cache_read_tokens, cache_write_tokens, confidence)
                     VALUES ('s1', 'p1', 'claude-code', 2000, 100, 10, 1000, 5, 'official'),
                            ('s1', 'p1', 'claude-code', 6000, 50, 20, 0, 0, 'official');",
            )?;
            Ok(())
        })
        .unwrap();

        let summary = summary(&db, "s1", 9000).unwrap();

        assert_eq!(summary.account.as_ref().map(|a| a.id.as_str()), Some("b"));
        assert_eq!(summary.started_at_ms, 1000);
        assert_eq!(summary.tokens_in, 150);
        assert_eq!(summary.tokens_out, 30);
        assert_eq!(summary.tokens_cache, 1005);
        assert_eq!(
            summary.segments,
            vec![
                Segment { account_id: "a".into(), from_ms: 1000, to_ms: Some(5000) },
                Segment { account_id: "b".into(), from_ms: 5000, to_ms: None },
            ]
        );
    }
}
