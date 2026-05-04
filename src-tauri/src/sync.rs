use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::{crypto, profile, transport};

const MAX_FILE_BYTES: usize = 5 * 1024 * 1024; // 5 MB

/// In-memory payload that is serialized → encrypted → uploaded.
/// File contents are base64-encoded to survive JSON round-trips.
#[derive(Serialize, Deserialize)]
struct SyncBundle {
    version: u8,
    created_at: u64,
    /// filename → base64-encoded bytes
    files: HashMap<String, String>,
}

/// Export the current Zen profile, encrypt it, upload to Litterbox,
/// and return the sync code (e.g. `ZEN-A3F9B2-ABC123`).
///
/// The caller (JS) must verify Zen is not running before invoking.
#[tauri::command]
pub async fn push_profile() -> Result<String, String> {
    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found. Is Zen Browser installed?")?;

    // Collect sync files
    let mut files = HashMap::new();
    for &name in profile::SYNC_FILES {
        let path = profile_dir.join(name);
        if !path.exists() {
            continue;
        }
        let bytes =
            std::fs::read(&path).map_err(|e| format!("Could not read {name}: {e}"))?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(format!("{name} exceeds the 5 MB per-file limit"));
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

    // 3 random bytes → 6 uppercase hex chars (the encryption key half of the code).
    // 2^24 × 100k PBKDF2 rounds ≈ 11 h to brute-force on a fast GPU,
    // well beyond the 1-hour Litterbox expiry window.
    let mut key_raw = [0u8; 3];
    rand::thread_rng().fill_bytes(&mut key_raw);
    let key_hex = format!(
        "{:02X}{:02X}{:02X}",
        key_raw[0], key_raw[1], key_raw[2]
    );

    let encrypted = crypto::encrypt(&json, &key_hex)?;
    let result = transport::upload(encrypted).await?;

    transport::url_to_sync_code(&result.url, &key_hex)
        .ok_or_else(|| format!("Could not parse Litterbox URL: {}", result.url))
}

/// Fetch and decrypt a profile bundle by sync code, back up the current
/// profile files, then write the synced files in place.
/// Returns the sorted list of file names that were written.
///
/// The caller (JS) must verify Zen is not running before invoking.
#[tauri::command]
pub async fn pull_profile(sync_code: String) -> Result<Vec<String>, String> {
    let (key_hex, url) = transport::parse_sync_code(&sync_code)
        .ok_or("Invalid sync code — expected format ZEN-XXXXXX-YYYYYY")?;

    let encrypted = transport::download(&url).await?;
    let json = crypto::decrypt(&encrypted, &key_hex)?;

    let bundle: SyncBundle = serde_json::from_slice(&json)
        .map_err(|e| format!("Bundle format error: {e}"))?;

    if bundle.version != 1 {
        return Err(format!(
            "Unsupported bundle version {} — update ZynC to pull this profile",
            bundle.version
        ));
    }

    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found. Is Zen Browser installed?")?;

    let file_names: Vec<String> = bundle.files.keys().cloned().collect();
    backup_profile(&profile_dir, &file_names)?;

    let mut written = Vec::new();
    for (name, b64) in &bundle.files {
        let bytes = BASE64
            .decode(b64)
            .map_err(|e| format!("Failed to decode {name}: {e}"))?;
        std::fs::write(profile_dir.join(name), &bytes)
            .map_err(|e| format!("Failed to write {name}: {e}"))?;
        written.push(name.clone());
    }

    written.sort();
    Ok(written)
}

/// Copy the listed profile files into a timestamped backup folder.
/// Silent no-op for files that don't exist yet.
fn backup_profile(profile_dir: &Path, file_names: &[String]) -> Result<(), String> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_dir = profile_dir.join(format!("zync-backup-{ts}"));
    std::fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("Failed to create backup directory: {e}"))?;
    for name in file_names {
        let src = profile_dir.join(name);
        if src.exists() {
            std::fs::copy(&src, backup_dir.join(name))
                .map_err(|e| format!("Failed to backup {name}: {e}"))?;
        }
    }
    Ok(())
}
