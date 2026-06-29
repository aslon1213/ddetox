//! Unprivileged Tauri backend for the blocker GUI.
//!
//! This process is a **view/controller** over the root daemon — it owns no
//! enforcement and no authoritative state. It bridges WebView `invoke(...)`
//! calls to the daemon over a Unix socket (see [`ipc_client`]) and exposes one
//! command per protocol message (see [`commands`]).

mod commands;
mod config_store;
mod ipc_client;
mod scheduler;
mod tray;

/// Application state held in Tauri's managed state: just where to find the daemon.
pub struct AppState {
    /// Path to the daemon's Unix socket (`$BLOCKERD_SOCKET` or the protocol default).
    pub socket_path: String,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Best-effort: don't panic if a global subscriber is already installed.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ddetox_lib=info".into()),
        )
        .try_init();

    let socket_path = protocol::socket_path();
    tracing::info!(socket = %socket_path, "starting blocker GUI backend");

    // The scheduler loop needs the socket path too; clone before it moves into state.
    let scheduler_socket = socket_path.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(AppState { socket_path })
        .setup(move |app| {
            scheduler::spawn(app.handle().clone(), scheduler_socket);
            tray::create(app.handle())?;
            Ok(())
        })
        // Keep the app alive in the menu bar: closing the window hides it rather
        // than quitting, so the scheduler keeps running. Quit from the tray menu.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_stats,
            commands::add_domains,
            commands::remove_domains,
            commands::add_addrs,
            commands::remove_addrs,
            commands::start_session,
            commands::stop_session,
            commands::get_library,
            commands::save_library,
            commands::get_sessions,
            commands::save_sessions,
            commands::get_schedule_state,
            commands::reconcile_now,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
