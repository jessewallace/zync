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
