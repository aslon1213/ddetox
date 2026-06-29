//! macOS status-bar (tray) item.
//!
//! Puts a small menu-bar icon with quick actions (show the window, jump to
//! statistics, reconcile the schedule, quit) so the app is reachable without a
//! visible window. Paired with close-to-hide in [`crate::run`], this lets the
//! controller live in the menu bar while the scheduler keeps running.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::{scheduler, AppState};

/// Monochrome template icon for the menu bar — macOS tints it for light/dark.
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray.png");

/// Build the tray icon and its menu. Call once from `setup`.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Blocker", true, None::<&str>)?;
    let stats = MenuItem::with_id(app, "stats", "Statistics…", true, None::<&str>)?;
    let reconcile =
        MenuItem::with_id(app, "reconcile", "Reconcile schedule now", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Blocker", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&show, &stats, &reconcile, &sep, &quit])?;

    let icon = tauri::image::Image::from_bytes(TRAY_ICON)?;

    TrayIconBuilder::with_id("blocker-tray")
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Blocker")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_window(app),
            "stats" => {
                show_window(app);
                let _ = app.emit("navigate", "/stats");
            }
            "reconcile" => reconcile_now(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Left-click the icon to reveal the window (menu opens on right-click).
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// Reveal and focus the main window (it is only ever hidden, never destroyed).
pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Fire a one-off schedule reconcile from the menu (fire-and-forget).
fn reconcile_now(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let socket = app.state::<AppState>().socket_path.clone();
        match scheduler::store(&app) {
            Ok(store) => {
                if let Err(e) = scheduler::reconcile(&store, &socket).await {
                    tracing::warn!(error = %e, "tray reconcile failed");
                }
            }
            Err(e) => tracing::warn!(error = %e, "tray reconcile: store unavailable"),
        }
    });
}
