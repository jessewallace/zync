use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::{Deserialize, Serialize};

const KEYCHAIN_SERVICE: &str = "zync";
const KEYCHAIN_ACCOUNT: &str = "github";

#[derive(Serialize, Deserialize)]
struct StoredCredentials {
    token: String,
    user_id: u64,
    username: String,
}

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
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .ok()
        .and_then(|e| e.get_password().ok())
        .is_some()
}

fn save_token(token: &str, user_id: u64, username: &str) -> Result<(), String> {
    let creds = StoredCredentials { token: token.to_string(), user_id, username: username.to_string() };
    let json = serde_json::to_string(&creds).map_err(|e| format!("Credential serialize error: {e}"))?;
    keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| format!("Keychain error: {e}"))?
        .set_password(&json)
        .map_err(|e| format!("Failed to save credentials: {e}"))
}

pub fn remove_stored_token() -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .map_err(|e| format!("Keychain error: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Failed to remove credentials: {e}")),
    }
}

fn load_token_parts() -> Option<(String, u64, String)> {
    let json = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT)
        .ok()?
        .get_password()
        .ok()?;
    let creds: StoredCredentials = serde_json::from_str(&json).ok()?;
    Some((creds.token, creds.user_id, creds.username))
}

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
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;
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

    /// Returns `true` if the repo was just created (caller should wait for init).
    async fn ensure_repo(&self) -> Result<bool, String> {
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
                    "description": "Zync profile storage — do not modify",
                    "auto_init": true
                }))
                .send()
                .await
                .map_err(|e| format!("Repo create failed: {e}"))?
                .error_for_status()
                .map_err(|e| format!("Repo create error: {e}"))?;
            return Ok(true); // newly created — caller must wait for GitHub to init
        }
        Ok(false)
    }

    /// Poll until the repo's default branch has at least one commit.
    /// GitHub initialises repos asynchronously after creation; creating a
    /// release (or uploading assets) before that completes causes the
    /// uploads.github.com endpoint to stall indefinitely with no timeout.
    async fn wait_for_repo_ready(&self) -> Result<(), String> {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Branch { name: String }

        for attempt in 0..10u32 {
            tokio::time::sleep(std::time::Duration::from_millis(500 * (1 << attempt.min(4)))).await;

            let resp = self.http
                .get(format!("{API_BASE}/repos/{}/{REPO_NAME}/branches", self.username))
                .headers(self.api_headers())
                .send()
                .await
                .map_err(|e| format!("Repo-ready check failed: {e}"))?;

            if resp.status().is_success() {
                let branches: Vec<Branch> = resp.json().await
                    .map_err(|e| format!("Repo-ready parse error: {e}"))?;
                if !branches.is_empty() {
                    return Ok(());
                }
            }
        }
        Err("Timed out waiting for GitHub to initialise the new repository".to_string())
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
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;
        let mut client = GitHubClient {
            token, user_id, username, release_id: 0,
            encryption_key: [0u8; 32], http,
        };
        let repo_created = client.ensure_repo().await?;
        if repo_created {
            // GitHub initialises the repo asynchronously after creation.  Wait
            // for the default branch commit to exist before creating a release,
            // otherwise the uploads.github.com endpoint stalls indefinitely.
            client.wait_for_repo_ready().await?;
        }
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
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("HTTP client error: {e}"))?;
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

    // ── Metadata (Contents API — SHA-locked) ─────────────────────────────────────

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
