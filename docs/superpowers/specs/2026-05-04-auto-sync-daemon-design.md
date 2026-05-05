# ZynC Auto-Sync Daemon — Design Spec

**Date:** 2026-05-04
**Status:** Approved, ready for implementation

---

## Overview

ZynC gains a persistent background daemon that auto-pushes when Zen closes, auto-pulls when another machine pushes, and keeps the Litterbox link alive by re-uploading before expiry. Machines find each other via a shared passphrase that drives both encryption and an anonymous ntfy.sh pub/sub topic — no account, no cost, no relay server to operate.

---

## Architecture

ZynC becomes a **system tray daemon**: launches on login, lives in the menu bar, opens a window only on demand. The existing push/pull window is unchanged and remains available for manual use.

A single Tokio background task runs continuously, doing three things:
1. Watching the Zen process for close events → auto-push
2. Polling the ntfy topic for incoming file IDs → auto-pull
3. Running a 55-minute refresh timer → re-upload before Litterbox expiry

---

## Pairing Mechanism

### User setup
The user enters the same passphrase on each machine once, in a new Settings screen. ZynC stores it in the OS keychain via the `keyring` crate — never written to disk in plaintext.

### Key derivation (at runtime, never stored)
| Derived value | Method |
|---|---|
| Encryption key | `PBKDF2-HMAC-SHA256(passphrase, APP_SALT, 100_000 rounds)` — same as today, passphrase-based instead of random |
| ntfy topic name | `lowercase-hex(SHA-256(passphrase))` → e.g. `a3f92b14...` |

### What goes over ntfy
Only the Litterbox **file ID** (e.g. `ABC123`). The encryption key is never transmitted. An intercepted ntfy message is useless without the passphrase to re-derive the key. The topic name is a 256-bit secret — not guessable without the passphrase.

### Manual fallback
The `ZEN-KEY-FILEID` sync code still works. With pairing, the KEY segment is deterministic from the passphrase. An unpaired machine can still pull by entering a full code.

---

## Background Daemon

### Auto-push
Trigger: Zen process transitions from running → not running.

1. Wait 3 seconds (allows Zen to finish flushing SQLite)
2. Run WAL checkpoint on `places.sqlite` (already a TODO in `sync.rs`)
3. Call `push_profile()` with passphrase-derived key
4. Publish new Litterbox file ID to ntfy topic
5. Show tray notification: "Profile synced"
6. Start 55-minute refresh timer

### Auto-pull
Trigger: ntfy poll (every 60 seconds) returns a file ID with a message ID newer than the last-seen ID.

The daemon tracks the last-seen ntfy message ID so already-processed notifications are never re-applied.

- **Zen closed** → pull immediately, show tray notification: "Profile updated"
- **Zen open** → store the latest pending file ID (overwrite any earlier queued ID), badge tray icon, notify: "New profile available — will pull when Zen closes"
- **On next Zen close** (with queued pull) → **push first** (B's just-closed changes are authoritative), then **discard the queued pull** — A will auto-pull B's newly published version via ntfy

### Refresh-before-expiry
Trigger: 55-minute timer fires after any push.

1. Check if Zen is running — if yes, skip this cycle and retry in 5 minutes
2. Re-read profile files from disk
3. Re-upload with same passphrase-derived key → new Litterbox file ID
4. Publish new file ID to ntfy
5. Reset timer to 55 minutes

The link stays perpetually live as long as at least one paired machine is running ZynC.

---

## Conflict Handling

**Policy: last-push-wins.**

Both machines can push. Whoever pushed most recently is authoritative. The other machine auto-pulls that version on next notification. Since SQLite files cannot be merged, this is the only practical resolution strategy.

**Data safety:** the existing backup system creates a timestamped `zync-backup-{ts}/` directory before every pull. No data is permanently lost.

**Queued-pull-then-push scenario:** Machine B closes Zen with a queued pull pending. B pulls A's version (backup created), then immediately pushes B's version. B's changes win — correct, since B just finished a local session.

No conflict UI, no merge prompts for v1. If real-world use reveals edge cases, a conflicts/history screen can be added later.

---

## UI Changes

### System tray
- Launches on login; no window on startup (unless first run)
- **Tray menu:** Open ZynC / Last synced: N min ago / Sync now (grayed if Zen open) / Quit
- **Icon states:** idle, syncing (animated), update available (badge dot), error (red)
- Tray notifications for: auto-push success, auto-pull success, errors

### Settings screen (new tab in existing window)
- Passphrase field — masked, with reveal toggle
- Pairing status: "Paired" (green) / "Not paired" (grey)
- Auto-push toggle (default: on)
- Auto-pull toggle (default: on)
- "Forget passphrase" button

### First-run experience
- Window opens automatically to Settings on first launch
- Prompt: "Enter a shared passphrase on each machine to enable automatic sync"
- After saving: switch to main screen; tray takes over

### Deferred
All other UX/UI changes deferred until after first build. Existing push/pull screen untouched.

---

## New Rust Modules

| Module | Responsibility |
|---|---|
| `ntfy.rs` | Publish file ID to topic; poll topic for new file IDs |
| `pairing.rs` | Save/load passphrase via OS keychain; derive encryption key and topic name |
| `daemon.rs` | Background Tokio task: Zen watcher, ntfy poller, refresh timer |

### Modified modules
| Module | Change |
|---|---|
| `sync.rs` | Accept passphrase-derived key instead of generating random; add WAL checkpoint (existing TODO) |
| `lib.rs` | Register tray icon, start daemon task, register new Tauri commands |
| `Cargo.toml` | Add: `sha2`, `keyring`; remove unused: `zip`, `uuid` |

### New Tauri commands
| Command | Purpose |
|---|---|
| `save_passphrase(passphrase)` | Store in OS keychain |
| `get_pairing_status() → bool` | Is passphrase set? |
| `clear_passphrase()` | Forget pairing |
| `get_last_synced() → Option<u64>` | Unix timestamp of last push/pull |
| `manual_sync_now()` | Force push (respects Zen-open check) |

---

## New Frontend
- Settings tab/screen: passphrase field, status indicator, toggles, forget button
- Tray icon asset (all required sizes for macOS/Windows/Linux)
- Notification text strings (no window shown for auto events)

---

## Non-Goals for This Feature
- Merge/diff for SQLite conflicts
- Sync history or rollback UI (backup folder is the recovery path)
- Self-hosted ntfy instance setup (noted as future option if ntfy.sh changes terms)
- Push notification on mobile
