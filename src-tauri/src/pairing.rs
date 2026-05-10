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
    match load_passphrase() {
        Ok(p) => p.is_some(),
        Err(e) => {
            eprintln!("Zync: keychain read failed: {e}");
            false
        }
    }
}

#[tauri::command]
pub fn clear_passphrase_cmd() -> Result<(), String> {
    clear_passphrase()
}

#[tauri::command]
pub fn get_passphrase_cmd() -> Result<Option<String>, String> {
    load_passphrase()
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
