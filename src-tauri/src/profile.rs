use std::path::{Path, PathBuf};

/// Files synced by default. Order matters for display.
///
/// extensions.json is intentionally excluded: it stores absolute paths to XPI files
/// that are OS-specific (e.g. /Users/… on macOS vs C:\Users\… on Windows), so syncing
/// it cross-platform greys out all extensions on the receiving machine.
pub const SYNC_FILES: &[&str] = &[
    "places.sqlite",
    "prefs.js",
    "zen-themes.json",
    "zen-keyboard-shortcuts.json",
    "zen-sessions.jsonlz4",
    "zen-live-folders.jsonlz4",
    "chrome/zen-themes.css",
    "containers.json",
];

/// Files never synced — machine-specific, sensitive, or too large.
const EXCLUDE_PATTERNS: &[&str] = &[
    "key4.db",
    "logins.json",
    "sessionstore.jsonlz4",
    "storage",
];

/// Returns the path to the active Zen profile (.release folder), or an error
/// if none can be found on this OS.
#[tauri::command]
pub fn detect_profile_path() -> Result<String, String> {
    find_zen_profile()
        .map(|p| p.to_string_lossy().into_owned())
        .ok_or_else(|| "Zen profile folder not found. Is Zen Browser installed?".into())
}

/// Returns the list of file names that will be included in a sync bundle.
#[tauri::command]
pub fn collect_sync_files() -> Result<Vec<String>, String> {
    let profile = find_zen_profile()
        .ok_or_else(|| "Zen profile folder not found".to_string())?;

    let files: Vec<String> = SYNC_FILES
        .iter()
        .filter(|&&name| {
            let path = profile.join(name);
            path.exists() && !is_excluded(name)
        })
        .map(|&s| s.to_string())
        .collect();

    if files.is_empty() {
        return Err("No sync files found in profile folder".into());
    }
    Ok(files)
}

pub fn find_zen_profile() -> Option<PathBuf> {
    let bases = zen_profiles_bases();

    for dir in &bases {
        if !dir.exists() {
            continue;
        }
        eprintln!("[zync] checking profiles_dir = {}", dir.display());

        if let Some(from_ini) = read_active_profile_from_ini(dir) {
            eprintln!("[zync] profiles.ini result = {:?}", from_ini);
            return Some(from_ini);
        }

        if let Some(fallback) = first_release_profile(dir) {
            eprintln!("[zync] release-folder fallback = {:?}", fallback);
            return Some(fallback);
        }
    }

    eprintln!("[zync] final profile = None");
    None
}

/// Read `profiles.ini` in the Zen data directory and extract the path recorded in the
/// first `[InstallXXXXXX]` section. That entry is written by Zen itself each launch and
/// reflects the profile the browser will open next — more reliable than matching folder names.
fn read_active_profile_from_ini(dir: &Path) -> Option<PathBuf> {
    let zen_dir = if dir.file_name().map_or(false, |name| name == "Profiles") {
        dir.parent()?
    } else {
        dir
    };

    let ini_path = zen_dir.join("profiles.ini");
    let content = std::fs::read_to_string(ini_path).ok()?;

    let mut in_install_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_install_section = line.to_lowercase().starts_with("[install");
        } else if in_install_section {
            if let Some(rel_path) = line.strip_prefix("Default=") {
                // Value is relative to the zen_dir, e.g. "Profiles/tcuo77lt.Default (release)" or "k39t6h5g.Default (release)"
                let target = zen_dir.join(rel_path);
                if target.is_dir() {
                    return Some(target);
                }
            }
        }
    }
    None
}

fn zen_profiles_bases() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "macos")]
    if let Some(d) = dirs::data_dir() {
        dirs.push(d.join("zen/Profiles"));
        dirs.push(d.join("zen"));
    }

    #[cfg(target_os = "windows")]
    if let Some(d) = dirs::data_dir() {
        dirs.push(d.join("zen\\Profiles"));
        dirs.push(d.join("zen"));
    }

    #[cfg(target_os = "linux")]
    {
        // 1. ~/.zen/Profiles & ~/.zen
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".zen/Profiles"));
            dirs.push(home.join(".zen"));
        }
        // 2. ~/.config/zen/Profiles & ~/.config/zen (XDG Config Home)
        if let Some(config) = dirs::config_dir() {
            dirs.push(config.join("zen/Profiles"));
            dirs.push(config.join("zen"));
        }
        // 3. ~/.local/share/zen/Profiles & ~/.local/share/zen (XDG Data Home)
        if let Some(data_local) = dirs::data_local_dir() {
            dirs.push(data_local.join("zen/Profiles"));
            dirs.push(data_local.join("zen"));
        }
        // 4. Flatpak
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".var/app/app.zen_browser.zen/zen/Profiles"));
            dirs.push(home.join(".var/app/app.zen_browser.zen/zen"));
        }
    }

    dirs
}

fn first_release_profile(profiles_dir: &Path) -> Option<PathBuf> {
    // Zen names its active profile either "xxxxxxxx.Default (release)" or
    // "xxxxxxxx.default-release" depending on version/platform.
    // Match any folder whose name contains "release" (case-insensitive).
    std::fs::read_dir(profiles_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .find(|e| {
            let name = e.file_name().to_string_lossy().to_lowercase();
            name.contains("release")
        })
        .map(|e| e.path())
}

fn is_excluded(name: &str) -> bool {
    EXCLUDE_PATTERNS.iter().any(|&pat| name == pat || name.starts_with(pat))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_active_profile_from_ini_custom_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let zen_dir = temp_dir.path();
        let ini_path = zen_dir.join("profiles.ini");
        let profile_dir = zen_dir.join("abc1234.Default (release)");

        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            ini_path,
            "[Install12345]\nDefault=abc1234.Default (release)\nLocked=1\n",
        )
        .unwrap();

        let result = read_active_profile_from_ini(zen_dir);
        assert_eq!(result, Some(profile_dir));
    }

    #[test]
    fn test_read_active_profile_from_ini_profiles_subdir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let zen_dir = temp_dir.path();
        let profiles_sub = zen_dir.join("Profiles");
        let ini_path = zen_dir.join("profiles.ini");
        let profile_dir = profiles_sub.join("xyz5678.Default (release)");

        std::fs::create_dir_all(&profile_dir).unwrap();
        std::fs::write(
            ini_path,
            "[Install67890]\nDefault=Profiles/xyz5678.Default (release)\nLocked=1\n",
        )
        .unwrap();

        let result = read_active_profile_from_ini(&profiles_sub);
        assert_eq!(result, Some(profile_dir));
    }
}


