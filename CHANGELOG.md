# Changelog

## 0.4.0

### Added
- Connect a GitHub account in the new Sync tab to enable automatic sync and version history across machines.
- Profile snapshots — up to 10 versions are kept, with machine name and timestamp, so you can restore any previous state.
- Version-aware conflict resolution — if another machine pushed while Zen was open, Zync applies their changes when Zen closes instead of overwriting with your local session.

### Changed
- Replaced the Pair tab and passphrase-based pairing with GitHub-backed sync. The manual Push/Pull flow (ZEN-CODE) is unchanged.
- Automatic sync now uses GitHub Releases for storage instead of Litterbox, so synced profiles are persistent and versioned rather than expiring after one hour.

## 0.3.13

## 0.3.12

### Added
- Render markdown in update release notes

## 0.3.11

### Fixed
- Suppress refresh re-uploads when Zen hasn't been opened since last sync

## 0.3.10

### Fixed
- Guard pending pull against machines that already pushed this session

## 0.3.9

### Fixed
- Drain pending pull before push on Zen close

## 0.3.8

### Fixed
- Strip `zen.updates.*` prefs on pull to prevent update modal from reappearing after sync

## 0.3.7

### Fixed
- Enable updater artifact generation in CI release workflow

## 0.3.6

### Fixed
- Preserve local update prefs on pull
- Simplify pair status message

## 0.3.5

### Fixed
- Prevent new pair from overwriting peers
- Strip Zen update prefs on pull
- Track push count

## 0.3.4

### Fixed
- Surface update check errors in the UI
- Show window when a manual update check finds a new version

## 0.3.3

### Fixed
- Prevent sync direction reversal and inflated sync count

## 0.3.2

### Added
- Check for Updates in tray menu and native menu bar

## 0.3.1

### Added
- Keychain primer screen for first-time pairing setup
- Background-load passphrase cache at startup for existing users

### Fixed
- Refresh pair tab on returning from primer screen
- Guard forget button against double-tap

## 0.3.0

### Added
- Live sync status on Pair tab showing last sync time and count
- Automatic push when Zen closes in paired mode
- Sync-updated events for real-time UI updates

## 0.2.0

### Added
- Initial paired (automatic) sync mode via ntfy.sh
- Daemon background loops: Zen watcher, ntfy poller, refresh timer
- System tray with Open, Sync now, and Quit actions
- Close-to-tray behavior
- Launch on login via autostart plugin

## 0.1.3

### Fixed
- CI build and signing fixes

## 0.1.0

### Added
- Manual Push/Pull sync with `ZEN-KEY-FILEID` codes
- AES-256-GCM encryption with PBKDF2 key derivation
- Litterbox transport (anonymous, no account required)
- Profile auto-detection on macOS, Windows, and Linux
- Zen running detection blocks push/pull while browser is open
- Automatic profile backup before every pull
- macOS code signing and notarization
