# Pairing Verification & Sync Count — Design

**Date:** 2026-05-15
**Status:** Approved

---

## Problem

When two (or more) machines pair with mismatched passphrases, they derive different ntfy topics and never see each other's messages. The failure is completely silent — the UI just says "Paired!" and nothing ever happens. There is also no indication in the UI that pairing is actively working across multiple machines.

---

## Goals

1. Replace the static "Paired!" message with a live status that tells the user whether pairing is actually working.
2. Show a sync count so the user can see that profile exchanges are happening.
3. Surface a passphrase-mismatch hint without requiring any manual action on any machine.

---

## Non-Goals

- Real-time "machine is online now" presence detection (no heartbeat protocol).
- Persistent sync count across app restarts (session-only count is sufficient).
- Proactive timeout timers or alerts.

---

## Approach

**Passive verification through the existing sync flow (Option A).** No protocol changes. The Pair tab's status message reflects what the daemon has observed: if auto-pulls have happened, pairing is working; if not, the message includes a mismatch hint. No manual steps required on any machine; works for N machines.

---

## Data Model

### `DaemonState` (daemon.rs)

Add one field:

```rust
pub sync_count: u32,   // count of successful auto-pulls this session; resets on restart
```

Initialize to `0` in `Default`.

### New Tauri command

```rust
#[derive(serde::Serialize)]
pub struct SyncStatus {
    pub sync_count: u32,
    pub last_synced: Option<u64>,   // unix timestamp; already exists on DaemonState
}

#[tauri::command]
pub fn get_sync_status_cmd(
    state: tauri::State<Arc<Mutex<DaemonState>>>,
) -> SyncStatus {
    let s = state.lock().unwrap();
    SyncStatus { sync_count: s.sync_count, last_synced: s.last_synced }
}
```

Register in `lib.rs` `invoke_handler` alongside other daemon commands.

### Incrementing `sync_count`

Increment at both existing auto-pull success paths (the same two places that already set `last_synced`):

1. `ntfy_poll_tick` — after a successful `sync::auto_pull` call
2. `zen_watcher_tick` drain path — after a successful `sync::auto_pull` call

Do **not** increment on manual push/pull (those are user-initiated via sync code, not peer activity).

---

## Pair Tab Status States

The `pair-msg` element shows one of three states:

| State | Condition | Message | Color |
|---|---|---|---|
| Unpaired | No passphrase saved | "Enter the same passphrase on each machine to enable automatic sync." | `msg-neutral` |
| Paired, waiting | Passphrase saved, `sync_count == 0` | "Paired — waiting for other machines. Check passphrases match if nothing syncs." | `msg-neutral` |
| Active | `sync_count > 0` | "Active — N syncs · last X ago" | `msg-success` |

The waiting message deliberately folds the mismatch hint inline. A user who typed the passphrase correctly will see the status flip to "Active" within 60 seconds of the first auto-pull. A user who typed it wrong will read the hint immediately.

Both messages are sized to fit within two lines in the 324px text area (420px window, 32px content padding, 16px msg-box padding).

---

## Frontend Changes (main.js)

### `timeAgo` helper

```js
function timeAgo(unixSecs) {
  const diff = Math.floor(Date.now() / 1000) - unixSecs;
  if (diff < 60)    return "just now";
  if (diff < 3600)  return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} hr ago`;
  return `${Math.floor(diff / 86400)} days ago`;
}
```

### Updated `loadPairTab`

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

Status refreshes on tab switch and after "Save & Pair". No polling loop — the count does not tick up in real time while the tab is open, which is an acceptable trade-off for simplicity.

---

## Files Changed

| File | Change |
|---|---|
| `src-tauri/src/daemon.rs` | Add `sync_count` field; add `SyncStatus` struct and `get_sync_status_cmd`; increment `sync_count` in two pull success paths |
| `src-tauri/src/lib.rs` | Register `get_sync_status_cmd` in `invoke_handler` |
| `src/main.js` | Add `timeAgo` helper; update `loadPairTab` to call `get_sync_status_cmd` and render the three states |

No changes to `index.html`, `style.css`, `ntfy.rs`, `pairing.rs`, or any other files.

---

## Testing

- **Unpaired state:** Clear passphrase, open Pair tab — should show the unpaired message.
- **Paired, waiting:** Save passphrase, open Pair tab before any auto-pull — should show the waiting message.
- **Active:** Trigger an auto-pull (e.g., via manual sync from another machine or by simulating a daemon pull) — count should increment and message should flip to "Active — 1 sync · last just now".
- **Pluralisation:** After 2+ syncs, confirm "syncs" (plural) is shown.
- **timeAgo formatting:** Verify each time bucket (just now, min ago, hr ago, days ago) displays correctly.
