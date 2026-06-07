use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ZenInstallation {
    pub path: String,
    pub install_type: String,
    pub base_path: String,
    pub last_used: Option<u64>,
}

pub enum ResolveError {
    NotFound,
    MultipleInstallations(Vec<ZenInstallation>),
    SavedPathInvalid(String),
}

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

#[tauri::command]
pub fn scan_zen_installations_cmd() -> Result<Vec<ZenInstallation>, String> {
    Ok(scan_zen_installations())
}

#[tauri::command]
pub fn validate_custom_path_cmd(path: String) -> Result<Option<ZenInstallation>, String> {
    Ok(validate_custom_path(&path))
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

pub fn scan_zen_installations() -> Vec<ZenInstallation> {
    let mut results = Vec::new();

    #[cfg(target_os = "linux")]
    {
        scan_one(&mut results, dirs::home_dir().map(|d| d.join(".zen")), "native");
        scan_one(&mut results, dirs::data_local_dir().map(|d| d.join("zen")), "xdg");
        scan_one(&mut results, dirs::home_dir().map(|d| d.join(".var/app/app.zen_browser.zen/.zen")), "flatpak");
        scan_one(&mut results, dirs::home_dir().map(|d| d.join("snap/zen-browser/common/.zen")), "snap");
    }

    #[cfg(target_os = "macos")]
    {
        scan_one(&mut results, dirs::data_dir().map(|d| d.join("zen")), "native");
    }

    #[cfg(target_os = "windows")]
    {
        scan_one(&mut results, dirs::data_dir().map(|d| d.join("zen")), "native");
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));
    results.dedup_by(|a, b| a.path == b.path);

    results
}

fn scan_one(results: &mut Vec<ZenInstallation>, base_dir: Option<PathBuf>, install_type: &'static str) {
    let base_dir = match base_dir {
        Some(d) if d.exists() => d,
        _ => return,
    };

    // Read profiles.ini from the base Zen directory — works on all platforms.
    let profile = read_active_profile_from_ini(&base_dir)
        .filter(|p| p.is_dir())
        // Fallback: look for release folders directly in base_dir (Linux)
        .or_else(|| first_release_profile(&base_dir))
        // Fallback: look for release folders inside a Profiles/ subdir (macOS/Windows)
        .or_else(|| {
            let profiles_subdir = base_dir.join("Profiles");
            if profiles_subdir.is_dir() {
                first_release_profile(&profiles_subdir)
            } else {
                None
            }
        });

    if let Some(profile_path) = profile {
        let last_used = profile_path.join("places.sqlite")
            .metadata()
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());

        results.push(ZenInstallation {
            path: profile_path.to_string_lossy().into_owned(),
            install_type: install_type.to_string(),
            base_path: base_dir.to_string_lossy().into_owned(),
            last_used,
        });
    }
}

pub fn resolve_zen_profile(saved_path: Option<&Path>) -> Result<PathBuf, ResolveError> {
    if let Some(path) = saved_path {
        if path.exists() {
            return Ok(path.to_path_buf());
        }
        return Err(ResolveError::SavedPathInvalid(path.to_string_lossy().into_owned()));
    }

    let installations = scan_zen_installations();
    match installations.len() {
        0 => Err(ResolveError::NotFound),
        1 => Ok(PathBuf::from(&installations[0].path)),
        _ => Err(ResolveError::MultipleInstallations(installations)),
    }
}

pub fn validate_custom_path(path_str: &str) -> Option<ZenInstallation> {
    let path = Path::new(path_str);
    if !path.exists() || !path.is_dir() {
        return None;
    }
    let has_profile_files = path.join("places.sqlite").exists()
        || path.join("prefs.js").exists();
    if !has_profile_files {
        return None;
    }
    let last_used = path.join("places.sqlite")
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    Some(ZenInstallation {
        path: path.to_string_lossy().into_owned(),
        install_type: "custom".to_string(),
        base_path: path.parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        last_used,
    })
}

pub fn find_zen_profile() -> Option<PathBuf> {
    match resolve_zen_profile(None) {
        Ok(p) => Some(p),
        Err(_) => None,
    }
}

/// Read `profiles.ini` in the Zen data directory and extract the path recorded in the
/// first `[InstallXXXXXX]` section. That entry is written by Zen itself each launch and
/// reflects the profile the browser will open next — more reliable than matching folder names.
fn read_active_profile_from_ini(profiles_dir: &Path) -> Option<PathBuf> {
    let ini_path = profiles_dir.join("profiles.ini");
    let content = std::fs::read_to_string(ini_path).ok()?;

    let mut in_install_section = false;
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_install_section = line.to_lowercase().starts_with("[install");
        } else if in_install_section {
            if let Some(rel_path) = line.strip_prefix("Default=") {
                // Value is relative to the zen/ dir, e.g. "Profiles/tcuo77lt.Default (release)"
                return Some(profiles_dir.join(rel_path));
            }
        }
    }
    None
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
