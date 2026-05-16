# Keychain Onboarding Design

**Date:** 2026-05-16  
**Status:** Approved

## Problem

On first launch, macOS shows a security dialog — "Zync wants to access key 'zync' in your keychain" — before the user has seen any app UI. Keychain is hit in seven places today:

- `lib.rs:85` — Rust startup, before window appears (`get_pairing_status_cmd`)
- `main.js:452` — JS `init()` call to `get_pairing_status_cmd`
- `main.js:287` — `loadPairTab()` call to `get_pairing_status_cmd`
- `main.js:299`, `:321` — two calls to `get_passphrase_cmd` in `loadPairTab`
- `daemon.rs` — three background loops reading keychain every 5 s / 60 s / 5 min

If the user clicks "Allow" instead of "Always Allow," the daemon re-prompts every 5 seconds.

## Solution: Flag file + memory cache + priming screen

### 1. Flag file

A zero-byte file at `{app_data_dir}/paired.flag` (e.g. `~/Library/Application Support/com.zync.app/paired.flag` on macOS) is the source of truth for "has this user ever saved a passphrase."

**Changes to `pairing.rs`:**

Add three functions that take `dir: &Path` (so the module stays free of Tauri types):
- `write_paired_flag(dir: &Path)` — creates the file on save
- `clear_paired_flag(dir: &Path)` — deletes it on forget (no-op if missing)
- `is_paired_flag(dir: &Path) -> bool` — checks existence

**Changes to `lib.rs`:**

- Replace the `get_pairing_status_cmd()` call at line 85 with `pairing::is_paired_flag(&data_dir)`, where `data_dir` comes from `app.path().app_data_dir()`.

**New Tauri command `is_paired_cmd`:**

Reads the flag file only. Replaces all `get_pairing_status_cmd` calls in JS `init()` and `loadPairTab()`. The existing `get_pairing_status_cmd` (which reads the keychain) is kept for internal use but removed from the JS surface.

**`save_passphrase_cmd` and `clear_passphrase_cmd`:**

Both gain `app: tauri::AppHandle` so they can resolve the app data dir and call `write_paired_flag` / `clear_paired_flag` alongside existing keychain logic.

### 2. Memory cache

`DaemonState` gains:

```rust
pub passphrase: Option<String>,
```

**Daemon tick functions** (`zen_watcher_tick`, `ntfy_poll_tick`, `refresh_tick`) stop calling `pairing::load_passphrase()` directly. Each reads `state.lock().unwrap().passphrase.clone()` and returns early if `None`.

**Cache population:**

1. **User saves passphrase** — `save_passphrase_cmd` also sets `state.passphrase = Some(passphrase)` on the managed `DaemonState`.
2. **App restart with existing pairing** — a one-time async task spawned in `lib.rs` setup: if `is_paired_flag()` is true, read keychain once and write to `DaemonState.passphrase`. Runs in background after the window-show decision, so the user sees the window before any keychain access.

**Cache clearing:** `clear_passphrase_cmd` sets `state.passphrase = None`.

### 3. Priming screen

A new `screen-keychain-primer` screen in `index.html` (same structure as existing screens).

**Contents:**
- Lock icon (Phosphor style, matching app aesthetic)
- Heading: "Secure storage"
- Body: "Your passphrase will be stored in your Mac's Keychain. macOS will ask for permission — click **Always Allow** so you're not asked again."
- Buttons: **Go back** (secondary) | **Yes, continue** (primary)
- Error area below buttons for save failures

**Flow:**

1. "Save & Pair" click → validate passphrase (existing length check) → store passphrase in JS local variable → `showScreen("screen-keychain-primer")`
2. "Go back" → `showScreen("screen-main")`, `showTab("pair")` — keychain untouched
3. "Yes, continue" → `invoke("save_passphrase_cmd")` → `set_auto_push_cmd` / `set_auto_pull_cmd` → `showScreen("screen-main")`, `showTab("pair")` → `loadPairTab()` renders paired/active state
4. Error during save → stay on primer screen, show inline error below buttons

The priming screen is shown on every "Save & Pair" click (not just first launch) because the keychain dialog can re-appear if the user previously chose "Allow" instead of "Always Allow."

## Files changed

| File | Change |
|------|--------|
| `src-tauri/src/pairing.rs` | Add `write_paired_flag`, `clear_paired_flag`, `is_paired_flag`; add `app: AppHandle` to save/clear commands; add `is_paired_cmd` Tauri command |
| `src-tauri/src/daemon.rs` | Add `passphrase: Option<String>` to `DaemonState`; update three tick functions to read from state |
| `src-tauri/src/lib.rs` | Replace startup keychain check with flag file check; spawn one-time passphrase-load background task; update `invoke_handler` to register `is_paired_cmd` |
| `src/index.html` | Add `screen-keychain-primer` screen markup |
| `src/main.js` | Replace `get_pairing_status_cmd` calls with `is_paired_cmd`; rewrite `handleSavePassphrase` to go through priming screen; add primer screen event handlers |

## Success criteria

- Fresh install: no keychain prompt until user clicks "Yes, continue" on the priming screen
- Existing user restart: keychain read happens silently in background (they've already "Always Allowed")
- Daemon never triggers a keychain prompt under any circumstance
- "Go back" from priming screen leaves Pair tab state exactly as the user left it
