//! Preview (§46) — seeing what the agent just built.
//!
//! The loop this closes is **ask → modify → run → see → inspect → fix**. Every
//! step but *see* already existed here: an agent edits files (§41/§42), runs a
//! dev server in a real terminal (§21), and the diff is in Review (§43). The
//! missing step was looking at the result, which meant leaving the application
//! for a browser and coming back — and losing, on the way, the one thing this
//! product knows that a browser does not: **which session started this server**.
//!
//! ## What makes this ours rather than a browser in a tab
//!
//! `detect` reads the dev server's URL out of the session's own PTY output
//! (§23). Nothing to configure and no port to guess: when an agent runs
//! `npm run dev`, the URL it prints is already in the log this product keeps.
//! A general-purpose browser cannot know that, and asking the user to type a
//! port they just watched scroll past is the friction §46 exists to remove.
//!
//! ## Where the page is actually rendered
//!
//! In a **separate Tauri window**, not an iframe inside the app.
//!
//! An iframe was the obvious first choice and does not work: this app's CSP is
//! `default-src 'self'` (see `tauri.conf.json`), so a `localhost:5173` frame is
//! blocked outright. Widening the CSP to allow it would mean the dev server's
//! page shares an origin-adjacent context with the surface that can invoke
//! every Tauri command in this application — a real escalation, in exchange for
//! a layout convenience. A separate window is its own webview with its own
//! (empty) capability set, which is the correct boundary and also gives the
//! thing people actually want: the preview beside the editor rather than
//! squeezed inside it.

pub mod detect;

use serde::Serialize;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::session::SessionLogReader;
use crate::AppState;

pub type Result<T> = std::result::Result<T, String>;

/// How much of a session's output is scanned for a server URL.
///
/// The banner appears when the server starts and is not reprinted after, so a
/// long-running session's URL can be a long way back — but a full scan of an
/// unbounded log on every poll is not something to do on a UI thread. 512 KB
/// covers a dev server plus a great deal of subsequent output.
const SCAN_LIMIT: usize = 512 * 1024;

/// The label of the preview window.
///
/// One window, reused. Opening a second preview for the same project would
/// leave two pages claiming to be "the app", and closing the right one becomes
/// a puzzle.
const WINDOW_LABEL: &str = "preview";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detected {
    /// The best guess, or `None` when this session has not started a server.
    pub url: Option<String>,
    /// Every distinct local URL seen, newest first, for the case where a
    /// session serves more than one thing.
    pub all: Vec<String>,
}

/// What this session appears to be serving, read from its own output.
#[tauri::command]
pub fn preview_detect(state: State<'_, AppState>, session_id: String) -> Result<Detected> {
    let log_dir: String = state
        .db
        .with(|conn| {
            conn.query_row("SELECT log_dir FROM sessions WHERE id = ?1", [&session_id], |row| {
                row.get(0)
            })
        })
        .map_err(|e| e.to_string())?;

    let reader = SessionLogReader::open(&log_dir).map_err(|e| e.to_string())?;
    let bytes = reader.replay_pty(SCAN_LIMIT).map_err(|e| e.to_string())?;
    // Lossy on purpose: a PTY carries arbitrary bytes and a partial UTF-8
    // sequence at the window boundary is normal, not an error worth failing on.
    let text = String::from_utf8_lossy(&bytes);

    Ok(Detected { url: detect::best(&text), all: detect::all(&text) })
}

/// Open — or navigate — the preview window.
///
/// The URL is re-checked here rather than trusted from the webview. The
/// renderer got it from `preview_detect` in the ordinary case, but a command
/// that will open a window must not rely on its caller having been careful:
/// this is the enforcement point, and `detect::is_local` is the rule.
#[tauri::command]
pub async fn preview_open(app: tauri::AppHandle, url: String) -> Result<()> {
    let target = detect::normalise(&url);
    // Parsed, not pattern-matched. `Url` rejects the malformed inputs a
    // hand-rolled check would let through, and gives an authority to test
    // rather than a substring to hope about.
    let parsed = url::Url::parse(&target).map_err(|_| "preview.invalidUrl".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("preview.invalidUrl".into());
    }
    if !detect::is_loopback_host(parsed.host_str().unwrap_or_default()) {
        return Err("preview.notLocal".into());
    }

    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        // Reuse: navigating the existing window is what makes the preview feel
        // like one thing that follows the work, rather than a pile of windows.
        window
            .navigate(parsed.clone())
            .map_err(|e| e.to_string())?;
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    WebviewWindowBuilder::new(&app, WINDOW_LABEL, WebviewUrl::External(parsed))
        .title("Preview — J.A.R.V.I.S.")
        .inner_size(960.0, 720.0)
        // No decorations of ours and no drag region: this window shows someone
        // else's page, and dressing it in our chrome would blur whose content
        // the person is looking at.
        .build()
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Reload whatever the preview is showing.
///
/// The reason this exists rather than leaving it to the page: after an agent
/// edits a file, a dev server with hot reload updates on its own — and one
/// without it does not. This is the "did that actually change anything?"
/// button, and it is the difference between trusting the preview and
/// second-guessing it.
#[tauri::command]
pub fn preview_reload(app: tauri::AppHandle) -> Result<()> {
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
    window.eval("location.reload()").map_err(|e| e.to_string())
}

#[tauri::command]
pub fn preview_close(app: tauri::AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Whether a preview window is currently open.
#[tauri::command]
pub fn preview_is_open(app: tauri::AppHandle) -> bool {
    app.get_webview_window(WINDOW_LABEL).is_some()
}
