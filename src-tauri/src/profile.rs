use std::path::{Path, PathBuf};

/// Files synced by default. Order matters for display.
pub const SYNC_FILES: &[&str] = &[
    "places.sqlite",
    "prefs.js",
    "extensions.json",
    "zen-themes.json",
    "zen-keyboard-shortcuts.json",
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
    let profiles_dir = zen_profiles_base()?;
    eprintln!("[zync] profiles_dir = {}", profiles_dir.display());

    let from_ini = read_active_profile_from_ini(&profiles_dir);
    eprintln!("[zync] profiles.ini result = {:?}", from_ini);

    let fallback = first_release_profile(&profiles_dir);
    eprintln!("[zync] release-folder fallback = {:?}", fallback);

    // Primary: read profiles.ini [InstallXXX] section — this is what Zen actually used last.
    // Fallback: first folder with "release" in name (covers older Zen versions).
    let result = from_ini
        .filter(|p| p.is_dir())
        .or(fallback)
        .or_else(|| {
            #[cfg(target_os = "linux")]
            {
                let flatpak = dirs::home_dir()?
                    .join(".var/app/app.zen_browser.zen/zen/Profiles");
                read_active_profile_from_ini(&flatpak)
                    .filter(|p| p.is_dir())
                    .or_else(|| first_release_profile(&flatpak))
            }
            #[cfg(not(target_os = "linux"))]
            None
        });
    eprintln!("[zync] final profile = {:?}", result);
    result
}

/// Read `profiles.ini` in the Zen data directory and extract the path recorded in the
/// first `[InstallXXXXXX]` section. That entry is written by Zen itself each launch and
/// reflects the profile the browser will open next — more reliable than matching folder names.
fn read_active_profile_from_ini(profiles_dir: &Path) -> Option<PathBuf> {
    let zen_dir = profiles_dir.parent()?;
    let ini_path = zen_dir.join("profiles.ini");
    let content = std::fs::read_to_string(ini_path).ok()?;

    let mut in_install_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_install_section = line.to_lowercase().starts_with("[install");
        } else if in_install_section {
            if let Some(rel_path) = line.strip_prefix("Default=") {
                // Value is relative to the zen/ dir, e.g. "Profiles/tcuo77lt.Default (release)"
                return Some(zen_dir.join(rel_path));
            }
        }
    }
    None
}

fn zen_profiles_base() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    // ~/Library/Application Support/zen/Profiles
    return dirs::data_dir().map(|d| d.join("zen/Profiles"));

    #[cfg(target_os = "windows")]
    // %APPDATA%\zen\Profiles
    return dirs::data_dir().map(|d| d.join("zen\\Profiles"));

    #[cfg(target_os = "linux")]
    // ~/.zen/Profiles
    return dirs::home_dir().map(|d| d.join(".zen/Profiles"));

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    None
}

fn first_release_profile(profiles_dir: &PathBuf) -> Option<PathBuf> {
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
