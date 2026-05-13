# Auto-Updater Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add automatic update checking (on launch + every 24h) with a release-notes dialog and one-click install via `tauri-plugin-updater` and GitHub release assets.

**Architecture:** A background async task checks the GitHub releases `latest.json` manifest on startup (after a 5s delay) and every 24h. When an update is found, it fires an OS notification, rebuilds the tray menu to show an "Install update" item, and emits an `update-available` event to the frontend. The frontend renders an in-app dialog with version and release notes; clicking "Install & Restart" calls the `install_update` Tauri command which downloads, verifies (Ed25519 signature), and installs the update.

**Tech Stack:** `tauri-plugin-updater 2`, `tokio::sync::Mutex`, Tauri managed state, vanilla JS event listener, GitHub releases as manifest host.

---

## File Map

| File | Change |
|------|--------|
| `src-tauri/Cargo.toml` | Add `tauri-plugin-updater = "2"` |
| `src-tauri/tauri.conf.json` | Add `plugins.updater` block (pubkey + endpoint) |
| `src-tauri/capabilities/default.json` | Add `"updater:default"` |
| `src-tauri/src/lib.rs` | Register plugin; add `UpdateStore` state; update `setup_tray` to use `.with_id`; add update check loop; add `install_update` command |
| `src/index.html` | Add `#update-dialog` overlay div |
| `src/style.css` | Add dialog overlay styles |
| `src/main.js` | Listen for `update-available` event; show/hide dialog; call `install_update` |
| `.github/workflows/release.yml` | `includeUpdaterJson: true`; add `TAURI_SIGNING_PRIVATE_KEY` env var |

---

## Task 1: Generate signing keypair (one-time manual setup)

**Files:** `src-tauri/tauri.conf.json` (pubkey added here), GitHub repo Settings → Secrets

This task is manual — no automated tests. Run locally, then add a secret to GitHub.

- [ ] **Step 1: Generate the keypair**

```bash
cd src-tauri && cargo tauri signer generate -w ~/.tauri/zync.key
```

Output looks like:
```
Private: <PRIVATE_KEY_BASE64>
Public: <PUBLIC_KEY_BASE64>

Your private key was saved to /Users/you/.tauri/zync.key — keep it secret.
```

- [ ] **Step 2: Add the public key to `tauri.conf.json`**

Open `src-tauri/tauri.conf.json`. After the `"bundle"` block, add a `"plugins"` key. The full file should look like:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Zync",
  "version": "0.1.0",
  "identifier": "app.zync.zensync",
  "build": {
    "frontendDist": "../src",
    "beforeBuildCommand": "",
    "beforeDevCommand": ""
  },
  "app": {
    "withGlobalTauri": true,
    "windows": [
      {
        "title": "Zync",
        "width": 420,
        "height": 460,
        "resizable": false,
        "fullscreen": false,
        "center": true,
        "visible": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "macOS": {
      "entitlements": "./entitlements.plist",
      "minimumSystemVersion": "10.15"
    }
  },
  "plugins": {
    "updater": {
      "pubkey": "PASTE_YOUR_PUBLIC_KEY_HERE",
      "endpoints": [
        "https://github.com/jessewallace/zync/releases/latest/download/latest.json"
      ]
    }
  }
}
```

Replace `PASTE_YOUR_PUBLIC_KEY_HERE` with the public key printed in step 1.

- [ ] **Step 3: Add the private key to GitHub secrets**

Go to https://github.com/jessewallace/zync/settings/secrets/actions → New repository secret:
- Name: `TAURI_SIGNING_PRIVATE_KEY`
- Value: paste the private key from step 1 (the `Private:` line, including the full base64 string)

- [ ] **Step 4: Commit tauri.conf.json**

```bash
git add src-tauri/tauri.conf.json
git commit -m "feat: add updater pubkey to tauri.conf.json"
```

---

## Task 2: Add plugin dependency and capability

**Files:** `src-tauri/Cargo.toml`, `src-tauri/capabilities/default.json`

- [ ] **Step 1: Add plugin to Cargo.toml**

In `src-tauri/Cargo.toml`, under the `# Tauri plugins` section, add:

```toml
tauri-plugin-updater = "2"
```

The plugins section should look like:
```toml
# Tauri plugins
tauri-plugin-notification = "2"
tauri-plugin-autostart = "2"
tauri-plugin-updater = "2"
```

- [ ] **Step 2: Add permission to capabilities/default.json**

```json
{
  "identifier": "default",
  "description": "Default capability for Zync",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:tray:default",
    "notification:default",
    "autostart:default",
    "updater:default"
  ]
}
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors. If `tauri-plugin-updater` isn't found, run `cargo update` first.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/capabilities/default.json
git commit -m "feat: add tauri-plugin-updater dependency and capability"
```

---

## Task 3: Add UpdateStore state and update setup_tray

**Files:** `src-tauri/src/lib.rs`

- [ ] **Step 1: Add UpdateStore struct and imports at the top of lib.rs**

After the existing `use` statements, add:

```rust
use tauri_plugin_updater::UpdaterExt;

struct UpdateStore {
    update: tokio::sync::Mutex<Option<tauri_plugin_updater::Update>>,
    version: std::sync::Mutex<Option<String>>,
    notes: std::sync::Mutex<Option<String>>,
}

impl UpdateStore {
    fn new() -> Self {
        Self {
            update: tokio::sync::Mutex::new(None),
            version: std::sync::Mutex::new(None),
            notes: std::sync::Mutex::new(None),
        }
    }
}
```

- [ ] **Step 2: Change TrayIconBuilder to use a named ID**

In `setup_tray`, change:
```rust
TrayIconBuilder::new()
```
to:
```rust
TrayIconBuilder::with_id("zync-tray")
```

- [ ] **Step 3: Add "install_update" handler to the tray on_menu_event match**

In the `on_menu_event` closure, add a new arm before `_ => {}`:

```rust
"install_update" => {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.show();
            let _ = w.set_focus();
        }
        let store = app.state::<std::sync::Arc<UpdateStore>>();
        let version = store.version.lock().unwrap().clone().unwrap_or_default();
        let notes = store.notes.lock().unwrap().clone().unwrap_or_default();
        let _ = app.emit("update-available", serde_json::json!({
            "version": version,
            "notes": notes,
        }));
    });
}
```

- [ ] **Step 4: Register UpdateStore in managed state and register the updater plugin**

In the `run()` function, add the plugin registration before `.setup(`:

```rust
.plugin(tauri_plugin_updater::Builder::new().build())
```

Inside `.setup(|app|`, after `app.manage(state.clone());`, add:

```rust
let update_store = std::sync::Arc::new(UpdateStore::new());
app.manage(update_store.clone());
```

- [ ] **Step 5: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: add UpdateStore state and named tray ID"
```

---

## Task 4: Implement background update check loop

**Files:** `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the check_for_updates async function**

Add this function at the bottom of `lib.rs`, before or after `setup_tray`:

```rust
async fn check_for_updates(app: &tauri::AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => { eprintln!("[updater] init error: {e}"); return; }
    };

    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => return,
        Err(e) => { eprintln!("[updater] check error: {e}"); return; }
    };

    let version = update.version.clone();
    let notes = update.body.clone().unwrap_or_default();

    // Store for later install and tray re-emit
    let store = app.state::<std::sync::Arc<UpdateStore>>();
    *store.update.lock().await = Some(update);
    *store.version.lock().unwrap() = Some(version.clone());
    *store.notes.lock().unwrap() = Some(notes.clone());

    // OS notification
    let _ = app.notification()
        .builder()
        .title("Zync update available")
        .body(format!("Zync {} is ready — open the tray to install", version))
        .show();

    // Emit to frontend (catches it if window is open)
    let _ = app.emit("update-available", serde_json::json!({
        "version": version,
        "notes": notes,
    }));

    // Rebuild tray menu with install item
    rebuild_tray_with_update(app, &version);
}

fn rebuild_tray_with_update(app: &tauri::AppHandle, version: &str) {
    let Ok(install) = MenuItem::with_id(
        app,
        "install_update",
        format!("Install update ({})", version),
        true,
        None::<&str>,
    ) else { return };
    let Ok(sep1) = PredefinedMenuItem::separator(app) else { return };
    let Ok(open) = MenuItem::with_id(app, "open", "Open Zync", true, None::<&str>) else { return };
    let Ok(sync_now) = MenuItem::with_id(app, "sync_now", "Sync now", true, None::<&str>) else { return };
    let Ok(sep2) = PredefinedMenuItem::separator(app) else { return };
    let Ok(quit) = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>) else { return };
    let Ok(menu) = Menu::with_items(app, &[&install, &sep1, &open, &sync_now, &sep2, &quit]) else { return };

    if let Some(tray) = app.tray_by_id("zync-tray") {
        let _ = tray.set_menu(Some(menu));
    }
}
```

- [ ] **Step 2: Spawn the background loop in setup**

Inside `.setup(|app|`, after `daemon::start(...)`, add:

```rust
// Spawn update check loop: check on launch (after 5s) then every 24h
let app_handle = app.handle().clone();
tauri::async_runtime::spawn(async move {
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    loop {
        check_for_updates(&app_handle).await;
        tokio::time::sleep(std::time::Duration::from_secs(24 * 60 * 60)).await;
    }
});
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: add background update check loop"
```

---

## Task 5: Add install_update Tauri command

**Files:** `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the install_update command function**

Add this function near the bottom of `lib.rs`:

```rust
#[tauri::command]
async fn install_update(
    app: tauri::AppHandle,
    store: tauri::State<'_, std::sync::Arc<UpdateStore>>,
) -> Result<(), String> {
    let update = store.update.lock().await.take()
        .ok_or_else(|| "No pending update".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
```

- [ ] **Step 2: Register the command in invoke_handler**

In the `invoke_handler` call, add `install_update` to the list:

```rust
.invoke_handler(tauri::generate_handler![
    zen_check::is_zen_running,
    profile::detect_profile_path,
    profile::collect_sync_files,
    sync::push_profile,
    sync::pull_profile,
    pairing::save_passphrase_cmd,
    pairing::get_pairing_status_cmd,
    pairing::clear_passphrase_cmd,
    pairing::get_passphrase_cmd,
    daemon::get_last_synced_cmd,
    daemon::set_auto_push_cmd,
    daemon::set_auto_pull_cmd,
    daemon::manual_sync_now_cmd,
    install_update,
])
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check
```

Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: add install_update Tauri command"
```

---

## Task 6: Frontend update dialog

**Files:** `src/index.html`, `src/style.css`, `src/main.js`

- [ ] **Step 1: Add the dialog overlay to index.html**

Inside `<div id="app">`, before the closing `</div>`, add the update dialog after the pull result screen:

```html
  <!-- ── Update dialog overlay ─────────────────────────────── -->
  <div id="update-dialog" class="update-overlay hidden" role="dialog" aria-modal="true" aria-labelledby="update-title">
    <div class="update-card">
      <p class="update-version" id="update-title">Zync <span id="update-version-number"></span> is available</p>
      <p class="update-current">You have <span id="update-current-version"></span></p>
      <div class="update-notes-wrap">
        <div id="update-notes" class="update-notes"></div>
      </div>
      <div class="update-actions">
        <button id="btn-update-later" class="btn btn-ghost">Later</button>
        <button id="btn-update-install" class="btn btn-primary">Install &amp; Restart</button>
      </div>
      <p id="update-error" class="update-error hidden"></p>
    </div>
  </div>
```

- [ ] **Step 2: Add dialog styles to style.css**

At the bottom of `style.css`, before the `/* ── Reduced motion ── */` block:

```css
/* ── Update dialog overlay ────────────────────────────────── */

.update-overlay {
  position: fixed;
  inset: 0;
  background: var(--bg);
  z-index: 100;
  display: flex;
  align-items: center;
  justify-content: center;
  animation: screen-enter 220ms var(--ease) both;
}
.update-overlay.hidden {
  display: none;
}

.update-card {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 32px;
  width: 100%;
}

.update-version {
  font-size: 24px;
  font-weight: 700;
  color: var(--text);
  text-align: center;
}

.update-current {
  font-size: 13px;
  color: var(--text-muted);
  text-align: center;
  margin-top: -8px;
}

.update-notes-wrap {
  background: var(--surface);
  border-radius: 8px;
  padding: 12px 16px;
  max-height: 160px;
  overflow-y: auto;
}

.update-notes {
  font-size: 13px;
  color: var(--text);
  line-height: 1.6;
  white-space: pre-wrap;
}

.update-actions {
  display: flex;
  gap: 10px;
}
.update-actions .btn {
  font-size: 16px;
  padding: 12px 16px;
  width: auto;
}
.update-actions #btn-update-later { flex: 1; }
.update-actions #btn-update-install { flex: 2; }

.update-error {
  font-size: 12px;
  color: var(--error-text);
  text-align: center;
}
.update-error.hidden { display: none; }
```

- [ ] **Step 3: Add update dialog logic to main.js**

After the `// ── Init ──` section and before `document.addEventListener("DOMContentLoaded", init);`, add:

```js
// ── Update dialog ─────────────────────────────────────────

const { listen } = window.__TAURI__.event;

let pendingUpdateVersion = null;

async function showUpdateDialog(version, notes) {
  pendingUpdateVersion = version;
  document.getElementById("update-version-number").textContent = version;
  try {
    const current = await window.__TAURI__.app.getVersion();
    document.getElementById("update-current-version").textContent = current;
  } catch (_) {
    document.getElementById("update-current-version").textContent = "";
  }
  document.getElementById("update-notes").textContent = notes || "No release notes provided.";
  document.getElementById("update-error").classList.add("hidden");
  document.getElementById("btn-update-install").disabled = false;
  document.getElementById("btn-update-install").textContent = "Install & Restart";
  document.getElementById("update-dialog").classList.remove("hidden");
}

function hideUpdateDialog() {
  document.getElementById("update-dialog").classList.add("hidden");
}

document.getElementById("btn-update-later").addEventListener("click", hideUpdateDialog);

document.getElementById("btn-update-install").addEventListener("click", async () => {
  const btn = document.getElementById("btn-update-install");
  const errEl = document.getElementById("update-error");
  btn.disabled = true;
  btn.textContent = "Downloading…";
  errEl.classList.add("hidden");
  try {
    await invoke("install_update");
    // App restarts — code below never runs
  } catch (err) {
    btn.disabled = false;
    btn.textContent = "Install & Restart";
    errEl.textContent = `Update failed — download manually at github.com/jessewallace/zync/releases`;
    errEl.classList.remove("hidden");
  }
});

async function initUpdateListener() {
  await listen("update-available", (event) => {
    const { version, notes } = event.payload;
    showUpdateDialog(version, notes); // async, fire-and-forget is fine here
  });
}
```

- [ ] **Step 4: Call initUpdateListener in the init function**

In the existing `init()` function in `main.js`, add `await initUpdateListener();` as the first line:

```js
async function init() {
  await initUpdateListener();
  console.log(
    "%cZync",
    // ... rest unchanged
```

- [ ] **Step 5: Verify the app runs**

```bash
cd src-tauri && cargo tauri dev
```

Expected: app opens normally, no console errors. The update dialog is hidden on launch.

- [ ] **Step 6: Commit**

```bash
git add src/index.html src/style.css src/main.js
git commit -m "feat: add update dialog UI with release notes"
```

---

## Task 7: Update CI to generate and sign the updater manifest

**Files:** `.github/workflows/release.yml`

- [ ] **Step 1: Update the tauri-action step**

In `.github/workflows/release.yml`, find the `Build and release` step. Make two changes:

1. Add `TAURI_SIGNING_PRIVATE_KEY` to the `env` block:
```yaml
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}
          APPLE_CERTIFICATE_PASSWORD: ${{ secrets.APPLE_CERTIFICATE_PASSWORD }}
          APPLE_SIGNING_IDENTITY: ${{ secrets.APPLE_SIGNING_IDENTITY }}
          APPLE_ID: ${{ secrets.APPLE_ID }}
          APPLE_PASSWORD: ${{ secrets.APPLE_PASSWORD }}
          APPLE_TEAM_ID: ${{ secrets.APPLE_TEAM_ID }}
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
```

2. Change `includeUpdaterJson: false` to `includeUpdaterJson: true`:
```yaml
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'Zync ${{ github.ref_name }}'
          releaseBody: |
            ...
          releaseDraft: false
          prerelease: false
          includeUpdaterJson: true
          args: ${{ matrix.args }}
```

- [ ] **Step 2: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "feat: enable signed updater JSON in CI release workflow"
```

---

## Task 8: End-to-end smoke test

- [ ] **Step 1: Tag a test release**

```bash
git tag v0.2.0
git push origin v0.2.0
```

- [ ] **Step 2: Verify CI generates latest.json**

Go to https://github.com/jessewallace/zync/releases — after the workflow completes, the release should have a `latest.json` asset attached alongside the platform binaries.

- [ ] **Step 3: Verify latest.json structure**

Download `latest.json` from the release and confirm it has entries for each platform with `signature` fields (not empty):

```json
{
  "version": "0.2.0",
  "notes": "...",
  "pub_date": "...",
  "platforms": {
    "darwin-aarch64": { "signature": "...", "url": "..." },
    "darwin-x86_64": { "signature": "...", "url": "..." },
    "windows-x86_64": { "signature": "...", "url": "..." },
    "linux-x86_64": { "signature": "...", "url": "..." }
  }
}
```

- [ ] **Step 4: Manually test update dialog (optional dev test)**

To test the dialog locally without a real update, temporarily add this in `main.js` after `initUpdateListener()`:

```js
// TEMP: remove before merge
setTimeout(() => showUpdateDialog("0.2.0", "• Fixed profile detection\n• Improved sync reliability"), 2000);
```

Run `cargo tauri dev`, verify the dialog appears after 2 seconds, "Later" hides it, "Install & Restart" shows the loading state. Remove the line before committing.
