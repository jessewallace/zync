mod crypto;
mod daemon;
mod github;
mod local_state;
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
        .plugin(tauri_plugin_oauth::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let tray_menu = setup_tray(app)?;
            app.manage(Arc::new(TrayMenuState { menu: tray_menu }));
            setup_native_menu(app)?;

            // Shared daemon state — managed so Tauri commands can access it
            let state = Arc::new(Mutex::new(daemon::DaemonState::default()));
            app.manage(state.clone());

            let update_store = std::sync::Arc::new(UpdateStore::new());
            app.manage(update_store.clone());

            // Start background daemon
            let state_for_cache = state.clone();
            daemon::start(app.handle().clone(), state);

            // Spawn update check loop: check on launch (after 5s) then every 24h
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                loop {
                    check_for_updates(&app_handle, false).await;
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

            // First run: show window so user can connect GitHub
            if !github::has_stored_token() {
                window.show().unwrap();
                let _ = window.set_focus();
            }

            let config_dir = app.path().app_config_dir().unwrap_or_default();
            {
                let mut s = state_for_cache.lock().unwrap();
                s.config_dir = config_dir.clone();
                s.local_state = crate::local_state::LocalState::load(&config_dir);
            }

            // Restore GitHub client from keychain in the background
            {
                let state_clone = state_for_cache.clone();
                tauri::async_runtime::spawn(async move {
                    match github::GitHubClient::from_keychain().await {
                        Ok(Some(client)) => {
                            state_clone.lock().unwrap().github_client = Some(Arc::new(client));
                            eprintln!("[zync] GitHub client restored from keychain");
                        }
                        Ok(None) => eprintln!("[zync] No GitHub token found"),
                        Err(e) => eprintln!("[zync] GitHub restore failed: {e}"),
                    }
                });
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
            daemon::get_sync_status_cmd,
            daemon::manual_sync_now_cmd,
            connect_github_cmd,
            disconnect_github_cmd,
            get_snapshots_cmd,
            restore_snapshot_cmd,
            set_machine_name_cmd,
            install_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn check_for_updates(app: &tauri::AppHandle, manual: bool) {
    use tauri_plugin_notification::NotificationExt;

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            eprintln!("[updater] init error: {e}");
            if manual {
                let _ = app.notification()
                    .builder()
                    .title("Update check failed")
                    .body("Could not check for updates. Please check your internet connection.")
                    .show();
            }
            return;
        }
    };

    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => {
            if manual {
                let _ = app.notification()
                    .builder()
                    .title("Zync is up to date")
                    .body("You're running the latest version.")
                    .show();
            }
            return;
        }
        Err(e) => {
            eprintln!("[updater] check error: {e}");
            if manual {
                let _ = app.notification()
                    .builder()
                    .title("Update check failed")
                    .body("Could not check for updates. Please check your internet connection.")
                    .show();
            }
            return;
        }
    };

    let version = update.version.clone();
    let notes = update.body.clone().unwrap_or_default();

    // Store for later install and tray re-emit
    let store = app.state::<std::sync::Arc<UpdateStore>>();
    *store.update.lock().await = Some(update);
    *store.version.lock().unwrap() = Some(version.clone());
    *store.notes.lock().unwrap() = Some(notes.clone());

    // For manual checks, show the window so the dialog is visible
    if manual {
        match app.get_webview_window("main") {
            Some(w) => {
                let _ = w.show();
                let _ = w.set_focus();
            }
            None => {
                eprintln!("[updater] main window not found; falling back to notification");
                let _ = app.notification()
                    .builder()
                    .title("Zync update available")
                    .body(format!("Zync {} is ready — open the tray to install", version))
                    .show();
            }
        }
    } else {
        // Background check: notify via OS notification
        let _ = app.notification()
            .builder()
            .title("Zync update available")
            .body(format!("Zync {} is ready — open the tray to install", version))
            .show();
    }

    // Emit to frontend
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

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    pub slot: u8,
    pub version: u32,
    pub pushed_at: String,
    pub machine_name: String,
    pub size_mb: f32,
    pub is_current: bool,
}

#[tauri::command]
async fn connect_github_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<daemon::SyncStatus, String> {
    let client = github::GitHubClient::connect(&app).await?;
    {
        let mut s = state.lock().unwrap();
        s.github_client = Some(Arc::new(client));
    } // lock released before get_sync_status_cmd re-acquires it
    Ok(daemon::get_sync_status_cmd(state))
}

#[tauri::command]
fn disconnect_github_cmd(
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<(), String> {
    github::remove_stored_token()?;
    state.lock().unwrap().github_client = None;
    Ok(())
}

#[tauri::command]
async fn get_snapshots_cmd(
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<Vec<SnapshotInfo>, String> {
    let client = state.lock().unwrap().github_client.clone()
        .ok_or("Not connected to GitHub")?;
    let metadata = client.read_metadata().await?
        .ok_or("No sync data found — push from another machine first")?;
    let current_slot = metadata.metadata.current_slot;
    let infos = metadata.metadata.slots.iter().map(|s| SnapshotInfo {
        slot: s.slot,
        version: s.version,
        pushed_at: s.pushed_at.clone(),
        machine_name: s.machine_name.clone(),
        size_mb: s.size_bytes as f32 / 1_048_576.0,
        is_current: s.slot == current_slot,
    }).collect();
    Ok(infos)
}

#[tauri::command]
async fn restore_snapshot_cmd(
    slot: u8,
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<(), String> {
    if zen_check::is_zen_running() {
        return Err("Close Zen before restoring a snapshot.".into());
    }
    let (client, machine_name, last_known, config_dir) = {
        let s = state.lock().unwrap();
        let c = s.github_client.clone().ok_or("Not connected to GitHub")?;
        (c, s.local_state.machine_name.clone(), s.local_state.last_known_version, s.config_dir.clone())
    };

    sync::github_pull(&client, slot).await?;

    match sync::github_push(&client, &machine_name, last_known).await {
        Ok(Some((new_version, _))) => {
            let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());
            let _ = ntfy::publish_version(&topic, new_version).await;
            {
                let mut s = state.lock().unwrap();
                s.local_state.last_known_version = new_version;
                s.last_synced = Some(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                );
                s.last_synced_from = None;
                let _ = s.local_state.save(&config_dir);
            }
            let _ = app.emit("sync-updated", ());
            Ok(())
        }
        Ok(None) => Err("Another machine pushed while restoring — try again".into()),
        Err(e) => Err(e),
    }
}

#[tauri::command]
fn set_machine_name_cmd(
    name: String,
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<(), String> {
    let mut s = state.lock().unwrap();
    s.local_state.machine_name = name;
    let config_dir = s.config_dir.clone();
    s.local_state.save(&config_dir)
}

fn setup_native_menu(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem, MenuItemKind};

    let check_item = MenuItem::with_id(
        app,
        "check_updates_native",
        "Check for Updates\u{2026}",
        true,
        None::<&str>,
    )?;

    let menu = Menu::default(app.handle())?;

    #[cfg(target_os = "macos")]
    if let Some(MenuItemKind::Submenu(app_submenu)) = menu.items()?.into_iter().next() {
        // Insert after "About Zync" (position 0)
        app_submenu.insert(&check_item, 1)?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        use tauri::menu::Submenu;
        let help = Submenu::with_items(
            app,
            "Help",
            true,
            &[&check_item as &dyn tauri::menu::IsMenuItem<tauri::Wry>],
        )?;
        menu.append(&help)?;
    }

    menu.set_as_app_menu()?;

    app.on_menu_event(|app, event| {
        if event.id.as_ref() == "check_updates_native" {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                check_for_updates(&app, true).await;
            });
        }
    });

    Ok(())
}

fn setup_tray(app: &mut tauri::App) -> Result<Menu<tauri::Wry>, Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "Open Zync", true, None::<&str>)?;
    let sync_now = MenuItem::with_id(app, "sync_now", "Sync now", true, None::<&str>)?;
    let check_updates = MenuItem::with_id(app, "check_updates", "Check for updates", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sync_now, &check_updates, &sep, &quit])?;

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
            "check_updates" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    check_for_updates(&app, true).await;
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
