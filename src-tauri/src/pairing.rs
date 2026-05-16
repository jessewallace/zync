use sha2::{Digest, Sha256};
use std::path::Path;

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

pub fn is_paired_flag(dir: &Path) -> bool {
    dir.join("paired.flag").exists()
}

pub fn write_paired_flag(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create data dir: {e}"))?;
    std::fs::write(dir.join("paired.flag"), b"")
        .map_err(|e| format!("Failed to write paired flag: {e}"))
}

pub fn clear_paired_flag(dir: &Path) -> Result<(), String> {
    let path = dir.join("paired.flag");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to clear paired flag: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paired_flag_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_paired_flag(dir.path()));
        write_paired_flag(dir.path()).unwrap();
        assert!(is_paired_flag(dir.path()));
        clear_paired_flag(dir.path()).unwrap();
        assert!(!is_paired_flag(dir.path()));
    }

    #[test]
    fn clear_flag_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        assert!(clear_paired_flag(dir.path()).is_ok());
    }

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
