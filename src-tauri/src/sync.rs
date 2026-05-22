use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::{crypto, profile, transport};
use crate::github::{GitHubClient, SyncMetadata, SlotEntry};

/// In-memory payload that is serialized → encrypted → uploaded.
/// File contents are base64-encoded to survive JSON round-trips.
#[derive(Serialize, Deserialize)]
struct SyncBundle {
    version: u8,
    created_at: u64,
    /// filename → base64-encoded bytes
    files: HashMap<String, String>,
}

fn checkpoint_wal(db_path: &Path) -> Result<(), String> {
    let conn = rusqlite::Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE,
    )
    .map_err(|e| format!("Could not open {}: {e}", db_path.display()))?;
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        .map_err(|e| format!("WAL checkpoint failed: {e}"))
}

/// Strip machine-local update state from prefs.js before syncing.
/// Both `app.update.*` (Firefox update timers/state) and `zen.updates.*`
/// (Zen's own version tracking) must be excluded — syncing them across machines
/// causes the receiving machine to show "Update Ready" or trigger an immediate
/// update check because the version recorded in prefs doesn't match the binary.
fn strip_update_prefs(content: &str) -> String {
    content
        .lines()
        .filter(|line| {
            let t = line.trim_start();
            !t.starts_with("user_pref(\"app.update.")
                && !t.starts_with("user_pref(\"zen.updates.")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn collect_sync_files(profile_dir: &std::path::Path) -> Result<HashMap<String, String>, String> {
    let mut files = HashMap::new();
    for &name in profile::SYNC_FILES {
        let path = profile_dir.join(name);
        if !path.exists() {
            continue;
        }
        let bytes = if name == "prefs.js" {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("Could not read {name}: {e}"))?;
            strip_update_prefs(&content).into_bytes()
        } else {
            std::fs::read(&path)
                .map_err(|e| format!("Could not read {name}: {e}"))?
        };
        files.insert(name.to_string(), BASE64.encode(&bytes));
    }
    if files.is_empty() {
        return Err("No sync files found in the Zen profile folder".into());
    }
    Ok(files)
}

fn write_bundle_files(
    profile_dir: &std::path::Path,
    bundle: &SyncBundle,
) -> Result<Vec<String>, String> {
    let file_names: Vec<String> = bundle.files.keys().cloned().collect();
    backup_profile(profile_dir, &file_names)?;
    let mut written = Vec::new();
    for (name, b64) in &bundle.files {
        let bytes = BASE64
            .decode(b64)
            .map_err(|e| format!("Failed to decode {name}: {e}"))?;
        let dest = profile_dir.join(name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory for {name}: {e}"))?;
        }
        // For prefs.js: preserve the local machine's app.update.* lines so that
        // machine-local Zen update state (downloaded update path, dialog shown flag)
        // isn't erased when the synced prefs.js is applied. The incoming bundle
        // already has app.update.* stripped on push; appending the local lines
        // ensures Zen doesn't re-prompt for an update the user already accepted.
        let final_bytes = if name == "prefs.js" {
            let local_update_lines: Vec<String> = if dest.exists() {
                std::fs::read_to_string(&dest)
                    .unwrap_or_default()
                    .lines()
                    .filter(|l| {
                        let t = l.trim_start();
                        t.starts_with("user_pref(\"app.update.")
                            || t.starts_with("user_pref(\"zen.updates.")
                    })
                    .map(String::from)
                    .collect()
            } else {
                Vec::new()
            };
            if local_update_lines.is_empty() {
                bytes
            } else {
                let synced = String::from_utf8_lossy(&bytes);
                format!("{}\n{}\n", synced.trim_end(), local_update_lines.join("\n")).into_bytes()
            }
        } else {
            bytes
        };
        std::fs::write(&dest, &final_bytes)
            .map_err(|e| format!("Failed to write {name}: {e}"))?;
        // Remove stale WAL/SHM after writing SQLite files. If a leftover WAL from
        // the destination's previous session shares page numbers with the new db,
        // SQLite may apply it on open and partially overwrite the synced data.
        if name.ends_with(".sqlite") {
            let _ = std::fs::remove_file(profile_dir.join(format!("{name}-wal")));
            let _ = std::fs::remove_file(profile_dir.join(format!("{name}-shm")));
        }
        written.push(name.clone());
    }
    written.sort();
    Ok(written)
}

/// Export the current Zen profile, encrypt it, upload to Litterbox,
/// and return the sync code (e.g. `ZEN-A3F9B2-ABC123`).
///
/// The caller (JS) must verify Zen is not running before invoking.
#[tauri::command]
pub async fn push_profile() -> Result<String, String> {
    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found. Is Zen Browser installed?")?;

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
    eprintln!("[zync] pull: downloading from {url}");

    let encrypted = transport::download(&url).await?;
    eprintln!("[zync] pull: downloaded {} bytes", encrypted.len());

    let json = crypto::decrypt(&encrypted, &key_hex)?;
    eprintln!("[zync] pull: decrypted {} bytes", json.len());

    let bundle: SyncBundle = serde_json::from_slice(&json)
        .map_err(|e| format!("Bundle format error: {e}"))?;
    eprintln!("[zync] pull: bundle v{}, {} files: {:?}",
        bundle.version,
        bundle.files.len(),
        bundle.files.keys().collect::<Vec<_>>()
    );

    if bundle.version != 1 {
        return Err(format!(
            "Unsupported bundle version {} — update Zync to pull this profile",
            bundle.version
        ));
    }

    let profile_dir = profile::find_zen_profile()
        .ok_or("Zen profile folder not found. Is Zen Browser installed?")?;
    eprintln!("[zync] pull: writing to {}", profile_dir.display());

    let written = write_bundle_files(&profile_dir, &bundle)?;
    eprintln!("[zync] pull: wrote {:?}", written);
    Ok(written)
}

/// Push the current Zen profile to GitHub. Returns (new_version, new_metadata).
/// Returns Ok(None) if version conflict (another machine pushed — caller should pull).
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

    let now_str = chrono_secs_to_iso(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

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
        // SHA conflict — caller should pull first
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

fn hex_key(key: &[u8; 32]) -> String {
    key.iter().map(|b| format!("{b:02x}")).collect()
}

fn chrono_secs_to_iso(secs: u64) -> String {
    let (y, mo, d, h, mi, sec) = epoch_to_ymd_hms(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{sec:02}Z")
}

fn epoch_to_ymd_hms(epoch: u64) -> (u32, u32, u32, u32, u32, u32) {
    let sec = (epoch % 60) as u32;
    let min = ((epoch / 60) % 60) as u32;
    let hour = ((epoch / 3600) % 24) as u32;
    let days = (epoch / 86400) as u32;
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
            let dest = backup_dir.join(name);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create backup directory for {name}: {e}"))?;
            }
            std::fs::copy(&src, dest)
                .map_err(|e| format!("Failed to backup {name}: {e}"))?;
        }
    }
    Ok(())
}

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
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute_batch("CREATE TABLE t (x INTEGER);").unwrap();
        drop(conn);
        assert!(checkpoint_wal(&db_path).is_ok());
    }

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

    #[test]
    fn epoch_to_iso_known_date() {
        // 2026-05-20T00:00:00Z = 1779235200
        let s = super::chrono_secs_to_iso(1779235200);
        assert_eq!(s, "2026-05-20T00:00:00Z");
    }
}
