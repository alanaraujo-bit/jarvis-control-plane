//! J.A.R.V.I.S. desktop core.
//!
//! The Rust side owns everything that must stay local and real: the filesystem,
//! Git, PTY sessions, the session event log, provider adapters and secure
//! credential storage (§3 local-first). The webview owns presentation only.

mod db;
mod envscan;
mod git;
mod mission;
mod project;
mod providers;
mod pty;
mod session;
mod window;

use std::path::PathBuf;

use tauri::Manager;

use db::Database;
use session::SessionManager;

/// Shared application state, resolved once during setup.
pub struct AppState {
    pub db: Database,
    /// Every live session in the application.
    pub sessions: SessionManager,
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
        // Single instance, registered first so it runs before anything else.
        //
        // The local database and the session logs have one owner. A second
        // window writing the same SQLite file and the same append-only logs
        // risks the users history, and data integrity outranks convenience
        // (§91). Launching again focuses the window that already exists.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let db = Database::open(data_dir.join("jarvis.db"))?;
            tracing::info!(path = ?data_dir, "local data directory ready");

            app.manage(AppState {
                db,
                data_dir,
                sessions: SessionManager::default(),
            });
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
            session::commands::session_start,
            session::commands::session_attach,
            session::commands::session_detach,
            session::commands::session_write,
            session::commands::session_resize,
            session::commands::session_close,
            session::commands::session_replay,
            session::commands::session_list,
            session::commands::session_conversation,
            session::commands::mission_sessions,
            providers::list_providers,
            mission::store::list_missions,
            mission::store::mission_summaries,
            mission::store::mission_detail,
            mission::store::create_mission,
            mission::store::set_mission_status,
            mission::store::verify_mission_now,
            mission::store::confirm_criterion,
            mission::store::withdraw_mission_criterion,
            mission::store::set_mission_task_done,
        ])
        .run(tauri::generate_context!())
        .expect("error while running J.A.R.V.I.S.");
}
