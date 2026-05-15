# Pairing Verification & Sync Count Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the static "Paired!" message with a live status that shows how many auto-syncs have occurred and surfaces a passphrase-mismatch hint when no activity has been observed.

**Architecture:** Add `sync_count: u32` to `DaemonState`, expose it through a new `get_sync_status_cmd` Tauri command, increment it in both existing auto-pull success paths, and update `loadPairTab` in `main.js` to render one of three status states based on whether pairing is new or has seen activity.

**Tech Stack:** Rust / Tauri v2, vanilla JS (`src/main.js`)

---

## Task 1: Add `sync_count` to `DaemonState` with a unit test

**Files:**
- Modify: `src-tauri/src/daemon.rs`

- [ ] **Step 1: Write a failing unit test**

Add this test at the bottom of `src-tauri/src/daemon.rs` inside a new `#[cfg(test)]` block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_count_defaults_to_zero() {
        let state = DaemonState::default();
        assert_eq!(state.sync_count, 0);
    }
}
```

- [ ] **Step 2: Run the test and confirm it fails**

```bash
cd src-tauri && cargo test sync_count_defaults_to_zero 2>&1
```

Expected: compile error — `no field 'sync_count' on type 'DaemonState'`

- [ ] **Step 3: Add the field to `DaemonState` and its `Default`**

In the `DaemonState` struct (after `is_pushing`, around line 24), add:

```rust
    /// Count of successful auto-pulls received from peers this session. Resets on restart.
    pub sync_count: u32,
```

In `impl Default for DaemonState`, add `sync_count: 0,` alongside the other fields:

```rust
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
        }
    }
}
```

- [ ] **Step 4: Run the test and confirm it passes**

```bash
cd src-tauri && cargo test sync_count_defaults_to_zero 2>&1
```

Expected:
```
test daemon::tests::sync_count_defaults_to_zero ... ok
```

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/daemon.rs
git commit -m "feat: add sync_count field to DaemonState"
```

---

## Task 2: Add `SyncStatus` struct, `get_sync_status_cmd`, and register it

**Files:**
- Modify: `src-tauri/src/daemon.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: Add `SyncStatus` and `get_sync_status_cmd` to `daemon.rs`**

After the existing `get_last_synced_cmd` function (around line 50), add:

```rust
#[derive(serde::Serialize)]
pub struct SyncStatus {
    pub sync_count: u32,
    pub last_synced: Option<u64>,
}

#[tauri::command]
pub fn get_sync_status_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> SyncStatus {
    let s = state.lock().unwrap();
    SyncStatus { sync_count: s.sync_count, last_synced: s.last_synced }
}
```

- [ ] **Step 2: Register the command in `lib.rs`**

In the `invoke_handler` list in `src-tauri/src/lib.rs` (around line 108), add `daemon::get_sync_status_cmd` alongside the other daemon commands:

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
            daemon::get_sync_status_cmd,
            daemon::set_auto_push_cmd,
            daemon::set_auto_pull_cmd,
            daemon::manual_sync_now_cmd,
            install_update,
        ])
```

- [ ] **Step 3: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Run all tests**

```bash
cd src-tauri && cargo test 2>&1
```

Expected: all tests pass (currently 12 + the new one from Task 1 = 13 total).

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/daemon.rs src-tauri/src/lib.rs
git commit -m "feat: add get_sync_status_cmd Tauri command"
```

---

## Task 3: Increment `sync_count` in both auto-pull success paths

**Files:**
- Modify: `src-tauri/src/daemon.rs`

There are exactly two places where a peer auto-pull succeeds and `last_synced` is set. Both need `sync_count += 1` added in the same lock scope.

- [ ] **Step 1: Update `ntfy_poll_tick`**

Find the auto-pull success arm inside `ntfy_poll_tick` (around line 279). It currently reads:

```rust
            Ok(_) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                state.lock().unwrap().last_synced = Some(now);
                show_notification(app, "Profile updated from another machine");
            }
```

Replace it with:

```rust
            Ok(_) => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                {
                    let mut s = state.lock().unwrap();
                    s.last_synced = Some(now);
                    s.sync_count += 1;
                }
                show_notification(app, "Profile updated from another machine");
            }
```

- [ ] **Step 2: Update `zen_watcher_tick` drain path**

Find the auto-pull success arm inside the `zen_watcher_tick` drain block (around line 208). It currently reads:

```rust
                    Ok(_) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        state.lock().unwrap().last_synced = Some(now);
                        show_notification(app, "Profile updated from another machine");
                    }
```

Replace it with:

```rust
                    Ok(_) => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        {
                            let mut s = state.lock().unwrap();
                            s.last_synced = Some(now);
                            s.sync_count += 1;
                        }
                        show_notification(app, "Profile updated from another machine");
                    }
```

- [ ] **Step 3: Run all tests**

```bash
cd src-tauri && cargo test 2>&1
```

Expected: all 13 tests pass, no warnings about unused fields.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/daemon.rs
git commit -m "feat: increment sync_count on every successful auto-pull"
```

---

## Task 4: Update `main.js` — `timeAgo` helper and new `loadPairTab`

**Files:**
- Modify: `src/main.js`

- [ ] **Step 1: Add the `timeAgo` helper**

In `src/main.js`, add the following function immediately before the `// ── Pair tab ──` comment block (around line 272):

```js
// ── Time helper ───────────────────────────────────────────────

function timeAgo(unixSecs) {
  const diff = Math.floor(Date.now() / 1000) - unixSecs;
  if (diff < 60)    return "just now";
  if (diff < 3600)  return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} hr ago`;
  return `${Math.floor(diff / 86400)} days ago`;
}
```

- [ ] **Step 2: Replace `loadPairTab`**

Find the existing `loadPairTab` function (around line 274) and replace it entirely with:

```js
async function loadPairTab() {
  const paired = await invoke("get_pairing_status_cmd");
  if (!paired) {
    setPairMsg("Enter the same passphrase on each machine to enable automatic sync.");
    document.getElementById("passphrase-input").value = "";
    return;
  }

  const { sync_count, last_synced } = await invoke("get_sync_status_cmd");

  if (sync_count === 0) {
    setPairMsg(
      "Paired — waiting for other machines. Check passphrases match if nothing syncs.",
      "neutral"
    );
  } else {
    setPairMsg(
      `Active — ${sync_count} sync${sync_count === 1 ? "" : "s"}${last_synced ? ` · last ${timeAgo(last_synced)}` : ""}`,
      "success"
    );
  }

  try {
    const passphrase = await invoke("get_passphrase_cmd");
    document.getElementById("passphrase-input").value = passphrase ?? "";
  } catch (_) {
    document.getElementById("passphrase-input").value = "";
  }
}
```

- [ ] **Step 3: Build and smoke-test in dev mode**

```bash
cd /path/to/zync && cargo tauri dev
```

Open the app, click the **Pair** tab and verify:
- If no passphrase is set: shows "Enter the same passphrase on each machine…"
- After saving a passphrase: shows "Paired — waiting for other machines…" in neutral style
- The passphrase input is populated when already paired

(Testing the "Active" state requires an actual auto-pull from a peer, or temporarily setting `sync_count` to a non-zero value in a debug build.)

- [ ] **Step 4: Commit**

```bash
git add src/main.js
git commit -m "feat: show live sync status on Pair tab"
```
