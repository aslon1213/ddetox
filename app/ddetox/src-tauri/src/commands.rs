//! `#[tauri::command]` wrappers — one per [`protocol::Request`].
//!
//! These are intentionally thin: validate nothing here beyond shaping the call
//! (the frontend pre-validates and the daemon validates authoritatively), and
//! never swallow a daemon error — map [`IpcError`] straight to a `String` so the
//! WebView sees rejections as rejections.
//!
//! Note the JS↔Rust arg casing: Tauri maps camelCase JS keys (`untilUnix`) to
//! these snake_case parameters (`until_unix`).

use protocol::config::{Session, WebsiteItem};
use protocol::{Request, Status};

use crate::ipc_client::{self, IpcError};
use crate::scheduler::{self, ScheduleState};
use crate::AppState;

/// Reduce a typed IPC error to the string the frontend renders.
fn surface(e: IpcError) -> String {
    e.to_string()
}

#[tauri::command]
pub async fn get_status(state: tauri::State<'_, AppState>) -> Result<Status, String> {
    ipc_client::request_status(&state.socket_path, Request::GetStatus)
        .await
        .map_err(surface)
}

#[tauri::command]
pub async fn add_domains(
    domains: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    ipc_client::request_ok(&state.socket_path, Request::AddDomains { domains })
        .await
        .map_err(surface)
}

#[tauri::command]
pub async fn remove_domains(
    domains: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    ipc_client::request_ok(&state.socket_path, Request::RemoveDomains { domains })
        .await
        .map_err(surface)
}

#[tauri::command]
pub async fn add_addrs(
    cidrs: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    ipc_client::request_ok(&state.socket_path, Request::AddAddrs { cidrs })
        .await
        .map_err(surface)
}

#[tauri::command]
pub async fn remove_addrs(
    cidrs: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    ipc_client::request_ok(&state.socket_path, Request::RemoveAddrs { cidrs })
        .await
        .map_err(surface)
}

#[tauri::command]
pub async fn start_session(
    until_unix: u64,
    committed: bool,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // The protocol field is signed (matching the daemon); reject an out-of-range
    // timestamp cleanly rather than panicking on the conversion.
    let until_unix = i64::try_from(until_unix)
        .map_err(|_| "session end time is out of range".to_string())?;
    ipc_client::request_ok(
        &state.socket_path,
        Request::StartSession { until_unix, committed },
    )
    .await
    .map_err(surface)
}

#[tauri::command]
pub async fn stop_session(state: tauri::State<'_, AppState>) -> Result<(), String> {
    // May be refused by the daemon during a committed session — that rejection
    // propagates to the UI rather than being faked into a success.
    ipc_client::request_ok(&state.socket_path, Request::StopSession)
        .await
        .map_err(surface)
}

// ---- App-owned config: website library + scheduled sessions ----
//
// These persist locally (the daemon never sees them). Mutations save the whole
// collection; the frontend then calls `reconcile_now` to apply the schedule.

#[tauri::command]
pub async fn get_library(app: tauri::AppHandle) -> Result<Vec<WebsiteItem>, String> {
    scheduler::store(&app)?.load_library()
}

#[tauri::command]
pub async fn save_library(app: tauri::AppHandle, items: Vec<WebsiteItem>) -> Result<(), String> {
    scheduler::store(&app)?.save_library(&items)
}

#[tauri::command]
pub async fn get_sessions(app: tauri::AppHandle) -> Result<Vec<Session>, String> {
    scheduler::store(&app)?.load_sessions()
}

#[tauri::command]
pub async fn save_sessions(app: tauri::AppHandle, sessions: Vec<Session>) -> Result<(), String> {
    scheduler::store(&app)?.save_sessions(&sessions)
}

/// Read-only: which sessions are active right now, and what the scheduler has
/// currently pushed to the daemon. Does not touch the daemon.
#[tauri::command]
pub async fn get_schedule_state(app: tauri::AppHandle) -> Result<ScheduleState, String> {
    let store = scheduler::store(&app)?;
    let library = store.load_library()?;
    let sessions = store.load_sessions()?;
    let managed = store.load_managed()?;
    let now = scheduler::local_moment_now();
    let desired = scheduler::compute_desired(&library, &sessions, &now);
    Ok(ScheduleState {
        active_session_ids: desired.active_session_ids,
        managed_domains: managed.domains,
        managed_cidrs: managed.cidrs,
    })
}

/// Force an immediate reconcile (call after editing the library or sessions).
#[tauri::command]
pub async fn reconcile_now(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<ScheduleState, String> {
    let store = scheduler::store(&app)?;
    scheduler::reconcile(&store, &state.socket_path).await
}
