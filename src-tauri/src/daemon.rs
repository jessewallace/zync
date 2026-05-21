#[allow(unused_imports)]
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

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
    _app: tauri::AppHandle,
    _state: tauri::State<'_, Arc<Mutex<DaemonState>>>,
) -> Result<(), String> {
    Err("Not yet implemented".into())
}

// ── Push helper ───────────────────────────────────────────────────────────────

// TODO(task-10): will be fully rewritten
pub async fn trigger_push(
    _app: &tauri::AppHandle,
    _state: &Arc<Mutex<DaemonState>>,
    _passphrase: &str,
    _is_refresh: bool,
) -> Result<(), String> {
    Err("Not yet implemented".into())
}

// ── Background loops ──────────────────────────────────────────────────────────

/// Spawn the three background loops:
///   - Zen process watcher (every 5 s) — triggers push on close
///   - ntfy poller (every 60 s) — pulls new profiles from other machines
///   - Refresh timer (checked every 5 min) — re-uploads to keep link alive
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

// TODO(task-10): will be fully rewritten
async fn zen_watcher_tick(_app: &tauri::AppHandle, _state: &Arc<Mutex<DaemonState>>) {}
async fn ntfy_poll_tick(_app: &tauri::AppHandle, _state: &Arc<Mutex<DaemonState>>) {}
async fn refresh_tick(_app: &tauri::AppHandle, _state: &Arc<Mutex<DaemonState>>) {}

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
