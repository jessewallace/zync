# Keychain Onboarding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate unexpected macOS keychain prompts by replacing startup keychain reads with a flag file, caching the passphrase in memory for daemon loops, and adding a permission-priming screen before the first intentional keychain write.

**Architecture:** A zero-byte `paired.flag` file in the Tauri app data dir replaces all "is this user paired?" keychain reads. `DaemonState` gains an in-memory passphrase cache so daemon loops never touch the keychain. A new `screen-keychain-primer` screen intercepts "Save & Pair" clicks to explain the macOS prompt before it appears.

**Tech Stack:** Rust (Tauri v2), vanilla HTML/CSS/JS, `keyring` crate (existing), `std::fs` for flag file.

---

### Task 1: Flag file helpers in `pairing.rs`

**Files:**
- Modify: `src-tauri/src/pairing.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` block at the bottom of `src-tauri/src/pairing.rs`:

```rust
#[test]
fn paired_flag_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!is_paired_flag(dir.path()));
    write_paired_flag(dir.path()).unwrap();
    assert!(is_paired_flag(dir.path()));
    clear_paired_flag(dir.path()).unwrap();
    assert!(!is_paired_flag(dir.path()));
}

#[test]
fn clear_flag_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    assert!(clear_paired_flag(dir.path()).is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd src-tauri && cargo test pairing::tests --lib 2>&1 | tail -20
```

Expected: compile error — `is_paired_flag`, `write_paired_flag`, `clear_paired_flag` not found.

- [ ] **Step 3: Add the flag file functions**

Add below the `use sha2::{Digest, Sha256};` line at the top of `src-tauri/src/pairing.rs`:

```rust
use std::path::Path;
```

Add these three functions after the `clear_passphrase` function (before the `// ── Tauri commands` comment):

```rust
pub fn is_paired_flag(dir: &Path) -> bool {
    dir.join("paired.flag").exists()
}

pub fn write_paired_flag(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Failed to create data dir: {e}"))?;
    std::fs::write(dir.join("paired.flag"), b"")
        .map_err(|e| format!("Failed to write paired flag: {e}"))
}

pub fn clear_paired_flag(dir: &Path) -> Result<(), String> {
    let path = dir.join("paired.flag");
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to clear paired flag: {e}")),
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd src-tauri && cargo test pairing::tests --lib 2>&1 | tail -20
```

Expected output includes:
```
test pairing::tests::paired_flag_round_trip ... ok
test pairing::tests::clear_flag_is_idempotent ... ok
test pairing::tests::topic_is_deterministic ... ok
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/pairing.rs
git commit -m "feat: add paired flag file helpers to pairing.rs"
```

---

### Task 2: Passphrase cache in `DaemonState` and daemon tick functions

**Files:**
- Modify: `src-tauri/src/daemon.rs`

- [ ] **Step 1: Write the failing test**

Add to the `#[cfg(test)]` block at the bottom of `src-tauri/src/daemon.rs`:

```rust
#[test]
fn passphrase_defaults_to_none() {
    let state = DaemonState::default();
    assert!(state.passphrase.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd src-tauri && cargo test daemon::tests --lib 2>&1 | tail -10
```

Expected: compile error — `passphrase` field not found on `DaemonState`.

- [ ] **Step 3: Add `passphrase` field to `DaemonState`**

In `src-tauri/src/daemon.rs`, add `passphrase: Option<String>` to the struct and its `Default` impl:

```rust
pub struct DaemonState {
    pub auto_push_enabled: bool,
    pub auto_pull_enabled: bool,
    pub last_ntfy_id: Option<String>,
    pub last_poll_time: u64,
    pub pending_file_id: Option<String>,
    pub last_synced: Option<u64>,
    pub refresh_at: Option<std::time::Instant>,
    pub zen_was_running: bool,
    pub is_pushing: Arc<AtomicBool>,
    pub sync_count: u32,
    pub passphrase: Option<String>,
}

impl Default for DaemonState {
    fn default() -> Self {
        Self {
            auto_push_enabled: true,
            auto_pull_enabled: true,
            last_ntfy_id: None,
            last_poll_time: 0,
            pending_file_id: None,
            last_synced: None,
            refresh_at: None,
            zen_was_running: false,
            is_pushing: Arc::new(AtomicBool::new(false)),
            sync_count: 0,
            passphrase: None,
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd src-tauri && cargo test daemon::tests::passphrase_defaults_to_none --lib 2>&1 | tail -10
```

Expected: `test daemon::tests::passphrase_defaults_to_none ... ok`

- [ ] **Step 5: Update `zen_watcher_tick` to read from cache**

Replace the `pairing::load_passphrase()` call at the top of `zen_watcher_tick`:

```rust
// OLD — remove this block:
let passphrase = match pairing::load_passphrase() {
    Ok(Some(p)) => p,
    _ => return,
};

// NEW — replace with:
let passphrase = match state.lock().unwrap().passphrase.clone() {
    Some(p) => p,
    None => return,
};
```

- [ ] **Step 6: Update `ntfy_poll_tick` to read from cache**

Replace the `pairing::load_passphrase()` call and the separate `since` block at the top of `ntfy_poll_tick`:

```rust
// OLD — remove these two blocks:
let passphrase = match pairing::load_passphrase() {
    Ok(Some(p)) => p,
    _ => return,
};

let since = {
    let s = state.lock().unwrap();
    if !s.auto_pull_enabled {
        return;
    }
    s.last_ntfy_id.clone()
        .unwrap_or_else(|| s.last_poll_time.to_string())
};

// NEW — replace with a single lock:
let (passphrase, since) = {
    let s = state.lock().unwrap();
    let p = match s.passphrase.clone() { Some(p) => p, None => return };
    if !s.auto_pull_enabled { return; }
    let since = s.last_ntfy_id.clone()
        .unwrap_or_else(|| s.last_poll_time.to_string());
    (p, since)
};
```

- [ ] **Step 7: Update `refresh_tick` to read from cache**

Replace the `pairing::load_passphrase()` call and the separate `auto_push_enabled` read at the top of `refresh_tick`:

```rust
// OLD — remove these two statements:
let passphrase = match pairing::load_passphrase() {
    Ok(Some(p)) => p,
    _ => return,
};
let auto_push_enabled = state.lock().unwrap().auto_push_enabled;

// NEW — replace with a single lock:
let (passphrase, auto_push_enabled) = {
    let s = state.lock().unwrap();
    let p = match s.passphrase.clone() { Some(p) => p, None => return };
    (p, s.auto_push_enabled)
};
```

- [ ] **Step 8: Update `manual_sync_now_cmd` to read from cache**

Replace the `pairing::load_passphrase()` call in `manual_sync_now_cmd`:

```rust
// OLD:
let passphrase = pairing::load_passphrase()?
    .ok_or("No passphrase set — configure pairing in Settings first")?;

// NEW:
let passphrase = state.lock().unwrap().passphrase.clone()
    .ok_or("No passphrase set — configure pairing in Settings first")?;
```

- [ ] **Step 9: Build to verify no compile errors**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20
```

Expected: no `error` lines (warnings are fine).

- [ ] **Step 10: Run all daemon tests**

```bash
cd src-tauri && cargo test daemon::tests --lib 2>&1 | tail -15
```

Expected: all three tests pass (`sync_count_defaults_to_zero`, `passphrase_defaults_to_none`).

- [ ] **Step 11: Commit**

```bash
git add src-tauri/src/daemon.rs
git commit -m "feat: cache passphrase in DaemonState; daemon loops no longer read keychain"
```

---

### Task 3: New orchestration commands in `lib.rs`

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add the three new Tauri commands**

Add these three functions to `src-tauri/src/lib.rs`, just before the `fn setup_tray` function:

```rust
#[tauri::command]
fn is_paired_cmd(app: tauri::AppHandle) -> bool {
    match app.path().app_data_dir() {
        Ok(dir) => pairing::is_paired_flag(&dir),
        Err(_) => false,
    }
}

#[tauri::command]
fn save_passphrase_and_cache_cmd(
    app: tauri::AppHandle,
    passphrase: String,
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    pairing::save_passphrase(&passphrase)?;
    pairing::write_paired_flag(&data_dir)?;
    state.lock().unwrap().passphrase = Some(passphrase);
    Ok(())
}

#[tauri::command]
fn clear_passphrase_and_cache_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<Mutex<daemon::DaemonState>>>,
) -> Result<(), String> {
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    pairing::clear_passphrase()?;
    pairing::clear_paired_flag(&data_dir)?;
    state.lock().unwrap().passphrase = None;
    Ok(())
}
```

- [ ] **Step 2: Update `invoke_handler` registration**

Replace the entire `.invoke_handler(...)` block in `lib.rs`:

```rust
// OLD:
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
    daemon::get_sync_status_cmd,
    daemon::set_auto_push_cmd,
    daemon::set_auto_pull_cmd,
    daemon::manual_sync_now_cmd,
    install_update,
])

// NEW:
.invoke_handler(tauri::generate_handler![
    zen_check::is_zen_running,
    profile::detect_profile_path,
    profile::collect_sync_files,
    sync::push_profile,
    sync::pull_profile,
    pairing::get_passphrase_cmd,
    daemon::get_sync_status_cmd,
    daemon::set_auto_push_cmd,
    daemon::set_auto_pull_cmd,
    daemon::manual_sync_now_cmd,
    install_update,
    is_paired_cmd,
    save_passphrase_and_cache_cmd,
    clear_passphrase_and_cache_cmd,
])
```

- [ ] **Step 3: Build to verify no compile errors**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20
```

Expected: no `error` lines.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: add is_paired_cmd, save/clear_passphrase_and_cache_cmd to lib.rs"
```

---

### Task 4: Update `lib.rs` startup logic

**Files:**
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Replace startup keychain check with flag file check**

In `src-tauri/src/lib.rs`, find the `setup` closure. Replace the two-line first-run block (currently after `daemon::start`):

```rust
// OLD:
// First run: show window so user can enter passphrase
if !pairing::get_pairing_status_cmd() {
    window.show().unwrap();
    let _ = window.set_focus();
}

// NEW:
let data_dir = app.path().app_data_dir()
    .unwrap_or_else(|_| std::path::PathBuf::from("."));

if !pairing::is_paired_flag(&data_dir) {
    window.show().unwrap();
    let _ = window.set_focus();
}
```

- [ ] **Step 2: Add background passphrase-cache population task**

Immediately after the `if !pairing::is_paired_flag(...)` block (still inside `setup`), add:

```rust
// Populate passphrase cache for existing users.
// Runs after window-show decision so any keychain prompt (first-ever launch
// on a machine that somehow never clicked Always Allow) appears with the
// window already visible rather than before the user sees anything.
{
    let state_for_cache = state.clone();
    let data_dir_for_cache = data_dir.clone();
    tauri::async_runtime::spawn(async move {
        if pairing::is_paired_flag(&data_dir_for_cache) {
            if let Ok(Some(p)) = pairing::load_passphrase() {
                state_for_cache.lock().unwrap().passphrase = Some(p);
            }
        }
    });
}
```

- [ ] **Step 3: Build to verify no compile errors**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20
```

Expected: no `error` lines.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat: replace startup keychain check with flag file; background-load passphrase cache"
```

---

### Task 5: Priming screen HTML and CSS

**Files:**
- Modify: `src/index.html`
- Modify: `src/style.css`

- [ ] **Step 1: Add priming screen to `index.html`**

In `src/index.html`, add the new screen just before the closing `</div>` of `#app` (after the pull result screen `div`):

```html
  <!-- ── Keychain primer screen ────────────────────────────── -->
  <div id="screen-keychain-primer" class="screen">
    <div class="primer-body">
      <!-- Phosphor Lock icon -->
      <svg class="primer-icon" width="48" height="48" viewBox="0 0 256 256" fill="currentColor" aria-hidden="true">
        <path d="M208,80H176V56a48,48,0,0,0-96,0V80H48A16,16,0,0,0,32,96V208a16,16,0,0,0,16,16H208a16,16,0,0,0,16-16V96A16,16,0,0,0,208,80ZM96,56a32,32,0,0,1,64,0V80H96ZM208,208H48V96H208V208Zm-80-48a24,24,0,1,0-24-24A24,24,0,0,0,128,160Z"/>
      </svg>
      <p class="result-title">Secure storage</p>
      <p class="primer-text">Your passphrase will be stored in your Mac's Keychain. macOS will ask for permission — click <strong>Always Allow</strong> so you're not asked again.</p>
      <div id="primer-error" class="msg-box msg-error hidden" role="alert" aria-live="polite"></div>
    </div>
    <div class="primer-actions">
      <button id="btn-primer-back" class="btn btn-secondary">Go back</button>
      <button id="btn-primer-continue" class="btn btn-primary">Yes, continue</button>
    </div>
  </div>
```

- [ ] **Step 2: Add primer screen styles to `style.css`**

Add the following block to `src/style.css`, just before the `/* ── Reduced motion ───` section at the end:

```css
/* ── Keychain primer screen ───────────────────────────────── */

#screen-keychain-primer {
  align-items: center;
  justify-content: space-between;
  padding: 40px 32px 32px;
}

.primer-body {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  width: 100%;
  flex: 1;
  justify-content: center;
}

.primer-icon {
  color: var(--accent);
}

.primer-text {
  font-size: 15px;
  color: var(--text);
  text-align: center;
  line-height: 1.6;
  max-width: 300px;
}

.primer-actions {
  display: flex;
  gap: 10px;
  width: 100%;
}
.primer-actions .btn { width: auto; flex: 1; }
.primer-actions .btn-primary { flex: 2; }
```

- [ ] **Step 3: Commit**

```bash
git add src/index.html src/style.css
git commit -m "feat: add keychain primer screen HTML and CSS"
```

---

### Task 6: Update `main.js`

**Files:**
- Modify: `src/main.js`

- [ ] **Step 1: Replace `get_pairing_status_cmd` calls with `is_paired_cmd`**

In `src/main.js`, there are two invocations of `"get_pairing_status_cmd"`. Replace both:

In `loadPairTab()` (line ~287):
```js
// OLD:
const paired = await invoke("get_pairing_status_cmd");
// NEW:
const paired = await invoke("is_paired_cmd");
```

In `init()` (line ~452):
```js
// OLD:
const paired = await invoke("get_pairing_status_cmd");
// NEW:
const paired = await invoke("is_paired_cmd");
```

- [ ] **Step 2: Add `pendingPassphrase` variable and rewrite `handleSavePassphrase`**

In `src/main.js`, find the `// ── Passphrase generator` comment block. Add the `pendingPassphrase` variable just above the `handleSavePassphrase` function. Then replace the entire `handleSavePassphrase` function:

```js
// Holds passphrase between Pair tab and primer screen
let pendingPassphrase = null;

async function handleSavePassphrase() {
  const passphrase = document.getElementById("passphrase-input").value.trim();
  if (!passphrase) {
    setPairMsg("Enter a passphrase first.", "error");
    return;
  }
  if (passphrase.length < 8) {
    setPairMsg("Passphrase must be at least 8 characters.", "error");
    return;
  }
  pendingPassphrase = passphrase;
  document.getElementById("primer-error").classList.add("hidden");
  showScreen("screen-keychain-primer");
}
```

- [ ] **Step 3: Add primer screen handlers**

Add these two functions immediately after `handleSavePassphrase`:

```js
async function handlePrimerContinue() {
  const passphrase = pendingPassphrase;
  if (!passphrase) return;
  const continueBtn = document.getElementById("btn-primer-continue");
  const backBtn = document.getElementById("btn-primer-back");
  const errorEl = document.getElementById("primer-error");
  continueBtn.disabled = true;
  backBtn.disabled = true;
  errorEl.classList.add("hidden");
  try {
    await invoke("save_passphrase_and_cache_cmd", { passphrase });
    await invoke("set_auto_push_cmd", { enabled: document.getElementById("toggle-auto-push").checked });
    await invoke("set_auto_pull_cmd", { enabled: document.getElementById("toggle-auto-pull").checked });
    pendingPassphrase = null;
    showScreen("screen-main");
    showTab("pair");
    await loadPairTab();
    const pairMsg = document.getElementById("pair-msg");
    pairMsg.classList.remove("pair-bounce");
    void pairMsg.offsetWidth;
    pairMsg.classList.add("pair-bounce");
    pairMsg.addEventListener("animationend", () => pairMsg.classList.remove("pair-bounce"), { once: true });
  } catch (err) {
    errorEl.textContent = String(err);
    errorEl.classList.remove("hidden");
  } finally {
    continueBtn.disabled = false;
    backBtn.disabled = false;
  }
}

function handlePrimerBack() {
  pendingPassphrase = null;
  showScreen("screen-main");
  showTab("pair");
}
```

- [ ] **Step 4: Update `handleForgetPassphrase`**

Replace the entire `handleForgetPassphrase` function:

```js
async function handleForgetPassphrase() {
  try {
    await invoke("clear_passphrase_and_cache_cmd");
    document.getElementById("passphrase-input").value = "";
    await loadPairTab();
  } catch (err) {
    setPairMsg(String(err), "error");
  }
}
```

- [ ] **Step 5: Add event listeners for primer buttons**

In the `// ── Event listeners` section (near `document.getElementById("btn-save-passphrase")`), add:

```js
document.getElementById("btn-primer-continue").addEventListener("click", handlePrimerContinue);
document.getElementById("btn-primer-back").addEventListener("click", handlePrimerBack);
```

- [ ] **Step 6: Commit**

```bash
git add src/main.js
git commit -m "feat: add keychain primer screen flow; replace get_pairing_status_cmd with is_paired_cmd"
```

---

### Task 7: Smoke test

- [ ] **Step 1: Run all tests**

```bash
cd src-tauri && cargo test --lib 2>&1 | tail -20
```

Expected: all existing tests plus the two new `pairing::tests` pass.

- [ ] **Step 2: Start dev server**

```bash
cd src-tauri && cargo tauri dev
```

Wait for the window to appear (or not appear if already paired).

- [ ] **Step 3: Test first-launch flow (no passphrase saved)**

If currently paired, click "Forget" first, then restart dev server. Verify:
- Window appears on launch (paired.flag removed)
- Pair tab is active
- No keychain prompt appears at startup

Navigate to Pair tab, enter a passphrase, click "Save & Pair". Verify:
- Priming screen appears with lock icon, "Secure storage" heading, explanation text
- "Go back" returns to Pair tab without any keychain prompt
- Click "Save & Pair" again, then "Yes, continue" — macOS keychain prompt appears
- Click "Always Allow" — app returns to Pair tab with paired/active state

- [ ] **Step 4: Test restart flow (passphrase saved)**

Quit the dev server and restart it. Verify:
- Window does NOT appear on startup (flag file exists)
- No keychain prompt appears
- Daemon begins auto-push/pull behavior normally (passphrase loaded from background task)

- [ ] **Step 5: Test forget flow**

Open the window, go to Pair tab, click "Forget". Verify:
- Pair tab resets to unpaired state
- Restarting the dev server shows the window again (flag file deleted)
