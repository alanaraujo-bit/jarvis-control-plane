//! Window chrome commands.
//!
//! The window is undecorated (§82) so the titlebar can carry product identity,
//! session state and the command palette entry point. That means the caption
//! button behaviours have to be provided explicitly.

use tauri::{Manager, Runtime, Window};

#[tauri::command]
pub fn window_minimize<R: Runtime>(window: Window<R>) -> tauri::Result<()> {
    window.minimize()
}

/// Toggle between maximised and restored, mirroring the native caption button.
#[tauri::command]
pub fn window_toggle_maximize<R: Runtime>(window: Window<R>) -> tauri::Result<bool> {
    if window.is_maximized()? {
        window.unmaximize()?;
        Ok(false)
    } else {
        window.maximize()?;
        Ok(true)
    }
}

#[tauri::command]
pub fn window_close<R: Runtime>(window: Window<R>) -> tauri::Result<()> {
    window.close()
}

#[tauri::command]
pub fn window_is_maximized<R: Runtime>(window: Window<R>) -> tauri::Result<bool> {
    window.is_maximized()
}

/// Reveal the window once the UI has painted, so startup never flashes an
/// empty white frame (§11 — perceived performance is part of the finish).
#[tauri::command]
pub fn window_ready<R: Runtime>(window: Window<R>) -> tauri::Result<()> {
    if let Some(main) = window.get_webview_window("main") {
        main.show()?;
        main.set_focus()?;
    }
    Ok(())
}
