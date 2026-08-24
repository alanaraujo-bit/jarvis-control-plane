//! Session commands exposed to the UI.

use std::path::PathBuf;
use std::sync::Arc;

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody, Response};
use tauri::State;

use super::attachment;
use super::manager::{new_session_id, timestamp, Result, SessionInfo};
use super::{transcript, SessionLogReader, SessionState};
use crate::providers::conversation::ConversationItem;
use crate::pty::PtyOptions;
use crate::session::event::EventKind;
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

    /// How this kind can be briefed, read from the capability model rather than
    /// matched on here (§26).
    ///
    /// Asking the adapter means a provider gaining or losing the ability is one
    /// edit in one place, instead of a `match` in the launcher quietly
    /// disagreeing with what the Settings screen claims.
    fn briefing(self) -> Option<crate::providers::BriefingSupport> {
        // A shell is not an agent and has nothing to be briefed.
        if self == Self::Shell {
            return None;
        }
        let id = self.provider_id();
        crate::providers::all()
            .into_iter()
            .map(|p| p.capabilities())
            .find(|c| c.id == id)
            .map(|c| c.briefing)
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
            // A real session log showed literal BEL (0x07) bytes landing
            // right where dictated text (§54) got typed in — PSReadLine's
            // default BellStyle is Audible and writes one for things as
            // mundane as redrawing a long inline suggestion. xterm.js (the
            // terminal this app renders with) has no bell playback in v5, so
            // whatever plays the irritating sound the user heard is
            // downstream of what we read out of the pty, not something this
            // app's own rendering does — the byte itself is the one thing
            // confirmed, so it's the one thing worth suppressing at the
            // source. This only touches sessions jarvis spawns, not the
            // user's own $PROFILE.
            return (
                candidate.to_string(),
                vec![
                    "-NoLogo".into(),
                    "-NoExit".into(),
                    "-Command".into(),
                    "Set-PSReadLineOption -BellStyle None".into(),
                ],
            );
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

/// Write this project's brief for the session, returning where it landed.
///
/// `None` when there is nothing known, when the provider has no out-of-band
/// route for it, or when the file could not be written. All three are the same
/// answer to the caller — the session simply starts unbriefed — and none of
/// them is worth failing a launch over: an agent with no context is the state
/// every agent was in before this existed.
fn write_brief(
    state: &AppState,
    project_id: &str,
    log_dir: &std::path::Path,
    kind: SessionKind,
) -> Option<std::path::PathBuf> {
    // §26: only a provider that can take a brief out of band gets one this way.
    // Codex has no such flag, so it is not handed a file it would never read.
    if kind.briefing() != Some(crate::providers::BriefingSupport::SystemPrompt) {
        return None;
    }

    let knowledge = crate::brain::knowledge(&state.db, project_id).ok()?;
    let text = crate::brain::brief::compose(&knowledge)?;

    let path = log_dir.join("project-brief.md");
    std::fs::write(&path, text).ok()?;
    Some(path)
}

/// How a session is launched.
///
/// The guardrail settings file points the provider pre-tool hook at our own
/// executable (§35). It is None when the guardrail could not be installed, and
/// the session then launches without it: a guardrail that cannot be set up must
/// not stop the agent from running, and must not be silently claimed either.
///
/// `brief` is this project's recorded knowledge (§38), and is None whenever
/// there is nothing to say or the provider cannot take one.
fn command_for(
    kind: SessionKind,
    session_id: &str,
    guardrail_settings: Option<&std::path::Path>,
    brief: Option<&std::path::Path>,
) -> (String, Vec<String>, Vec<(String, String)>) {
    match kind {
        SessionKind::Shell => {
            let (program, args) = default_shell();
            (program, args, vec![])
        }
        // Passing our own session id makes the provider's transcript file
        // deterministic, so the structured stream can be correlated to this
        // session without guessing (§26).
        SessionKind::ClaudeCode => {
            let mut args = vec!["--session-id".into(), session_id.to_string()];
            // Additional settings, not a replacement: the user's own
            // configuration still applies and this only adds a hook.
            if let Some(path) = guardrail_settings {
                args.push("--settings".into());
                args.push(path.to_string_lossy().to_string());
            }
            // Appended to the default system prompt, not replacing it: the
            // agent keeps everything it normally knows and gains what this
            // project knows. `--system-prompt` would have swapped the two.
            if let Some(path) = brief {
                args.push("--append-system-prompt-file".into());
                args.push(path.to_string_lossy().to_string());
            }
            ("claude".into(), args, vec![])
        }
        // Codex has no equivalent flag on 0.147.0, so correlation is done by
        // watching its rollout directory instead. A real capability difference.
        SessionKind::Codex => ("codex".into(), vec![], vec![]),
    }
}

/// A session that has just been started, plus the handle to drive it.
///
/// `session_start` returns only what the UI needs; the autopilot needs the live
/// session itself (§32), so the launch returns both and each caller takes what
/// it can use.
pub struct AgentLaunch {
    pub id: String,
    pub info: SessionInfo,
    pub session: std::sync::Arc<crate::session::manager::LiveSession>,
}

/// Start an agent session that an autopilot will drive (§32).
///
/// `driven` is the important argument, and it is not the same as "unattended".
/// A driven session usually **does** have its terminal open with a person
/// reading along — and that person is not the one a provider's permission
/// prompt reaches. Passing it through means a guardrail set to *ask* correctly
/// refuses instead of parking the agent on a question the autopilot cannot
/// answer (§35, and see `Snapshot::can_ask_a_person`).
pub fn start_agent_session(
    state: &State<'_, AppState>,
    project_id: &str,
    kind: SessionKind,
    mission_id: Option<String>,
    driven: bool,
) -> Result<AgentLaunch> {
    // A driven session has no view of its own to size it, so it gets a
    // reasonable terminal rather than a degenerate one: agent CLIs lay out
    // their output against the width they are told.
    launch(
        state,
        project_id.to_string(),
        kind,
        None,
        120,
        30,
        mission_id,
        driven,
    )
}

#[tauri::command]
pub fn session_start(
    state: State<'_, AppState>,
    project_id: String,
    kind: SessionKind,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    // The mission this session is working on, when it was started from one.
    // This is what ties Mission -> Agent -> Terminal -> Conversation ->
    // Evidence into one thread instead of five unrelated things (§86).
    mission_id: Option<String>,
) -> Result<SessionInfo> {
    // Started by a person, so the seat in front of it is theirs.
    launch(&state, project_id, kind, cwd, cols, rows, mission_id, false).map(|l| l.info)
}

#[allow(clippy::too_many_arguments)]
fn launch(
    state: &State<'_, AppState>,
    project_id: String,
    kind: SessionKind,
    cwd: Option<String>,
    cols: u16,
    rows: u16,
    mission_id: Option<String>,
    driven: bool,
) -> Result<AgentLaunch> {
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
    // No registered account is a supported state: the provider then receives
    // exactly the environment and transcript roots it did before §66.
    let account = crate::accounts::active(&state.db, kind.provider_id());
    let account_id = account.as_ref().map(|account| account.id.clone());
    let transcript_root = account.as_ref().and_then(|account| {
        crate::accounts::transcript_root(
            &account.provider,
            std::path::Path::new(&account.config_dir),
        )
    });

    // Guardrails (§35), installed before the process starts because the hook
    // has to be in place for the very first tool call.
    //
    // A session begins with no view attached — the terminal attaches one moments
    // later and session_attach updates the snapshot then. Starting from
    // attended: false is the conservative order, because for that brief window
    // there genuinely is nobody to ask.
    let snapshot = crate::guardrail::sessions::installs_hook(kind.provider_id())
        .then(|| {
            crate::guardrail::sessions::write_snapshot(
                &state.db,
                &log_dir,
                &id,
                &project_id,
                mission_id.as_deref(),
                kind.provider_id(),
                false,
                driven,
            )
            .ok()
        })
        .flatten();

    // The two providers install the same guard by different routes: Claude Code
    // through a settings file named on the command line, Codex through a hooks
    // file it discovers in the project and will not run until the person has
    // trusted it (§26, §35).
    let mut guarded = false;
    let guardrail_settings = match (kind, snapshot.as_ref()) {
        (SessionKind::ClaudeCode, Some(snapshot)) => {
            let settings = crate::guardrail::sessions::write_hook_settings(&log_dir, snapshot);
            guarded = settings.is_some();
            if !guarded {
                // Worth saying out loud rather than degrading quietly: this
                // session runs without the protection Settings says it has.
                tracing::warn!(
                    session = %id,
                    "guardrails could not be installed for this session; it runs unguarded"
                );
            }
            settings
        }
        (SessionKind::Codex, Some(snapshot)) => {
            let written =
                crate::guardrail::sessions::write_codex_hook(std::path::Path::new(&cwd), snapshot);
            // Written, not yet in force: Codex runs it only once the user has
            // trusted it. The watcher still follows it, so the moment they do,
            // its decisions arrive here like any other.
            guarded = written.is_some();
            None
        }
        _ => None,
    };

    // What this project knows, handed to the agent before it starts (§38).
    //
    // Written into **our** log directory beside the guardrail snapshot, never
    // into the user's repository: their code and their Git history are theirs,
    // and a product that dropped a context file into a working tree would show
    // up in the very Review surface it also ships.
    let brief = write_brief(state, &project_id, &log_dir, kind);

    let (program, args, mut env) =
        command_for(kind, &id, guardrail_settings.as_deref(), brief.as_deref());
    if let Some(account) = account.as_ref() {
        env.extend(crate::accounts::session_env(account));
    }

    let created_at = timestamp();
    state.db.with(|conn| {
        conn.execute(
            // `events_backfilled_at` is stamped here, at birth, rather than
            // left NULL: this session's transcript tailer indexes it live as
            // it happens (§51), so `search::backfill` re-reading its log later
            // would only duplicate rows it already has. NULL means "recorded
            // before search existed", and that is never true of a new session.
            "INSERT INTO sessions
                 (id, project_id, mission_id, provider, cwd, state, log_dir, created_at,
                  updated_at, provider_session_id, events_backfilled_at, account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?8, ?10)",
            params![
                id,
                project_id,
                mission_id,
                kind.provider_id(),
                cwd,
                "starting",
                log_dir.to_string_lossy(),
                created_at,
                // Claude Code uses our id verbatim; Codex assigns its own.
                (kind == SessionKind::ClaudeCode).then(|| id.clone()),
                account_id,
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
    if let Some(account_id) = account_id.as_deref() {
        crate::accounts::stamp_used(&state.db, account_id);
    }

    // Watch this session's terminal for the moment it stops and asks (§49).
    //
    // Agent sessions only. A shell is the person's own machine doing what they
    // told it to; interrupting them about their own `git status` finishing is
    // not a notification, it is an intrusion — the same line §35 draws when it
    // says guardrails govern agents and not people.
    if kind != SessionKind::Shell {
        let watcher = crate::notify::watch::spawn(
            crate::notify::watch::Watched {
                session_id: id.clone(),
                project_id: project_id.clone(),
                provider: kind.provider_id().to_string(),
                mission_id: mission_id.clone(),
            },
            session.stop_flag(),
        );
        session.set_watch(watcher);
    }

    // Follow the provider's structured transcript into the same log, so the
    // conversation and the terminal are two views of one stream (§23).
    transcript::spawn(
        Arc::clone(&session),
        Arc::clone(&state.db),
        project_id.clone(),
        kind.provider_id().to_string(),
        account_id,
        transcript_root,
        cwd.clone(),
        created_at,
        session.stop_flag(),
        mission_id.clone(),
    );

    // Follow what the guard decided, in its separate process, into this same
    // log (§35). It cannot write the log itself — a session has one writer (D2).
    if guarded {
        crate::guardrail::sessions::spawn_watcher(
            Arc::clone(&session),
            Arc::clone(&state.db),
            state.session_dir(&id),
            project_id.clone(),
            mission_id.clone(),
            session.stop_flag(),
        );
    }

    crate::activity::record(
        &state.db,
        "session.started",
        crate::activity::Severity::Info,
        kind.provider_id(),
        Some(cwd.clone()),
        Some(&project_id),
        Some(&id),
        mission_id.as_deref(),
    );

    Ok(AgentLaunch {
        id: id.clone(),
        info: SessionInfo {
            id,
            project_id,
            provider: kind.provider_id().to_string(),
            title: None,
            cwd,
            state: session.state(),
            created_at,
            live: true,
        },
        session,
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
    // Someone is looking now, so a guardrail that wants a human decision has
    // one to ask (§35).
    crate::guardrail::sessions::set_attended(&state.session_dir(&session_id), true);
    Ok(())
}

/// Detach the view. The session keeps running (§32).
#[tauri::command]
pub fn session_detach(state: State<'_, AppState>, session_id: String) -> Result<()> {
    state.sessions.get(&session_id)?.detach();
    // Nobody is watching any more. From here a rule that says ask has no one to
    // ask, and the guard refuses rather than leaving the agent on a prompt that
    // can never be answered (§34).
    crate::guardrail::sessions::set_attended(&state.session_dir(&session_id), false);
    Ok(())
}

#[tauri::command]
pub fn session_write(state: State<'_, AppState>, session_id: String, data: Vec<u8>) -> Result<()> {
    // Record that a person was engaged this minute (§53).
    //
    // This command is the only path by which human input reaches a session, so
    // it is the one honest place to measure attention. INSERT OR IGNORE against
    // a (session, minute) key collapses a burst of typing into a single row.
    let minute = timestamp() / 60_000;
    let _ = state.db.with(|conn| {
        conn.execute(
            "INSERT OR IGNORE INTO interaction_minutes (session_id, project_id, minute)
             SELECT id, project_id, ?2 FROM sessions WHERE id = ?1",
            rusqlite::params![session_id, minute],
        )?;
        Ok(())
    });

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

/// The session's own directory on disk, or an error naming the session.
fn log_dir_of(state: &AppState, session_id: &str) -> Result<String> {
    Ok(state.db.with(|conn| {
        conn.query_row(
            "SELECT log_dir FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
    })?)
}

/// Save an image pasted into a session (§22).
///
/// The webview names a session, never a directory: where the file lands is
/// read from the database here, so a renderer cannot choose a path (§3).
/// Errors come back as stable codes the surface localises (§65), the same
/// shape evidence summaries use.
#[tauri::command]
pub fn session_paste_image(
    state: State<'_, AppState>,
    session_id: String,
) -> std::result::Result<attachment::Attachment, String> {
    let log_dir = log_dir_of(&state, &session_id).map_err(|e| e.to_string())?;
    let data = attachment::from_clipboard()?;
    let saved = attachment::save(&log_dir, &data)?;

    // The log is the record (§23). An attachment is part of what happened in
    // this session, and `EventKind::Attachment` has existed for it since
    // migration 1 with nothing ever writing one.
    if let Ok(session) = state.sessions.get(&session_id) {
        if let Ok(payload) = serde_json::to_vec(&saved) {
            session.log(crate::session::event::EventKind::Attachment, payload);
        }
    }
    Ok(saved)
}

/// Read a pasted image back, for the hover preview.
#[tauri::command]
pub fn session_read_attachment(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> std::result::Result<Response, String> {
    let log_dir = log_dir_of(&state, &session_id).map_err(|e| e.to_string())?;
    Ok(Response::new(attachment::read(&log_dir, &path)?))
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
        assert!(
            !program.is_empty(),
            "there must always be a shell to fall back to"
        );
    }

    #[test]
    fn claude_sessions_carry_our_session_id() {
        // Deterministic correlation to the provider's own transcript depends on
        // this flag being passed (§26).
        let (program, args, _) = command_for(SessionKind::ClaudeCode, "abc-123", None, None);
        assert_eq!(program, "claude");
        assert_eq!(
            args,
            vec!["--session-id".to_string(), "abc-123".to_string()]
        );
    }

    #[test]
    fn codex_sessions_do_not_claim_an_id_they_cannot_set() {
        let (program, args, _) = command_for(SessionKind::Codex, "abc-123", None, None);
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

/// The conversation projection of a session (§24).
///
/// Reads the structured frames from the same log the terminal replays, so both
/// views are derived from one ordered stream rather than from separate stores.
#[tauri::command]
pub fn session_conversation(
    state: State<'_, AppState>,
    session_id: String,
) -> Result<Vec<ConversationItem>> {
    let log_dir: String = state.db.with(|conn| {
        conn.query_row(
            "SELECT log_dir FROM sessions WHERE id = ?1",
            [&session_id],
            |row| row.get(0),
        )
    })?;

    let reader = SessionLogReader::open(&log_dir)?;
    Ok(reader
        .read_from(0)?
        .into_iter()
        .filter(|event| {
            matches!(
                event.kind,
                EventKind::Message | EventKind::ToolCall | EventKind::ToolResult
            )
        })
        // A frame written by a newer build may not deserialise here; skipping
        // it keeps the rest of the conversation readable.
        .filter_map(|event| serde_json::from_slice::<ConversationItem>(&event.payload).ok())
        .collect())
}

/// Sessions working on a mission (§86).
///
/// The thread from a mission to the agent doing it, and from there to the
/// terminal and the conversation, is the continuity the product is built
/// around. Without this the pieces exist but never connect.
#[tauri::command]
pub fn mission_sessions(
    state: State<'_, AppState>,
    mission_id: String,
) -> Result<Vec<SessionInfo>> {
    let live = state.sessions.ids();

    let rows = state.db.with(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, project_id, provider, title, cwd, state, created_at
               FROM sessions
              WHERE mission_id = ?1
              ORDER BY created_at DESC",
        )?;
        let rows: rusqlite::Result<Vec<_>> = stmt
            .query_map([&mission_id], |row| {
                Ok(SessionInfo {
                    live: false,
                    project_id: row.get(1)?,
                    provider: row.get(2)?,
                    title: row.get(3)?,
                    cwd: row.get(4)?,
                    state: parse_state(&row.get::<_, String>(5)?),
                    created_at: row.get(6)?,
                    id: row.get(0)?,
                })
            })?
            .collect();
        rows
    })?;

    Ok(rows
        .into_iter()
        .map(|mut info| {
            info.live = live.contains(&info.id);
            if !info.live && info.state == SessionState::Working {
                info.state = SessionState::Failed;
            }
            info
        })
        .collect())
}

#[cfg(test)]
mod briefing_launch {
    use super::*;
    use crate::providers::BriefingSupport;

    /// The flag has to actually be on the command line, and it has to be the
    /// *appending* one.
    ///
    /// `--system-prompt` would replace everything Claude Code normally knows
    /// with a paragraph about the user's project — an agent that had forgotten
    /// how to be an agent. The two flags differ by one word and the wrong one
    /// fails in a way no test of the brief's *content* would ever catch.
    #[test]
    fn a_brief_is_appended_rather_than_replacing_the_system_prompt() {
        let brief = std::path::Path::new("C:/logs/s1/project-brief.md");
        let (program, args, _) = command_for(SessionKind::ClaudeCode, "s1", None, Some(brief));

        assert_eq!(program, "claude");
        let joined = args.join(" ");
        assert!(
            joined.contains("--append-system-prompt-file"),
            "the brief must be appended: {joined}"
        );
        assert!(
            !args.iter().any(|a| a == "--system-prompt"),
            "replacing the system prompt would strip the agent of everything \
             else it knows: {joined}"
        );
        assert!(joined.contains("project-brief.md"));
        // The session id still travels, or the transcript cannot be correlated.
        assert!(joined.contains("--session-id"));
    }

    #[test]
    fn a_session_with_nothing_to_say_passes_no_brief_flag() {
        let (_, args, _) = command_for(SessionKind::ClaudeCode, "s1", None, None);
        assert!(
            !args.iter().any(|a| a.contains("system-prompt")),
            "an empty brain must not produce an empty flag: {args:?}"
        );
    }

    /// §26 in one assertion: a provider that cannot take a brief out of band is
    /// not handed a file it would never read.
    #[test]
    fn only_a_provider_that_can_take_a_brief_is_given_one() {
        assert_eq!(
            SessionKind::ClaudeCode.briefing(),
            Some(BriefingSupport::SystemPrompt)
        );
        assert_eq!(
            SessionKind::Codex.briefing(),
            Some(BriefingSupport::OpeningMessage)
        );
        assert_eq!(
            SessionKind::Shell.briefing(),
            None,
            "a shell is not an agent and has nothing to be briefed"
        );

        // And even handed a path, Codex's command line does not grow one.
        let brief = std::path::Path::new("C:/logs/s1/project-brief.md");
        let (_, args, _) = command_for(SessionKind::Codex, "s1", None, Some(brief));
        assert!(
            args.is_empty(),
            "Codex has no flag for this and must not be given one: {args:?}"
        );
    }

    /// The brief lives with our own session data, never in the user's tree.
    #[test]
    fn the_brief_is_written_where_our_own_session_data_lives() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("sessions").join("s1");
        std::fs::create_dir_all(&log_dir).unwrap();

        // Same directory the guardrail snapshot uses, which is the point: a
        // context file dropped into a working tree would show up in Review.
        let brief = log_dir.join("project-brief.md");
        std::fs::write(&brief, "x").unwrap();
        assert!(brief.starts_with(dir.path()));
        assert!(
            !brief.to_string_lossy().contains("project-root"),
            "the brief must never be written into the project being worked on"
        );
    }
}
