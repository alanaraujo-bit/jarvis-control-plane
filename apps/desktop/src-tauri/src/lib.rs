//! J.A.R.V.I.S. desktop core.
//!
//! The Rust side owns everything that must stay local and real: the filesystem,
//! Git, PTY sessions, the session event log, provider adapters and secure
//! credential storage (§3 local-first). The webview owns presentation only.

mod db;
mod envscan;
mod git;
mod project;
mod session;
mod window;

use std::path::PathBuf;

use tauri::Manager;

use db::Database;

/// Shared application state, resolved once during setup.
pub struct AppState {
    pub db: Database,
    /// Root for session logs and other bulk local data.
    pub data_dir: PathBuf,
}

impl AppState {
    /// Directory holding one session's append-only log.
    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.data_dir.join("sessions").join(session_id)
    }
}

/// Build and run the desktop application.
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("JARVIS_LOG")
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let db = Database::open(data_dir.join("jarvis.db"))?;
            tracing::info!(path = ?data_dir, "local data directory ready");

            app.manage(AppState { db, data_dir });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window::window_minimize,
            window::window_toggle_maximize,
            window::window_close,
            window::window_is_maximized,
            window::window_ready,
            envscan::scan_environment,
            project::list_projects,
            project::open_project,
            project::archive_project,
        ])
        .run(tauri::generate_context!())
        .expect("error while running J.A.R.V.I.S.");
}
