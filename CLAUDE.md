# ZynC — Claude Code Handoff

## What This Is

A cross-platform desktop app that syncs Zen Browser profiles between multiple computers.
Zen Browser is a Firefox-based browser with proprietary data (workspaces, pinned tabs, themes)
stored in a SQLite database that Firefox Sync does not cover. This app solves that gap.

---

## Build Status

**As of 2026-05-04 — core implementation complete, not yet end-to-end tested.**

| Module | Status |
|---|---|
| UI shell (Push/Pull/status screens) | Done |
| Profile folder auto-detection | Done |
| Zen running detection | Done |
| `crypto.rs` — AES-256-GCM + PBKDF2 | Done, unit tested |
| `transport.rs` — Litterbox upload/download | Done |
| `sync.rs` — bundle push/pull + backup | Done |
| WAL checkpoint before push | **TODO** |
| Same-machine round-trip test | **TODO** |
| Production macOS entitlements | **TODO** |
| Real app icon | **TODO** |

### To run in development
```
cd src-tauri && cargo tauri dev
```

### To run tests
```
cd src-tauri && cargo test
```

---

## Decisions Already Made

### Stack
- **Framework:** Tauri v2 (Rust backend + vanilla HTML/CSS/JS frontend)
- **UI:** HTML/CSS/JS (no framework — keep it lightweight)
- **Target platforms:** macOS (.dmg), Windows (.exe/.msi), Linux (.AppImage)
- **Build command:** `cargo tauri build` → produces platform installers

### Sync Transport
- **Service:** Litterbox (litterbox.catbox.moe) — free, anonymous, no account required
- **API:** Single POST endpoint, returns a URL, files auto-expire (1h, 12h, 24h, or 72h)
- **Default expiry:** 1 hour (sync sessions are short-lived by design)
- **Architecture:** Fully distributed — each user's device calls Litterbox directly.
  No proxy, no aggregation, no relay server.
- **Transport is abstracted** in `transport.rs` — swap to file.io or self-hosted by replacing
  `upload()` and `download()` only.

### Sync Model
- **Topology:** Star — one machine pushes, any number of machines pull using the same code
- **Code format:** `ZEN-{6-char-key}-{litterbox-file-id}` e.g. `ZEN-A3F9B2-ABC123`
  - First part (6 hex chars) is the encryption key derived via PBKDF2
  - Second part is the Litterbox file ID, which reconstructs the download URL
  - No relay server needed — the code encodes both WHERE to fetch and HOW to decrypt
- **Async:** Push and pull do NOT need to happen simultaneously
- **Encryption:** AES-256-GCM. Key = PBKDF2-HMAC-SHA256(key_hex, APP_SALT, 100k rounds).
  Litterbox never sees plaintext. Wire format: `[12-byte nonce][ciphertext+GCM tag]`.

### Why the code format changed from the original spec
The original spec said `ZEN-XXXX` (4 chars derived from the Litterbox URL hash). That's a
circular dependency: you need the URL to derive the code, but you need the code as the
encryption key before you upload. The two-part format (`ZEN-KEY-FILEID`) resolves this:
generate the key first, encrypt, upload, then embed the returned file ID in the code.

---

## What Gets Synced

Zen must be CLOSED before push or pull. The app detects this and blocks if Zen is running.

### Include by default
| Data | File |
|---|---|
| Pinned tabs + workspaces + bookmarks | `places.sqlite` |
| Workspace names, themes, tab assignments | `zen-sessions.jsonlz4` |
| Workspace icons + colors (stored as containers) | `containers.json` |
| Live folders | `zen-live-folders.jsonlz4` |
| Browser preferences | `prefs.js` |
| Extensions list | `extensions.json` |
| Mods config (enabled list) | `zen-themes.json` |
| Mods CSS (compiled active styles) | `chrome/zen-themes.css` |
| Keyboard shortcuts | `zen-keyboard-shortcuts.json` |

### Exclude
- Cache files and folders
- `key4.db` / `logins.json` (passwords — sensitive, opt-in only in future)
- `sessionstore.jsonlz4` (open tab session — machine-specific)
- `storage/` folder (extension data caches)
- Any file over 5 MB that isn't in the include list above

### Profile folder locations
- **macOS:** `~/Library/Application Support/zen/Profiles/` — look for a subfolder
  containing `release` in its name (e.g. `tcuo77lt.Default (release)`)
- **Windows:** `%APPDATA%/zen/Profiles/` — same pattern
- **Linux:** `~/.zen/Profiles/` — same pattern; Flatpak fallback:
  `~/.var/app/app.zen_browser.zen/zen/Profiles/`

**Known profile naming:** On macOS the active profile is named `{hash}.Default (release)`,
NOT `{hash}.release` as older docs suggest. Detection matches any folder containing "release".

---

## Core User Flows

### Push (export from this machine)
1. App detects Zen is closed (warns if open, blocks operation)
2. App finds profile folder automatically
3. User clicks **Push**
4. App reads the five sync files into a `SyncBundle` (files base64-encoded, JSON-serialized)
5. Generates 3 random bytes → 6 hex chars → encryption key
6. Encrypts bundle with AES-256-GCM
7. POSTs encrypted blob to Litterbox (`time=1h`)
8. Extracts Litterbox file ID from response URL
9. Displays `ZEN-{key}-{fileId}` prominently with 1-hour countdown

### Pull (import to this machine)
1. User enters sync code on another machine
2. App parses code → derives download URL and decryption key
3. Fetches encrypted blob from Litterbox
4. Decrypts and deserializes `SyncBundle`
5. Detects Zen is closed (warns if open, blocks operation)
6. Backs up current profile files to `{profile}/zync-backup-{timestamp}/`
7. Writes synced files to profile folder
8. Shows list of written files

### Multi-machine sync
- Same push code works for any number of machines while the 1h window is open
- Each pull is independent — machines don't need to be online simultaneously

---

## UI Design

- **Aesthetic:** Zen Browser's design language — dark, minimal, slightly rounded, purple accent
- **Window size:** 420×340px, not resizable
- **Screens:** Main (Push + Pull input) → Push result (code + countdown) → Pull result (file list)
- **No settings screen for v1** — auto-detect everything, sensible defaults
- **Error states:** Plain English inline below the buttons

---

## Non-Goals for v1

- Password sync (`logins.json` / `key4.db`)
- Extension settings sync (only the extensions list)
- Selective workspace sync
- Sync history or rollback UI (backup is silent)
- Auto-sync / background daemon
- Mobile support

---

## Key Technical Notes

### SQLite / WAL (TODO before shipping)
- `places.sqlite` is locked while Zen is open — enforced by zen_check
- When Zen is closed, SQLite may leave a WAL file (`places.sqlite-wal`) with un-checkpointed
  data. Before reading `places.sqlite` for push, run:
  `PRAGMA wal_checkpoint(TRUNCATE)` via `rusqlite` to merge WAL into the main file.
- This is not yet implemented — add to `sync.rs` `push_profile()` before the file read.

### Sync code security
- Key space: 2^24 (16M values) × 100k PBKDF2 rounds ≈ 11h GPU brute-force
- Litterbox expiry: 1h — the attacker window is shorter than the crack time
- If higher security is needed in future, increase key bytes from 3 → 4 (32-bit = 11 days)

### Litterbox API
```
POST https://litterbox.catbox.moe/resources/internals/api.php
Content-Type: multipart/form-data

reqtype=fileupload
time=1h
fileToUpload=<binary>

Response: plain text URL e.g. https://litter.catbox.moe/abc123.bin
```

### Tauri v2 specifics
- `withGlobalTauri: true` in `tauri.conf.json` → JS uses `window.__TAURI__.core.invoke`
- Rust commands use `#[tauri::command]` + registered in `lib.rs` `invoke_handler`
- Process detection via `sysinfo` crate (not deprecated Tauri v1 API)
- No frontend filesystem plugin needed — file I/O is done entirely in Rust commands
- macOS production builds need `entitlements.plist` with file access rights (not yet added)

### Crates to remove (unused)
`zip` and `uuid` are in `Cargo.toml` but never used — remove before production build.

---

## Project Structure

```
zync/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs          Entry point — calls lib::run()
│   │   ├── lib.rs           Tauri builder, registers commands
│   │   ├── profile.rs       Profile folder detection + file collection
│   │   ├── sync.rs          Push/pull bundle logic (TODO: WAL checkpoint)
│   │   ├── transport.rs     Litterbox adapter (upload/download/code parsing)
│   │   ├── crypto.rs        AES-256-GCM encrypt/decrypt (unit tested)
│   │   └── zen_check.rs     Detect if Zen process is running
│   ├── capabilities/
│   │   └── default.json     Tauri v2 capability declarations
│   ├── icons/
│   │   └── icon.png         Placeholder — replace with real icon
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── index.html
│   ├── main.js
│   └── style.css
└── CLAUDE.md
```
