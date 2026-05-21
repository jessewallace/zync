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
