use sysinfo::System;

/// Returns true if the main Zen browser process is running **under the current user**.
/// Must be checked before any push or pull — places.sqlite is locked while Zen is open.
/// Note: on macOS, closing the Zen window does NOT quit the app; the process stays alive.
/// Users must quit Zen (⌘Q) before syncing.
///
/// We restrict to the current user because on multi-user Macs, Zen running under
/// a different account cannot lock this user's profile files.
#[tauri::command]
pub fn is_zen_running() -> bool {
    let mut sys = System::new_all();
    sys.refresh_all();

    let my_pid = sysinfo::Pid::from(std::process::id() as usize);
    let my_uid = sys.process(my_pid).and_then(|p| p.user_id()).cloned();

    sys.processes().values().any(|p| {
        let raw = p.name().to_lowercase();
        let name = raw.trim_end_matches(".exe");
        let is_zen = name == "zen" || name == "zen browser" || name.starts_with("zen-");
        let is_subprocess = name.contains("helper") || name.contains("crashreporter");
        if !(is_zen && !is_subprocess) {
            return false;
        }
        match (&my_uid, p.user_id()) {
            (Some(mine), Some(theirs)) => mine == theirs,
            _ => false,
        }
    })
}
