//! Typing into a live session, paced like a person.
//!
//! Two callers need this: the autopilot (§32, D16) typing its next instruction,
//! and voice dictation (§54) typing a transcript. Both are the same problem —
//! text that did not come from a keyboard has to *become* keystrokes before an
//! agent CLI's line editor will accept it intact — so both go through the same
//! tested path rather than each growing their own.

use std::time::Duration;

use super::manager::LiveSession;

/// Pause between chunks of text as it is typed in.
///
/// An agent CLI's line editor re-renders on every keystroke; writing a long
/// instruction in one go outruns it and characters are lost. See `type_text`.
const TYPING_PACE: Duration = Duration::from_millis(30);

/// Pause between the last of the text and the key that submits it.
///
/// Without this the editor is still catching up when the carriage return
/// arrives and swallows it, leaving the text in the prompt unsent — a failure
/// that looks entirely correct on screen. See `submit`.
const SUBMIT_PAUSE: Duration = Duration::from_millis(400);

/// Bytes per write. Both halves of this constant were found by driving a real
/// agent and reading the session log, not by reasoning about it: sending a
/// whole line in one write **drops characters** (observed: "so tere is no"),
/// because the line editor re-renders as it goes and a large burst outruns it.
/// Writing in small pieces with a breath between them arrives intact.
const CHUNK: usize = 48;

/// Type text into a session, paced like a keyboard. Does **not** submit it —
/// call `submit` separately once the caller is done, or leave it in the prompt
/// for a person to look at before sending (voice dictation's own reason for
/// wanting the two halves apart).
///
/// Newlines are flattened to spaces: a multi-line string sent keystroke by
/// keystroke would submit itself a fragment at a time the moment a `\n` hit
/// the session's own line editor.
pub fn type_text(session: &LiveSession, text: &str) -> Result<(), ()> {
    let single_line = text.replace('\n', " ").replace('\r', " ");

    for chunk in char_boundary_chunks(&single_line, CHUNK) {
        session.write(chunk.as_bytes()).map_err(|e| {
            tracing::warn!(error = %e, "could not write to the session");
        })?;
        std::thread::sleep(TYPING_PACE);
    }
    Ok(())
}

/// Split `text` into pieces of at most `max_bytes`, never inside a UTF-8
/// character.
///
/// Chunking by raw byte offset can land mid-character for anything outside
/// ASCII — an accented letter's continuation byte (à, ç, ã, ...) split across
/// two writes `TYPING_PACE` apart. A stateful line editor may reassemble it,
/// but a TUI that decodes each read on its own (an Ink-based CLI, for one —
/// this is exactly what broke dictation (§54) into Claude Code's terminal for
/// non-English sentences, while a plain shell echoed the same text back
/// intact) sees an invalid sequence and renders U+FFFD in its place. Walking
/// a byte boundary back to the nearest char boundary costs nothing for plain
/// ASCII and fixes it for everything else.
fn char_boundary_chunks(text: &str, max_bytes: usize) -> Vec<&str> {
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max_bytes).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

/// Submit whatever is currently in the prompt, as its own write after a
/// settle pause — appending this to the last chunk of text leaves the editor
/// still processing when it arrives, and it is swallowed.
pub fn submit(session: &LiveSession) -> Result<(), ()> {
    std::thread::sleep(SUBMIT_PAUSE);
    session.write(b"\r").map_err(|e| {
        tracing::warn!(error = %e, "could not submit to the session");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::PtyOptions;
    use crate::session::log::SessionLogReader;
    use crate::session::manager::SessionManager;
    use std::sync::Arc;
    use std::time::Instant;
    use uuid::Uuid;

    fn spawn_idle_shell(manager: &SessionManager, dir: &std::path::Path) -> Arc<LiveSession> {
        let id = Uuid::new_v4().to_string();
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        manager
            .start(
                id,
                dir.to_path_buf(),
                PtyOptions {
                    program: program.to_string(),
                    args: vec![],
                    cwd: std::env::temp_dir(),
                    cols: 80,
                    rows: 24,
                    env: vec![],
                },
            )
            .expect("start session")
    }

    fn wait_for(dir: &std::path::Path, needle: &str) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
            let reader = SessionLogReader::open(dir).unwrap();
            let replayed = reader.replay_pty(usize::MAX).unwrap();
            if String::from_utf8_lossy(&replayed).contains(needle) {
                return true;
            }
        }
        false
    }

    #[test]
    fn typed_text_reaches_the_session_without_a_trailing_submit() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let session = spawn_idle_shell(&manager, dir.path());

        // A shell echoes what it reads from its own input line; typing without
        // submitting must still show the characters landing, with no newline
        // that would have run it as a command.
        type_text(&session, "echo hello-from-typing").expect("type succeeds");

        assert!(
            wait_for(dir.path(), "echo hello-from-typing"),
            "typed characters must reach the session"
        );

        let _ = session.kill();
    }

    #[test]
    fn submit_sends_the_text_typed_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let session = spawn_idle_shell(&manager, dir.path());

        type_text(&session, "echo submitted-ok").expect("type succeeds");
        submit(&session).expect("submit succeeds");

        assert!(
            wait_for(dir.path(), "submitted-ok"),
            "the command must actually run once submitted"
        );

        let _ = session.kill();
    }

    #[test]
    fn a_multiline_string_is_flattened_before_typing() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let session = spawn_idle_shell(&manager, dir.path());

        // Sent byte-for-byte instead of flattened, the embedded \n would have
        // submitted "echo one" on its own the moment it reached the shell.
        type_text(&session, "echo one\necho two").expect("type succeeds");

        assert!(
            wait_for(dir.path(), "echo one echo two"),
            "the newline must become a space, not a submit"
        );

        let _ = session.kill();
    }

    #[test]
    fn chunking_never_splits_a_multibyte_character() {
        // 47 ASCII bytes then "ção" (a 2-byte, then two more chars) — byte
        // offset 48 (`CHUNK`) lands squarely inside "ç"'s two-byte encoding.
        // The old byte-offset chunker would hand the session an invalid
        // UTF-8 tail; this asserts every returned piece is valid on its own
        // and that rejoining them recovers the original string exactly.
        let text = format!("{}ção mais texto depois para garantir mais de um pedaço", "a".repeat(47));
        let chunks = char_boundary_chunks(&text, CHUNK);

        assert!(chunks.len() > 1, "the string must actually span more than one chunk");
        for chunk in &chunks {
            assert!(std::str::from_utf8(chunk.as_bytes()).is_ok());
        }
        assert_eq!(chunks.concat(), text);
    }

    #[test]
    fn a_multibyte_sentence_arrives_unmangled_through_a_real_pty() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let session = spawn_idle_shell(&manager, dir.path());

        // The exact failure mode reported against Claude Code's own TUI: a
        // plain shell just echoes bytes back, so this alone would not have
        // caught the regression before `char_boundary_chunks` — it is here
        // as a real-PTY companion to the unit test above, not a substitute.
        let text = format!("echo {}ção", "a".repeat(47));
        type_text(&session, &text).expect("type succeeds");

        assert!(
            wait_for(dir.path(), "ação"),
            "the accented character must survive chunked typing intact"
        );

        let _ = session.kill();
    }

    /// The actual bug report: dictation garbled specifically inside Claude
    /// Code's own terminal, while a plain shell echoed the same text back
    /// intact. Not run by default — it spawns the real `claude` CLI, which
    /// is not installed in CI — but this is exactly what was run by hand to
    /// confirm the char-boundary fix against the real regression rather than
    /// just the isolated chunker logic above.
    ///
    /// `cargo test -- --ignored a_multibyte_sentence_survives_claude_codes_own_tui`
    #[test]
    #[ignore]
    fn a_multibyte_sentence_survives_claude_codes_own_tui() {
        let dir = tempfile::tempdir().unwrap();
        let manager = SessionManager::default();
        let id = Uuid::new_v4().to_string();
        let session = manager
            .start(
                id.clone(),
                dir.path().to_path_buf(),
                PtyOptions {
                    program: "claude".to_string(),
                    args: vec!["--session-id".into(), id],
                    cwd: std::env::temp_dir(),
                    cols: 194,
                    rows: 34,
                    env: vec![],
                },
            )
            .expect("start claude session");

        // A workspace Claude Code has not seen before gates on a one-time
        // "trust this folder?" prompt before the chat prompt ever appears;
        // "1" (already the default) plus Enter clears it. Give the TUI time
        // to render that screen first, then time again for the real prompt
        // to replace it — typing too early lands on a menu that ignores it.
        std::thread::sleep(Duration::from_secs(4));
        session.write(b"1\r").expect("accept the trust prompt");
        std::thread::sleep(Duration::from_secs(2));

        type_text(&session, "testando informação e configuração").expect("type succeeds");

        assert!(
            wait_for(dir.path(), "informação"),
            "an accented word must render intact in Claude Code's own TUI, not as U+FFFD"
        );

        let _ = session.kill();
    }
}
