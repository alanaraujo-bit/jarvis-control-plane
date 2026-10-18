//! Moving a live session to another account, in place (M26).
//!
//! The session keeps its id, log, terminal and tailer; only the provider
//! process changes. The conversation travels with it: the transcript is copied
//! into the destination account's directory and the new process resumes it
//! (`claude --resume <id>`, measured to cross accounts on 2.1.302 when the file
//! is present — see the design doc §2.1).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::accounts::Account;
use crate::db::Database;
use crate::pty::PtyOptions;
use crate::session::event::{EventKind, Lifecycle};
use crate::session::manager::LiveSession;

/// Whether `codex resume <id>` has been measured to continue a rollout copied
/// into another `CODEX_HOME`. Until it has, Codex sessions are not moved: a
/// fresh conversation presented as a continuation would be a lie (§81).
pub const CODEX_RESUME_VERIFIED: bool = false;

/// A destination that exits this soon after starting refused the work — a
/// signed-out or broken login — and the session goes back where it was.
const REFUSAL_WINDOW: Duration = Duration::from_secs(5);

/// Typed to the agent when its turn was cut short by a usage limit.
pub const CONTINUE_INSTRUCTION: &str =
    "Continue exactly where you left off; the previous turn was interrupted by a usage limit.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    NearLimit,
    Exhausted,
    Manual,
    ResumeAfterWait,
}

impl Reason {
    /// Stable id for the log and the surface (§65: never prose).
    pub fn id(self) -> &'static str {
        match self {
            Reason::NearLimit => "nearLimit",
            Reason::Exhausted => "exhausted",
            Reason::Manual => "manual",
            Reason::ResumeAfterWait => "resumeAfterWait",
        }
    }

    /// Whether the agent's turn was interrupted and needs to be told to go on.
    pub fn interrupts(self) -> bool {
        matches!(self, Reason::Exhausted | Reason::ResumeAfterWait)
    }
}

/// Arguments that resume a conversation on the same transcript.
///
/// Claude Code: `--resume <id>` without `--fork-session` keeps writing to the
/// same `<id>.jsonl`, so correlation stays deterministic.
pub fn continue_args(provider: &str, provider_session_id: &str) -> Result<Vec<String>, String> {
    match provider {
        "claude-code" => Ok(vec!["--resume".into(), provider_session_id.into()]),
        "codex" => Ok(vec!["resume".into(), provider_session_id.into()]),
        _ => Err("accounts.unknownProvider".into()),
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = to.join(entry.file_name());
        if kind.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn find_rollout(dir: &Path, provider_session_id: &str) -> Option<PathBuf> {
    let suffix = format!("-{provider_session_id}.jsonl");
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_rollout(&path, provider_session_id) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(&suffix)) {
            return Some(path);
        }
    }
    None
}

/// Where a conversation's transcript lives under one account directory.
fn find_conversation(provider: &str, dir: &Path, provider_session_id: &str) -> Option<PathBuf> {
    match provider {
        "claude-code" => crate::providers::claude::find_transcript_in(&dir.join("projects"), provider_session_id),
        "codex" => find_rollout(&dir.join("sessions"), provider_session_id),
        _ => None,
    }
}

/// Copy a conversation's transcript from one account directory to another, at
/// the same relative path, and return where it landed. The source stays.
pub fn carry_conversation(
    provider: &str,
    from: &Path,
    to: &Path,
    provider_session_id: &str,
) -> Result<PathBuf, String> {
    if !matches!(provider, "claude-code" | "codex") {
        return Err("accounts.unknownProvider".into());
    }
    let source = find_conversation(provider, from, provider_session_id)
        .ok_or_else(|| "accounts.transcriptMissing".to_string())?;
    let relative = source.strip_prefix(from).map_err(|e| e.to_string())?;
    let target = to.join(relative);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&source, &target).map_err(|e| e.to_string())?;
    // Claude Code keeps subagent transcripts in a directory named for the
    // session beside the file; a resumed conversation reads them too.
    let subagents = source.with_extension("");
    if provider == "claude-code" && subagents.is_dir() {
        copy_dir(&subagents, &target.with_extension("")).map_err(|e| e.to_string())?;
    }
    Ok(target)
}

/// Return what a refusing destination appended to the conversation. Its copy
/// is the source plus new lines, so only the new bytes are appended: the
/// source is never rewritten under the tailer that follows it.
fn carry_back(provider: &str, carried: &Path, source: &Path) -> std::io::Result<()> {
    use std::io::Write;
    let have = std::fs::metadata(source)?.len() as usize;
    let theirs = std::fs::read(carried)?;
    if theirs.len() > have {
        std::fs::OpenOptions::new().append(true).open(source)?.write_all(&theirs[have..])?;
    }
    let subagents = carried.with_extension("");
    if provider == "claude-code" && subagents.is_dir() {
        copy_dir(&subagents, &source.with_extension(""))?;
    }
    Ok(())
}

/// Put the conversation back on its original account after a refusal. If even
/// that fails the session has no process left, and it ends as failed rather
/// than reading as still running.
fn resume_on_source(session: &LiveSession, options: PtyOptions, code: String) -> String {
    match session.replace_process(options) {
        Ok(()) => code,
        Err(_) => {
            session.end_without_process();
            "accounts.sessionLost".into()
        }
    }
}

fn log_switch(session: &LiveSession, from: &Account, to: &Account, reason: &str) {
    let event = Lifecycle::AccountSwitch {
        from: from.id.clone(),
        to: to.id.clone(),
        reason: reason.to_string(),
    };
    session.log(EventKind::Lifecycle, serde_json::to_vec(&event).unwrap_or_default());
}

fn set_session_account(db: &Database, session_id: &str, account_id: &str) {
    let _ = db.with(|conn| {
        conn.execute(
            "UPDATE sessions SET account_id = ?2 WHERE id = ?1",
            rusqlite::params![session_id, account_id],
        )?;
        Ok(())
    });
}

/// Move `session` from `from` to `to`, keeping the conversation.
///
/// `options_for` builds the provider command for an account (resume included);
/// injected so the sequence can be tested with stand-in processes. On any
/// failure the conversation is resumed on `from` and the error code returned.
pub fn relay_with(
    db: &Database,
    session: &LiveSession,
    from: &Account,
    to: &Account,
    provider_session_id: &str,
    reason: Reason,
    options_for: &dyn Fn(&Account) -> Result<PtyOptions, String>,
) -> Result<(), String> {
    if from.id == to.id {
        return Err("rotation.sameAccount".into());
    }
    if from.provider == "codex" && !CODEX_RESUME_VERIFIED {
        return Err("accounts.codexResumeUnverified".into());
    }
    let to_options = options_for(to)?;
    let from_options = options_for(from)?;
    let from_dir = PathBuf::from(&from.config_dir);
    let to_dir = PathBuf::from(&to.config_dir);
    // Nothing is killed before there is a conversation to carry (spec §7.2):
    // a session that has not written its transcript yet stays as it is.
    let source = find_conversation(&from.provider, &from_dir, provider_session_id)
        .ok_or_else(|| "accounts.transcriptMissing".to_string())?;

    log_switch(session, from, to, reason.id());
    let mut carried = None;
    let moved = session.replace_process_with(to_options, || {
        carried = Some(carry_conversation(&from.provider, &from_dir, &to_dir, provider_session_id)?);
        Ok(())
    });
    if let Err(code) = moved {
        log_switch(session, to, from, "refused");
        return Err(resume_on_source(session, from_options, code));
    }

    let deadline = Instant::now() + REFUSAL_WINDOW;
    while Instant::now() < deadline {
        if session.exited().is_some() {
            // Whatever the destination appended belongs to the conversation.
            if let Some(carried) = &carried {
                let _ = carry_back(&from.provider, carried, &source);
            }
            log_switch(session, to, from, "refused");
            return Err(resume_on_source(session, from_options, "accounts.destinationRefused".into()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    set_session_account(db, &session.id, &to.id);
    crate::accounts::stamp_used(db, &to.id);
    if let (Some(tail), Some(path)) = (session.tail(), carried) {
        tail.retarget(crate::session::transcript::Retarget {
            path,
            account_id: Some(to.id.clone()),
        });
    }
    Ok(())
}

/// Move a live session to another account and, when its turn was cut short,
/// tell the agent to go on. The real-process entry point over `relay_with`.
pub fn relay_session(
    state: &crate::AppState,
    session_id: &str,
    to: &Account,
    reason: Reason,
) -> Result<(), String> {
    let session = state.sessions.get(session_id).map_err(|e| e.to_string())?;
    let (from_id, provider_session_id, project_id): (Option<String>, Option<String>, String) = state
        .db
        .with(|conn| {
            conn.query_row(
                "SELECT account_id, provider_session_id, project_id FROM sessions WHERE id = ?1",
                [session_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
        })
        .map_err(|e| e.to_string())?;
    let from = crate::accounts::get(&state.db, from_id.as_deref().ok_or("accounts.noAccount")?)?
        .ok_or_else(|| "accounts.notFound".to_string())?;
    let provider_session_id = provider_session_id.ok_or("accounts.transcriptMissing")?;
    let options_for = |account: &Account| {
        crate::session::commands::relaunch_options(state, session_id, account, &provider_session_id)
    };
    if let Err(code) = relay_with(&state.db, &session, &from, to, &provider_session_id, reason, &options_for) {
        crate::activity::record(
            &state.db,
            "account.relayFailed",
            crate::activity::Severity::Attention,
            &to.provider,
            Some(
                serde_json::json!({
                    "fromAccountId": from.id,
                    "toAccountId": to.id,
                    "reason": reason.id(),
                    "sessionId": session_id,
                    "code": code,
                })
                .to_string(),
            ),
            Some(&project_id),
            Some(session_id),
            None,
        );
        return Err(code);
    }

    crate::activity::record(
        &state.db,
        "account.relayed",
        crate::activity::Severity::Info,
        &to.provider,
        Some(
            serde_json::json!({
                "fromAccountId": from.id,
                "toAccountId": to.id,
                "reason": reason.id(),
                "sessionId": session_id,
            })
            .to_string(),
        ),
        Some(&project_id),
        Some(session_id),
        None,
    );

    if reason.interrupts() {
        let session = std::sync::Arc::clone(&session);
        std::thread::spawn(move || {
            // Wait for the resumed CLI to finish drawing before typing into it.
            let deadline = Instant::now() + Duration::from_secs(60);
            std::thread::sleep(Duration::from_secs(2));
            while Instant::now() < deadline
                && session.state() != crate::session::event::SessionState::Idle
            {
                std::thread::sleep(Duration::from_millis(250));
            }
            if crate::session::typing::type_text(&session, CONTINUE_INSTRUCTION).is_ok() {
                let _ = crate::session::typing::submit(&session);
            }
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use crate::pty::PtyOptions;
    use crate::session::manager::{new_session_id, SessionManager};
    use std::time::{Duration, Instant};

    fn account(id: &str, provider: &str, dir: &Path) -> Account {
        Account {
            id: id.into(),
            provider: provider.into(),
            label: id.into(),
            config_dir: dir.to_string_lossy().to_string(),
            adopted: false,
            email: Some(format!("{id}@example.test")),
            account_uuid: Some(format!("uuid-{id}")),
            org_id: None,
            org_name: None,
            plan: Some("pro".into()),
            signed_in: true,
            checked_at: Some(1),
            identity_attempted_at: Some(1),
            subscription_since: 0,
            active: false,
            paused: false,
            position: 0,
            created_at: 1,
            last_used_at: None,
            credential_origin: "own".into(),
        }
    }

    fn shell(command: &str) -> PtyOptions {
        let (program, args) = if cfg!(windows) {
            ("cmd".to_string(), vec!["/c".to_string(), command.to_string()])
        } else {
            ("sh".to_string(), vec!["-c".to_string(), command.to_string()])
        };
        PtyOptions {
            program,
            args,
            cwd: std::env::temp_dir(),
            cols: 80,
            rows: 24,
            env: vec![],
            env_remove: vec![],
        }
    }

    fn long_running(word: &str) -> PtyOptions {
        shell(&if cfg!(windows) {
            format!("echo {word}& ping -n 30 127.0.0.1 >NUL")
        } else {
            format!("echo {word}; sleep 30")
        })
    }

    fn write_transcript(root: &Path, id: &str, lines: &str) -> PathBuf {
        let dir = root.join("projects").join("C--scratch-repo");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{id}.jsonl"));
        std::fs::write(&path, lines).unwrap();
        path
    }

    fn db_with_session(id: &str, account_id: &str) -> Database {
        let db = Database::open_in_memory().unwrap();
        db.with(|conn| {
            conn.execute(
                "INSERT INTO projects (id, name, path, created_at, last_opened_at)
                 VALUES ('p1', 'demo', 'C:/demo', 0, 0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO sessions (id, project_id, provider, cwd, state, log_dir, created_at, account_id)
                 VALUES (?1, 'p1', 'claude-code', 'C:/demo', 'idle', 'C:/logs', 0, ?2)",
                rusqlite::params![id, account_id],
            )?;
            Ok(())
        })
        .unwrap();
        db
    }

    fn session_account(db: &Database, id: &str) -> Option<String> {
        db.with(|conn| {
            conn.query_row("SELECT account_id FROM sessions WHERE id = ?1", [id], |row| row.get(0))
        })
        .unwrap()
    }

    #[test]
    fn claude_continue_args_have_resume_without_fork() {
        let args = continue_args("claude-code", "abc").unwrap();
        assert_eq!(args, vec!["--resume".to_string(), "abc".to_string()]);
        assert_eq!(
            continue_args("codex", "abc").unwrap(),
            vec!["resume".to_string(), "abc".to_string()]
        );
    }

    #[test]
    fn carry_conversation_copies_claude_transcript_and_subagents() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let source = write_transcript(from.path(), "sid", "{\"a\":1}\n");
        let subagents = source.with_extension("");
        std::fs::create_dir_all(&subagents).unwrap();
        std::fs::write(subagents.join("agent-1.jsonl"), "{}\n").unwrap();

        let copied = carry_conversation("claude-code", from.path(), to.path(), "sid").unwrap();

        assert_eq!(copied, to.path().join("projects").join("C--scratch-repo").join("sid.jsonl"));
        assert_eq!(std::fs::read_to_string(&copied).unwrap(), "{\"a\":1}\n");
        assert!(copied.with_extension("").join("agent-1.jsonl").is_file());
        assert!(source.is_file(), "the source copy stays where it was");
    }

    #[test]
    fn carry_conversation_finds_codex_rollout_by_id() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        let id = "0199a1b2-c3d4-7e5f-8a9b-0c1d2e3f4a5b";
        let day = from.path().join("sessions").join("2026").join("10").join("18");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join(format!("rollout-2026-10-18T10-00-00-{id}.jsonl")), "{}\n").unwrap();

        let copied = carry_conversation("codex", from.path(), to.path(), id).unwrap();

        assert_eq!(
            copied,
            to.path()
                .join("sessions")
                .join("2026")
                .join("10")
                .join("18")
                .join(format!("rollout-2026-10-18T10-00-00-{id}.jsonl"))
        );
        assert!(copied.is_file());
    }

    fn wait_log(dir: &Path, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let reader = crate::session::log::SessionLogReader::open(dir).unwrap();
            if String::from_utf8_lossy(&reader.replay_pty(usize::MAX).unwrap()).contains(needle) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        false
    }

    #[test]
    fn relay_switches_account_row_and_logs() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let (a, b) = (account("a", "claude-code", a_dir.path()), account("b", "claude-code", b_dir.path()));
        let id = new_session_id();
        write_transcript(a_dir.path(), &id, "{}\n");
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        assert!(wait_log(logs.path(), "on-a"));

        let options = |acct: &Account| Ok(long_running(&format!("on-{}", acct.id)));
        relay_with(&db, &session, &a, &b, &id, Reason::Manual, &options).expect("relay");

        assert!(wait_log(logs.path(), "on-b"), "the destination process runs in the same session");
        assert_eq!(session_account(&db, &id).as_deref(), Some("b"));
        assert!(b_dir.path().join("projects").join("C--scratch-repo").join(format!("{id}.jsonl")).is_file());
        let reader = crate::session::log::SessionLogReader::open(logs.path()).unwrap();
        let switched = reader
            .read_from(0)
            .unwrap()
            .iter()
            .any(|e| String::from_utf8_lossy(&e.payload).contains("accountSwitch"));
        assert!(switched, "the switch is part of the session's own record");
        let _ = session.kill();
    }

    #[test]
    fn relay_falls_back_when_destination_refuses() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let (a, b) = (account("a", "claude-code", a_dir.path()), account("b", "claude-code", b_dir.path()));
        let id = new_session_id();
        write_transcript(a_dir.path(), &id, "{}\n");
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        assert!(wait_log(logs.path(), "on-a"));

        // The destination exits at once, as a signed-out CLI would.
        let options = |acct: &Account| {
            Ok(if acct.id == "b" { shell("exit 1") } else { long_running("back-on-a") })
        };
        let outcome = relay_with(&db, &session, &a, &b, &id, Reason::Manual, &options);

        assert_eq!(outcome, Err("accounts.destinationRefused".to_string()));
        assert!(wait_log(logs.path(), "back-on-a"), "the conversation resumes on the original account");
        assert_eq!(session_account(&db, &id).as_deref(), Some("a"));
        let _ = session.kill();
    }


    /// Terminal output with escape sequences and whitespace removed, lowercased.
    fn squash(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '' {
                // CSI: ESC [ ... final byte in @..~; OSC: ESC ] ... BEL.
                match chars.next() {
                    Some('[') => {
                        for n in chars.by_ref() {
                            if ('@'..='~').contains(&n) {
                                break;
                            }
                        }
                    }
                    Some(']') => {
                        for n in chars.by_ref() {
                            if n == '' {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            } else if !c.is_whitespace() {
                out.extend(c.to_lowercase());
            }
        }
        out
    }

    /// The whole point of M26, against the real CLI: an interactive Claude Code
    /// session on account A hears a word, the session moves to account B in
    /// place, and the agent on B still knows the word.
    ///
    /// Needs two signed-in account directories, given as
    /// `JARVIS_TEST_CLAUDE_A` and `JARVIS_TEST_CLAUDE_B`. Runs in a scratch git
    /// repository under the temp dir. Answers the CLI's own trust prompt for
    /// that scratch folder — never written by the product.
    ///
    /// `cargo test real_claude_relay -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_claude_relay_continues_conversation() {
        let (Ok(a_dir), Ok(b_dir)) =
            (std::env::var("JARVIS_TEST_CLAUDE_A"), std::env::var("JARVIS_TEST_CLAUDE_B"))
        else {
            panic!("set JARVIS_TEST_CLAUDE_A and JARVIS_TEST_CLAUDE_B");
        };
        let (a, b) = (account("a", "claude-code", Path::new(&a_dir)), account("b", "claude-code", Path::new(&b_dir)));
        let repo = tempfile::tempdir().unwrap();
        let status = std::process::Command::new("git").arg("init").arg("-q").current_dir(repo.path()).status().unwrap();
        assert!(status.success());
        let logs = tempfile::tempdir().unwrap();
        let id = new_session_id();
        let db = db_with_session(&id, "a");

        let options = |acct: &Account, args: Vec<String>| {
            let env = crate::accounts::provider_env(acct);
            PtyOptions {
                program: "claude".into(),
                args,
                cwd: repo.path().to_path_buf(),
                cols: 120,
                rows: 30,
                env: env.set,
                env_remove: env.remove,
            }
        };
        let manager = SessionManager::default();
        let session = manager
            .start(id.clone(), logs.path().to_path_buf(), options(&a, vec!["--session-id".into(), id.clone()]))
            .unwrap();

        let transcript = |dir: &str| crate::providers::claude::find_transcript_in(&Path::new(dir).join("projects"), &id);
        let screen = || {
            let reader = crate::session::log::SessionLogReader::open(logs.path()).unwrap();
            String::from_utf8_lossy(&reader.replay_pty(usize::MAX).unwrap()).to_string()
        };
        // Wait for the prompt, answering the trust question for the scratch folder.
        let ready = |label: &str, start: usize| {
            let deadline = Instant::now() + Duration::from_secs(90);
            let mut answered = 0;
            let mut answered_at = start;
            let mut previous = 0;
            let mut stable = 0;
            while Instant::now() < deadline {
                let text = screen();
                // Only what was drawn since the last answer: earlier screens
                // stay in the log after they are gone from the terminal. The
                // TUI moves the cursor instead of printing spaces, so compare
                // with escapes and whitespace removed.
                let fresh = squash(&text[answered_at.min(text.len())..]);
                let asking = fresh.contains("pressenter")
                    || fresh.contains("entertoconfirm")
                    || fresh.contains("textstyle")
                    || (fresh.contains("trust") && fresh.contains("yes"));
                stable = if text.len() == previous { stable + 1 } else { 0 };
                previous = text.len();
                if asking && answered < 8 && stable >= 2 {
                    // The CLI's own first-run and trust screens, answered with
                    // their default as a person pressing Enter would.
                    // Claude Code focuses "No, exit" on its trust dialog; the
                    // confirm option sits below it.
                    if fresh.contains("itrustthisfolder") {
                        session.write(b"\x1b[B").unwrap();
                        std::thread::sleep(Duration::from_millis(500));
                    }
                    session.write(b"\r").unwrap();
                    answered += 1;
                    answered_at = text.len();
                    stable = 0;
                } else if !asking
                    && stable >= 6
                    && session.state() == crate::session::event::SessionState::Idle
                {
                    return;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            panic!("{label}: the CLI never became ready:\n{}", screen().chars().rev().take(3000).collect::<String>().chars().rev().collect::<String>());
        };
        let ask = |text: &str| {
            crate::session::typing::type_text(&session, text).unwrap();
            crate::session::typing::submit(&session).unwrap();
        };
        let wait_reply = |dir: &str, needle: &str| {
            let deadline = Instant::now() + Duration::from_secs(180);
            while Instant::now() < deadline {
                if let Some(path) = transcript(dir) {
                    let text = std::fs::read_to_string(&path).unwrap_or_default();
                    if text.lines().any(|l| l.contains("\"type\":\"assistant\"") && l.contains(needle)) {
                        return;
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            panic!("no assistant reply containing {needle} in {dir}:\n{}", screen().chars().rev().take(3000).collect::<String>().chars().rev().collect::<String>());
        };

        ready("account A", 0);
        ask("Remember the secret word OSPREY-42. Reply with exactly: NOTED");
        wait_reply(&a_dir, "NOTED");
        std::thread::sleep(Duration::from_secs(3));

        let resume = |acct: &Account| Ok(options(acct, vec!["--resume".into(), id.clone()]));
        let before_relay = screen().len();
        relay_with(&db, &session, &a, &b, &id, Reason::Manual, &resume).expect("relay to B");
        assert_eq!(session_account(&db, &id).as_deref(), Some("b"));

        ready("account B", before_relay);
        ask("What was the secret word? Reply with only the word.");
        wait_reply(&b_dir, "OSPREY-42");
        let b_text = std::fs::read_to_string(transcript(&b_dir).unwrap()).unwrap();
        println!(
            "B transcript: {} lines; contains NOTED from A: {}",
            b_text.lines().count(),
            b_text.contains("NOTED")
        );
        let _ = session.kill();
    }

    #[test]
    fn relay_without_a_transcript_kills_nothing() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let (a, b) = (account("a", "claude-code", a_dir.path()), account("b", "claude-code", b_dir.path()));
        let id = new_session_id();
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        assert!(wait_log(logs.path(), "on-a"));

        let options = |acct: &Account| Ok(long_running(&format!("on-{}", acct.id)));
        let outcome = relay_with(&db, &session, &a, &b, &id, Reason::Manual, &options);

        assert_eq!(outcome, Err("accounts.transcriptMissing".to_string()));
        assert!(session.exited().is_none(), "a brand-new session keeps running where it is");
        assert_eq!(session_account(&db, &id).as_deref(), Some("a"));
        let reader = crate::session::log::SessionLogReader::open(logs.path()).unwrap();
        let touched = reader
            .read_from(0)
            .unwrap()
            .iter()
            .any(|e| String::from_utf8_lossy(&e.payload).contains("accountSwitch"));
        assert!(!touched, "its process was never replaced");
        let _ = session.kill();
    }

    #[test]
    fn relay_to_the_same_account_is_refused() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let a = account("a", "claude-code", a_dir.path());
        let id = new_session_id();
        write_transcript(a_dir.path(), &id, "{}\n");
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        let options = |_: &Account| Ok(long_running("never"));
        let outcome = relay_with(&db, &session, &a, &a, &id, Reason::Manual, &options);
        assert_eq!(outcome, Err("rotation.sameAccount".to_string()));
        assert!(session.exited().is_none());
        let _ = session.kill();
    }

    #[test]
    fn a_refusal_appends_what_the_destination_wrote() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let (a, b) = (account("a", "claude-code", a_dir.path()), account("b", "claude-code", b_dir.path()));
        let id = new_session_id();
        let source = write_transcript(a_dir.path(), &id, "{\"a\":1}\n");
        let carried = b_dir.path().join("projects").join("C--scratch-repo").join(format!("{id}.jsonl"));
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        assert!(wait_log(logs.path(), "on-a"));

        // The destination writes one line, then exits like a refusing CLI.
        let append = if cfg!(windows) {
            format!("echo next>> {} & exit 1", carried.display())
        } else {
            format!("echo next >> '{}'; exit 1", carried.display())
        };
        let options = |acct: &Account| Ok(if acct.id == "b" { shell(&append) } else { long_running("back-on-a") });
        let outcome = relay_with(&db, &session, &a, &b, &id, Reason::Manual, &options);

        assert_eq!(outcome, Err("accounts.destinationRefused".to_string()));
        assert!(std::fs::read_to_string(&carried).unwrap().contains("next"), "the destination wrote");
        let ours = std::fs::read(&source).unwrap();
        assert!(ours.starts_with(b"{\"a\":1}\n"), "the original lines stay first");
        assert_eq!(ours, std::fs::read(&carried).unwrap(), "and the destination's line follows them");
        let _ = session.kill();
    }

    #[test]
    fn a_failed_fallback_ends_the_session() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let (a, b) = (account("a", "claude-code", a_dir.path()), account("b", "claude-code", b_dir.path()));
        let id = new_session_id();
        write_transcript(a_dir.path(), &id, "{}\n");
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        assert!(wait_log(logs.path(), "on-a"));

        let options = |acct: &Account| {
            Ok(if acct.id == "b" {
                shell("exit 1")
            } else {
                PtyOptions { program: "jarvis-no-such-program-m26".into(), ..shell("") }
            })
        };
        let outcome = relay_with(&db, &session, &a, &b, &id, Reason::Manual, &options);

        assert_eq!(outcome, Err("accounts.sessionLost".to_string()));
        assert_eq!(session.state(), crate::session::event::SessionState::Failed);
    }

    #[test]
    fn codex_relay_refused_until_verified() {
        let logs = tempfile::tempdir().unwrap();
        let a_dir = tempfile::tempdir().unwrap();
        let b_dir = tempfile::tempdir().unwrap();
        let (a, b) = (account("a", "codex", a_dir.path()), account("b", "codex", b_dir.path()));
        let id = new_session_id();
        let day = a_dir.path().join("sessions").join("2026").join("10").join("18");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(day.join(format!("rollout-2026-10-18T10-00-00-{id}.jsonl")), "{}\n").unwrap();
        let db = db_with_session(&id, "a");
        let manager = SessionManager::default();
        let session = manager.start(id.clone(), logs.path().to_path_buf(), long_running("on-a")).unwrap();
        let options = |_: &Account| Ok(long_running("never"));
        let outcome = relay_with(&db, &session, &a, &b, &id, Reason::Manual, &options);
        if CODEX_RESUME_VERIFIED {
            assert!(outcome.is_ok());
        } else {
            assert_eq!(outcome, Err("accounts.codexResumeUnverified".to_string()));
            assert_eq!(session_account(&db, &id).as_deref(), Some("a"));
        }
        let _ = session.kill();
    }
}
