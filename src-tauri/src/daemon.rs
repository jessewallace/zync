use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::Emitter;

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
    pub sync_count: u32,
    /// Cached passphrase — loaded once from the OS keychain so daemon loops never
    /// hit the keychain (which can trigger macOS security prompts).
    pub passphrase: Option<String>,
    /// File ID we most recently published to ntfy — skipped by the ntfy poller to
    /// prevent a machine from pulling its own push.
    pub last_published_file_id: Option<String>,
    /// Set after the first ntfy poll attempt with the current passphrase completes
    /// (success or network error). The initial push is gated on this flag so that a
    /// newly paired machine always checks for peer data before pushing its own profile.
    pub ntfy_polled_since_pair: bool,
    /// Set when a pull succeeds this session. Suppresses the initial push so we don't
    /// overwrite a profile we just received from a peer on startup.
    pub pulled_this_session: bool,
    /// Count of successful auto-pushes sent to peers this session.
    pub push_count: u32,
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
            passphrase: None,
            last_published_file_id: None,
            ntfy_polled_since_pair: false,
            pulled_this_session: false,
            push_count: 0,
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────────────────────

#[derive(serde::Serialize, Clone)]
pub struct SyncStatus {
    pub sync_count: u32,
    pub push_count: u32,
    pub last_synced: Option<u64>,
}

#[tauri::command]
pub fn get_sync_status_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> SyncStatus {
    let s = state.lock().unwrap();
    SyncStatus { sync_count: s.sync_count, push_count: s.push_count, last_synced: s.last_synced }
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
    let passphrase = state.lock().unwrap().passphrase.clone()
        .ok_or("No passphrase set — configure pairing in Settings first")?;
    trigger_push(&app, &state, &passphrase, false).await
}

// ── Push helper ───────────────────────────────────────────────────────────────

/// Upload the current profile and publish the file ID to ntfy.
/// Updates `last_synced` and `refresh_at` on success.
/// Returns immediately (Ok) if another push is already in flight.
/// `is_refresh` — true for 55-min keepalive re-uploads (skips count/event update).
pub async fn trigger_push(
    app: &tauri::AppHandle,
    state: &Arc<Mutex<DaemonState>>,
    passphrase: &str,
    is_refresh: bool,
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

        let status = {
            let mut s = state.lock().unwrap();
            s.last_synced = Some(now);
            s.refresh_at = Some(
                std::time::Instant::now() + std::time::Duration::from_secs(55 * 60),
            );
            s.last_published_file_id = Some(file_id);
            if !is_refresh {
                s.push_count += 1;
            }
            SyncStatus { sync_count: s.sync_count, push_count: s.push_count, last_synced: s.last_synced }
        };

        if !is_refresh {
            let _ = app.emit("sync-updated", status);
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

/// Spawn a one-shot ntfy poll immediately. Used by `save_passphrase_and_cache_cmd`
/// so that a newly paired machine checks for peer data before its initial push.
pub fn trigger_ntfy_poll_now(app: tauri::AppHandle, state: Arc<Mutex<DaemonState>>) {
    tauri::async_runtime::spawn(async move {
        ntfy_poll_tick(&app, &state).await;
    });
}

// ── Tick helpers ──────────────────────────────────────────────────────────────

/// Detect the Zen→closed edge. If auto-push is enabled, triggers a push and
/// discards any queued pull (local session wins). If auto-push is disabled but
/// auto-pull is enabled, drains the queued pull instead.
async fn zen_watcher_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match state.lock().unwrap().passphrase.clone() {
        Some(p) => p,
        None => return,
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
        // Always consume the pending slot — if a peer pushed while Zen was open,
        // their data is newer than our local session and takes priority.
        let pending = state.lock().unwrap().pending_file_id.take();

        if let Some(file_id) = pending {
            // Peer data is newer — drain the pull and skip pushing our local session.
            if auto_pull_enabled {
                match sync::auto_pull(&file_id, &passphrase).await {
                    Ok(_) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let status = {
                            let mut s = state.lock().unwrap();
                            s.last_synced = Some(now);
                            s.sync_count += 1;
                            s.pulled_this_session = true;
                            SyncStatus { sync_count: s.sync_count, push_count: s.push_count, last_synced: s.last_synced }
                        };
                        let _ = app.emit("sync-updated", status);
                        show_notification(app, "Profile updated from another machine");
                    }
                    Err(e) => show_notification(app, &format!("Auto-pull failed: {e}")),
                }
            }
            // Do not push — a peer already has the authoritative profile.
        } else if auto_push_enabled {
            // No pending pull — safe to push our local session.
            if let Err(e) = trigger_push(app, state, &passphrase, false).await {
                show_notification(app, &format!("Auto-push failed: {e}"));
            }
        }
        return;
    }

    // Initial push: if Zen has been closed since we started watching (not the
    // close edge above) and we haven't pushed yet this session, push now.
    // Gated on ntfy_polled_since_pair so a newly paired machine always checks
    // for peer data before pushing — this replaces the old startup_ticks guard,
    // which was ineffective when the passphrase was set mid-session (startup_ticks
    // was already large and the initial push fired before the 60-s ntfy cycle ran).
    // If auto-pull is disabled there is nothing to wait for; push immediately.
    if auto_push_enabled && !zen_running && !was_running {
        let needs_initial_push = {
            let s = state.lock().unwrap();
            let polled_or_no_pull = s.ntfy_polled_since_pair || !s.auto_pull_enabled;
            s.refresh_at.is_none() && !s.pulled_this_session && polled_or_no_pull
        };
        if needs_initial_push {
            eprintln!("[zync] initial push: starting");
            match trigger_push(app, state, &passphrase, false).await {
                Ok(()) => eprintln!("[zync] initial push: ok"),
                Err(e) => eprintln!("[zync] initial push: failed: {e}"),
            }
        }
    }
}

/// Poll ntfy for new file IDs. Pull immediately if Zen is closed; queue if open.
async fn ntfy_poll_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let (passphrase, since) = {
        let s = state.lock().unwrap();
        let p = match s.passphrase.clone() { Some(p) => p, None => return };
        if !s.auto_pull_enabled { return; }
        // Prefer the last message ID (ntfy advances from that point).
        // Fall back to last_poll_time so messages published between polls are not skipped.
        let since = s.last_ntfy_id.clone()
            .unwrap_or_else(|| s.last_poll_time.to_string());
        (p, since)
    };

    // Capture the start time BEFORE the HTTP call. If a message is published
    // during the network round-trip it will have a timestamp >= poll_start,
    // so the next iteration (using poll_start as `since`) will catch it.
    let poll_start = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let topic = pairing::derive_ntfy_topic(&passphrase);
    eprintln!("[zync] ntfy poll: since={since} topic={}...", &topic[..8]);
    let messages = match ntfy::poll_since(&topic, &since).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ntfy poll error: {e}");
            {
                let mut s = state.lock().unwrap();
                // Advance last_poll_time even on error so recovery doesn't re-fetch ancient messages.
                s.last_poll_time = poll_start;
                // Allow the initial push to proceed even when ntfy is unreachable.
                s.ntfy_polled_since_pair = true;
            }
            return;
        }
    };

    eprintln!("[zync] ntfy poll: {} message(s) found", messages.len());
    {
        let mut s = state.lock().unwrap();
        s.last_poll_time = poll_start;
        s.ntfy_polled_since_pair = true;
    }

    // Use the message with the highest timestamp in case ntfy ever returns
    // messages out of arrival order (e.g., two machines push simultaneously).
    let Some(latest) = messages.iter().max_by_key(|m| m.time) else { return };

    state.lock().unwrap().last_ntfy_id = Some(latest.id.clone());

    let file_id = latest.message.trim().to_string();

    // Skip if this is a file ID we published ourselves — prevents a machine
    // from pulling its own push and inflating the sync count.
    let is_own_push = state.lock().unwrap()
        .last_published_file_id.as_deref() == Some(file_id.as_str());
    if is_own_push {
        eprintln!("[zync] ntfy poll: skipping own file_id={file_id}");
        return;
    }

    let zen_running = zen_check::is_zen_running();
    eprintln!("[zync] ntfy poll: file_id={file_id} zen_running={zen_running}");

    if zen_running {
        state.lock().unwrap().pending_file_id = Some(file_id);
        show_notification(app, "New profile available — will pull when Zen closes");
    } else {
        eprintln!("[zync] auto-pull starting: file_id={file_id}");
        match sync::auto_pull(&file_id, &passphrase).await {
            Ok(written) => {
                eprintln!("[zync] auto-pull ok: wrote {:?}", written);
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let status = {
                    let mut s = state.lock().unwrap();
                    s.last_synced = Some(now);
                    s.sync_count += 1;
                    s.pulled_this_session = true;
                    SyncStatus { sync_count: s.sync_count, push_count: s.push_count, last_synced: s.last_synced }
                };
                let _ = app.emit("sync-updated", status);
                show_notification(app, "Profile updated from another machine");
            }
            Err(e) => {
                eprintln!("[zync] auto-pull failed: {e}");
                show_notification(app, &format!("Auto-pull failed: {e}"));
            }
        }
    }
}

/// Re-upload the profile every 55 min to keep the Litterbox link alive.
/// If Zen is open, defers the refresh by 5 min.
async fn refresh_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let (passphrase, auto_push_enabled) = {
        let s = state.lock().unwrap();
        let p = match s.passphrase.clone() { Some(p) => p, None => return };
        (p, s.auto_push_enabled)
    };
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

    if let Err(e) = trigger_push(app, state, &passphrase, true).await {
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

    #[test]
    fn passphrase_defaults_to_none() {
        let state = DaemonState::default();
        assert!(state.passphrase.is_none());
    }
}
