//! Window chrome commands.
//!
//! The window is undecorated (§82) so the titlebar can carry product identity,
//! session state and the command palette entry point. That means the caption
//! button behaviours have to be provided explicitly.

use tauri::{Runtime, Window};

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
pub fn window_ready<R: Runtime>(
    window: Window<R>,
    state: tauri::State<'_, crate::AppState>,
) -> tauri::Result<()> {
    // `Window` here is already the native window associated with the webview
    // that invoked the command. Trying to look the webview up through it is a
    // category error: `get_webview_window` has no managed webviews on this
    // handle and quietly returns `None`, leaving every first-run window hidden.
    window.show()?;

    // Started by Windows at login, and asked to stay out of the way (§93).
    //
    // Shown first and then minimised rather than left hidden: a hidden window
    // is a process with no way back to it — this product has no tray icon, so
    // the taskbar button *is* the way back, and it only exists once the window
    // has been shown. Focus is deliberately not taken; the person was doing
    // something else when their machine finished booting.
    if crate::settings::started_by_system()
        && crate::settings::get_or(&state.db, crate::settings::START_MINIMIZED_KEY, true)
    {
        window.minimize()?;
        return Ok(());
    }

    window.set_focus()?;
    Ok(())
}
