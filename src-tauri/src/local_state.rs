use serde::{Deserialize, Serialize};
use std::path::Path;

const STATE_FILE: &str = "state.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LocalState {
    pub last_known_version: u32,
    pub machine_name: String,
    pub selected_profile_path: Option<String>,
}

impl Default for LocalState {
    fn default() -> Self {
        Self {
            last_known_version: 0,
            machine_name: default_machine_name(),
            selected_profile_path: None,
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
        assert_eq!(state.selected_profile_path, None);
    }

    #[test]
    fn save_and_load_round_trip() {
        let dir = tempdir().unwrap();
        let original = LocalState {
            last_known_version: 42,
            machine_name: "Test Machine".to_string(),
            selected_profile_path: Some("/home/user/.zen/Profiles/test.release".to_string()),
        };
        original.save(dir.path()).unwrap();
        let loaded = LocalState::load(dir.path());
        assert_eq!(loaded, original);
    }
}
