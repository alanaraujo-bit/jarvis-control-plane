//! J.A.R.V.I.S. desktop core.
//!
//! The Rust side owns everything that must stay local and real: the filesystem,
//! Git, PTY sessions, the session event log, provider adapters and secure
//! credential storage (§3 local-first). The webview owns presentation only.

mod envscan;
mod window;

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
        .invoke_handler(tauri::generate_handler![
            window::window_minimize,
            window::window_toggle_maximize,
            window::window_close,
            window::window_is_maximized,
            window::window_ready,
            envscan::scan_environment,
        ])
        .run(tauri::generate_context!())
        .expect("error while running J.A.R.V.I.S.");
}
