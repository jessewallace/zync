# GitHub Sync Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Litterbox + session-heuristic conflict resolution with GitHub Releases storage, version-aware conflict detection, and a 10-snapshot rollback UI.

**Architecture:** A new `github.rs` module owns all GitHub API interaction (OAuth, repo/release provisioning, metadata via Contents API, profile blobs via Release assets). A new `local_state.rs` persists `last_known_version` and `machine_name` across restarts — the missing piece that made session heuristics necessary. The daemon's push path checks GitHub's version counter before uploading; if behind, it pulls instead.

**Tech Stack:** Rust/Tauri v2, `tauri-plugin-oauth` (local OAuth redirect server), `tauri-plugin-opener` (open browser), `reqwest` (already present), `keyring` (already present), `hostname` crate for default machine name.

**Spec:** `docs/superpowers/specs/2026-05-20-github-sync-redesign-design.md`

**Note on transport.rs:** The manual Push/Pull tabs (ZEN-CODE flow) still use Litterbox and `transport.rs`. Only the daemon's auto-sync path switches to GitHub. `transport.rs` is not deleted.

---

## File Map

| File | Action | Responsibility |
|---|---|---|
| `src-tauri/src/github.rs` | **Create** | GitHubClient, data types, OAuth, provisioning, metadata ops, blob ops |
| `src-tauri/src/local_state.rs` | **Create** | Persistent state across restarts (last_known_version, machine_name) |
| `src-tauri/Cargo.toml` | **Modify** | Add tauri-plugin-oauth, tauri-plugin-opener, hostname |
| `src-tauri/capabilities/default.json` | **Modify** | Add oauth permission |
| `src-tauri/src/lib.rs` | **Modify** | Plugin registration, new commands, remove passphrase commands, startup init |
| `src-tauri/src/daemon.rs` | **Modify** | New DaemonState, version-check push flow, remove refresh loop |
| `src-tauri/src/sync.rs` | **Modify** | Add github_push / github_pull; remove auto_push / auto_pull |
| `src-tauri/src/ntfy.rs` | **Modify** | Parse version number from message; publish version instead of file ID |
| `src-tauri/src/pairing.rs` | **Modify** | Remove passphrase/flag functions; keep derive_ntfy_topic |
| `src/index.html` | **Modify** | Rename Pair → Sync tab; new Sync and Rollback views |
| `src/main.js` | **Modify** | Sync tab logic (connect, rollback, status) |
| `src-tauri/src/transport.rs` | **Unchanged** | Manual push/pull still uses Litterbox |
| `src-tauri/src/crypto.rs` | **Unchanged** | |
| `src-tauri/src/zen_check.rs` | **Unchanged** | |
| `src-tauri/src/profile.rs` | **Unchanged** | |

---

## Task 1: Dependencies and Plugin Registration

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/capabilities/default.json`
- Modify: `src-tauri/src/lib.rs` (plugin registration only)

- [ ] **Step 1: Add dependencies to Cargo.toml**

In `src-tauri/Cargo.toml`, under `[dependencies]`, add:

```toml
# GitHub OAuth + browser opener
tauri-plugin-oauth = "2"
tauri-plugin-opener = "2"
hostname = "0.4"
```

Change the existing `reqwest` line to add the `json` feature (needed for GitHub API):

```toml
reqwest = { version = "0.12", features = ["multipart", "json"] }
```

- [ ] **Step 2: Add oauth permission to capabilities**

Replace the contents of `src-tauri/capabilities/default.json`:

```json
{
  "identifier": "default",
  "description": "Default capability for Zync",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:tray:default",
    "notification:default",
    "autostart:default",
    "updater:default",
    "oauth:default",
    "opener:default"
  ]
}
```

- [ ] **Step 3: Register new plugins in lib.rs**

In `src-tauri/src/lib.rs`, inside `tauri::Builder::default()` chain, add after the existing plugins:

```rust
.plugin(tauri_plugin_oauth::init())
.plugin(tauri_plugin_opener::init())
```

- [ ] **Step 4: Add module declarations to lib.rs**

At the top of `src-tauri/src/lib.rs`, add:

```rust
mod github;
mod local_state;
```

- [ ] **Step 5: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | head -40
```

Expected: warnings only — no errors. The new modules don't exist yet so expect "file not found" errors for `github` and `local_state`. That is fine for this step.

---

## Task 2: `local_state.rs`

**Files:**
- Create: `src-tauri/src/local_state.rs`
- Test: inline `#[cfg(test)]` block

- [ ] **Step 1: Write failing tests**

Create `src-tauri/src/local_state.rs` with the test module first:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocalState {
    pub last_known_version: u32,
    pub machine_name: String,
}

impl LocalState {
    pub fn load(_config_dir: &Path) -> Self { todo!() }
    pub fn save(&self, _config_dir: &Path) -> Result<(), String> { todo!() }
}

impl Default for LocalState {
    fn default() -> Self {
        Self {
            last_known_version: 0,
            machine_name: default_machine_name(),
        }
    }
}

fn default_machine_name() -> String { todo!() }

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let state = LocalState::load(dir.path());
        assert_eq!(state.last_known_version, 0);
        assert!(!state.machine_name.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let original = LocalState {
            last_known_version: 42,
            machine_name: "Test Machine".to_string(),
        };
        original.save(dir.path()).unwrap();
        let loaded = LocalState::load(dir.path());
        assert_eq!(loaded, original);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd src-tauri && cargo test local_state 2>&1 | tail -20
```

Expected: FAIL with `not yet implemented` (todo! panics).

- [ ] **Step 3: Implement LocalState**

Replace the entire `src-tauri/src/local_state.rs` with the full implementation:

```rust
use serde::{Deserialize, Serialize};
use std::path::Path;

const STATE_FILE: &str = "state.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocalState {
    pub last_known_version: u32,
    pub machine_name: String,
}

impl Default for LocalState {
    fn default() -> Self {
        Self {
            last_known_version: 0,
            machine_name: default_machine_name(),
        }
    }
}

impl LocalState {
    pub fn load(config_dir: &Path) -> Self {
        let path = config_dir.join(STATE_FILE);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config_dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(config_dir)
            .map_err(|e| format!("Failed to create config dir: {e}"))?;
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Serialize error: {e}"))?;
        std::fs::write(config_dir.join(STATE_FILE), json)
            .map_err(|e| format!("Failed to write state: {e}"))
    }
}

fn default_machine_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .unwrap_or_else(|| "My Machine".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let state = LocalState::load(dir.path());
        assert_eq!(state.last_known_version, 0);
        assert!(!state.machine_name.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let original = LocalState {
            last_known_version: 42,
            machine_name: "Test Machine".to_string(),
        };
        original.save(dir.path()).unwrap();
        let loaded = LocalState::load(dir.path());
        assert_eq!(loaded, original);
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd src-tauri && cargo test local_state 2>&1 | tail -20
```

Expected: `test local_state::tests::load_missing_file_returns_default ... ok` and `test local_state::tests::save_and_load_round_trip ... ok`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/local_state.rs src-tauri/Cargo.toml src-tauri/capabilities/default.json
git commit -m "feat: add local_state module for persistent sync state"
```

---

## Task 3: `github.rs` — Data Types and Token Management

**Files:**
- Create: `src-tauri/src/github.rs`

- [ ] **Step 1: Write the data types and token management skeleton with tests**

Create `src-tauri/src/github.rs`:

```rust
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

const KEYCHAIN_SERVICE: &str = "zync";
const KEYCHAIN_GITHUB_TOKEN: &str = "github_token";
const KEYCHAIN_GITHUB_USER_ID: &str = "github_user_id";
const KEYCHAIN_GITHUB_USERNAME: &str = "github_username";

pub const GITHUB_CLIENT_ID: &str = env!("GITHUB_CLIENT_ID");
const GITHUB_CLIENT_SECRET: &str = env!("GITHUB_CLIENT_SECRET");
const REPO_NAME: &str = "zync-sync";
const RELEASE_TAG: &str = "storage";
const ENCRYPTION_KEY_ASSET: &str = "encryption-key.b64";
const METADATA_PATH: &str = "metadata.json";
const API_BASE: &str = "https://api.github.com";
const UPLOAD_BASE: &str = "https://uploads.github.com";

// ── Data types ────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SlotEntry {
    pub slot: u8,
    pub version: u32,
    pub pushed_at: String,   // ISO 8601 UTC e.g. "2026-05-20T14:30:00Z"
    pub machine_name: String,
    pub size_bytes: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SyncMetadata {
    pub version: u32,
    pub current_slot: u8,
    pub slots: Vec<SlotEntry>, // sorted newest-first, max 10
}

impl SyncMetadata {
    /// The slot that the next push should write to (oldest slot in ring buffer).
    pub fn next_slot(&self) -> u8 {
        if self.slots.len() < 10 {
            self.slots.len() as u8
        } else {
            (self.current_slot + 1) % 10
        }
    }
}

pub struct MetadataWithSha {
    pub metadata: SyncMetadata,
    pub sha: String,
}

#[derive(Clone)]
pub struct GitHubClient {
    pub token: String,
    pub user_id: u64,
    pub username: String,
    pub release_id: u64,
    pub encryption_key: [u8; 32],
    pub http: reqwest::Client,
}

// ── Token management ──────────────────────────────────────────────────────────

pub fn has_stored_token() -> bool {
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_GITHUB_TOKEN)
        .ok()
        .and_then(|e| e.get_password().ok())
        .is_some()
}

fn save_token(token: &str, user_id: u64, username: &str) -> Result<(), String> {
    for (account, value) in [
        (KEYCHAIN_GITHUB_TOKEN, token.to_string()),
        (KEYCHAIN_GITHUB_USER_ID, user_id.to_string()),
        (KEYCHAIN_GITHUB_USERNAME, username.to_string()),
    ] {
        keyring::Entry::new(KEYCHAIN_SERVICE, account)
            .map_err(|e| format!("Keychain error: {e}"))?
            .set_password(&value)
            .map_err(|e| format!("Failed to save {account}: {e}"))?;
    }
    Ok(())
}

pub fn remove_stored_token() -> Result<(), String> {
    for account in [KEYCHAIN_GITHUB_TOKEN, KEYCHAIN_GITHUB_USER_ID, KEYCHAIN_GITHUB_USERNAME] {
        let entry = keyring::Entry::new(KEYCHAIN_SERVICE, account)
            .map_err(|e| format!("Keychain error: {e}"))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(format!("Failed to remove {account}: {e}")),
        }
    }
    Ok(())
}

fn load_token_parts() -> Option<(String, u64, String)> {
    let load = |account: &str| -> Option<String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, account)
            .ok()?
            .get_password()
            .ok()
    };
    let token = load(KEYCHAIN_GITHUB_TOKEN)?;
    let user_id: u64 = load(KEYCHAIN_GITHUB_USER_ID)?.parse().ok()?;
    let username = load(KEYCHAIN_GITHUB_USERNAME)?;
    Some((token, user_id, username))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_slot_empty() {
        let m = SyncMetadata { version: 0, current_slot: 0, slots: vec![] };
        assert_eq!(m.next_slot(), 0);
    }

    #[test]
    fn next_slot_filling_up() {
        let make_slot = |i: u8| SlotEntry {
            slot: i, version: i as u32, pushed_at: String::new(),
            machine_name: String::new(), size_bytes: 0,
        };
        let slots: Vec<_> = (0..5).map(make_slot).collect();
        let m = SyncMetadata { version: 5, current_slot: 4, slots };
        assert_eq!(m.next_slot(), 5);
    }

    #[test]
    fn next_slot_full_ring_wraps() {
        let make_slot = |i: u8| SlotEntry {
            slot: i, version: i as u32, pushed_at: String::new(),
            machine_name: String::new(), size_bytes: 0,
        };
        let slots: Vec<_> = (0..10).map(make_slot).collect();
        let m = SyncMetadata { version: 10, current_slot: 3, slots };
        // oldest slot = (3 + 1) % 10 = 4
        assert_eq!(m.next_slot(), 4);
    }
}
```

- [ ] **Step 2: Run the unit tests**

```bash
cd src-tauri && cargo test github::tests 2>&1 | tail -20
```

Expected: `test github::tests::next_slot_empty ... ok` etc.

Note: This step will require `GITHUB_CLIENT_ID` and `GITHUB_CLIENT_SECRET` env vars to be set (even to dummy values) since they're used with `env!()`. Run with:

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cargo test github::tests 2>&1 | tail -20
```

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/github.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: add github module data types and token management"
```

---

## Task 4: `github.rs` — OAuth Connect Flow

**Files:**
- Modify: `src-tauri/src/github.rs`

- [ ] **Step 1: Add the OAuth helper and fetch_user function**

Append to `src-tauri/src/github.rs` (before the `#[cfg(test)]` block):

```rust
// ── OAuth ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct GitHubUser {
    id: u64,
    login: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn fetch_user(http: &reqwest::Client, token: &str) -> Result<(u64, String), String> {
    let user: GitHubUser = http
        .get(format!("{API_BASE}/user"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "zync-app")
        .send()
        .await
        .map_err(|e| format!("GitHub user fetch failed: {e}"))?
        .error_for_status()
        .map_err(|e| format!("GitHub user fetch error: {e}"))?
        .json()
        .await
        .map_err(|e| format!("GitHub user parse error: {e}"))?;
    Ok((user.id, user.login))
}

/// Open a browser OAuth flow. Returns (token, user_id, username).
pub async fn oauth_connect(app: &tauri::AppHandle) -> Result<(String, u64, String), String> {
    use tauri_plugin_opener::OpenerExt;

    let (tx, rx) = tokio::sync::oneshot::channel::<String>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));

    let port = tauri_plugin_oauth::start(move |url| {
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(url);
        }
    })
    .map_err(|e| format!("OAuth server start failed: {e}"))?;

    let auth_url = format!(
        "https://github.com/login/oauth/authorize?client_id={}&scope=repo&redirect_uri=http://127.0.0.1:{}",
        GITHUB_CLIENT_ID, port
    );

    app.opener()
        .open_url(auth_url, None::<&str>)
        .map_err(|e| format!("Failed to open browser: {e}"))?;

    let redirect_url = rx.await.map_err(|_| "OAuth cancelled or timed out".to_string())?;

    // Parse ?code= from redirect URL
    let code = redirect_url
        .split('?')
        .nth(1)
        .and_then(|q| {
            q.split('&').find_map(|kv| {
                let mut parts = kv.splitn(2, '=');
                if parts.next()? == "code" { parts.next().map(str::to_string) } else { None }
            })
        })
        .ok_or("OAuth redirect did not contain a code")?;

    // Exchange code for token
    let http = reqwest::Client::new();
    let resp: TokenResponse = http
        .post("https://github.com/login/oauth/access_token")
        .header("Accept", "application/json")
        .header("User-Agent", "zync-app")
        .json(&serde_json::json!({
            "client_id": GITHUB_CLIENT_ID,
            "client_secret": GITHUB_CLIENT_SECRET,
            "code": code,
            "redirect_uri": format!("http://127.0.0.1:{port}"),
        }))
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Token exchange parse error: {e}"))?;

    if let Some(err) = resp.error {
        return Err(format!("OAuth error: {err} — {}", resp.error_description.unwrap_or_default()));
    }

    let token = resp.access_token.ok_or("No access_token in response")?;
    let (user_id, username) = fetch_user(&http, &token).await?;
    Ok((token, user_id, username))
}
```

- [ ] **Step 2: Verify it compiles**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

Expected: no errors (warnings about unused items are fine at this stage).

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/github.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: add github OAuth connect flow"
```

---

## Task 5: `github.rs` — GitHubClient Factory (Provisioning + Encryption Key)

**Files:**
- Modify: `src-tauri/src/github.rs`

- [ ] **Step 1: Add GitHub API helper and provisioning methods**

Append to `src-tauri/src/github.rs` (before `#[cfg(test)]`):

```rust
// ── GitHubClient impl ─────────────────────────────────────────────────────────

impl GitHubClient {
    fn api_headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("Authorization", format!("Bearer {}", self.token).parse().unwrap());
        h.insert("Accept", "application/vnd.github+json".parse().unwrap());
        h.insert("X-GitHub-Api-Version", "2022-11-28".parse().unwrap());
        h.insert("User-Agent", "zync-app".parse().unwrap());
        h
    }

    async fn ensure_repo(&self) -> Result<(), String> {
        let check = self.http
            .get(format!("{API_BASE}/repos/{}/{REPO_NAME}", self.username))
            .headers(self.api_headers())
            .send()
            .await
            .map_err(|e| format!("Repo check failed: {e}"))?;

        if check.status() == reqwest::StatusCode::NOT_FOUND {
            self.http
                .post(format!("{API_BASE}/user/repos"))
                .headers(self.api_headers())
                .json(&serde_json::json!({
                    "name": REPO_NAME,
                    "private": true,
                    "description": "Zync profile storage — do not modify"
                }))
                .send()
                .await
                .map_err(|e| format!("Repo create failed: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Repo create error: {e}"))?;
        }
        Ok(())
    }

    async fn ensure_release(&self) -> Result<u64, String> {
        #[derive(Deserialize)]
        struct Release { id: u64 }

        let check = self.http
            .get(format!("{API_BASE}/repos/{}/{REPO_NAME}/releases/tags/{RELEASE_TAG}", self.username))
            .headers(self.api_headers())
            .send()
            .await
            .map_err(|e| format!("Release check failed: {e}"))?;

        if check.status() == reqwest::StatusCode::NOT_FOUND {
            let r: Release = self.http
                .post(format!("{API_BASE}/repos/{}/{REPO_NAME}/releases", self.username))
                .headers(self.api_headers())
                .json(&serde_json::json!({
                    "tag_name": RELEASE_TAG,
                    "name": "Zync Storage",
                    "body": "Managed by Zync — do not modify",
                    "draft": false,
                    "prerelease": false
                }))
                .send()
                .await
                .map_err(|e| format!("Release create failed: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Release create error: {e}"))?
                .json()
                .await
                .map_err(|e| format!("Release create parse error: {e}"))?;
            Ok(r.id)
        } else {
            let r: Release = check
                .error_for_status()
                .map_err(|e| format!("Release fetch error: {e}"))?
                .json()
                .await
                .map_err(|e| format!("Release parse error: {e}"))?;
            Ok(r.id)
        }
    }

    async fn get_or_create_encryption_key(&self) -> Result<[u8; 32], String> {
        // Try to download existing key asset
        if let Some(asset_id) = self.get_asset_id(ENCRYPTION_KEY_ASSET).await? {
            let b64 = self.http
                .get(format!("{API_BASE}/repos/{}/{REPO_NAME}/releases/assets/{asset_id}", self.username))
                .headers({
                    let mut h = self.api_headers();
                    h.insert("Accept", "application/octet-stream".parse().unwrap());
                    h
                })
                .send()
                .await
                .map_err(|e| format!("Key download failed: {e}"))?
                .text()
                .await
                .map_err(|e| format!("Key read failed: {e}"))?;
            let bytes = BASE64.decode(b64.trim())
                .map_err(|e| format!("Key decode failed: {e}"))?;
            if bytes.len() != 32 {
                return Err(format!("Encryption key wrong length: {}", bytes.len()));
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }

        // Generate and upload new key
        use rand::RngCore;
        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        let b64 = BASE64.encode(&key);
        self.upload_asset(ENCRYPTION_KEY_ASSET, b64.as_bytes(), "text/plain").await?;
        Ok(key)
    }

    /// Build a fully-initialised client from the OAuth flow. Saves token to keychain.
    pub async fn connect(app: &tauri::AppHandle) -> Result<Self, String> {
        let (token, user_id, username) = oauth_connect(app).await?;
        let http = reqwest::Client::new();
        let mut client = GitHubClient {
            token, user_id, username, release_id: 0,
            encryption_key: [0u8; 32], http,
        };
        client.ensure_repo().await?;
        client.release_id = client.ensure_release().await?;
        client.encryption_key = client.get_or_create_encryption_key().await?;
        save_token(&client.token, client.user_id, &client.username)?;
        Ok(client)
    }

    /// Restore a client from the keychain on app startup. Returns None if not connected.
    pub async fn from_keychain() -> Result<Option<Self>, String> {
        let Some((token, user_id, username)) = load_token_parts() else {
            return Ok(None);
        };
        let http = reqwest::Client::new();
        // Verify token is still valid by fetching user info
        if let Err(e) = fetch_user(&http, &token).await {
            eprintln!("[github] token invalid on restore: {e}");
            return Ok(None);
        }
        let mut client = GitHubClient {
            token, user_id, username, release_id: 0,
            encryption_key: [0u8; 32], http,
        };
        client.release_id = client.ensure_release().await?;
        client.encryption_key = client.get_or_create_encryption_key().await?;
        Ok(Some(client))
    }
}
```

- [ ] **Step 2: Verify compilation**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

Expected: no errors.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/github.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: add GitHubClient connect/restore and provisioning"
```

---

## Task 6: `github.rs` — Metadata and Blob Operations

**Files:**
- Modify: `src-tauri/src/github.rs`

- [ ] **Step 1: Add metadata read/write and blob upload/download**

Append to `src-tauri/src/github.rs` (before `#[cfg(test)]`):

```rust
// ── Metadata (Contents API — SHA-locked) ─────────────────────────────────────

impl GitHubClient {
    /// Read metadata.json. Returns None if the file doesn't exist yet (first push).
    pub async fn read_metadata(&self) -> Result<Option<MetadataWithSha>, String> {
        #[derive(Deserialize)]
        struct ContentsResp { content: String, sha: String }

        let resp = self.http
            .get(format!("{API_BASE}/repos/{}/{REPO_NAME}/contents/{METADATA_PATH}", self.username))
            .headers(self.api_headers())
            .send()
            .await
            .map_err(|e| format!("Metadata fetch failed: {e}"))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let c: ContentsResp = resp
            .error_for_status()
            .map_err(|e| format!("Metadata fetch error: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Metadata parse error: {e}"))?;

        // GitHub returns base64 with newlines — strip them before decoding
        let decoded = BASE64.decode(c.content.replace('\n', ""))
            .map_err(|e| format!("Metadata decode error: {e}"))?;
        let metadata: SyncMetadata = serde_json::from_slice(&decoded)
            .map_err(|e| format!("Metadata JSON error: {e}"))?;

        Ok(Some(MetadataWithSha { metadata, sha: c.sha }))
    }

    /// Write metadata.json. `expected_sha` is the SHA from the previous read;
    /// pass None only when creating the file for the first time.
    /// Returns Ok(true) on success, Ok(false) on SHA conflict (another machine pushed).
    pub async fn write_metadata(
        &self,
        metadata: &SyncMetadata,
        expected_sha: Option<&str>,
    ) -> Result<bool, String> {
        let json = serde_json::to_vec(metadata)
            .map_err(|e| format!("Metadata serialize error: {e}"))?;
        let content = BASE64.encode(&json);

        let mut body = serde_json::json!({
            "message": format!("sync: version {}", metadata.version),
            "content": content,
        });
        if let Some(sha) = expected_sha {
            body["sha"] = serde_json::json!(sha);
        }

        let resp = self.http
            .put(format!("{API_BASE}/repos/{}/{REPO_NAME}/contents/{METADATA_PATH}", self.username))
            .headers(self.api_headers())
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Metadata write failed: {e}"))?;

        if resp.status() == reqwest::StatusCode::CONFLICT {
            return Ok(false); // SHA mismatch — another machine pushed
        }
        resp.error_for_status()
            .map_err(|e| format!("Metadata write error: {e}"))?;
        Ok(true)
    }

// ── Release asset helpers ─────────────────────────────────────────────────────

    pub async fn get_asset_id(&self, name: &str) -> Result<Option<u64>, String> {
        #[derive(Deserialize)]
        struct Asset { id: u64, name: String }

        let assets: Vec<Asset> = self.http
            .get(format!("{API_BASE}/repos/{}/{REPO_NAME}/releases/{}/assets", self.username, self.release_id))
            .headers(self.api_headers())
            .send()
            .await
            .map_err(|e| format!("Asset list failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Asset list error: {e}"))?
            .json()
            .await
            .map_err(|e| format!("Asset list parse error: {e}"))?;

        Ok(assets.into_iter().find(|a| a.name == name).map(|a| a.id))
    }

    async fn delete_asset(&self, asset_id: u64) -> Result<(), String> {
        self.http
            .delete(format!("{API_BASE}/repos/{}/{REPO_NAME}/releases/assets/{asset_id}", self.username))
            .headers(self.api_headers())
            .send()
            .await
            .map_err(|e| format!("Asset delete failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Asset delete error: {e}"))?;
        Ok(())
    }

    async fn upload_asset(&self, name: &str, data: &[u8], content_type: &str) -> Result<(), String> {
        self.http
            .post(format!(
                "{UPLOAD_BASE}/repos/{}/{REPO_NAME}/releases/{}/assets?name={name}",
                self.username, self.release_id
            ))
            .headers(self.api_headers())
            .header("Content-Type", content_type)
            .body(data.to_vec())
            .send()
            .await
            .map_err(|e| format!("Asset upload failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Asset upload error: {e}"))?;
        Ok(())
    }

    pub async fn upload_profile(&self, slot: u8, data: &[u8]) -> Result<(), String> {
        let name = format!("profile-{slot}.enc");
        // Delete existing asset for this slot if present (release assets can't be overwritten)
        if let Some(id) = self.get_asset_id(&name).await? {
            self.delete_asset(id).await?;
        }
        self.upload_asset(&name, data, "application/octet-stream").await
    }

    pub async fn download_profile(&self, slot: u8) -> Result<Vec<u8>, String> {
        let name = format!("profile-{slot}.enc");
        let asset_id = self.get_asset_id(&name).await?
            .ok_or(format!("Profile asset {name} not found"))?;

        let bytes = self.http
            .get(format!("{API_BASE}/repos/{}/{REPO_NAME}/releases/assets/{asset_id}", self.username))
            .headers({
                let mut h = self.api_headers();
                h.insert("Accept", "application/octet-stream".parse().unwrap());
                h
            })
            .send()
            .await
            .map_err(|e| format!("Profile download failed: {e}"))?
            .error_for_status()
            .map_err(|e| format!("Profile download error: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("Profile read failed: {e}"))?
            .to_vec();
        Ok(bytes)
    }
}
```

- [ ] **Step 2: Run existing tests to confirm nothing regressed**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: all existing tests still pass.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/github.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: add GitHub metadata and blob operations"
```

---

## Task 7: `sync.rs` — github_push and github_pull

**Files:**
- Modify: `src-tauri/src/sync.rs`

- [ ] **Step 1: Add failing test for github_push bundle assembly**

At the bottom of `src-tauri/src/sync.rs`, add to the `#[cfg(test)]` block:

```rust
    #[test]
    fn bundle_serializes_and_deserializes() {
        use std::collections::HashMap;
        let mut files = HashMap::new();
        files.insert("prefs.js".to_string(), base64::engine::general_purpose::STANDARD.encode(b"user_pref(\"test\", 1);"));
        let bundle = SyncBundle { version: 1, created_at: 1234567890, files };
        let json = serde_json::to_vec(&bundle).unwrap();
        let back: SyncBundle = serde_json::from_slice(&json).unwrap();
        assert_eq!(back.version, 1);
        assert_eq!(back.created_at, 1234567890);
        assert!(back.files.contains_key("prefs.js"));
    }
```

- [ ] **Step 2: Run to verify test passes (SyncBundle already exists)**

```bash
cd src-tauri && cargo test bundle_serializes 2>&1 | tail -10
```

Expected: PASS (SyncBundle is already defined in sync.rs).

- [ ] **Step 3: Add github_push and github_pull to sync.rs**

In `src-tauri/src/sync.rs`, add the import at the top:

```rust
use crate::github::{GitHubClient, SyncMetadata, SlotEntry};
```

Then add these two functions before the `#[cfg(test)]` block:

```rust
/// Push the current Zen profile to GitHub. Returns (new_version, metadata_sha_used).
/// Returns Err if behind (version mismatch) — caller should pull instead.
/// Returns Ok(None) if another machine pushed between our check and our commit (conflict retry needed).
pub async fn github_push(
    client: &GitHubClient,
    machine_name: &str,
    last_known_version: u32,
) -> Result<Option<(u32, SyncMetadata)>, String> {
    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found.")?;

    let places_path = profile_dir.join("places.sqlite");
    if places_path.exists() {
        checkpoint_wal(&places_path)?;
    }

    let files = collect_sync_files(&profile_dir)?;
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let bundle = SyncBundle { version: 1, created_at, files };
    let json = serde_json::to_vec(&bundle).map_err(|e| e.to_string())?;
    let encrypted = crate::crypto::encrypt(&json, &hex_key(&client.encryption_key))?;

    // Read current metadata to verify we're still up to date
    let (current_metadata, current_sha) = match client.read_metadata().await? {
        Some(m) => (m.metadata, Some(m.sha)),
        None => (SyncMetadata { version: 0, current_slot: 0, slots: vec![] }, None),
    };

    if current_metadata.version != last_known_version {
        // Another machine pushed while we were collecting files — caller should pull
        return Ok(None);
    }

    let slot = current_metadata.next_slot();
    client.upload_profile(slot, &encrypted).await?;

    let now_str = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Simple ISO 8601 UTC format
        let s = chrono_secs_to_iso(secs);
        s
    };

    let new_entry = SlotEntry {
        slot,
        version: current_metadata.version + 1,
        pushed_at: now_str,
        machine_name: machine_name.to_string(),
        size_bytes: encrypted.len() as u64,
    };

    let mut new_slots = current_metadata.slots.clone();
    new_slots.insert(0, new_entry);
    new_slots.truncate(10);

    let new_metadata = SyncMetadata {
        version: current_metadata.version + 1,
        current_slot: slot,
        slots: new_slots,
    };

    let committed = client
        .write_metadata(&new_metadata, current_sha.as_deref())
        .await?;

    if !committed {
        // SHA conflict — caller should re-read and decide (pull first)
        return Ok(None);
    }

    Ok(Some((new_metadata.version, new_metadata)))
}

/// Download and apply a profile from GitHub slot. Returns written file names.
/// Caller must verify Zen is not running before calling.
pub async fn github_pull(client: &GitHubClient, slot: u8) -> Result<Vec<String>, String> {
    let encrypted = client.download_profile(slot).await?;
    let json = crate::crypto::decrypt(&encrypted, &hex_key(&client.encryption_key))?;

    let bundle: SyncBundle = serde_json::from_slice(&json)
        .map_err(|e| format!("Bundle format error: {e}"))?;

    if bundle.version != 1 {
        return Err(format!(
            "Unsupported bundle version {} — update Zync",
            bundle.version
        ));
    }

    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found.")?;

    write_bundle_files(&profile_dir, &bundle)
}

/// Remove old `auto_push` and `auto_pull` — replaced by `github_push` / `github_pull`.
/// (Delete the auto_push and auto_pull function bodies below this comment.)

fn hex_key(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

fn chrono_secs_to_iso(secs: u64) -> String {
    // Minimal ISO 8601 UTC without chrono dependency.
    // Accurate for dates 1970–2099.
    let s = secs;
    let (y, mo, d, h, mi, sec) = epoch_to_ymd_hms(s);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z")
}

fn epoch_to_ymd_hms(epoch: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (epoch % 60) as u32;
    let min = ((epoch / 60) % 60) as u32;
    let hour = ((epoch / 3600) % 24) as u32;
    let days = (epoch / 86400) as u32;
    // Days since 1970-01-01
    let mut y = 1970u32;
    let mut remaining = days;
    loop {
        let dy = if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) { 366 } else { 365 };
        if remaining < dy { break; }
        remaining -= dy;
        y += 1;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let month_days = [31u32, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut mo = 1u32;
    for &md in &month_days {
        if remaining < md { break; }
        remaining -= md;
        mo += 1;
    }
    (y, mo, remaining + 1, hour, min, sec)
}
```

- [ ] **Step 4: Remove auto_push and auto_pull from sync.rs**

Delete the `auto_push` function (lines 212–236) and `auto_pull` function (lines 244–267) from `src-tauri/src/sync.rs`. These are replaced by `github_push` and `github_pull`.

- [ ] **Step 5: Add a test for epoch_to_ymd_hms**

In the `#[cfg(test)]` block of `sync.rs`, add:

```rust
    #[test]
    fn epoch_to_iso_known_date() {
        // 2026-05-20T00:00:00Z = 1779235200
        let s = super::chrono_secs_to_iso(1779235200);
        assert_eq!(s, "2026-05-20T00:00:00Z");
    }
```

- [ ] **Step 6: Run tests**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test sync:: 2>&1 | tail -20
```

Expected: all sync tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/sync.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: add github_push and github_pull, remove auto_push/auto_pull"
```

---

## Task 8: `ntfy.rs` and `pairing.rs` Updates

**Files:**
- Modify: `src-tauri/src/ntfy.rs`
- Modify: `src-tauri/src/pairing.rs`

- [ ] **Step 1: Add parse_version_message test to ntfy.rs**

In `src-tauri/src/ntfy.rs`, add to the `#[cfg(test)]` block:

```rust
    #[test]
    fn parse_version_message_valid() {
        assert_eq!(super::parse_version_message("7"), Some(7));
        assert_eq!(super::parse_version_message("  7\n"), Some(7));
    }

    #[test]
    fn parse_version_message_invalid() {
        assert_eq!(super::parse_version_message("ABC123"), None);
        assert_eq!(super::parse_version_message(""), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd src-tauri && cargo test ntfy::tests::parse_version 2>&1 | tail -10
```

Expected: FAIL with "function not found".

- [ ] **Step 3: Add parse_version_message and update publish to ntfy.rs**

In `src-tauri/src/ntfy.rs`, add:

```rust
/// Parse a version number from an ntfy message body.
/// New format is a plain decimal integer (e.g. "7").
pub fn parse_version_message(msg: &str) -> Option<u32> {
    msg.trim().parse::<u32>().ok()
}

/// Publish a version number to the ntfy topic.
pub async fn publish_version(topic: &str, version: u32) -> Result<(), String> {
    publish(topic, &version.to_string()).await
}
```

- [ ] **Step 4: Run ntfy tests**

```bash
cd src-tauri && cargo test ntfy:: 2>&1 | tail -20
```

Expected: all ntfy tests pass including the two new ones.

- [ ] **Step 5: Gut pairing.rs**

Replace the entire contents of `src-tauri/src/pairing.rs` with:

```rust
use sha2::{Digest, Sha256};

/// Derive the ntfy.sh topic from an arbitrary string (the GitHub user ID).
/// Topic = lowercase hex of SHA-256(input). Never transmitted.
pub fn derive_ntfy_topic(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_is_deterministic() {
        assert_eq!(derive_ntfy_topic("12345678"), derive_ntfy_topic("12345678"));
    }

    #[test]
    fn topic_is_64_hex_chars() {
        let t = derive_ntfy_topic("12345678");
        assert_eq!(t.len(), 64);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn different_inputs_produce_different_topics() {
        assert_ne!(derive_ntfy_topic("111"), derive_ntfy_topic("222"));
    }
}
```

- [ ] **Step 6: Run pairing tests**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test pairing:: 2>&1 | tail -10
```

Expected: 3 pairing tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/ntfy.rs src-tauri/src/pairing.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: update ntfy to version-number messages; simplify pairing.rs"
```

---

## Task 9: `daemon.rs` — New DaemonState

**Files:**
- Modify: `src-tauri/src/daemon.rs`

- [ ] **Step 1: Write failing test for new DaemonState defaults**

In the `#[cfg(test)]` block of `daemon.rs`, replace or add:

```rust
    #[test]
    fn daemon_state_defaults() {
        let state = DaemonState::default();
        assert!(state.github_client.is_none());
        assert_eq!(state.local_state.last_known_version, 0);
        assert!(state.pending_version.is_none());
        assert!(state.last_synced.is_none());
        assert!(!state.zen_was_running);
    }
```

- [ ] **Step 2: Run to confirm it fails (old DaemonState)**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test daemon::tests 2>&1 | tail -10
```

Expected: FAIL (fields don't exist yet).

- [ ] **Step 3: Replace DaemonState and its Default impl**

Replace the entire `DaemonState` struct and `impl Default` in `src-tauri/src/daemon.rs` with:

```rust
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
```

Also update `SyncStatus` to match the new fields:

```rust
#[derive(serde::Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub connected: bool,
    pub username: Option<String>,
    pub last_synced: Option<u64>,
    pub last_synced_from: Option<String>,
    pub machine_name: String,
}
```

Update `get_sync_status_cmd`:

```rust
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
```

Remove `set_auto_push_cmd` and `set_auto_pull_cmd` entirely.

Remove `trigger_ntfy_poll_now` (no longer needed; caller-driven polling is gone).

- [ ] **Step 4: Run tests**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test daemon::tests 2>&1 | tail -10
```

Expected: `daemon_state_defaults` passes.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/daemon.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: simplify DaemonState for GitHub-based sync"
```

---

## Task 10: `daemon.rs` — New zen_watcher_tick (Version-Aware Push)

**Files:**
- Modify: `src-tauri/src/daemon.rs`

- [ ] **Step 1: Replace zen_watcher_tick**

Replace the existing `zen_watcher_tick` function in `daemon.rs` with:

```rust
async fn zen_watcher_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let (client, local_state, was_running, config_dir) = {
        let mut s = state.lock().unwrap();
        let client = match s.github_client.clone() {
            Some(c) => c,
            None => return,
        };
        let was = s.zen_was_running;
        let now_running = zen_check::is_zen_running();
        s.zen_was_running = now_running;
        (client, s.local_state.clone(), was, s.config_dir.clone())
    };

    let zen_running = zen_check::is_zen_running();

    if !was_running || zen_running {
        return; // Not a close edge
    }

    // Zen just closed — check if there's a pending pull first
    let pending = state.lock().unwrap().pending_version.take();

    if let Some(pending_ver) = pending {
        // A peer pushed while Zen was open. Always pull their data first.
        handle_pull(&client, pending_ver, app, state, &config_dir).await;
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
        // We're behind — pull instead of push
        is_syncing.store(false, Ordering::SeqCst);
        handle_pull(&client, github_version, app, state, &config_dir).await;
        return;
    }

    // Up to date — push
    let machine_name = state.lock().unwrap().local_state.machine_name.clone();
    match sync::github_push(&client, &machine_name, last_known).await {
        Ok(Some((new_version, _metadata))) => {
            let now = unix_now();
            // Publish to ntfy so other machines are notified
            let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());
            let _ = ntfy::publish_version(&topic, new_version).await;
            {
                let mut s = state.lock().unwrap();
                s.last_synced = Some(now);
                s.last_synced_from = None;
                s.local_state.last_known_version = new_version;
                let _ = s.local_state.save(&config_dir);
            }
            let _ = app.emit("sync-updated", get_status_payload(state));
            show_notification(app, "Profile synced.");
        }
        Ok(None) => {
            // Another machine pushed between our version check and commit — pull their data
            let current_ver = match client.read_metadata().await {
                Ok(Some(m)) => m.metadata.version,
                _ => last_known + 1,
            };
            handle_pull(&client, current_ver, app, state, &config_dir).await;
        }
        Err(e) => show_notification(app, &format!("Auto-push failed: {e}")),
    }

    is_syncing.store(false, Ordering::SeqCst);
}

async fn handle_pull(
    client: &crate::github::GitHubClient,
    version: u32,
    app: &tauri::AppHandle,
    state: &Arc<Mutex<DaemonState>>,
    config_dir: &std::path::Path,
) {
    // Get the current slot for this version by reading metadata
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

    match sync::github_pull(client, slot).await {
        Ok(_) => {
            let now = unix_now();
            {
                let mut s = state.lock().unwrap();
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
        Err(e) => show_notification(app, &format!("Auto-pull failed: {e}")),
    }
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
```

- [ ] **Step 2: Replace ntfy_poll_tick**

Replace the existing `ntfy_poll_tick` function with:

```rust
async fn ntfy_poll_tick(app: &tauri::AppHandle, state: &Arc<Mutex<DaemonState>>) {
    let (client, since, config_dir) = {
        let s = state.lock().unwrap();
        let client = match s.github_client.clone() { Some(c) => c, None => return };
        let since = s.last_ntfy_id.clone()
            .unwrap_or_else(|| s.last_poll_time.to_string());
        (client, since, s.config_dir.clone())
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

    // Skip if we already have this version
    let last_known = state.lock().unwrap().local_state.last_known_version;
    if version <= last_known {
        return;
    }

    let zen_running = zen_check::is_zen_running();
    if zen_running {
        state.lock().unwrap().pending_version = Some(version);
        show_notification(app, "New profile available — will sync when Zen closes");
    } else {
        handle_pull(&client, version, app, state, &config_dir).await;
    }
}
```

- [ ] **Step 3: Remove the refresh_tick function and the refresh loop in start()**

Delete the `refresh_tick` function and the "Refresh timer (checked every 5 min)" block inside `start()`.

- [ ] **Step 3b: Replace manual_sync_now_cmd**

The existing `manual_sync_now_cmd` references `passphrase` and `trigger_push` which are both gone. Replace it with:

```rust
#[tauri::command]
pub async fn manual_sync_now_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<DaemonState>>>,
) -> Result<(), String> {
    if zen_check::is_zen_running() {
        return Err("Zen is running — close it before syncing".into());
    }
    let (client, machine_name, last_known, config_dir) = {
        let s = state.lock().unwrap();
        let c = s.github_client.clone()
            .ok_or("Not connected to GitHub — set up sync in the Sync tab first")?;
        (c, s.local_state.machine_name.clone(), s.local_state.last_known_version, s.config_dir.clone())
    };

    match sync::github_push(&client, &machine_name, last_known).await {
        Ok(Some((new_version, _))) => {
            let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());
            let _ = ntfy::publish_version(&topic, new_version).await;
            let mut s = state.lock().unwrap();
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
```

- [ ] **Step 4: Verify compilation**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo check 2>&1 | grep "^error" | head -30
```

Fix any compilation errors from removed fields or changed types.

- [ ] **Step 5: Run all tests**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/daemon.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: version-aware push flow in daemon; remove refresh loop"
```

---

## Task 11: `lib.rs` — New Commands and Startup Wiring

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add new commands to lib.rs**

Add these commands to `src-tauri/src/lib.rs`, replacing the removed passphrase commands:

```rust
#[tauri::command]
async fn connect_github_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<daemon::SyncStatus, String> {
    let client = github::GitHubClient::connect(&app).await?;
    let status = {
        let mut s = state.lock().unwrap();
        s.github_client = Some(Arc::new(client));
        daemon::get_sync_status_cmd(state.clone())
    };
    Ok(status)
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

    // Pull the selected snapshot locally
    sync::github_pull(&client, slot).await?;

    // Push it as a new version so other machines get it
    match sync::github_push(&client, &machine_name, last_known).await {
        Ok(Some((new_version, _))) => {
            let topic = pairing::derive_ntfy_topic(&client.user_id.to_string());
            let _ = ntfy::publish_version(&topic, new_version).await;
            {
                let mut s = state.lock().unwrap();
                s.local_state.last_known_version = new_version;
                s.last_synced = Some(std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs());
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
```

- [ ] **Step 2: Update the invoke_handler**

Replace the `invoke_handler` block in `lib.rs`:

```rust
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
```

- [ ] **Step 3: Update the setup block — startup GitHub restoration and first-run window logic**

Replace the section that loads the passphrase in `setup()`:

```rust
// Replace this block:
// let data_dir = app.path().app_data_dir().unwrap_or_default();
// if !pairing::is_paired_flag(&data_dir) { window.show()... }
// ...populate passphrase cache...

// With:
let config_dir = app.path().app_config_dir().unwrap_or_default();
{
    let mut s = state_for_cache.lock().unwrap();
    s.config_dir = config_dir.clone();
    s.local_state = crate::local_state::LocalState::load(&config_dir);
}

// Show window on first run (no GitHub token in keychain)
if !github::has_stored_token() {
    window.show().unwrap();
    let _ = window.set_focus();
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
```

- [ ] **Step 4: Remove pairing module usage from lib.rs**

Remove the import of `pairing` where it's used for flags and passphrase (the derivation function is now only used in daemon.rs). Check that `mod pairing` is still declared (it is — it still contains `derive_ntfy_topic`).

- [ ] **Step 5: Verify compilation**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo check 2>&1 | grep "^error" | head -30
```

Fix any compilation errors.

- [ ] **Step 6: Run all tests**

```bash
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cd src-tauri && cargo test 2>&1 | tail -20
```

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/lib.rs
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: add GitHub commands and update startup wiring"
```

---

## Task 12: UI — Sync Tab (All Three States)

**Files:**
- Modify: `src/index.html`
- Modify: `src/main.js`
- Modify: `src/style.css` (minor additions)

- [ ] **Step 1: Update the tab button in index.html**

In `src/index.html`, find the tab button for "Pair" and rename it to "Sync":

```html
<!-- Change this: -->
<button class="tab-btn" data-tab="pair">Pair</button>

<!-- To this: -->
<button class="tab-btn" data-tab="sync">Sync</button>
```

Find the Pair tab panel (`<div id="pair" class="tab-panel">` or similar) and rename its `id` to `sync`. Replace its contents with the three-state Sync tab markup:

```html
<div id="sync" class="tab-panel">

  <!-- State: not connected -->
  <div id="sync-disconnected">
    <p class="sync-intro">Connect a GitHub account to enable automatic sync and version history across your machines.</p>
    <button id="btn-connect-github" class="btn-primary">Connect GitHub</button>
    <p id="sync-connect-error" class="error-msg"></p>
  </div>

  <!-- State: connected (main view) -->
  <div id="sync-connected" style="display:none">
    <p id="sync-account-label" class="sync-account"></p>
    <p id="sync-repo-label" class="sync-repo-name"></p>

    <label class="field-label" for="input-machine-name">Machine name</label>
    <input id="input-machine-name" class="text-input" type="text" placeholder="My Machine" />

    <p id="sync-last-synced" class="sync-status-text"></p>

    <button id="btn-restore-version" class="btn-secondary">Restore Previous Version</button>
    <p id="sync-main-error" class="error-msg"></p>
    <button id="btn-disconnect" class="btn-danger-link">Disconnect</button>
  </div>

  <!-- State: rollback view -->
  <div id="sync-rollback" style="display:none">
    <button id="btn-back-rollback" class="btn-back">← Back</button>
    <hr class="divider" />
    <p class="rollback-description">Restoring replaces your current profile. Your current version is saved as a snapshot first, so you can always come back to it.</p>
    <hr class="divider" />
    <div id="snapshot-list"></div>
    <p id="rollback-error" class="error-msg"></p>
  </div>

</div>
```

- [ ] **Step 2: Add CSS for new elements**

In `src/style.css`, append:

```css
.sync-intro { color: var(--text-secondary); margin-bottom: 16px; }
.sync-account { font-weight: 600; margin-bottom: 2px; }
.sync-repo-name { color: var(--text-secondary); font-size: 12px; margin-bottom: 16px; }
.sync-status-text { color: var(--text-secondary); font-size: 12px; margin: 12px 0; }
.rollback-description { color: var(--text-secondary); font-size: 13px; margin: 8px 0; }
.btn-back { background: none; border: none; color: var(--accent); cursor: pointer; padding: 0; font-size: 13px; }
.btn-danger-link { background: none; border: none; color: var(--text-muted, #888); cursor: pointer; font-size: 12px; margin-top: 16px; }
.divider { border: none; border-top: 1px solid var(--border); margin: 8px 0; }
.snapshot-row { display: flex; align-items: center; padding: 8px 0; border-bottom: 1px solid var(--border); gap: 8px; }
.snapshot-row .snapshot-dot { width: 8px; height: 8px; border-radius: 50%; background: var(--accent); flex-shrink: 0; }
.snapshot-row .snapshot-dot.hidden { visibility: hidden; }
.snapshot-info { flex: 1; }
.snapshot-machine { font-size: 13px; font-weight: 500; }
.snapshot-date { font-size: 11px; color: var(--text-secondary); }
.snapshot-size { font-size: 11px; color: var(--text-muted, #888); }
.btn-restore { padding: 4px 10px; font-size: 12px; }
```

- [ ] **Step 3: Add Sync tab JavaScript to main.js**

In `src/main.js`, add the Sync tab logic. Locate where the old Pair tab logic was and replace it with:

```javascript
// ── Sync tab ──────────────────────────────────────────────────────────────────

const syncDisconnected = document.getElementById('sync-disconnected');
const syncConnected    = document.getElementById('sync-connected');
const syncRollback     = document.getElementById('sync-rollback');

function showSyncState(state) {
  syncDisconnected.style.display = state === 'disconnected' ? '' : 'none';
  syncConnected.style.display    = state === 'connected'    ? '' : 'none';
  syncRollback.style.display     = state === 'rollback'     ? '' : 'none';
}

async function loadSyncStatus() {
  try {
    const status = await invoke('get_sync_status_cmd');
    if (status.connected) {
      document.getElementById('sync-account-label').textContent = `✓ Connected as ${status.username}`;
      document.getElementById('sync-repo-label').textContent    = 'zync-sync · private';
      document.getElementById('input-machine-name').value       = status.machineName || '';
      if (status.lastSynced) {
        const d = new Date(status.lastSynced * 1000);
        const from = status.lastSyncedFrom ? ` from ${status.lastSyncedFrom}` : '';
        document.getElementById('sync-last-synced').textContent =
          `Last synced: ${d.toLocaleString()}${from}`;
      } else {
        document.getElementById('sync-last-synced').textContent = 'Not yet synced this session';
      }
      showSyncState('connected');
    } else {
      showSyncState('disconnected');
    }
  } catch (e) {
    showSyncState('disconnected');
  }
}

document.getElementById('btn-connect-github').addEventListener('click', async () => {
  const btn = document.getElementById('btn-connect-github');
  const err = document.getElementById('sync-connect-error');
  btn.disabled = true;
  btn.textContent = 'Connecting…';
  err.textContent = '';
  try {
    await invoke('connect_github_cmd');
    await loadSyncStatus();
  } catch (e) {
    err.textContent = e;
    btn.disabled = false;
    btn.textContent = 'Connect GitHub';
  }
});

document.getElementById('btn-disconnect').addEventListener('click', async () => {
  try {
    await invoke('disconnect_github_cmd');
    showSyncState('disconnected');
  } catch (e) {
    document.getElementById('sync-main-error').textContent = e;
  }
});

document.getElementById('input-machine-name').addEventListener('change', async (ev) => {
  try {
    await invoke('set_machine_name_cmd', { name: ev.target.value });
  } catch (e) {
    document.getElementById('sync-main-error').textContent = e;
  }
});

document.getElementById('btn-restore-version').addEventListener('click', async () => {
  const err = document.getElementById('sync-main-error');
  err.textContent = '';
  try {
    const snapshots = await invoke('get_snapshots_cmd');
    renderSnapshotList(snapshots);
    showSyncState('rollback');
  } catch (e) {
    err.textContent = e;
  }
});

document.getElementById('btn-back-rollback').addEventListener('click', () => {
  showSyncState('connected');
});

function renderSnapshotList(snapshots) {
  const list = document.getElementById('snapshot-list');
  list.innerHTML = '';
  snapshots.forEach(snap => {
    const row = document.createElement('div');
    row.className = 'snapshot-row';

    const dot = document.createElement('span');
    dot.className = snap.isCurrent ? 'snapshot-dot' : 'snapshot-dot hidden';
    row.appendChild(dot);

    const info = document.createElement('div');
    info.className = 'snapshot-info';
    const machine = document.createElement('div');
    machine.className = 'snapshot-machine';
    machine.textContent = snap.machineName;
    const date = document.createElement('div');
    date.className = 'snapshot-date';
    date.textContent = new Date(snap.pushedAt).toLocaleString();
    const size = document.createElement('div');
    size.className = 'snapshot-size';
    size.textContent = `${snap.sizeMb.toFixed(1)} MB`;
    info.appendChild(machine);
    info.appendChild(date);
    info.appendChild(size);
    row.appendChild(info);

    if (!snap.isCurrent) {
      const btn = document.createElement('button');
      btn.className = 'btn-secondary btn-restore';
      btn.textContent = 'Restore';
      btn.addEventListener('click', async () => {
        btn.disabled = true;
        btn.textContent = 'Restoring…';
        document.getElementById('rollback-error').textContent = '';
        try {
          await invoke('restore_snapshot_cmd', { slot: snap.slot });
          showSyncState('connected');
          await loadSyncStatus();
        } catch (e) {
          document.getElementById('rollback-error').textContent = e;
          btn.disabled = false;
          btn.textContent = 'Restore';
        }
      });
      row.appendChild(btn);
    }

    list.appendChild(row);
  });
}

// Listen for sync updates from daemon
window.__TAURI__.event.listen('sync-updated', () => {
  if (syncConnected.style.display !== 'none') {
    loadSyncStatus();
  }
});

// Load status when Sync tab is activated
document.querySelectorAll('.tab-btn').forEach(btn => {
  btn.addEventListener('click', () => {
    if (btn.dataset.tab === 'sync') loadSyncStatus();
  });
});
```

- [ ] **Step 4: Build and verify the UI manually**

```bash
cd src-tauri && GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy cargo tauri dev
```

Open the app. Go to the Sync tab. Verify:
- Without a GitHub token in keychain: "not connected" state shows with Connect GitHub button
- Clicking Connect GitHub opens a browser (it will fail with dummy credentials — confirm the browser opens)
- Tab label reads "Sync" not "Pair"
- Clicking Restore Previous Version (once connected) shows the rollback view
- Back button returns to main connected view

- [ ] **Step 5: Commit**

```bash
git add src/index.html src/main.js src/style.css
GITHUB_CLIENT_ID=dummy GITHUB_CLIENT_SECRET=dummy git commit -m "feat: Sync tab with connected/rollback states"
```

---

## Task 13: GitHub OAuth App Setup and Build Configuration

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `src-tauri/tauri.conf.json` (if needed for env vars)

- [ ] **Step 1: Register a GitHub OAuth App**

Go to `https://github.com/settings/developers` → "OAuth Apps" → "New OAuth App":
- Application name: `Zync`
- Homepage URL: `https://github.com/jessewallace/zync` (or your repo URL)
- Authorization callback URL: `http://127.0.0.1` (tauri-plugin-oauth uses a random port; GitHub accepts the base URL)
- Click "Register application"
- Note the **Client ID** and generate a **Client secret**

- [ ] **Step 2: Add secrets to GitHub Actions**

In your GitHub repo → Settings → Secrets and variables → Actions → New repository secret:
- `GITHUB_CLIENT_ID` = the Client ID from step 1
- `GITHUB_CLIENT_SECRET` = the Client Secret from step 1

- [ ] **Step 3: Update release.yml to pass secrets as env vars**

In `.github/workflows/release.yml`, find the build step(s) and add env vars:

```yaml
- name: Build
  env:
    GITHUB_CLIENT_ID: ${{ secrets.GITHUB_CLIENT_ID }}
    GITHUB_CLIENT_SECRET: ${{ secrets.GITHUB_CLIENT_SECRET }}
  run: cargo tauri build
```

Do this for each platform's build step.

- [ ] **Step 4: Verify local dev build works**

```bash
GITHUB_CLIENT_ID=your_real_client_id GITHUB_CLIENT_SECRET=your_real_client_secret cd src-tauri && cargo tauri dev
```

Go to the Sync tab → Connect GitHub → browser opens → authorize → app connects.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: pass GitHub OAuth credentials to build steps"
```

---

## Task 14: End-to-End Verification

No new code — this task verifies the full sync flow works correctly across the two failure modes from the spec.

- [ ] **Step 1: Verify initial push flow**

On Machine A (with GitHub connected and Zen installed):
1. Close Zen
2. Verify Zync pushes automatically (notification: "Profile synced.")
3. Check GitHub: `zync-sync` repo exists, `storage` release has `profile-0.enc` and `metadata.json`
4. Open `metadata.json` in GitHub UI: `version` should be 1, `current_slot` should be 0

- [ ] **Step 2: Verify initial pull on Machine B**

On Machine B (same GitHub account, connected):
1. Verify Zync pulls Machine A's profile on startup or Zen close (notification: "Profile updated from Machine A")
2. Open Zen on Machine B — confirm Machine A's bookmarks/workspaces are present

- [ ] **Step 3: Verify the previously broken scenario is now fixed**

1. Push from Machine A (version becomes 2)
2. Open Zen on Machine B while Machine A's push arrives (ntfy queues `pending_version`)
3. Use Zen on Machine B briefly (don't close yet)
4. Close Zen on Machine B
5. Expected: Machine B PULLS Machine A's data, NOT pushes its own
6. Notification: "Machine A pushed a profile while Zen was open. Their profile has been applied. Your session's changes are saved as a snapshot."
7. Check GitHub: version is still 2 (Machine B did not push)

- [ ] **Step 4: Verify rollback**

1. In the Sync tab, click "Restore Previous Version"
2. Confirm the snapshot list shows up to 10 versions with machine names and timestamps
3. Click Restore on a previous version
4. Confirm Zen picks up the restored profile on next open
5. Confirm GitHub `metadata.json` version incremented (rollback is a new push)

- [ ] **Step 5: Tag and release**

Once all verification passes:

```bash
git tag v0.4.0
git push && git push --tags
```

---

## Environment Variables Reference

| Variable | Where | Value |
|---|---|---|
| `GITHUB_CLIENT_ID` | Build env / CI secret | From GitHub OAuth App settings |
| `GITHUB_CLIENT_SECRET` | Build env / CI secret | From GitHub OAuth App settings |

For local dev:
```bash
export GITHUB_CLIENT_ID=your_client_id
export GITHUB_CLIENT_SECRET=your_client_secret
cd src-tauri && cargo tauri dev
```
