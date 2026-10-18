//! The community boundary (M28). Local work never depends on this service.
//!
//! The webview reaches the community through one command, `social_request`,
//! which attaches the J.A.R.V.I.S. account's session and allows only
//! `/v1/social/*`: the server is the trust boundary, so a wrapper per endpoint
//! would add nothing. The core publishes on its own, while a profile exists: a
//! stats snapshot every 30 minutes and presence every minute, each skipped when
//! its visibility is private. Neither needs a screen open.
//!
//! What leaves the machine is aggregate numbers only — no project name, path,
//! prompt or transcript. A prompt leaves only when somebody presses Publish.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tauri::State;

use crate::db::Database;
use crate::identity::cloud::ORIGIN;
use crate::AppState;

const PRESENCE_EVERY: Duration = Duration::from_secs(60);
const STATS_EVERY: Duration = Duration::from_secs(30 * 60);
/// How often the publisher re-reads the profile's visibility choices.
const PROFILE_EVERY: Duration = Duration::from_secs(5 * 60);

/// Whether a webview-supplied request may go out with the account's session.
fn allowed(method: &str, path: &str) -> bool {
    matches!(method, "GET" | "POST" | "PUT" | "DELETE")
        && path.starts_with("/v1/social/")
        && !path.contains("..")
        && !path.contains('#')
        && !path.contains("//")
}

fn call(session: &str, method: &str, path: &str, body: Option<&Value>) -> Result<Value, String> {
    let request = ureq::request(method, &format!("{ORIGIN}{path}"))
        .set("authorization", &format!("Bearer {session}"))
        .set("content-type", "application/json")
        .timeout(Duration::from_secs(20));
    let response = match body {
        Some(body) => request.send_string(&body.to_string()),
        None => request.call(),
    };
    match response {
        Ok(response) => serde_json::from_reader(response.into_reader()).map_err(|e| e.to_string()),
        // The server answers refusals with a stable code; that is what the
        // surface translates.
        Err(ureq::Error::Status(status, response)) => {
            let value: Value = serde_json::from_reader(response.into_reader()).unwrap_or(Value::Null);
            Err(value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("social.http{status}")))
        }
        Err(_) => Err("social.unreachable".into()),
    }
}

#[tauri::command]
pub async fn social_request(
    state: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Value, String> {
    let method = method.to_uppercase();
    if !allowed(&method, &path) {
        return Err("social.pathRefused".into());
    }
    let session = crate::identity::cloud::bearer(&state.db).ok_or("social.signedOut")?;
    tauri::async_runtime::spawn_blocking(move || call(&session, &method, &path, body.as_ref()))
        .await
        .map_err(|e| e.to_string())?
}

/// The aggregate figures a profile may show, from the local analytics. Only
/// numbers: the per-project breakdown the analytics also hold never leaves.
pub fn snapshot(db: &Database) -> Result<Value, String> {
    let long = crate::analytics::report(db, 182, None).map_err(|e| e.to_string())?;
    let month = crate::analytics::report(db, 30, None).map_err(|e| e.to_string())?;
    let calendar = &long.calendar;
    let tail = |days: usize| calendar.iter().rev().take(days);
    let active = |days: usize| tail(days).filter(|c| c.tokens > 0).count() as i64;
    let tokens = |days: usize| tail(days).map(|c| c.tokens.max(0)).sum::<i64>();
    let total = |b: &crate::analytics::UsageBucket| (b.input + b.output + b.cache_read + b.cache_write).max(0);
    let all: i64 = month.by_model.iter().map(total).sum();
    let mut models: Vec<_> = month.by_model.iter().filter(|b| total(b) > 0).collect();
    models.sort_by_key(|b| std::cmp::Reverse(total(b)));
    Ok(json!({
        "streakCurrent": long.streaks.current.max(0),
        "streakLongest": long.streaks.longest.max(0),
        "activeDays30": active(30),
        "activeDays90": active(90),
        "tokens7": tokens(7),
        "tokens30": tokens(30),
        "agentMinutes30": month.leverage.agent_runtime_minutes.max(0),
        "humanMinutes30": month.leverage.human_active_minutes.max(0),
        "sessions30": month.leverage.sessions.max(0),
        "topModels": models.iter().take(5).map(|b| json!({
            "label": b.label,
            "share": if all > 0 { total(b) as f64 / all as f64 } else { 0.0 },
        })).collect::<Vec<_>>(),
        "heatmap": calendar.iter().map(|c| json!({"date": c.date, "tokens": c.tokens.max(0)})).collect::<Vec<_>>(),
    }))
}

fn active_sessions(db: &Database) -> i64 {
    db.with(|conn| {
        conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE state IN ('working','idle','starting') AND ended_at IS NULL",
            [],
            |row| row.get(0),
        )
    })
    .unwrap_or(0)
}

/// Publish stats and presence while a profile exists. Off every screen.
pub fn spawn_publisher(db: Arc<Database>) {
    std::thread::spawn(move || {
        let mut profile: Option<(Instant, Value)> = None;
        let mut last_stats: Option<Instant> = None;
        loop {
            std::thread::sleep(PRESENCE_EVERY);
            let Some(session) = crate::identity::cloud::bearer(&db) else {
                profile = None;
                continue;
            };
            if profile.as_ref().is_none_or(|(at, _)| at.elapsed() >= PROFILE_EVERY) {
                profile = call(&session, "GET", "/v1/social/me", None).ok().map(|me| (Instant::now(), me));
            }
            let Some((_, me)) = &profile else { continue };
            let Some(p) = me.get("profile").filter(|p| !p.is_null()) else { continue };
            let open = |key: &str| p.get(key).and_then(Value::as_str).is_some_and(|v| v != "private");
            if open("presenceVisibility") {
                let _ = call(&session, "POST", "/v1/social/presence", Some(&json!({"activeSessions": active_sessions(&db).min(100)})));
            }
            if open("statsVisibility") && last_stats.is_none_or(|at| at.elapsed() >= STATS_EVERY) {
                if let Ok(snapshot) = snapshot(&db) {
                    if call(&session, "PUT", "/v1/social/stats", Some(&json!({"snapshot": snapshot}))).is_ok() {
                        last_stats = Some(Instant::now());
                    }
                }
            }
        }
    });
}

/// Publish the stats now (after the person changes their visibility, say).
#[tauri::command]
pub async fn social_publish_stats(state: State<'_, AppState>) -> Result<(), String> {
    let db = Arc::clone(&state.db);
    let session = crate::identity::cloud::bearer(&db).ok_or("social.signedOut")?;
    tauri::async_runtime::spawn_blocking(move || {
        let snapshot = snapshot(&db)?;
        call(&session, "PUT", "/v1/social/stats", Some(&json!({"snapshot": snapshot}))).map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn only_community_paths_go_out_with_the_session() {
        assert!(allowed("GET", "/v1/social/feed?scope=discover"));
        assert!(allowed("DELETE", "/v1/social/posts/abc/like"));
        assert!(!allowed("GET", "/v1/sync/state"), "the account's own data is not reachable from here");
        assert!(!allowed("POST", "/v1/auth/sign-out"));
        assert!(!allowed("GET", "/v1/social/../sync/state"));
        assert!(!allowed("PATCH", "/v1/social/profile"));
        assert!(!allowed("GET", "https://evil.example/v1/social/me"));
    }

    #[test]
    fn the_snapshot_is_aggregate_numbers_only() {
        let db = Database::open_in_memory().unwrap();
        let now = crate::session::log::now_ms();
        db.with(|conn| {
            conn.execute("INSERT INTO projects (id, name, path, created_at, last_opened_at) VALUES ('p1', 'Secret Client', 'C:/secret-client', 0, 0)", [])?;
            conn.execute(
                "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at, ended_at) VALUES ('s1', 'p1', 'claude-code', 'C:/secret-client', 'completed', 'l', ?1, ?2)",
                params![now - 600_000, now],
            )?;
            conn.execute(
                "INSERT INTO usage_samples (session_id, project_id, provider, model, ts_ms, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, confidence)
                 VALUES ('s1', 'p1', 'claude-code', 'claude-opus-5-5', ?1, 100, 20, 0, 0, 'official')",
                params![now],
            )?;
            Ok(())
        })
        .unwrap();
        let snapshot = snapshot(&db).unwrap();
        assert_eq!(snapshot["tokens30"], 120);
        assert_eq!(snapshot["activeDays30"], 1);
        assert_eq!(snapshot["topModels"][0]["label"], "claude-opus-5-5");
        let text = snapshot.to_string();
        assert!(!text.contains("Secret Client") && !text.contains("secret-client"), "no project name or path leaves");
    }
}
