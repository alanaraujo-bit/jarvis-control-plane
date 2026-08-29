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
    /// A model running in this machine's own GPU (§92).
    ///
    /// Launched through the same agent binary as `Codex`, configured against a
    /// local server. See `crate::localai` for why that is the build rather than
    /// a chat client, and for the three ways it genuinely differs.
    Local,
}

impl SessionKind {
    fn provider_id(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Local => crate::localai::PROVIDER_ID,
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

    /// Whether a past conversation of this kind can be handed back to it
    /// (§88, D41). Read from the capability model, never matched on here (§26).
    fn can_resume(self) -> bool {
        let id = self.provider_id();
        crate::providers::all()
            .into_iter()
            .map(|p| p.capabilities())
            .find(|c| c.id == id)
            .map(|c| c.resume_support.is_available())
            .unwrap_or(false)
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

/// What a local-model session needs and a cloud provider does not.
///
/// Built by `crate::localai` from the runtime configuration, because which
/// model and which endpoint are settings a person changed, not constants this
/// launcher can know.
pub struct LocalLaunch {
    /// This runtime's own configuration root. Set as `CODEX_HOME` so local
    /// rollouts land in a tree only this provider writes to.
    pub home: PathBuf,
    pub args: Vec<String>,
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
///
/// `resume` is the provider's own id for a past conversation to continue
/// (§88, D41).
///
/// `local` is the prepared local runtime, and is Some only for a local-model
/// session (§92).
fn command_for(
    kind: SessionKind,
    session_id: &str,
    guardrail_settings: Option<&std::path::Path>,
    brief: Option<&std::path::Path>,
    resume: Option<&str>,
    local: Option<&LocalLaunch>,
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
            // Continuing a past conversation (§88, D41).
            //
            // `--fork-session` is not optional here, and the reason is
            // correlation. Verified against Claude Code 2.1.241: `--resume <id>`
            // alone keeps writing to the *original* transcript, so two of our
            // sessions would tail one file and each would claim the other's
            // turns. With `--fork-session` the CLI honours our `--session-id`
            // and writes a new transcript named for it — deterministic
            // correlation survives being a continuation.
            //
            // What that new transcript contains is the trap: it is a **full
            // copy** of the prior conversation followed by the new turns, and
            // every copied line is rewritten with the new session id, so the id
            // cannot tell them apart. Copied lines do keep their **original
            // timestamps**, which is the boundary `transcript::spawn` uses. See
            // D41 — without it, every token of the old conversation is counted
            // a second time in Analytics and every sentence found twice in
            // Global Search.
            if let Some(previous) = resume {
                args.push("--resume".into());
                args.push(previous.to_string());
                args.push("--fork-session".into());
            }
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
        // The same binary as Codex, pointed at a model in this machine.
        //
        // `CODEX_HOME` is the load-bearing part. Without it a local session
        // would write its rollout into the same `~/.codex/sessions` tree a
        // cloud Codex session writes to, and `codex::correlate` — which
        // matches on working directory and start time — could hand each of two
        // sessions started seconds apart in one project the other's transcript.
        SessionKind::Local => match local {
            Some(local) => (
                "codex".into(),
                local.args.clone(),
                vec![(
                    "CODEX_HOME".to_string(),
                    local.home.to_string_lossy().to_string(),
                )],
            ),
            // Unreachable in practice: `start_session` refuses before it gets
            // here. Launching into the user's own Codex configuration would be
            // the one wrong answer available, so this launches nothing.
            None => (String::new(), vec![], vec![]),
        },
    }
}

/// Get the local runtime ready for a session, or refuse with a reason.
///
/// Three things have to be true before a local agent can start, and each of
/// them fails differently enough that collapsing them into one message would
/// leave a person with nothing to act on: a model must be chosen, the server
/// must be answering, and the configuration root must be writable.
///
/// The context window is read from the **resident runner** when there is one,
/// because that is the process that will serve the request. Handing the agent
/// a window it does not have is the failure mode this whole path exists to
/// avoid — see `localai::prepare`.
fn prepare_local(state: &State<'_, AppState>) -> Result<LocalLaunch> {
    use crate::session::manager::SessionError::Refused;

    let config = crate::localai::config(&state.db);
    let Some(model) = config.model.clone() else {
        return Err(Refused("localAi.noModel".into()));
    };
    if crate::localai::ollama::version(&config.endpoint).is_none() {
        return Err(Refused("localAi.unreachable".into()));
    }

    let resident = crate::localai::ollama::resident(&config.endpoint).unwrap_or_default();
    let context = crate::localai::measured_context(&config, &resident);

    let home = crate::localai::prepare(&state.data_dir, &config, context)
        .map_err(|_| Refused("localAi.configWriteFailed".into()))?;

    // Warm the card before the first prompt, unless the person turned that off.
    //
    // On its own thread: a cold 27B Q4 load was measured at about ten seconds
    // on this machine, and blocking the launch for it would freeze the window
    // on a click. The session starts either way — the runner loads on the
    // first turn if this has not finished, which is exactly what would have
    // happened without a preload.
    if config.preload_on_start && !resident.iter().any(|r| r.name == model) {
        let endpoint = config.endpoint.clone();
        let keep_alive = config.keep_alive();
        std::thread::spawn(move || {
            if let Err(error) = crate::localai::ollama::load(&endpoint, &model, &keep_alive) {
                tracing::warn!(%error, "preloading the local model failed; it will load on the first turn");
            }
        });
    }

    Ok(LocalLaunch {
        home,
        args: crate::localai::launch_args(&config),
    })
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
    resume_from: Option<String>,
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
        resume_from,
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
    // A past session to pick up from (§88, D41). The new session is a new
    // process with its own log and its own row; this is what makes it a
    // continuation rather than an unrelated start.
    resume_from: Option<String>,
) -> Result<SessionInfo> {
    // Started by a person, so the seat in front of it is theirs.
    launch(
        &state,
        project_id,
        kind,
        cwd,
        cols,
        rows,
        mission_id,
        false,
        resume_from,
    )
    .map(|l| l.info)
}

/// How a past session is handed back to its provider (§88, D41).
///
/// Resolved from the stored row rather than taken from the caller: the webview
/// knows a J.A.R.V.I.S. session id, and what the CLI needs is the id the
/// *provider* used, which is not always the same thing.
struct Resume {
    /// The J.A.R.V.I.S. session being continued.
    session_id: String,
    /// The id to hand the provider on the command line.
    provider_session_id: String,
}

/// Work out whether a resume is possible, and refuse clearly when it is not.
///
/// Refusals are codes the surface localises (§65), never prose. Each one is a
/// different situation and they are deliberately not collapsed: "this provider
/// cannot do it" and "we never learned this session's provider id" lead to
/// different answers for the person reading them.
fn resolve_resume(
    state: &State<'_, AppState>,
    kind: SessionKind,
    resume_from: &str,
) -> Result<Resume> {
    let row: Option<(String, Option<String>)> = state.db.with(|conn| {
        conn.query_row(
            "SELECT provider, provider_session_id FROM sessions WHERE id = ?1",
            [resume_from],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })
    })?;

    let Some((provider, provider_session_id)) = row else {
        return Err(super::manager::SessionError::Refused(
            "resume.notFound".into(),
        ));
    };

    // Continuing a Claude Code conversation inside Codex is not a resume, it is
    // a different conversation with a borrowed id.
    if provider != kind.provider_id() {
        return Err(super::manager::SessionError::Refused(
            "resume.providerMismatch".into(),
        ));
    }

    // The capability model decides, not a `match` here (§26).
    if !kind.can_resume() {
        return Err(super::manager::SessionError::Refused(
            "resume.unsupported".into(),
        ));
    }

    // Claude Code is told our id at launch, so this is always present for one
    // of its sessions. Codex assigns its own and we only learn it once its
    // rollout has been located — a session that never got that far cannot be
    // handed back to it.
    let Some(provider_session_id) = provider_session_id else {
        return Err(super::manager::SessionError::Refused(
            "resume.unknownProviderSession".into(),
        ));
    };

    Ok(Resume {
        session_id: resume_from.to_string(),
        provider_session_id,
    })
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
    resume_from: Option<String>,
) -> Result<AgentLaunch> {
    // Resolved first, before anything exists: a resume that cannot happen must
    // fail before a session row, a log directory and a process are created for
    // it. Refusing afterwards would leave a started session that is not the
    // continuation it was asked to be.
    let resume = match resume_from.as_deref() {
        Some(previous) => Some(resolve_resume(state, kind, previous)?),
        None => None,
    };

    // The working directory defaults to the project folder.
    let project_path: String = state.db.with(|conn| {
        conn.query_row(
            "SELECT path FROM projects WHERE id = ?1",
            [&project_id],
            |row| row.get(0),
        )
    })?;

    let cwd = cwd.unwrap_or(project_path);

    // A session must start where it says it starts.
    //
    // ## Why this refuses instead of carrying on
    //
    // Found by continuing a session from History whose project folder had since
    // been deleted (a scratch project from an earlier milestone). The launch
    // "succeeded": a real Claude Code process started, drew its trust prompt,
    // and reported `Accessing workspace: C:\Users\Alan Araujo` — **the user's
    // home directory**. A non-existent working directory is not honoured by the
    // spawn, so the child lands wherever the parent happens to be, and the
    // product had just pointed an agent with read, write and execute at
    // everything the person owns while the header above it named a project.
    //
    // This is not specific to continuing a session — it is true of every launch
    // into a project whose folder has moved or been removed, which `Project`
    // already reports as `exists: false` and nothing acted on. Silently working
    // somewhere other than where you said is the worst available outcome, worse
    // than not starting, so this refuses with a code the surface localises.
    if !std::path::Path::new(&cwd).is_dir() {
        return Err(super::manager::SessionError::Refused(
            "session.cwdMissing".into(),
        ));
    }
    let id = new_session_id();
    let log_dir = state.session_dir(&id);
    // No registered account is a supported state: the provider then receives
    // exactly the environment and transcript roots it did before §66.
    let account = crate::accounts::active(&state.db, kind.provider_id());
    let account_id = account.as_ref().map(|account| account.id.clone());
    let mut transcript_root = account.as_ref().and_then(|account| {
        crate::accounts::transcript_root(
            &account.provider,
            std::path::Path::new(&account.config_dir),
        )
    });

    // A local model has no account, so everything an account would have
    // supplied — where the transcript lands, what configuration the runner
    // reads — comes from the runtime instead (§92).
    let local = match kind {
        SessionKind::Local => Some(prepare_local(state)?),
        _ => None,
    };
    if let Some(local) = local.as_ref() {
        transcript_root = Some(local.home.join("sessions"));
    }

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
        // A local session is the same runner reading the same hooks file, so
        // it is guarded by exactly the same route — and waits to be trusted in
        // exactly the same way.
        (SessionKind::Codex | SessionKind::Local, Some(snapshot)) => {
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
        command_for(
            kind,
            &id,
            guardrail_settings.as_deref(),
            brief.as_deref(),
            resume.as_ref().map(|r| r.provider_session_id.as_str()),
            local.as_ref(),
        );
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
                  updated_at, provider_session_id, events_backfilled_at, account_id,
                  resumed_from)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9, ?8, ?10, ?11)",
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
                // The session this one picked up from (§88, D41). What makes
                // two rows one thread.
                resume.as_ref().map(|r| r.session_id.clone()),
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
        // Only a resumed session needs a boundary, and it needs it badly —
        // see D41 and `transcript::spawn`. A session starting from nothing
        // has no prior conversation to be handed back, so it takes every line
        // its transcript ever contains, exactly as before.
        resume.as_ref().map(|_| created_at),
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
    attachment_id: String,
    channel: Channel<InvokeResponseBody>,
) -> Result<Response> {
    let log_dir = log_dir_of(&state, &session_id)?;
    let history = state.sessions.get(&session_id)?.attach_with_replay(
        attachment_id,
        channel,
        || {
            let reader = SessionLogReader::open(&log_dir)?;
            Ok(reader.replay_pty(REPLAY_LIMIT)?)
        },
    )?;
    // Someone is looking now, so a guardrail that wants a human decision has
    // one to ask (§35).
    crate::guardrail::sessions::set_attended(&state.session_dir(&session_id), true);
    Ok(Response::new(history))
}

/// Detach the view. The session keeps running (§32).
#[tauri::command]
pub fn session_detach(
    state: State<'_, AppState>,
    session_id: String,
    attachment_id: String,
) -> Result<()> {
    if !state.sessions.get(&session_id)?.detach(&attachment_id) {
        return Ok(());
    }
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
        let (program, args, _) = command_for(SessionKind::ClaudeCode, "abc-123", None, None, None, None);
        assert_eq!(program, "claude");
        assert_eq!(
            args,
            vec!["--session-id".to_string(), "abc-123".to_string()]
        );
    }

    #[test]
    /// Continuing a conversation must keep deterministic correlation (§88, D41).
    ///
    /// `--fork-session` is the load-bearing flag: without it Claude Code 2.1.241
    /// ignores `--session-id` and keeps writing to the *original* transcript,
    /// so two of our sessions would tail one file and each would claim the
    /// other's turns.
    #[test]
    fn resuming_forks_so_the_new_session_still_owns_its_own_transcript() {
        let (program, args, _) = command_for(
            SessionKind::ClaudeCode,
            "new-id",
            None,
            None,
            Some("old-provider-id"),
            None,
        );

        assert_eq!(program, "claude");
        assert!(
            args.windows(2)
                .any(|w| w == ["--resume", "old-provider-id"]),
            "the past conversation has to be named: {args:?}"
        );
        assert!(
            args.iter().any(|a| a == "--fork-session"),
            "without --fork-session the new session writes into the old \
             transcript and correlation breaks: {args:?}"
        );
        assert!(
            args.windows(2).any(|w| w == ["--session-id", "new-id"]),
            "the fork still has to be named for *our* id: {args:?}"
        );
    }

    /// A session that is not continuing anything must launch exactly as it did
    /// before this existed.
    #[test]
    fn a_session_that_resumes_nothing_carries_no_resume_flags() {
        let (_, args, _) = command_for(SessionKind::ClaudeCode, "s1", None, None, None, None);
        assert!(!args.iter().any(|a| a == "--resume"));
        assert!(!args.iter().any(|a| a == "--fork-session"));
    }

    #[test]
    fn codex_sessions_do_not_claim_an_id_they_cannot_set() {
        let (program, args, _) = command_for(SessionKind::Codex, "abc-123", None, None, None, None);
        assert_eq!(program, "codex");
        assert!(
            !args.iter().any(|a| a.contains("abc-123")),
            "Codex 0.147.0 has no session-id flag; pretending otherwise would \
             silently break correlation"
        );
    }

    /// The single most important line in the local launch (§92).
    ///
    /// A local session runs the same binary as Codex. Without its own
    /// `CODEX_HOME` both would write rollouts into `~/.codex/sessions`, and
    /// `codex::correlate` — which matches a session to a rollout by working
    /// directory and start time — could hand each of two sessions started
    /// seconds apart in one project the other's transcript. Every turn, every
    /// token count and every searched sentence would then be filed against the
    /// wrong session.
    #[test]
    fn a_local_session_writes_its_rollouts_somewhere_only_it_writes() {
        let local = LocalLaunch {
            home: PathBuf::from("C:\\app-data\\local-runtime\\codex-home"),
            args: vec!["--oss".into()],
        };
        let (program, _args, env) =
            command_for(SessionKind::Local, "abc-123", None, None, None, Some(&local));

        assert_eq!(program, "codex", "the local provider is the same runner");
        assert_eq!(
            env,
            vec![(
                "CODEX_HOME".to_string(),
                "C:\\app-data\\local-runtime\\codex-home".to_string()
            )],
        );
    }

    /// The launch carries whatever the runtime configuration produced, rather
    /// than flags spelled a second time here where they could drift.
    #[test]
    fn the_local_launch_uses_the_configured_arguments() {
        let local = LocalLaunch {
            home: PathBuf::from("home"),
            args: crate::localai::launch_args(&crate::localai::RuntimeConfig {
                model: Some("qwen3.8:latest".into()),
                ..Default::default()
            }),
        };
        let (_, args, _) = command_for(SessionKind::Local, "s1", None, None, None, Some(&local));
        assert!(args.windows(2).any(|w| w == ["-m", "qwen3.8:latest"]));
    }

    /// Reached only if a refusal upstream were ever removed. Launching into
    /// the user's own Codex configuration is the one wrong answer available
    /// here, so nothing launches at all.
    #[test]
    fn an_unprepared_local_session_launches_nothing_rather_than_the_wrong_thing() {
        let (program, _, env) = command_for(SessionKind::Local, "s1", None, None, None, None);
        assert!(program.is_empty());
        assert!(
            env.is_empty(),
            "no CODEX_HOME means the user's own configuration; better to start \
             nothing than to write into it"
        );
    }

    /// A local model is not a cloud subscription, and the capability model has
    /// to keep saying so where the launcher can see it.
    #[test]
    fn the_local_kind_maps_onto_the_provider_that_has_no_account() {
        assert_eq!(SessionKind::Local.provider_id(), "local");
        let capabilities = crate::providers::by_id("local")
            .expect("the local provider is registered")
            .capabilities();
        assert!(!capabilities.account_switching);
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
        let (program, args, _) = command_for(SessionKind::ClaudeCode, "s1", None, Some(brief), None, None);

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
        let (_, args, _) = command_for(SessionKind::ClaudeCode, "s1", None, None, None, None);
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
        let (_, args, _) = command_for(SessionKind::Codex, "s1", None, Some(brief), None, None);
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
