// AES-256-GCM encryption with PBKDF2-HMAC-SHA256 key derivation.
//
// Wire format: [12-byte nonce][ciphertext+GCM tag]
//
// The sync code KEY half (e.g. "A3F9B2" from "ZEN-A3F9B2-abc123") is used
// as the PBKDF2 password. The app salt prevents rainbow tables across apps.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm,
};
use generic_array::GenericArray;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;

const APP_SALT: &[u8] = b"zync-zen-sync-v1-salt";
const PBKDF2_ROUNDS: u32 = 100_000;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

fn derive_key(key_hex: &str) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(key_hex.as_bytes(), APP_SALT, PBKDF2_ROUNDS, &mut key);
    key
}

/// Encrypt `data` using AES-256-GCM. Returns `[nonce (12 B) || ciphertext+tag]`.
pub fn encrypt(data: &[u8], key_hex: &str) -> Result<Vec<u8>, String> {
    let key_bytes = derive_key(key_hex);
    let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes));

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    // Type of GenericArray size is inferred from cipher.encrypt signature
    let nonce = GenericArray::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| format!("Encryption error: {e}"))?;

    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt data produced by `encrypt`. Expects `[nonce (12 B) || ciphertext+tag]`.
pub fn decrypt(data: &[u8], key_hex: &str) -> Result<Vec<u8>, String> {
    if data.len() < NONCE_LEN {
        return Err("Payload too short — not a valid Zync bundle".into());
    }
    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);

    let key_bytes = derive_key(key_hex);
    let cipher = Aes256Gcm::new(aes_gcm::Key::<Aes256Gcm>::from_slice(&key_bytes));
    let nonce = GenericArray::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| "Decryption failed — wrong sync code or corrupted bundle".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let plaintext = b"hello from zen sync";
        let key = "A3F9B2";
        let ciphertext = encrypt(plaintext, key).expect("encrypt failed");
        assert_ne!(&ciphertext[NONCE_LEN..], plaintext); // actually encrypted
        let recovered = decrypt(&ciphertext, key).expect("decrypt failed");
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let ciphertext = encrypt(b"secret data", "AAAAAA").expect("encrypt failed");
        assert!(decrypt(&ciphertext, "BBBBBB").is_err());
    }

    #[test]
    fn truncated_payload_fails() {
        assert!(decrypt(&[0u8; 4], "AAAAAA").is_err());
    }
}
