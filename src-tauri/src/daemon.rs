use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

use crate::{ntfy, pairing, sync, zen_check};

// ── State ─────────────────────────────────────────────────────────────────────

pub struct DaemonState {
    /// Connected GitHub client. None until the user connects via OAuth.
    pub github_client: Option<std::sync::Arc<crate::github::GitHubClient>>,
    /// Persisted state (last_known_version, machine_name).
    pub local_state: crate::local_state::LocalState,
    /// Guard against concurrent syncs.
    pub is_syncing: Arc<AtomicBool>,
    /// Unix timestamp of last successful push or pull.
    pub last_synced: Option<u64>,
    /// Machine name of last sync source (None = this machine pushed).
    pub last_synced_from: Option<String>,
    /// Previous Zen running state (used for edge detection).
    pub zen_was_running: bool,
    /// Last ntfy message ID processed.
    pub last_ntfy_id: Option<String>,
    /// Unix timestamp of last ntfy poll start (fallback since value).
    pub last_poll_time: u64,
    /// Version number received from ntfy while Zen was open — drain when Zen closes.
    pub pending_version: Option<u32>,
    /// Tauri app config dir — used to save local_state to disk.
    pub config_dir: std::path::PathBuf,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            github_client: None,
            local_state: crate::local_state::LocalState::default(),
            is_syncing: Arc::new(AtomicBool::new(false)),
            last_synced: None,
            last_synced_from: None,
            zen_was_running: false,
            last_ntfy_id: None,
            last_poll_time: 0,
            pending_version: None,
            config_dir: std::path::PathBuf::new(),
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub connected: bool,
    pub username: Option<String>,
    pub last_synced: Option<u64>,
    pub last_synced_from: Option<String>,
    pub machine_name: String,
}

#[tauri::command]
pub fn get_sync_status_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> SyncStatus {
    let s = state.lock().unwrap();
    SyncStatus {
        connected: s.github_client.is_some(),
        username: s.github_client.as_ref().map(|c| c.username.clone()),
        last_synced: s.last_synced,
        last_synced_from: s.last_synced_from.clone(),
        machine_name: s.local_state.machine_name.clone(),
    }
}

#[tauri::command]
pub async fn manual_sync_now_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<DaemonState>>>,
) -> Result<(), String> {
    if zen_check::is_zen_running() {
        return Err("Zen is running — close it before syncing".into());
    }
    let (client, machine_name, last_known, config_dir, saved_path, mut local_state) = {
        let s = state.lock().unwrap();
        let c = s.github_client.clone()
            .ok_or("Not connected to GitHub — set up sync in the Sync tab first")?;
        (c, s.local_state.machine_name.clone(), s.local_state.last_known_version, s.config_dir.clone(), s.local_state.selected_profile_path.clone(), s.local_state.clone())
    };

    let saved = saved_path.as_deref().map(std::path::Path::new);
    match sync::github_push(&client, &machine_name, last_known, saved, &mut local_state, &config_dir).await {
        Ok(Some((new_version, _))) => {
            let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());
            let _ = ntfy::publish_version(&topic, new_version).await;
            let mut s = state.lock().unwrap();
            s.local_state = local_state;
            s.local_state.last_known_version = new_version;
            s.last_synced = Some(unix_now());
            s.last_synced_from = None;
            let _ = s.local_state.save(&config_dir);
            let _ = app.emit("sync-updated", get_status_payload(state.inner()));
            Ok(())
        }
        Ok(None) => Err("Another machine pushed recently — close Zen and try again".into()),
        Err(e) => Err(e),
    }
}

// ── Background loops ──────────────────────────────────────────────────────────

/// Spawn two background loops:
///   - Zen process watcher (every 5 s) — triggers push/pull on close
///   - ntfy poller (every 60 s) — pulls new profiles from other machines
pub fn start(app: tauri::AppHandle, state: Arc<Mutex<DaemonState>>) {
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(5));
            loop { interval.tick().await; zen_watcher_tick(&app, &state).await; }
        });
    }
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop { interval.tick().await; ntfy_poll_tick(&app, &state).await; }
        });
    }
}

// ── Tick helpers ──────────────────────────────────────────────────────────────

async fn zen_watcher_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let (client, was_running, zen_now_running, config_dir, saved_path, mut local_state) = {
        let mut s = state.lock().unwrap();
        let client = match s.github_client.clone() {
            Some(c) => c,
            None => return,
        };
        let was = s.zen_was_running;
        let now_running = zen_check::is_zen_running();
        s.zen_was_running = now_running;
        (client, was, now_running, s.config_dir.clone(), s.local_state.selected_profile_path.clone(), s.local_state.clone())
    };

    if !was_running || zen_now_running {
        return; // Not a close edge
    }

    // Zen just closed — check if there's a pending pull first
    let pending = state.lock().unwrap().pending_version.take();

    if let Some(pending_ver) = pending {
        handle_pull(&client, pending_ver, app, state, &config_dir, saved_path.as_deref().map(std::path::Path::new), &mut local_state).await;
        return;
    }

    // Guard against concurrent syncs
    let is_syncing = state.lock().unwrap().is_syncing.clone();
    if is_syncing.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }

    // Version check: are we up to date?
    let last_known = state.lock().unwrap().local_state.last_known_version;
    let github_version = match client.read_metadata().await {
        Ok(Some(m)) => m.metadata.version,
        Ok(None) => 0,
        Err(e) => {
            show_notification(app, &format!("Sync check failed: {e}"));
            is_syncing.store(false, Ordering::SeqCst);
            return;
        }
    };

    if github_version > last_known {
        is_syncing.store(false, Ordering::SeqCst);
        handle_pull(&client, github_version, app, state, &config_dir, saved_path.as_deref().map(std::path::Path::new), &mut local_state).await;
        return;
    }

    // Up to date — push
    let machine_name = local_state.machine_name.clone();
    match sync::github_push(&client, &machine_name, last_known, saved_path.as_deref().map(std::path::Path::new), &mut local_state, &config_dir).await {
        Ok(Some((new_version, _metadata))) => {
            let now = unix_now();
            let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());
            let _ = ntfy::publish_version(&topic, new_version).await;
            {
                let mut s = state.lock().unwrap();
                s.local_state = local_state.clone();
                s.last_synced = Some(now);
                s.last_synced_from = None;
                s.local_state.last_known_version = new_version;
                let _ = s.local_state.save(&config_dir);
            }
            let _ = app.emit("sync-updated", get_status_payload(state));
            show_notification(app, "Profile synced.");
        }
        Ok(None) => {
            let current_ver = match client.read_metadata().await {
                Ok(Some(m)) => m.metadata.version,
                _ => last_known + 1,
            };
            handle_pull(&client, current_ver, app, state, &config_dir, saved_path.as_deref().map(std::path::Path::new), &mut local_state).await;
        }
        Err(e) => notify_sync_error(app, "Auto-push", &e),
    }

    is_syncing.store(false, Ordering::SeqCst);
}

async fn handle_pull(
    client: &crate::github::GitHubClient,
    version: u32,
    app: &tauri::AppHandle,
    state: &Arc<Mutex<DaemonState>>,
    config_dir: &std::path::Path,
    saved_profile: Option<&std::path::Path>,
    local_state: &mut crate::local_state::LocalState,
) {
    let (slot, pusher) = match client.read_metadata().await {
        Ok(Some(m)) if m.metadata.version == version => {
            let pusher = m.metadata.slots.first()
                .map(|s| s.machine_name.clone())
                .unwrap_or_else(|| "another machine".to_string());
            (m.metadata.current_slot, pusher)
        }
        Ok(Some(m)) => (m.metadata.current_slot, "another machine".to_string()),
        _ => {
            show_notification(app, "Auto-pull failed: could not read sync metadata");
            return;
        }
    };

    match sync::github_pull(client, slot, saved_profile, local_state, config_dir).await {
        Ok(_) => {
            let now = unix_now();
            {
                let mut s = state.lock().unwrap();
                s.local_state = local_state.clone();
                s.last_synced = Some(now);
                s.last_synced_from = Some(pusher.clone());
                s.local_state.last_known_version = version;
                let _ = s.local_state.save(config_dir);
            }
            let _ = app.emit("sync-updated", get_status_payload(state));
            show_notification(
                app,
                &format!("{pusher} pushed a profile while Zen was open. Their profile has been applied. Your session's changes are saved as a snapshot."),
            );
        }
        Err(e) => notify_sync_error(app, "Auto-pull", &e),
    }
}

async fn ntfy_poll_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let (client, since, config_dir, saved_path, mut local_state) = {
        let s = state.lock().unwrap();
        let client = match s.github_client.clone() { Some(c) => c, None => return };
        let since = s.last_ntfy_id.clone()
            .unwrap_or_else(|| s.last_poll_time.to_string());
        (client, since, s.config_dir.clone(), s.local_state.selected_profile_path.clone(), s.local_state.clone())
    };

    let poll_start = unix_now();
    let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());

    let messages = match ntfy::poll_since(&topic, &since).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[zync] ntfy poll error: {e}");
            state.lock().unwrap().last_poll_time = poll_start;
            return;
        }
    };

    {
        let mut s = state.lock().unwrap();
        s.last_poll_time = poll_start;
    }

    let Some(latest) = messages.iter().max_by_key(|m| m.time) else { return };
    state.lock().unwrap().last_ntfy_id = Some(latest.id.clone());

    let version = match ntfy::parse_version_message(&latest.message) {
        Some(v) => v,
        None => {
            eprintln!("[zync] ntfy: unrecognised message: {}", latest.message);
            return;
        }
    };

    let last_known = state.lock().unwrap().local_state.last_known_version;
    if version <= last_known {
        return;
    }

    let zen_running = zen_check::is_zen_running();
    if zen_running {
        state.lock().unwrap().pending_version = Some(version);
        show_notification(app, "New profile available — will sync when Zen closes");
    } else {
        handle_pull(&client, version, app, state, &config_dir, saved_path.as_deref().map(std::path::Path::new), &mut local_state).await;
    }
}

// ── Helper functions ──────────────────────────────────────────────────────────

/// Shows an OS notification. For profile-resolution errors, uses friendlier wording
/// that tells the user to open Zync and select an installation.
fn notify_sync_error(app: &tauri::AppHandle, prefix: &str, details: &str) {
    let msg = if details.starts_with("MULTIPLE_INSTALLATIONS:") {
        format!("{prefix}: multiple Zen installations found — open Zync to select one.")
    } else if details.contains("no longer exists") {
        format!("{prefix}: saved Zen profile not found — open Zync to select an installation.")
    } else {
        format!("{prefix}: {details}")
    };
    show_notification(app, &msg);
}

fn get_status_payload(state: &Arc<Mutex<DaemonState>>) -> serde_json::Value {
    let s = state.lock().unwrap();
    serde_json::json!({
        "connected": s.github_client.is_some(),
        "lastSynced": s.last_synced,
        "lastSyncedFrom": s.last_synced_from,
        "machineName": s.local_state.machine_name,
    })
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Notification helper ───────────────────────────────────────────────────────

fn show_notification(app: &tauri::AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title("Zync").body(body).show();
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_state_defaults() {
        let state = DaemonState::default();
        assert!(state.github_client.is_none());
        assert_eq!(state.local_state.last_known_version, 0);
        assert!(state.pending_version.is_none());
        assert!(state.last_synced.is_none());
        assert!(!state.zen_was_running);
    }
}
