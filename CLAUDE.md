# ZynC — Claude Code Handoff

## What This Is

A cross-platform desktop app that syncs Zen Browser profiles between multiple computers.
Zen Browser is a Firefox-based browser with proprietary data (workspaces, pinned tabs, themes)
stored in a SQLite database that Firefox Sync does not cover. This app solves that gap.

---

## Build Status

**As of 2026-05-12 — fully implemented and shipping signed/notarized releases via GitHub Actions.**

| Module | Status |
|---|---|
| UI shell (Push/Pull/Pair tabs, settings) | Done |
| Profile folder auto-detection | Done |
| Zen running detection | Done |
| `crypto.rs` — AES-256-GCM + PBKDF2 | Done, unit tested |
| `transport.rs` — Litterbox upload/download | Done |
| `sync.rs` — bundle push/pull + backup + WAL checkpoint | Done |
| `pairing.rs` — shared passphrase in OS keychain, ntfy topic derivation | Done |
| `ntfy.rs` — ntfy.sh pub/sub adapter | Done |
| `daemon.rs` — background auto-sync loops | Done |
| System tray (Open, Sync now, Quit) | Done |
| Close-to-tray | Done |
| Launch-on-login (autostart plugin) | Done |
| macOS code signing + notarization | Done (Developer ID Application: Jesse Wallace J4Z24X9XFQ) |
| Same-machine round-trip test | **TODO** |
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
- **Manual mode (Push/Pull tab):** One machine pushes, generates a `ZEN-KEY-FILEID` code.
  Any machine enters the code to pull. No pairing required.
- **Automatic mode (Pair tab):** Machines share a passphrase. The daemon watches for Zen
  to close, auto-pushes the profile to Litterbox, and publishes the file ID to an ntfy.sh
  topic derived from the passphrase. Other paired machines poll ntfy every 60 seconds
  and auto-pull when a new file ID arrives.
- **Code format (manual):** `ZEN-{6-char-key}-{litterbox-file-id}` e.g. `ZEN-A3F9B2-ABC123`
  - First part (6 uppercase hex chars) is a random encryption key
  - Second part is the Litterbox file ID, which reconstructs the download URL
- **Daemon encryption:** Uses the raw passphrase string as the crypto key (no random component)
  so receiving machines can decrypt without out-of-band key exchange.
- **ntfy topic:** SHA-256(passphrase) as 64-char lowercase hex — deterministic, never
  transmitted in plaintext.
- **Async:** Push and pull do NOT need to happen simultaneously. The daemon re-uploads
  every 55 minutes to keep the Litterbox link alive.
- **Encryption:** AES-256-GCM. Key derived via PBKDF2-HMAC-SHA256 (100k rounds).
  Wire format: `[12-byte nonce][ciphertext+GCM tag]`.

### Daemon background loops (daemon.rs)
Three loops run from app startup:
1. **Zen watcher (every 5s)** — detects Zen→closed edge. If a pull is queued (peer
   pushed while Zen was open), drains that pull and skips the local push — peer data
   takes priority. If no pull is queued and auto-push is enabled, pushes the local profile.
2. **ntfy poller (every 60s)** — polls the shared ntfy topic for new file IDs.
   Pulls immediately if Zen is closed; queues the file ID if Zen is open (pulls when Zen closes).
3. **Refresh timer (every 5min, triggers at 55min mark)** — re-uploads to keep the
   Litterbox link alive before the 1h expiry. Defers by 5min if Zen is open.

Concurrent push is guarded by `Arc<AtomicBool>` — if a push is already in flight, the
second trigger is silently dropped.

### Tray and window behavior
- App starts hidden (window `visible: false`) — shows on first launch only if no passphrase
  is saved (so new users see the Pair tab immediately).
- Close button hides to tray instead of quitting.
- Tray menu: Open Zync | Sync now | — | Quit.
- Launch-on-login enabled automatically via `tauri-plugin-autostart` (MacosLauncher::LaunchAgent).

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

### Manual Push (export from this machine)
1. App detects Zen is closed (warns if open, blocks operation)
2. User clicks **Push** tab → **Push**
3. App runs WAL checkpoint on `places.sqlite`, collects sync files
4. Generates 3 random bytes → 6 uppercase hex chars → encryption key
5. Encrypts bundle with AES-256-GCM
6. POSTs encrypted blob to Litterbox (`time=1h`)
7. Displays `ZEN-{key}-{fileId}` with 1-hour countdown

### Manual Pull (import to this machine)
1. User enters sync code on the Pull tab
2. App parses code → derives download URL and decryption key
3. Fetches encrypted blob from Litterbox, decrypts, deserializes `SyncBundle`
4. Detects Zen is closed (warns if open, blocks operation)
5. Backs up current profile files to `{profile}/zync-backup-{timestamp}/`
6. Writes synced files; removes stale WAL/SHM files alongside any written `.sqlite`
7. Shows list of written files

### Automatic sync (paired machines)
1. User enters a shared passphrase on the Pair tab on both machines; saves it to OS keychain
2. When Zen closes on machine A, daemon detects the edge, auto-pushes the profile,
   publishes the Litterbox file ID to the ntfy topic
3. Machine B polls ntfy every 60s. If Zen is closed it pulls immediately; if open it
   queues and pulls when Zen closes
4. Both machines show OS notifications on sync events

---

## UI Design

- **Aesthetic:** Zen Browser's design language — dark, minimal, slightly rounded, purple/coral accent
- **Window size:** 420×460px, not resizable
- **Tabs:** Pull | Pair (shown on main screen)
- **Screens:** Main (tabbed) → Push result (code + countdown) → Pull result (file list)
- **Pair tab:** Passphrase input with random generator, auto-push/pull toggles, save/forget buttons
- **Error states:** Plain English inline below the buttons

---

## Release Process

When the user asks to cut a release or bump the version, follow these steps in order:

1. **Determine the new version** — ask the user if not specified (semver: patch for fixes, minor for new features).

2. **Write the changelog entry** — run `git log v{PREVIOUS_VERSION}..HEAD --oneline` to get all commits since the last tag. Synthesize them into a `## {NEW_VERSION}` block at the top of `CHANGELOG.md` using these four sections (omit any section that has no entries):
   - `### Security` — anything affecting encryption, auth, or data integrity
   - `### Fixed` — bug fixes
   - `### Changed` — behavior changes to existing features
   - `### Added` — new features or capabilities
   
   Write entries as short, user-facing sentences (not commit message fragments). Only include things a user of the app would notice. Exclude everything else — specifically: version bumps, `chore:` commits, `docs:` commits, changes to `CLAUDE.md` or other internal docs, release workflow changes, CI fixes, refactors with no behavior change, and any internal tooling or process work.

3. **Bump the version** — update `version` in both `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` to the new version string.

4. **Commit** — stage `CHANGELOG.md`, `src-tauri/tauri.conf.json`, and `src-tauri/Cargo.toml`. Commit message: `chore: bump to {NEW_VERSION}`.

5. **Tag** — `git tag v{NEW_VERSION}`.

6. **Confirm before pushing** — show the user the changelog entry and ask them to confirm before running `git push && git push --tags`.

---

## CI / Release

- **Workflow:** `.github/workflows/release.yml` — triggers on `v*.*.*` tags
- **Platforms:** macOS (universal arm64+x86_64), Windows, Linux
- **macOS signing:** Developer ID Application: Jesse Wallace (J4Z24X9XFQ)
- **Notarization:** Uses `APPLE_ID` + `APPLE_PASSWORD` (app-specific) + `APPLE_TEAM_ID`
- **Secrets required:** `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`,
  `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD`, `APPLE_TEAM_ID`
- **Known:** GitHub Actions Node.js 20 deprecation warning (non-breaking until 2026-09-16)

---

## Non-Goals for v1

- Password sync (`logins.json` / `key4.db`)
- Extension settings sync (only the extensions list)
- Selective workspace sync
- Sync history or rollback UI (backup is silent)
- Mobile support

---

## Key Technical Notes

### SQLite / WAL
- `places.sqlite` is locked while Zen is open — enforced by zen_check
- Before reading `places.sqlite` for push, `sync.rs` runs `PRAGMA wal_checkpoint(TRUNCATE)`
  via `rusqlite` to merge any un-checkpointed WAL data into the main file
- After writing `.sqlite` files on pull, stale `-wal` and `-shm` files are deleted to prevent
  the destination's old WAL from being replayed on top of the synced data

### ntfy security model
- ntfy topic = SHA-256(passphrase) — publicly guessable only if the passphrase is known
- Only the Litterbox file ID is published to ntfy, never the encryption key
- An attacker who intercepts ntfy messages gets a file ID they can't decrypt
- The passphrase (stored in OS keychain) is never transmitted

### Sync code security (manual mode)
- Key space: 2^24 (16M values) × 100k PBKDF2 rounds ≈ 11h GPU brute-force
- Litterbox expiry: 1h — the attacker window is shorter than the crack time

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
- Process detection via `sysinfo` crate
- No frontend filesystem plugin — file I/O is done entirely in Rust commands
- macOS entitlements: `com.apple.security.cs.allow-jit` (WKWebView) +
  `com.apple.security.network.client` (Litterbox + ntfy)
- App is NOT sandboxed — keyring works without `keychain-access-groups` entitlement

### Linux known issue
The `keyring` crate uses `sync-secret-service` feature which requires `libdbus-1` at
AppImage build time. May need to switch to `linux-secret-service` (async) to avoid
native lib linking issues in CI.

---

## Project Structure

```
zync/
├── src-tauri/
│   ├── src/
│   │   ├── main.rs          Entry point — calls lib::run()
│   │   ├── lib.rs           Tauri builder, tray setup, daemon spawn, command registration
│   │   ├── profile.rs       Profile folder detection + file collection
│   │   ├── sync.rs          Push/pull bundle logic, WAL checkpoint, auto_push/auto_pull
│   │   ├── transport.rs     Litterbox adapter (upload/download/code parsing)
│   │   ├── crypto.rs        AES-256-GCM encrypt/decrypt (unit tested)
│   │   ├── zen_check.rs     Detect if Zen process is running
│   │   ├── pairing.rs       OS keychain passphrase storage, ntfy topic derivation
│   │   ├── ntfy.rs          ntfy.sh publish + poll_since adapter
│   │   └── daemon.rs        Background loops: Zen watcher, ntfy poller, refresh timer
│   ├── capabilities/
│   │   └── default.json     Tauri v2 capability declarations (tray, notification, autostart)
│   ├── icons/
│   │   └── icon.png         Placeholder — replace with real icon
│   ├── entitlements.plist   macOS hardened runtime entitlements
│   ├── Cargo.toml
│   └── tauri.conf.json
├── src/
│   ├── index.html
│   ├── main.js
│   └── style.css
├── .github/
│   └── workflows/
│       └── release.yml      CI: builds + signs + notarizes on v* tags
└── CLAUDE.md
```
