mod crypto;
mod daemon;
mod ntfy;
mod pairing;
mod profile;
mod sync;
mod transport;
mod zen_check;

use std::sync::{Arc, Mutex};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

/// Holds the base tray menu so `rebuild_tray_with_update` can look up existing items.
struct TrayMenuState {
    menu: Menu<tauri::Wry>,
}
#[allow(unused_imports)]
use tauri_plugin_updater::UpdaterExt;

struct UpdateStore {
    update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    version: std::sync::Mutex<Option<String>>,
    notes: std::sync::Mutex<Option<String>>,
}

impl UpdateStore {
    fn new() -> Self {
        Self {
            update: tokio::sync::Mutex::new(None),
            version: std::sync::Mutex::new(None),
            notes: std::sync::Mutex::new(None),
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let tray_menu = setup_tray(app)?;
            app.manage(Arc::new(TrayMenuState { menu: tray_menu }));

            // Shared daemon state — managed so Tauri commands can access it
            let state = Arc::new(Mutex::new(daemon::DaemonState::default()));
            app.manage(state.clone());

            let update_store = std::sync::Arc::new(UpdateStore::new());
            app.manage(update_store.clone());

            // Start background daemon
            daemon::start(app.handle().clone(), state);

            // Spawn update check loop: check on launch (after 5s) then every 24h
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                loop {
                    check_for_updates(&app_handle).await;
                    tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
                }
            });

            // Close button hides to tray instead of quitting
            let window = app.get_webview_window("main")
                .ok_or("main window not found")?;
            let win = window.clone();
            window.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = win.hide();
                }
            });

            // First run: show window so user can enter passphrase
            if !pairing::get_pairing_status_cmd() {
                window.show().unwrap();
                let _ = window.set_focus();
            }

            // Enable launch-on-login whenever the app runs
            {
                use tauri_plugin_autostart::ManagerExt;
                let _ = app.autolaunch().enable();
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            zen_check::is_zen_running,
            profile::detect_profile_path,
            profile::collect_sync_files,
            sync::push_profile,
            sync::pull_profile,
            pairing::save_passphrase_cmd,
            pairing::get_pairing_status_cmd,
            pairing::clear_passphrase_cmd,
            pairing::get_passphrase_cmd,
            daemon::get_last_synced_cmd,
            daemon::get_sync_status_cmd,
            daemon::set_auto_push_cmd,
            daemon::set_auto_pull_cmd,
            daemon::manual_sync_now_cmd,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn check_for_updates(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => { eprintln!("[updater] init error: {e}"); return; }
    };

    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => return,
        Err(e) => { eprintln!("[updater] check error: {e}"); return; }
    };

    let version = update.version.clone();
    let notes = update.body.clone().unwrap_or_default();

    // Store for later install and tray re-emit
    let store = app.state::<std::sync::Arc<UpdateStore>>();
    *store.update.lock().await = Some(update);
    *store.version.lock().unwrap() = Some(version.clone());
    *store.notes.lock().unwrap() = Some(notes.clone());

    // OS notification
    let _ = app.notification()
        .builder()
        .title("Zync update available")
        .body(format!("Zync {} is ready — open the tray to install", version))
        .show();

    // Emit to frontend (catches it if window is open)
    let _ = app.emit("update-available", serde_json::json!({
        "version": version,
        "notes": notes,
    }));

    // Rebuild tray menu with install item
    rebuild_tray_with_update(app, &version);
}

fn rebuild_tray_with_update(app: &tauri::AppHandle, version: &str) {
    let Ok(install) = MenuItem::with_id(
        app,
        "install_update",
        format!("Install update ({})", version),
        true,
        None::<&str>,
    ) else { return };
    let Ok(sep1) = PredefinedMenuItem::separator(app) else { return };

    // Reuse existing menu items by looking them up from the stored base menu.
    let base_menu = app.state::<Arc<TrayMenuState>>();
    let Some(open) = base_menu.menu.get("open") else { return };
    let Some(sync_now) = base_menu.menu.get("sync_now") else { return };
    let Ok(sep2) = PredefinedMenuItem::separator(app) else { return };
    let Some(quit) = base_menu.menu.get("quit") else { return };

    // MenuItemKind implements IsMenuItem, so it can be used directly in with_items.
    let Ok(menu) = Menu::with_items(app, &[
        &install as &dyn tauri::menu::IsMenuItem<tauri::Wry>,
        &sep1,
        &open as &dyn tauri::menu::IsMenuItem<tauri::Wry>,
        &sync_now as &dyn tauri::menu::IsMenuItem<tauri::Wry>,
        &sep2,
        &quit as &dyn tauri::menu::IsMenuItem<tauri::Wry>,
    ]) else { return };

    if let Some(tray) = app.tray_by_id("zync-tray") {
        if let Err(e) = tray.set_menu(Some(menu)) {
            eprintln!("[updater] failed to set tray menu: {e}");
        }
    }
}

fn setup_tray(app: &mut tauri::App) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "Open Zync", true, None::<&str>)?;
    let sync_now = MenuItem::with_id(app, "sync_now", "Sync now", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync_now, &sep, &quit])?;

    let tray_icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray-icon@2x.png"))?;

    TrayIconBuilder::with_id("zync-tray")
        .icon(tray_icon)
        .icon_as_template(true)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "sync_now" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let state = app.state::<Arc<Mutex<daemon::DaemonState>>>();
                    if let Err(e) = daemon::manual_sync_now_cmd(app.clone(), state).await {
                        eprintln!("Manual sync failed: {e}");
                    }
                });
            }
            "install_update" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                    let store = app.state::<std::sync::Arc<UpdateStore>>();
                    let version = store.version.lock().unwrap().clone().unwrap_or_default();
                    let notes = store.notes.lock().unwrap().clone().unwrap_or_default();
                    let _ = app.emit("update-available", serde_json::json!({
                        "version": version,
                        "notes": notes,
                    }));
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(menu)
}

#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    store: tauri::State<'_, std::sync::Arc<UpdateStore>>,
) -> Result<(), String> {
    let update = store.update.lock().await.take()
        .ok_or_else(|| "No pending update".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
