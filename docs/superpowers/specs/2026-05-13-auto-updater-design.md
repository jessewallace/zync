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
1. Retrieves the stored `Update` from the shared state
2. Calls `update.download_and_install(|_, _| {}, || {}).await`
3. App restarts automatically after install

### 6. `release.yml`

Two changes:
- Set `includeUpdaterJson: true` in the `tauri-action` step
- Add `TAURI_SIGNING_PRIVATE_KEY` to the env block

`tauri-action` will generate `latest.json` for each platform, merge them, and attach the result to the GitHub release.

---

## Error handling

- If the update check network request fails (no internet, GitHub down), log the error and retry at the next 24h interval. No user-facing error.
- If `download_and_install` fails, show an OS notification: `"Update failed — download Zync manually from github.com/jessewallace/zync/releases"`. Restore the tray menu to its normal state.

---

## Files changed

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `tauri-plugin-updater = "2"` |
| `src-tauri/tauri.conf.json` | Add `plugins.updater` block with pubkey + endpoint |
| `src-tauri/capabilities/default.json` | Add `"updater:default"` permission |
| `src-tauri/src/lib.rs` | Register plugin, spawn update-check loop, handle tray "Install" item |
| `.github/workflows/release.yml` | `includeUpdaterJson: true`, add `TAURI_SIGNING_PRIVATE_KEY` env var |

No new files or modules. All updater logic is in `lib.rs`.

---

## Out of scope

- Release notes shown in the update notification (Tauri's updater can fetch these from the manifest but adds complexity for minimal gain)
- Rollback / version pinning
- Differential/delta updates
- "Check for updates" manual menu item (not needed given 24h auto-check)
