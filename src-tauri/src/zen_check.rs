use sysinfo::System;

/// Returns true if any process with "zen" in its name is currently running.
/// Must be checked before any push or pull operation — places.sqlite is locked
/// while Zen is open and cannot be safely copied.
#[tauri::command]
pub fn is_zen_running() -> bool {
    let mut sys = System::new_all();
    sys.refresh_all();
    sys.processes().values().any(|p| {
        let name = p.name().to_lowercase();
        // Match zen, zen-bin, zen-browser, etc. but not "zendesk" or similar.
        // Process names on macOS/Linux tend to be short ("zen" or "zen-bin").
        name == "zen" || name.starts_with("zen-") || name.starts_with("zen ")
    })
}
