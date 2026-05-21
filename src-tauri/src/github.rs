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
