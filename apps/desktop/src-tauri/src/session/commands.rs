//! Session commands exposed to the UI.

use std::path::PathBuf;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::State;

use super::manager::{new_session_id, timestamp, Result, SessionInfo};
use super::{SessionLogReader, SessionState};
use crate::pty::PtyOptions;
use crate::AppState;

/// How much terminal history is restored into a reattached view.
///
/// Bounded because xterm's own scrollback is finite and writing megabytes into
/// it on attach would stall the first paint (§11).
const REPLAY_LIMIT: usize = 256 * 1024;

/// What kind of session to start.
///
/// Agent providers are named rather than described by an executable, so the UI
/// never has to know how a provider is launched — that belongs to the adapter.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SessionKind {
    Shell,
    ClaudeCode,
    Codex,
}

impl SessionKind {
    fn provider_id(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
        }
    }
}

/// Pick the user's shell.
///
/// Prefers modern PowerShell, then Windows PowerShell, then `cmd`. Respects
/// `COMSPEC` last so an explicitly configured shell still wins over nothing.
#[cfg(windows)]
fn default_shell() -> (String, Vec<String>) {
    for candidate in ["pwsh.exe", "powershell.exe"] {
        if which(candidate).is_some() {
            return (candidate.to_string(), vec!["-NoLogo".into()]);
        }
    }
    (
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()),
        vec![],
    )
}

#[cfg(not(windows))]
fn default_shell() -> (String, Vec<String>) {
    (
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
        vec!["-l".into()],
    )
}

#[cfg(windows)]
fn which(bin: &str) -> Option<String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("where")
        .arg(bin)
        .creation_flags(0x0800_0000)
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    })
}

fn command_for(kind: SessionKind, session_id: &str) -> (String, Vec<String>, Vec<(String, String)>) {
    match kind {
        SessionKind::Shell => {
            let (program, args) = default_shell();
            (program, args, vec![])
        }
        // Passing our own session id makes the provider's transcript file
        // deterministic, so the structured stream can be correlated to this
        // session without guessing (§26).
        SessionKind::ClaudeCode => (
            "claude".into(),
            vec!["--session-id".into(), session_id.to_string()],
            vec![],
        ),
        // Codex has no equivalent flag on 0.147.0, so correlation is done by
        // watching its rollout directory instead. A real capability difference.
        SessionKind::Codex => ("codex".into(), vec![], vec![]),
    }
}

#[tauri::command]
pub fn session_start(
    state: State<'_, AppState>,
    project_id: String,
    kind: SessionKind,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<SessionInfo> {
    // The working directory defaults to the project folder.
    let project_path: String = state.db.with(|conn| {
        conn.query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [&project_id],
            |row| row.get(0),
        )
    })?;

    let cwd = cwd.unwrap_or(project_path);
    let id = new_session_id();
    let log_dir = state.session_dir(&id);
    let (program, args, env) = command_for(kind, &id);

    let created_at = timestamp();
    state.db.with(|conn| {
        conn.execute(
            "INSERT INTO sessions
                 (id, project_id, provider, cwd, state, log_dir, created_at, updated_at,
                  provider_session_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)",
            params![
                id,
                project_id,
                kind.provider_id(),
                cwd,
                "starting",
                log_dir.to_string_lossy(),
                created_at,
                // Claude Code uses our id verbatim; Codex assigns its own.
                (kind == SessionKind::ClaudeCode).then(|| id.clone()),
            ],
        )?;
        Ok(())
    })?;

    let session = state.sessions.start(
        id.clone(),
        log_dir,
        PtyOptions {
            program,
            args,
            cwd: PathBuf::from(&cwd),
            cols,
            rows,
            env,
        },
    )?;

    Ok(SessionInfo {
        id,
        project_id,
        provider: kind.provider_id().to_string(),
        title: None,
        cwd,
        state: session.state(),
        created_at,
        live: true,
    })
}

/// Attach a terminal view. Output flows to `channel` from here on.
#[tauri::command]
pub fn session_attach(
    state: State<'_, AppState>,
    session_id: String,
    channel: Channel<InvokeResponseBody>,
) -> Result<()> {
    state.sessions.get(&session_id)?.attach(channel);
    Ok(())
}

/// Detach the view. The session keeps running (§32).
#[tauri::command]
pub fn session_detach(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sessions.get(&session_id)?.detach();
    Ok(())
}

#[tauri::command]
pub fn session_write(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> Result<()> {
    state.sessions.get(&session_id)?.write(&data)
}

#[tauri::command]
pub fn session_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<()> {
    state.sessions.get(&session_id)?.resize(cols, rows)
}

#[tauri::command]
pub fn session_close(state: State<'_, AppState>, session_id: String) -> Result<()> {
    let session = state.sessions.get(&session_id)?;
    session.kill()?;
    state.sessions.remove(&session_id);

    state.db.with(|conn| {
        conn.execute(
            "UPDATE sessions SET state = 'completed', ended_at = ?2, updated_at = ?2
              WHERE id = ?1",
            params![session_id, timestamp()],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Terminal history for a reattaching view, as raw bytes.
///
/// Returned as a binary response rather than JSON: this is terminal data, and
/// encoding it as a string would both inflate it and risk mangling sequences.
#[tauri::command]
pub fn session_replay(state: State<'_, AppState>, session_id: String) -> Result<Response> {
    let log_dir: String = state.db.with(|conn| {
        conn.query_row(
            "SELECT log_dir FROM sessions WHERE id = ?1",
            [&session_id],
            |row| row.get(0),
        )
    })?;

    let reader = SessionLogReader::open(&log_dir)?;
    Ok(Response::new(reader.replay_pty(REPLAY_LIMIT)?))
}

#[tauri::command]
pub fn session_list(state: State<'_, AppState>, project_id: String) -> Result<Vec<SessionInfo>> {
    let live = state.sessions.ids();

    let rows = state.db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, provider, title, cwd, state, created_at
               FROM sessions
              WHERE project_id = ?1 AND ended_at IS NULL
              ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([&project_id], |row| {
            let id: String = row.get(0)?;
            Ok(SessionInfo {
                live: false, // filled in below
                project_id: row.get(1)?,
                provider: row.get(2)?,
                title: row.get(3)?,
                cwd: row.get(4)?,
                state: parse_state(&row.get::<_, String>(5)?),
                created_at: row.get(6)?,
                id,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;

    Ok(rows
        .into_iter()
        .map(|mut info| {
            // A row can outlive its process — after a crash, for instance — so
            // liveness comes from the manager, never from the stored state.
            info.live = live.contains(&info.id);
            if !info.live && info.state == SessionState::Working {
                info.state = SessionState::Failed;
            }
            info
        })
        .collect())
}

fn parse_state(text: &str) -> SessionState {
    match text {
        "working" => SessionState::Working,
        "waiting" => SessionState::Waiting,
        "idle" => SessionState::Idle,
        "completed" => SessionState::Completed,
        "blocked" => SessionState::Blocked,
        "failed" => SessionState::Failed,
        _ => SessionState::Starting,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_default_shell_is_always_resolvable() {
        let (program, _args) = default_shell();
        assert!(!program.is_empty(), "there must always be a shell to fall back to");
    }

    #[test]
    fn claude_sessions_carry_our_session_id() {
        // Deterministic correlation to the provider's own transcript depends on
        // this flag being passed (§26).
        let (program, args, _) = command_for(SessionKind::ClaudeCode, "abc-123");
        assert_eq!(program, "claude");
        assert_eq!(args, vec!["--session-id".to_string(), "abc-123".to_string()]);
    }

    #[test]
    fn codex_sessions_do_not_claim_an_id_they_cannot_set() {
        let (program, args, _) = command_for(SessionKind::Codex, "abc-123");
        assert_eq!(program, "codex");
        assert!(
            !args.iter().any(|a| a.contains("abc-123")),
            "Codex 0.147.0 has no session-id flag; pretending otherwise would \
             silently break correlation"
        );
    }

    #[test]
    fn session_states_round_trip_from_storage() {
        assert_eq!(parse_state("working"), SessionState::Working);
        assert_eq!(parse_state("blocked"), SessionState::Blocked);
        assert_eq!(parse_state("nonsense"), SessionState::Starting);
    }
}
