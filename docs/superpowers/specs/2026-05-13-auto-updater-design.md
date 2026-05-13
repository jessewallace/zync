# Auto-Updater Design

**Date:** 2026-05-13  
**Scope:** Add automatic update checking and installation to Zync using `tauri-plugin-updater`

---

## Goal

Zync should silently check for new releases and notify the user via the system tray when one is available. The user installs on their own schedule by clicking a tray item.

---

## Approach

Use `tauri-plugin-updater` (official Tauri v2 plugin) with GitHub releases as the manifest host. `tauri-action` in CI generates and uploads a signed `latest.json` manifest automatically when `includeUpdaterJson: true`. Updates are cryptographically signed with an Ed25519 keypair — the app will not install an update that doesn't verify against the baked-in public key.

---

## Components

### 1. Keypair (one-time setup)

Generate an Ed25519 signing keypair with `tauri signer generate`. Output:
- **Private key** → GitHub secret `TAURI_SIGNING_PRIVATE_KEY`
- **Public key** → stored in `tauri.conf.json` under `plugins.updater.pubkey`

The private key is used by CI to sign each release artifact. The public key is embedded in the binary and used at runtime to verify downloads.

### 2. `Cargo.toml`

Add:
```toml
tauri-plugin-updater = "2"
```

### 3. `tauri.conf.json`

Add an `updater` block under `plugins`:
```json
"plugins": {
  "updater": {
    "pubkey": "<BASE64_PUBLIC_KEY>",
    "endpoints": [
      "https://github.com/jessewallace/zync/releases/latest/download/latest.json"
    ]
  }
}
```

The endpoint is the stable URL where `tauri-action` uploads the manifest on each tagged release.

### 4. `capabilities/default.json`

Add `"updater:default"` to the permissions list so the plugin is authorized.

### 5. `lib.rs` — update check logic

Register the plugin:
```rust
.plugin(tauri_plugin_updater::Builder::new().build())
```

After the app starts, spawn a background task that:
1. Calls `app.updater()?.check().await`
2. If an update is available:
   - Stores it in an `Arc<Mutex<Option<Update>>>` accessible from the tray handler
   - Fires an OS notification: `"Zync {version} is available — open the tray to install"`
   - Rebuilds the tray menu with an "Install update (X.Y)" item prepended
3. If no update or the check fails, logs and continues silently
4. Sleeps 24 hours and repeats

When the user clicks "Install update" in the tray:
1. Opens the Zync window (if hidden) and emits an `update-available` event to the frontend carrying `{ version, notes }`
2. The frontend renders an update dialog overlaid on the current screen (see UI section below)
3. If the user clicks **Install**: calls `update.download_and_install(|_, _| {}, || {}).await` via a new `install_update` Tauri command; app restarts automatically
4. If the user clicks **Later**: dismisses the dialog; the tray item remains for next time

### 6. Update dialog UI

An overlay rendered in `index.html` / `main.js`, shown only when the `update-available` event fires. Styled to match the existing dark Zync aesthetic.

Layout:
```
┌─────────────────────────────────┐
│  Zync 1.2.0 is available        │
│  You have 1.1.0                 │
│                                 │
│  ─────────────────────────────  │
│  • Fixed profile detection on   │
│    macOS Sequoia                │
│  • Improved sync reliability    │
│                                 │
│        [Later]  [Install & Restart] │
└─────────────────────────────────┘
```

- Release notes come from the `notes` field in `latest.json`, which `tauri-action` populates from the GitHub release body
- The dialog replaces the current tab content (not a separate window); the existing tabs are hidden while it's shown
- "Install & Restart" triggers the `install_update` command; "Later" hides the overlay

### 7. New Tauri command: `install_update`

A `#[tauri::command]` in `lib.rs` that:
1. Retrieves the stored `Update` from shared state
2. Calls `update.download_and_install(|_, _| {}, || {}).await`
3. Returns an error string to the frontend if it fails (frontend then shows the failure notification)

### 8. `release.yml`

Two changes:
- Set `includeUpdaterJson: true` in the `tauri-action` step
- Add `TAURI_SIGNING_PRIVATE_KEY` to the env block

`tauri-action` will generate `latest.json` for each platform, merge them, and attach the result to the GitHub release.

---

## Error handling

- If the update check network request fails (no internet, GitHub down), log the error and retry at the next 24h interval. No user-facing error.
- If `install_update` fails, the command returns an error string; the frontend replaces the dialog with an inline error message: "Update failed — download Zync manually at github.com/jessewallace/zync/releases". The tray item remains.

---

## Files changed

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `tauri-plugin-updater = "2"` |
| `src-tauri/tauri.conf.json` | Add `plugins.updater` block with pubkey + endpoint |
| `src-tauri/capabilities/default.json` | Add `"updater:default"` permission |
| `src-tauri/src/lib.rs` | Register plugin, spawn update-check loop, handle tray "Install" item, add `install_update` command |
| `src/index.html` + `src/main.js` + `src/style.css` | Add update dialog overlay |
| `.github/workflows/release.yml` | `includeUpdaterJson: true`, add `TAURI_SIGNING_PRIVATE_KEY` env var |

No new files or modules. All updater logic is in `lib.rs`.

---

## Out of scope

- Rollback / version pinning
- Differential/delta updates
- "Check for updates" manual menu item (not needed given 24h auto-check)
