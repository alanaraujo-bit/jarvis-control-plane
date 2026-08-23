//! J.A.R.V.I.S. desktop core.
//!
//! The Rust side owns everything that must stay local and real: the filesystem,
//! Git, PTY sessions, the session event log, provider adapters and secure
//! credential storage (§3 local-first). The webview owns presentation only.

mod activity;
mod analytics;
mod autopilot;
mod brain;
mod db;
mod envscan;
mod files;
mod git;
mod guardrail;
mod mission;
mod onboarding;
mod project;
mod providers;
mod pty;
mod review;
mod search;
mod session;
mod voice;
mod window;
mod worktrees;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::Manager;

use db::Database;
use session::SessionManager;

/// Argument that runs this executable as the guardrail hook instead of the
/// application (§35). Re-exported so `main` can check it before Tauri starts.
pub use guardrail::guard::HOOK_FLAG as GUARDRAIL_HOOK_FLAG;

/// Run as a provider pre-tool hook. Returns the process exit code.
pub fn run_guardrail_hook(snapshot_path: &str) -> i32 {
    guardrail::guard::run(snapshot_path)
}

/// Shared application state, resolved once during setup.
pub struct AppState {
    /// Shared so background workers — the transcript tailer, the session pump —
    /// can record what they observe without routing everything through a
    /// command handler.
    pub db: Arc<Database>,
    /// Every live session in the application.
    pub sessions: SessionManager,
    /// Missions being driven by an agent right now (§32).
    pub autopilots: autopilot::driver::Autopilots,
    /// The one voice recording that can be in progress at a time (§54).
    pub voice: voice::VoiceState,
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

            let db = Arc::new(db);

            // Everything said in a session recorded before Global Search
            // existed is on disk and not in the index (D25). This walks those
            // logs once, off the startup path — see `search::backfill` for why
            // it is deliberately not a migration.
            search::backfill::spawn(
                Arc::clone(&db),
                Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );

            app.manage(AppState {
                db,
                data_dir,
                sessions: SessionManager::default(),
                autopilots: autopilot::driver::Autopilots::default(),
                voice: voice::VoiceState::default(),
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
            onboarding::onboarding_status,
            onboarding::onboarding_mark_seen,
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
            session::commands::session_save_attachment,
            session::commands::session_read_attachment,
            session::commands::session_list,
            session::commands::session_conversation,
            session::commands::mission_sessions,
            files::list_files,
            files::read_file,
            files::write_file,
            review::review_report,
            review::review_file_diff,
            review::actions::review_git_action,
            review::actions::review_commit,
            worktrees::worktree_report,
            worktrees::worktree_create,
            worktrees::worktree_remove,
            brain::brain_report,
            brain::brain_add_knowledge,
            brain::brain_update_knowledge,
            brain::brain_archive_knowledge,
            brain::brain_add_note,
            brain::brain_update_note,
            brain::brain_set_note_pinned,
            brain::brain_delete_note,
            brain::brain_promote_note,
            brain::brain_preview_brief,
            providers::list_providers,
            activity::list_activity,
            analytics::analytics_report,
            mission::store::list_missions,
            mission::store::mission_summaries,
            mission::store::mission_detail,
            mission::store::create_mission,
            mission::store::set_mission_status,
            mission::store::verify_mission_now,
            mission::store::confirm_criterion,
            mission::store::withdraw_mission_criterion,
            mission::store::set_mission_task_done,
            mission::store::set_mission_autonomy,
            mission::store::autonomy_chain,
            mission::store::set_global_autonomy,
            mission::store::set_project_autonomy,
            guardrail::commands::guardrail_policies,
            guardrail::commands::set_guardrail_policy,
            guardrail::commands::guardrail_events,
            guardrail::commands::guardrail_pending,
            guardrail::commands::decide_guardrail,
            guardrail::commands::guardrail_classify,
            autopilot::commands::autopilot_status,
            autopilot::commands::autopilot_start,
            autopilot::commands::autopilot_stop,
            search::global_search,
            voice::voice_model_status,
            voice::voice_download_model,
            voice::voice_start_recording,
            voice::voice_cancel_recording,
            voice::voice_stop_recording,
        ])
        .run(tauri::generate_context!())
        .expect("error while running J.A.R.V.I.S.");
}
