mod crypto;
mod pairing;
mod profile;
mod sync;
mod transport;
mod zen_check;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            zen_check::is_zen_running,
            profile::detect_profile_path,
            profile::collect_sync_files,
            sync::push_profile,
            sync::pull_profile,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
