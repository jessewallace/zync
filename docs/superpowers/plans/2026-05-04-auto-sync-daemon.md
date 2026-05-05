# ZynC Auto-Sync Daemon — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn ZynC into a persistent system-tray daemon that auto-pushes when Zen closes, auto-pulls when another machine pushes (via ntfy.sh pub/sub), and keeps the Litterbox link alive by re-uploading every 55 minutes.

**Architecture:** Machines pair via a shared passphrase entered once in a new Settings screen. The passphrase is stored in the OS keychain and drives both AES-256-GCM encryption (via PBKDF2, same as today) and an ntfy.sh topic name (SHA-256 hash of passphrase). A three-loop background daemon watches Zen process state, polls ntfy, and manages the refresh timer.

**Tech Stack:** Rust / Tauri v2 — add `keyring 3` (OS keychain), `rusqlite 0.31` (WAL checkpoint), `tauri-plugin-notification 2`, `tauri-plugin-autostart 2`; ntfy.sh free public pub/sub (no account).

**Spec:** `docs/superpowers/specs/2026-05-04-auto-sync-daemon-design.md`

---

## File Map

| Action | File | Responsibility |
|---|---|---|
| Modify | `src-tauri/Cargo.toml` | Add keyring, rusqlite, notification/autostart plugins; add tray-icon feature; remove zip/uuid |
| Create | `src-tauri/src/pairing.rs` | Save/load passphrase via OS keychain; derive ntfy topic; expose Tauri commands |
| Create | `src-tauri/src/ntfy.rs` | Publish file ID to ntfy topic; poll topic for new messages |
| Modify | `src-tauri/src/sync.rs` | Add WAL checkpoint helper; add `auto_push` and `auto_pull` functions |
| Create | `src-tauri/src/daemon.rs` | `DaemonState` struct; `trigger_push` helper; three background async loops |
| Modify | `src-tauri/src/lib.rs` | Wire tray, plugins, state, close-to-tray, daemon spawn, new commands |
| Modify | `src-tauri/tauri.conf.json` | Set `visible: false` on window (tray-first startup) |
| Modify | `src-tauri/capabilities/default.json` | Add tray, notification, autostart permissions |
| Modify | `src/index.html` | Add settings screen markup |
| Modify | `src/main.js` | Settings screen logic; first-run detection |
| Modify | `src/style.css` | Settings screen styles |

---

## Task 1: Update Cargo.toml

**Files:**
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Remove unused crates and add new dependencies**

Replace the `[dependencies]` block in `src-tauri/Cargo.toml` with:

```toml
[package]
name = "zync"
version = "0.1.0"
edition = "2021"
description = "Sync Zen Browser profiles between machines"

[lib]
name = "zync_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = ["tray-icon"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Process detection
sysinfo = "0.30"

# Profile discovery
dirs = "5"

# Crypto (AES-256-GCM + PBKDF2)
aes-gcm = "0.10"
generic-array = "0.14"
pbkdf2 = { version = "0.12", features = ["simple"] }
sha2 = "0.10"
rand = "0.8"
base64 = "0.22"

# Transport (Litterbox HTTP upload)
reqwest = { version = "0.12", features = ["multipart"] }
tokio = { version = "1", features = ["full"] }

# Passphrase storage in OS keychain
keyring = { version = "3", features = ["apple-native", "windows-native", "sync-secret-service"] }

# SQLite WAL checkpoint
rusqlite = { version = "0.31", features = ["bundled"] }

# Tauri plugins
tauri-plugin-notification = "2"
tauri-plugin-autostart = "2"

[features]
custom-protocol = ["tauri/custom-protocol"]
```

- [ ] **Step 2: Verify it compiles (no code changes yet)**

```bash
cd src-tauri && cargo check
```

Expected: compiles with warnings about unused imports in existing files, no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/Cargo.toml
git commit -m "chore: update deps — add keyring, rusqlite, tray, notification, autostart plugins; remove zip/uuid"
```

---

## Task 2: pairing.rs — Passphrase keychain + ntfy topic derivation

**Files:**
- Create: `src-tauri/src/pairing.rs`

The passphrase is the PBKDF2 password for encryption AND the source of the ntfy topic name. It is stored in the OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service). The ntfy topic is `SHA-256(passphrase)` as lowercase hex — a 256-bit secret that the ntfy server never sees in plaintext.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/pairing.rs` with just the test module:

```rust
mod tests {
    use super::*;

    #[test]
    fn topic_is_deterministic() {
        let t1 = derive_ntfy_topic("purple-fox-mountain-7");
        let t2 = derive_ntfy_topic("purple-fox-mountain-7");
        assert_eq!(t1, t2);
    }

    #[test]
    fn topic_is_64_hex_chars() {
        let t = derive_ntfy_topic("any passphrase");
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_passphrases_produce_different_topics() {
        let t1 = derive_ntfy_topic("passphrase-a");
        let t2 = derive_ntfy_topic("passphrase-b");
        assert_ne!(t1, t2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd src-tauri && cargo test pairing
```

Expected: compile error — `derive_ntfy_topic` not defined.

- [ ] **Step 3: Implement the full module**

Replace the file contents with:

```rust
use sha2::{Digest, Sha256};

const SERVICE: &str = "zync";
const ACCOUNT: &str = "passphrase";

/// Derive the ntfy.sh topic name from the passphrase.
/// Topic = lowercase hex of SHA-256(passphrase). Never transmitted.
pub fn derive_ntfy_topic(passphrase: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(passphrase.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn save_passphrase(passphrase: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("Keychain error: {e}"))?;
    entry.set_password(passphrase)
        .map_err(|e| format!("Failed to save passphrase: {e}"))
}

pub fn load_passphrase() -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("Keychain error: {e}"))?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Failed to load passphrase: {e}")),
    }
}

pub fn clear_passphrase() -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, ACCOUNT)
        .map_err(|e| format!("Keychain error: {e}"))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to clear passphrase: {e}")),
    }
}

// ── Tauri commands ────────────────────────────────────────────

#[tauri::command]
pub fn save_passphrase_cmd(passphrase: String) -> Result<(), String> {
    save_passphrase(&passphrase)
}

#[tauri::command]
pub fn get_pairing_status_cmd() -> bool {
    load_passphrase().map(|p| p.is_some()).unwrap_or(false)
}

#[tauri::command]
pub fn clear_passphrase_cmd() -> Result<(), String> {
    clear_passphrase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_is_deterministic() {
        let t1 = derive_ntfy_topic("purple-fox-mountain-7");
        let t2 = derive_ntfy_topic("purple-fox-mountain-7");
        assert_eq!(t1, t2);
    }

    #[test]
    fn topic_is_64_hex_chars() {
        let t = derive_ntfy_topic("any passphrase");
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_passphrases_produce_different_topics() {
        let t1 = derive_ntfy_topic("passphrase-a");
        let t2 = derive_ntfy_topic("passphrase-b");
        assert_ne!(t1, t2);
    }
}
```

- [ ] **Step 4: Add `mod pairing;` to lib.rs**

In `src-tauri/src/lib.rs`, add to the top:

```rust
mod pairing;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd src-tauri && cargo test pairing
```

Expected: all 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/pairing.rs src-tauri/src/lib.rs
git commit -m "feat: add pairing.rs — passphrase keychain storage and ntfy topic derivation"
```

---

## Task 3: ntfy.rs — Publish and poll

**Files:**
- Create: `src-tauri/src/ntfy.rs`

On push: `POST https://ntfy.sh/{topic}` with the file ID as the body (plain text).
On poll: `GET https://ntfy.sh/{topic}/json?poll=1&since={last_id}` — returns newline-delimited JSON, one message per line, then closes. We skip `event != "message"` lines (ntfy also emits `"open"` events). Track the last-seen message ID to avoid re-processing.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/ntfy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_event() {
        let line = r#"{"id":"abc123","time":1234567890,"event":"message","topic":"test","message":"FILEID1"}"#;
        let msgs = parse_lines(line);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, "abc123");
        assert_eq!(msgs[0].message, "FILEID1");
    }

    #[test]
    fn skip_open_event() {
        let lines = "{\"id\":\"x\",\"time\":1,\"event\":\"open\",\"topic\":\"t\",\"message\":\"\"}\n\
                     {\"id\":\"y\",\"time\":2,\"event\":\"message\",\"topic\":\"t\",\"message\":\"ABC\"}";
        let msgs = parse_lines(lines);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].message, "ABC");
    }

    #[test]
    fn empty_body_returns_empty_vec() {
        assert_eq!(parse_lines("").len(), 0);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd src-tauri && cargo test ntfy
```

Expected: compile error — `parse_lines` not defined.

- [ ] **Step 3: Implement the module**

```rust
const NTFY_BASE: &str = "https://ntfy.sh";

#[derive(serde::Deserialize, Debug)]
pub struct NtfyMessage {
    pub id: String,
    pub event: String,
    pub message: String,
}

/// Parse newline-delimited JSON from ntfy, returning only `event == "message"` entries.
pub fn parse_lines(body: &str) -> Vec<NtfyMessage> {
    body.lines()
        .filter_map(|line| serde_json::from_str::<NtfyMessage>(line).ok())
        .filter(|m| m.event == "message")
        .collect()
}

/// Publish a file ID to the ntfy topic. Fire-and-forget.
pub async fn publish(topic: &str, file_id: &str) -> Result<(), String> {
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{NTFY_BASE}/{topic}"))
        .header("Content-Type", "text/plain")
        .body(file_id.to_string())
        .send()
        .await
        .map_err(|e| format!("ntfy publish failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("ntfy returned HTTP {}", resp.status()));
    }
    Ok(())
}

/// Poll the ntfy topic for messages newer than `since_id`.
/// Pass `None` on first call; subsequent calls pass the last seen message ID.
pub async fn poll_since(
    topic: &str,
    since_id: Option<&str>,
) -> Result<Vec<NtfyMessage>, String> {
    let url = match since_id {
        Some(id) => format!("{NTFY_BASE}/{topic}/json?poll=1&since={id}"),
        None => format!("{NTFY_BASE}/{topic}/json?poll=1&since=all"),
    };

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("ntfy poll failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("ntfy returned HTTP {}", resp.status()));
    }

    let body = resp.text().await.map_err(|e| format!("ntfy read failed: {e}"))?;
    Ok(parse_lines(&body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_message_event() {
        let line = r#"{"id":"abc123","time":1234567890,"event":"message","topic":"test","message":"FILEID1"}"#;
        let msgs = parse_lines(line);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].id, "abc123");
        assert_eq!(msgs[0].message, "FILEID1");
    }

    #[test]
    fn skip_open_event() {
        let lines = "{\"id\":\"x\",\"time\":1,\"event\":\"open\",\"topic\":\"t\",\"message\":\"\"}\n\
                     {\"id\":\"y\",\"time\":2,\"event\":\"message\",\"topic\":\"t\",\"message\":\"ABC\"}";
        let msgs = parse_lines(lines);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].message, "ABC");
    }

    #[test]
    fn empty_body_returns_empty_vec() {
        assert_eq!(parse_lines("").len(), 0);
    }
}
```

- [ ] **Step 4: Add `mod ntfy;` to lib.rs**

```rust
mod ntfy;
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd src-tauri && cargo test ntfy
```

Expected: all 3 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/ntfy.rs src-tauri/src/lib.rs
git commit -m "feat: add ntfy.rs — publish file ID and poll for incoming sync notifications"
```

---

## Task 4: sync.rs — WAL checkpoint + auto_push + auto_pull

**Files:**
- Modify: `src-tauri/src/sync.rs`

Add three things: (1) a WAL checkpoint helper that runs before reading `places.sqlite`; (2) `auto_push` which takes a passphrase and returns a file ID; (3) `auto_pull` which takes a file ID + passphrase and writes profile files. Both reuse the existing `backup_profile` and `SyncBundle` internals. Also add WAL checkpoint to the existing `push_profile` (resolves the existing TODO).

- [ ] **Step 1: Write the failing WAL checkpoint test**

Add to the test module in `sync.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn checkpoint_nonexistent_path_errors() {
        let result = checkpoint_wal(&PathBuf::from("/tmp/does_not_exist.sqlite"));
        assert!(result.is_err());
    }

    #[test]
    fn checkpoint_valid_db_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.sqlite");
        // Create a minimal SQLite file
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
        drop(conn);
        assert!(checkpoint_wal(&db_path).is_ok());
    }
}
```

Note: add `tempfile = "3"` to `[dev-dependencies]` in `Cargo.toml` for this test.

- [ ] **Step 2: Add tempfile to dev-dependencies**

In `src-tauri/Cargo.toml`, add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 3: Run the test to verify it fails**

```bash
cd src-tauri && cargo test sync
```

Expected: compile error — `checkpoint_wal` not defined.

- [ ] **Step 4: Add WAL checkpoint to sync.rs**

Add at the top of `sync.rs` (with the existing `use` statements):

```rust
use rusqlite::Connection;
use std::path::Path;
```

Then add the function (before `push_profile`):

```rust
fn checkpoint_wal(db_path: &Path) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Could not open {}: {e}", db_path.display()))?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|e| format!("WAL checkpoint failed: {e}"))
}
```

Also add the checkpoint call to the existing `push_profile` (resolves the existing TODO). In `push_profile`, after `let profile_dir = ...`, add:

```rust
let places_path = profile_dir.join("places.sqlite");
if places_path.exists() {
    checkpoint_wal(&places_path)?;
}
```

- [ ] **Step 5: Run tests to verify they pass**

```bash
cd src-tauri && cargo test sync
```

Expected: all tests pass.

- [ ] **Step 6: Add `auto_push` and `auto_pull`**

Add a helper to extract the Litterbox file ID from a URL (append after `checkpoint_wal`):

```rust
fn extract_file_id(url: &str) -> Option<String> {
    let filename = url.trim().rsplit('/').next()?;
    let id = filename.strip_suffix(".bin").unwrap_or(filename);
    if id.is_empty() { return None; }
    Some(id.to_uppercase())
}
```

Add `auto_push` (collects files and uploads using the passphrase as the PBKDF2 key):

```rust
/// Push the profile using a passphrase-derived key. Returns the Litterbox file ID.
/// Used by the daemon for automatic syncing; does not return a sync code.
pub async fn auto_push(passphrase: &str) -> Result<String, String> {
    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found.")?;

    let places_path = profile_dir.join("places.sqlite");
    if places_path.exists() {
        checkpoint_wal(&places_path)?;
    }

    let mut files = HashMap::new();
    for &name in profile::SYNC_FILES {
        let path = profile_dir.join(name);
        if !path.exists() { continue; }
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Could not read {name}: {e}"))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(format!("{name} exceeds the 5 MB limit"));
        }
        files.insert(name.to_string(), BASE64.encode(&bytes));
    }
    if files.is_empty() {
        return Err("No sync files found in the Zen profile folder".into());
    }

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let bundle = SyncBundle { version: 1, created_at, files };
    let json = serde_json::to_vec(&bundle).map_err(|e| e.to_string())?;
    let encrypted = crypto::encrypt(&json, passphrase)?;
    let result = transport::upload(encrypted).await?;

    extract_file_id(&result.url)
        .ok_or_else(|| format!("Could not parse Litterbox URL: {}", result.url))
}
```

Add `auto_pull` (fetches and decrypts using passphrase, writes profile files):

```rust
/// Pull a profile bundle by file ID using a passphrase-derived key.
/// Used by the daemon for automatic syncing.
/// Returns the sorted list of written file names.
pub async fn auto_pull(file_id: &str, passphrase: &str) -> Result<Vec<String>, String> {
    let url = format!(
        "https://litter.catbox.moe/{}.bin",
        file_id.to_lowercase()
    );
    let encrypted = transport::download(&url).await?;
    let json = crypto::decrypt(&encrypted, passphrase)?;

    let bundle: SyncBundle = serde_json::from_slice(&json)
        .map_err(|e| format!("Bundle format error: {e}"))?;

    if bundle.version != 1 {
        return Err(format!(
            "Unsupported bundle version {} — update ZynC",
            bundle.version
        ));
    }

    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found.")?;

    let file_names: Vec<String> = bundle.files.keys().cloned().collect();
    backup_profile(&profile_dir, &file_names)?;

    let mut written = Vec::new();
    for (name, b64) in &bundle.files {
        let bytes = BASE64.decode(b64)
            .map_err(|e| format!("Failed to decode {name}: {e}"))?;
        std::fs::write(profile_dir.join(name), &bytes)
            .map_err(|e| format!("Failed to write {name}: {e}"))?;
        written.push(name.clone());
    }

    written.sort();
    Ok(written)
}
```

- [ ] **Step 7: Run all sync tests**

```bash
cd src-tauri && cargo test sync
```

Expected: all existing tests still pass, no new failures.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/sync.rs src-tauri/Cargo.toml
git commit -m "feat: add WAL checkpoint, auto_push, auto_pull to sync.rs"
```

---

## Task 5: daemon.rs — Background state and loops

**Files:**
- Create: `src-tauri/src/daemon.rs`

Three Tokio tasks run in parallel for the lifetime of the app:
- **Zen watcher** (every 5 s): detects Zen running→closed transition → triggers push; if a pull is queued, push wins and the queued pull is discarded.
- **ntfy poller** (every 60 s): checks for new file IDs; if Zen is closed, pulls immediately; if Zen is open, queues the latest file ID.
- **Refresh timer** (every 5 min): re-uploads the last bundle before the 55-minute deadline; skips if Zen is open and retries 5 minutes later.

`trigger_push` is the shared helper used by both the Zen watcher and the `manual_sync_now` Tauri command.

- [ ] **Step 1: Create daemon.rs**

```rust
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::Manager;

use crate::{ntfy, pairing, sync, zen_check};

pub struct DaemonState {
    pub auto_push_enabled: bool,
    pub auto_pull_enabled: bool,
    /// Last ntfy message ID we processed — used to avoid re-pulling.
    pub last_ntfy_id: Option<String>,
    /// File ID from ntfy that arrived while Zen was open — waiting for Zen to close.
    pub pending_file_id: Option<String>,
    /// Unix timestamp of last successful push or pull.
    pub last_synced: Option<u64>,
    /// When to re-upload the profile to keep the Litterbox link alive.
    pub refresh_at: Option<Instant>,
    /// Previous Zen running state (used for edge detection).
    pub zen_was_running: bool,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            auto_push_enabled: true,
            auto_pull_enabled: true,
            last_ntfy_id: None,
            pending_file_id: None,
            last_synced: None,
            refresh_at: None,
            zen_was_running: false,
        }
    }
}

// ── Tauri commands ────────────────────────────────────────────

#[tauri::command]
pub fn get_last_synced_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> Option<u64> {
    state.lock().unwrap().last_synced
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

// ── Shared push helper ────────────────────────────────────────

/// Upload the current profile and publish the file ID to ntfy.
pub async fn trigger_push(
    app: &tauri::AppHandle,
    state: &Arc<Mutex<DaemonState>>,
    passphrase: &str,
) -> Result<(), String> {
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
        s.refresh_at = Some(Instant::now() + Duration::from_secs(55 * 60));
    }

    show_notification(app, "Profile synced");
    Ok(())
}

// ── Background daemon entry point ─────────────────────────────

/// Spawn the three background loops. Call once from lib.rs setup.
pub fn start(app: tauri::AppHandle, state: Arc<Mutex<DaemonState>>) {
    // Zen process watcher
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            loop {
                interval.tick().await;
                zen_watcher_tick(&app, &state).await;
            }
        });
    }

    // ntfy poller
    {
        let app = app.clone();
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
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
            let mut interval = tokio::time::interval(Duration::from_secs(5 * 60));
            loop {
                interval.tick().await;
                refresh_tick(&app, &state).await;
            }
        });
    }
}

// ── Loop tick helpers ─────────────────────────────────────────

async fn zen_watcher_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match pairing::load_passphrase() {
        Ok(Some(p)) => p,
        _ => return,
    };

    let zen_running = zen_check::is_zen_running();

    let (was_running, auto_push_enabled) = {
        let mut s = state.lock().unwrap();
        let was = s.zen_was_running;
        s.zen_was_running = zen_running;
        (was, s.auto_push_enabled)
    };

    // Zen just closed
    if was_running && !zen_running && auto_push_enabled {
        // Discard any queued pull — our just-closed session wins
        state.lock().unwrap().pending_file_id = None;

        if let Err(e) = trigger_push(app, state, &passphrase).await {
            show_notification(app, &format!("Auto-push failed: {e}"));
        }
    }
}

async fn ntfy_poll_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match pairing::load_passphrase() {
        Ok(Some(p)) => p,
        _ => return,
    };

    let (auto_pull_enabled, since_id) = {
        let s = state.lock().unwrap();
        (s.auto_pull_enabled, s.last_ntfy_id.clone())
    };

    if !auto_pull_enabled {
        return;
    }

    let topic = pairing::derive_ntfy_topic(&passphrase);
    let messages = match ntfy::poll_since(&topic, since_id.as_deref()).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ntfy poll error: {e}");
            return;
        }
    };

    // Take the latest message only
    let Some(latest) = messages.last() else { return };

    // Update last-seen ID
    state.lock().unwrap().last_ntfy_id = Some(latest.id.clone());

    let file_id = latest.message.trim().to_string();
    let zen_running = zen_check::is_zen_running();

    if zen_running {
        // Queue for when Zen closes; overwrite any older queued ID
        state.lock().unwrap().pending_file_id = Some(file_id);
        show_notification(app, "New profile available — will pull when Zen closes");
    } else {
        // Pull now
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

async fn refresh_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let passphrase = match pairing::load_passphrase() {
        Ok(Some(p)) => p,
        _ => return,
    };

    let should_refresh = {
        let s = state.lock().unwrap();
        s.refresh_at.map(|d| Instant::now() >= d).unwrap_or(false)
    };

    if !should_refresh {
        return;
    }

    if zen_check::is_zen_running() {
        // Retry 5 min later
        let mut s = state.lock().unwrap();
        if let Some(d) = s.refresh_at {
            s.refresh_at = Some(d + Duration::from_secs(5 * 60));
        }
        return;
    }

    if let Err(e) = trigger_push(app, state, &passphrase).await {
        show_notification(app, &format!("Refresh failed: {e}"));
    }
}

// ── Notification helper ───────────────────────────────────────

fn show_notification(app: &tauri::AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification()
        .builder()
        .title("ZynC")
        .body(body)
        .show();
}
```

- [ ] **Step 2: Add `mod daemon;` to lib.rs**

```rust
mod daemon;
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors (warnings about unused imports are fine).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/daemon.rs src-tauri/src/lib.rs
git commit -m "feat: add daemon.rs — background push/pull/refresh loops and DaemonState"
```

---

## Task 6: lib.rs — Tray, plugins, state, close-to-tray, commands

**Files:**
- Modify: `src-tauri/src/lib.rs`

Replace the entire file:

```rust
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
    Manager,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            setup_tray(app)?;

            // Shared daemon state — managed so Tauri commands can access it
            let state = Arc::new(Mutex::new(daemon::DaemonState::default()));
            app.manage(state.clone());

            // Start background daemon
            daemon::start(app.handle().clone(), state);

            // Close button hides to tray instead of quitting
            let window = app.get_webview_window("main").unwrap();
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
            daemon::get_last_synced_cmd,
            daemon::set_auto_push_cmd,
            daemon::set_auto_pull_cmd,
            daemon::manual_sync_now_cmd,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "Open ZynC", true, None::<&str>)?;
    let sync_now = MenuItem::with_id(app, "sync_now", "Sync now", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &sep, &sync_now, &sep, &quit])?;

    TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
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

    Ok(())
}
```

- [ ] **Step 1: Replace lib.rs with the above**

- [ ] **Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: wire tray, daemon, plugins, and close-to-tray in lib.rs"
```

---

## Task 7: tauri.conf.json + capabilities

**Files:**
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/capabilities/default.json`

- [ ] **Step 1: Update tauri.conf.json — hide window on startup**

Replace the `"windows"` array with:

```json
"windows": [
  {
    "title": "ZynC",
    "width": 420,
    "height": 340,
    "resizable": false,
    "fullscreen": false,
    "center": true,
    "visible": false
  }
]
```

(The `visible: false` means the window starts hidden; lib.rs shows it conditionally on first run. The tray icon is always shown.)

- [ ] **Step 2: Update capabilities/default.json**

```json
{
  "identifier": "default",
  "description": "Default capability for ZynC",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:tray:default",
    "notification:default",
    "autostart:default"
  ]
}
```

- [ ] **Step 3: Verify the app builds and starts without showing a window**

```bash
cd src-tauri && cargo tauri dev
```

Expected: app starts, tray icon appears in menu bar, no window on startup (unless keychain has no passphrase — then window appears showing settings).

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tauri.conf.json src-tauri/capabilities/default.json
git commit -m "feat: hide window on startup, enable tray/notification/autostart permissions"
```

---

## Task 8: Frontend — Settings screen

**Files:**
- Modify: `src/index.html`
- Modify: `src/main.js`
- Modify: `src/style.css`

- [ ] **Step 1: Add settings screen to index.html**

Add after the closing `</div>` of `screen-pull` and before the closing `</body>`:

```html
<!-- Settings screen -->
<div class="screen" id="screen-settings">
  <div class="settings-header">
    <h2>Pairing Setup</h2>
    <p class="settings-hint">Enter the same passphrase on each machine to enable automatic sync.</p>
  </div>

  <div class="settings-row">
    <label for="passphrase-input">Passphrase</label>
    <div class="passphrase-wrap">
      <input type="password" id="passphrase-input" placeholder="e.g. purple-fox-mountain-7" autocomplete="off" spellcheck="false" />
      <button id="btn-reveal" type="button">Show</button>
    </div>
  </div>

  <div class="settings-row toggles">
    <label>
      <input type="checkbox" id="toggle-auto-push" checked />
      Auto-push when Zen closes
    </label>
    <label>
      <input type="checkbox" id="toggle-auto-pull" checked />
      Auto-pull when another machine syncs
    </label>
  </div>

  <div class="settings-actions">
    <button id="btn-save-passphrase" class="btn-primary">Save &amp; Pair</button>
    <button id="btn-forget-passphrase" class="btn-secondary">Forget Passphrase</button>
  </div>

  <div id="pairing-status" class="pairing-status"></div>
  <div id="status-settings" class="status"></div>
</div>
```

Also add a Settings nav link to the main screen. In `screen-main`, add after the pull input row:

```html
<div class="nav-row">
  <button id="btn-open-settings" class="btn-link">⚙ Settings</button>
</div>
```

- [ ] **Step 2: Add settings logic to main.js**

Append to `main.js`:

```js
// ── Settings screen ───────────────────────────────────────────

async function loadSettingsScreen() {
  const paired = await invoke("get_pairing_status_cmd");
  const statusEl = document.getElementById("pairing-status");
  statusEl.textContent = paired ? "Status: Paired" : "Status: Not paired";
  statusEl.className = "pairing-status " + (paired ? "paired" : "unpaired");
  if (paired) {
    document.getElementById("passphrase-input").value = "";
    document.getElementById("passphrase-input").placeholder = "Enter new passphrase to change";
  }
}

async function handleSavePassphrase() {
  const passphrase = document.getElementById("passphrase-input").value.trim();
  if (!passphrase) {
    document.getElementById("status-settings").textContent = "Enter a passphrase first.";
    document.getElementById("status-settings").className = "status error";
    return;
  }
  if (passphrase.length < 8) {
    document.getElementById("status-settings").textContent = "Passphrase must be at least 8 characters.";
    document.getElementById("status-settings").className = "status error";
    return;
  }
  try {
    await invoke("save_passphrase_cmd", { passphrase });
    await invoke("set_auto_push_cmd", { enabled: document.getElementById("toggle-auto-push").checked });
    await invoke("set_auto_pull_cmd", { enabled: document.getElementById("toggle-auto-pull").checked });
    document.getElementById("status-settings").textContent = "Paired! Automatic sync is now active.";
    document.getElementById("status-settings").className = "status success";
    await loadSettingsScreen();
  } catch (err) {
    document.getElementById("status-settings").textContent = String(err);
    document.getElementById("status-settings").className = "status error";
  }
}

async function handleForgetPassphrase() {
  try {
    await invoke("clear_passphrase_cmd");
    document.getElementById("passphrase-input").value = "";
    document.getElementById("status-settings").textContent = "Passphrase cleared. Automatic sync disabled.";
    document.getElementById("status-settings").className = "status";
    await loadSettingsScreen();
  } catch (err) {
    document.getElementById("status-settings").textContent = String(err);
    document.getElementById("status-settings").className = "status error";
  }
}

document.getElementById("btn-save-passphrase").addEventListener("click", handleSavePassphrase);
document.getElementById("btn-forget-passphrase").addEventListener("click", handleForgetPassphrase);
document.getElementById("btn-open-settings").addEventListener("click", async () => {
  await loadSettingsScreen();
  showScreen("screen-settings");
});
document.getElementById("btn-reveal").addEventListener("click", () => {
  const input = document.getElementById("passphrase-input");
  const btn = document.getElementById("btn-reveal");
  const isPassword = input.type === "password";
  input.type = isPassword ? "text" : "password";
  btn.textContent = isPassword ? "Hide" : "Show";
});

// ── First-run detection ───────────────────────────────────────

async function init() {
  const paired = await invoke("get_pairing_status_cmd");
  if (!paired) {
    await loadSettingsScreen();
    showScreen("screen-settings");
  } else {
    showScreen("screen-main");
  }
}

document.addEventListener("DOMContentLoaded", init);
```

- [ ] **Step 3: Add settings styles to style.css**

Append to `style.css`:

```css
/* Settings screen */
.settings-header {
  margin-bottom: 14px;
}
.settings-header h2 {
  font-size: 15px;
  font-weight: 600;
  margin: 0 0 4px;
  color: var(--text);
}
.settings-hint {
  font-size: 11px;
  color: var(--text-muted);
  margin: 0;
  line-height: 1.4;
}
.settings-row {
  margin-bottom: 12px;
}
.settings-row label {
  display: block;
  font-size: 11px;
  color: var(--text-muted);
  margin-bottom: 4px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}
.passphrase-wrap {
  display: flex;
  gap: 6px;
}
.passphrase-wrap input {
  flex: 1;
  background: var(--input-bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text);
  font-size: 13px;
  padding: 6px 10px;
  outline: none;
  font-family: monospace;
}
.passphrase-wrap input:focus {
  border-color: var(--accent);
}
.passphrase-wrap button {
  background: var(--surface);
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-muted);
  font-size: 11px;
  padding: 6px 10px;
  cursor: pointer;
}
.toggles {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.toggles label {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12px;
  color: var(--text);
  text-transform: none;
  letter-spacing: 0;
  cursor: pointer;
}
.settings-actions {
  display: flex;
  gap: 8px;
  margin-bottom: 10px;
}
.pairing-status {
  font-size: 11px;
  margin-bottom: 6px;
}
.pairing-status.paired { color: #6fcf97; }
.pairing-status.unpaired { color: var(--text-muted); }
.btn-link {
  background: none;
  border: none;
  color: var(--text-muted);
  font-size: 11px;
  cursor: pointer;
  padding: 0;
  text-align: left;
}
.btn-link:hover { color: var(--text); }
.nav-row {
  margin-top: 10px;
  text-align: right;
}
```

- [ ] **Step 4: Run the dev server and verify the UI**

```bash
cd src-tauri && cargo tauri dev
```

Check:
- First run (no passphrase saved): Settings screen appears immediately.
- Enter a passphrase ≥ 8 chars, click "Save & Pair": status shows "Paired".
- Click "Forget Passphrase": status shows "Not paired".
- On subsequent opens (passphrase in keychain): main screen appears, Settings accessible via ⚙ link.

- [ ] **Step 5: Commit**

```bash
git add src/index.html src/main.js src/style.css
git commit -m "feat: add settings screen — passphrase entry, pairing status, auto-sync toggles"
```

---

## Post-Implementation Checklist

After all tasks are complete, verify the following manually before considering the feature done:

- [ ] **Tray icon appears on launch** — no window opens if passphrase is already set
- [ ] **First-run flow** — clear keychain, relaunch, settings screen appears
- [ ] **Pairing** — enter same passphrase on two machines (or same machine twice to test keychain)
- [ ] **Auto-push** — open Zen, make a change, close Zen, wait ≤5 s, tray notification appears
- [ ] **ntfy delivery** — on the second machine, within 60 s, notification "Profile updated from another machine" appears
- [ ] **Auto-pull** — second machine's profile files are updated (check timestamps in `~/Library/Application Support/zen/Profiles/…`)
- [ ] **Refresh** — wait 56+ minutes with app running, verify new notification appears (or check ntfy topic in browser)
- [ ] **Queued pull** — have Zen open on machine B when machine A pushes; verify notification queues and pull fires when Zen closes
- [ ] **Manual sync** — tray menu "Sync now" triggers push when Zen is closed; shows error when Zen is open
- [ ] **Forget passphrase** — daemon stops pushing/pulling after forget; re-pair restores behavior
- [ ] **Close to tray** — closing the window hides it; clicking tray icon restores it; Quit exits

---

## Notes for macOS Production Build

The `keyring` crate accesses macOS Keychain. When building a signed/notarized production `.dmg`, add to `src-tauri/entitlements.plist` (create if not present):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.app-sandbox</key>
  <true/>
  <key>com.apple.security.network.client</key>
  <true/>
  <key>com.apple.security.files.user-selected.read-write</key>
  <true/>
  <key>com.apple.security.keychain-access-groups</key>
  <array>
    <string>app.zync.zensync</string>
  </array>
</dict>
</plist>
```

This is a pre-existing TODO and does not block development builds.
