use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{ntfy, pairing, sync, zen_check};

// ── State ─────────────────────────────────────────────────────────────────────

pub struct DaemonState {
    pub auto_push_enabled: bool,
    pub auto_pull_enabled: bool,
    /// Last ntfy message ID we processed — used as `since` on next poll.
    pub last_ntfy_id: Option<String>,
    /// Unix timestamp of the end of the last ntfy poll. Used as `since` when
    /// no message ID exists yet, so messages published between polls are not skipped.
    pub last_poll_time: u64,
    /// File ID from ntfy that arrived while Zen was open — waiting for Zen to close.
    pub pending_file_id: Option<String>,
    /// Unix timestamp of last successful push or pull.
    pub last_synced: Option<u64>,
    /// When to re-upload the profile to keep the Litterbox link alive.
    pub refresh_at: Option<std::time::Instant>,
    /// Previous Zen running state (used for edge detection).
    pub zen_was_running: bool,
    pub is_pushing: Arc<AtomicBool>,
    /// Count of successful auto-pulls received from peers this session. Resets on restart.
    #[allow(dead_code)]
    pub sync_count: u32,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            auto_push_enabled: true,
            auto_pull_enabled: true,
            last_ntfy_id: None,
            last_poll_time: 0,
            pending_file_id: None,
            last_synced: None,
            refresh_at: None,
            zen_was_running: false,
            is_pushing: Arc::new(AtomicBool::new(false)),
            sync_count: 0,
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_last_synced_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> Option<u64> {
    state.lock().unwrap().last_synced
}

#[derive(serde::Serialize)]
pub struct SyncStatus {
    pub sync_count: u32,
    pub last_synced: Option<u64>,
}

#[tauri::command]
pub fn get_sync_status_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> SyncStatus {
    let s = state.lock().unwrap();
    SyncStatus { sync_count: s.sync_count, last_synced: s.last_synced }
}

#[tauri::command]
pub fn set_auto_push_cmd(enabled: bool, state: tauri::State<Arc<Mutex<DaemonState>>>) {
    state.lock().unwrap().auto_push_enabled = enabled;
}

#[tauri::command]
pub fn set_auto_pull_cmd(enabled: bool, state: tauri::State<Arc<Mutex<DaemonState>>>) {
    state.lock().unwrap().auto_pull_enabled = enabled;
}

#[tauri::command]
pub async fn manual_sync_now_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<DaemonState>>>,
) -> Result<(), String> {
    if zen_check::is_zen_running() {
        return Err("Zen is running — close it before syncing".into());
    }
    let passphrase = pairing::load_passphrase()?
        .ok_or("No passphrase set — configure pairing in Settings first")?;
    trigger_push(&app, &state, &passphrase).await
}

// ── Push helper ───────────────────────────────────────────────────────────────

/// Upload the current profile and publish the file ID to ntfy.
/// Updates `last_synced` and `refresh_at` on success.
/// Returns immediately (Ok) if another push is already in flight.
pub async fn trigger_push(
    app: &tauri::AppHandle,
    state: &Arc<Mutex<DaemonState>>,
    passphrase: &str,
) -> Result<(), String> {
    let is_pushing = state.lock().unwrap().is_pushing.clone();
    if is_pushing.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return Ok(()); // Another push is in flight; skip silently
    }

    let result = async {
        let file_id = sync::auto_push(passphrase).await?;
        let topic = pairing::derive_ntfy_topic(passphrase);
        ntfy::publish(&topic, &file_id).await?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        {
            let mut s = state.lock().unwrap();
            s.last_synced = Some(now);
            s.refresh_at = Some(
                std::time::Instant::now() + std::time::Duration::from_secs(55 * 60),
            );
        }

        show_notification(app, "Profile synced");
        Ok(())
    }.await;

    is_pushing.store(false, Ordering::SeqCst);
    result
}

// ── Background loops ──────────────────────────────────────────────────────────

/// Spawn the three background loops:
///   - Zen process watcher (every 5 s) — triggers push on close
///   - ntfy poller (every 60 s) — pulls new profiles from other machines
///   - Refresh timer (checked every 5 min) — re-uploads to keep link alive
pub fn start(app: tauri::AppHandle, state: Arc<Mutex<DaemonState>>) {
    // Initialize last_poll_time to 1 hour ago so the first ntfy poll catches
    // any push that happened while this machine had Zync closed.
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        state.lock().unwrap().last_poll_time = now.saturating_sub(3600);
    }

    // Zen process watcher (every 5 s)
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(5));
            loop {
                interval.tick().await;
                zen_watcher_tick(&app, &state).await;
            }
        });
    }

    // ntfy poller (every 60 s)
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                ntfy_poll_tick(&app, &state).await;
            }
        });
    }

    // Refresh timer (checked every 5 min)
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval =
                tokio::time::interval(std::time::Duration::from_secs(5 * 60));
            loop {
                interval.tick().await;
                refresh_tick(&app, &state).await;
            }
        });
    }
}

// ── Tick helpers ──────────────────────────────────────────────────────────────

/// Detect the Zen→closed edge and trigger an auto-push.
/// Any pending pull is discarded because the just-closed local session wins.
async fn zen_watcher_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match pairing::load_passphrase() {
        Ok(Some(p)) => p,
        _ => return,
    };

    let zen_running = zen_check::is_zen_running();

    let (was_running, auto_push_enabled, auto_pull_enabled) = {
        let mut s = state.lock().unwrap();
        let was = s.zen_was_running;
        s.zen_was_running = zen_running;
        (was, s.auto_push_enabled, s.auto_pull_enabled)
    };

    // Zen just closed
    if was_running && !zen_running {
        if auto_push_enabled {
            // Push wins — discard any queued pull
            state.lock().unwrap().pending_file_id = None;
            if let Err(e) = trigger_push(app, state, &passphrase).await {
                show_notification(app, &format!("Auto-push failed: {e}"));
            }
        } else if auto_pull_enabled {
            // Auto-push disabled, auto-pull enabled — drain any queued pull
            let pending = state.lock().unwrap().pending_file_id.take();
            if let Some(file_id) = pending {
                match sync::auto_pull(&file_id, &passphrase).await {
                    Ok(_) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        state.lock().unwrap().last_synced = Some(now);
                        show_notification(app, "Profile updated from another machine");
                    }
                    Err(e) => show_notification(app, &format!("Auto-pull failed: {e}")),
                }
            }
        } else {
            // Both disabled — discard the queue
            state.lock().unwrap().pending_file_id = None;
        }
    }
}

/// Poll ntfy for new file IDs. Pull immediately if Zen is closed; queue if open.
async fn ntfy_poll_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match pairing::load_passphrase() {
        Ok(Some(p)) => p,
        _ => return,
    };

    let since = {
        let s = state.lock().unwrap();
        if !s.auto_pull_enabled {
            return;
        }
        // Prefer the last message ID (ntfy advances from that point).
        // Fall back to last_poll_time so messages published between polls are not skipped.
        s.last_ntfy_id.clone()
            .unwrap_or_else(|| s.last_poll_time.to_string())
    };

    // Capture the start time BEFORE the HTTP call. If a message is published
    // during the network round-trip it will have a timestamp >= poll_start,
    // so the next iteration (using poll_start as `since`) will catch it.
    let poll_start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let topic = pairing::derive_ntfy_topic(&passphrase);
    let messages = match ntfy::poll_since(&topic, &since).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ntfy poll error: {e}");
            // Advance last_poll_time even on error so recovery doesn't re-fetch ancient messages.
            state.lock().unwrap().last_poll_time = poll_start;
            return;
        }
    };

    state.lock().unwrap().last_poll_time = poll_start;

    // Use the message with the highest timestamp in case ntfy ever returns
    // messages out of arrival order (e.g., two machines push simultaneously).
    let Some(latest) = messages.iter().max_by_key(|m| m.time) else { return };

    state.lock().unwrap().last_ntfy_id = Some(latest.id.clone());

    let file_id = latest.message.trim().to_string();
    let zen_running = zen_check::is_zen_running();

    if zen_running {
        state.lock().unwrap().pending_file_id = Some(file_id);
        show_notification(app, "New profile available — will pull when Zen closes");
    } else {
        match sync::auto_pull(&file_id, &passphrase).await {
            Ok(_) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                state.lock().unwrap().last_synced = Some(now);
                show_notification(app, "Profile updated from another machine");
            }
            Err(e) => show_notification(app, &format!("Auto-pull failed: {e}")),
        }
    }
}

/// Re-upload the profile every 55 min to keep the Litterbox link alive.
/// If Zen is open, defers the refresh by 5 min.
async fn refresh_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match pairing::load_passphrase() {
        Ok(Some(p)) => p,
        _ => return,
    };

    let auto_push_enabled = state.lock().unwrap().auto_push_enabled;
    if !auto_push_enabled {
        return;
    }

    let should_refresh = {
        let s = state.lock().unwrap();
        s.refresh_at
            .map(|d| std::time::Instant::now() >= d)
            .unwrap_or(false)
    };

    if !should_refresh {
        return;
    }

    if zen_check::is_zen_running() {
        let mut s = state.lock().unwrap();
        if s.refresh_at.is_some() {
            s.refresh_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(5 * 60));
        }
        return;
    }

    if let Err(e) = trigger_push(app, state, &passphrase).await {
        show_notification(app, &format!("Refresh failed: {e}"));
    }
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
    fn sync_count_defaults_to_zero() {
        let state = DaemonState::default();
        assert_eq!(state.sync_count, 0);
    }
}
