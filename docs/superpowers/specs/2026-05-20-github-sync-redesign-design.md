# Zync — GitHub-Backed Sync Redesign (Phase 1)

**Date:** 2026-05-20  
**Status:** Approved, ready for implementation planning  
**Scope:** Replace Litterbox transport and session-heuristic conflict resolution with GitHub Releases storage and version-aware conflict detection. Add 10-snapshot rollback. Simplify Pair tab into Sync tab.

---

## Problem Statement

The current sync architecture uses Litterbox (temporary file hosting) as transport and a set of in-memory session heuristics to decide whether to push or pull when Zen closes. These heuristics are fragile:

- They reset on every Zync restart, making restart a source of data loss
- `push_count > 0` cannot distinguish "I pushed before the peer" from "I pushed after the peer"
- The 60-second ntfy poll window means a peer push can arrive after the local push decision has already fired
- The 55-minute refresh loop exists only to keep Litterbox links alive, adding state complexity and spurious re-uploads

**Root scenario that motivated this redesign:**

1. Machine A closes Zen → Zync pushes profile at T1
2. Machine B has Zen open at T1 → ntfy queues the notification
3. Machine B closes Zen at T2 (T2 > T1) → Zync pushes Machine B's profile, which never incorporated Machine A's T1 changes
4. Machine B "wins" because T2 > T1 — Machine A's work is silently overwritten

The fundamental rule this redesign enforces: **you cannot overwrite data you haven't seen.**

---

## What Changes

### Removed

| Component | Why |
|---|---|
| `transport.rs` (Litterbox) | Replaced by GitHub Releases storage |
| Refresh loop in `daemon.rs` | No expiring links to keep alive |
| Session heuristics: `push_count`, `pulled_this_session`, `ntfy_polled_since_pair`, `zen_opened_since_last_push`, `last_published_file_id` | Version counter makes all of these obsolete |
| File ID in ntfy messages | ntfy now carries a version number, not a Litterbox file ID |
| Passphrase UI (input, Save, Pair, Forget buttons) | Key management is fully automatic |
| Auto-push / auto-pull toggles | Always-on; new model is reliable without manual overrides |

### Unchanged

| Component | Notes |
|---|---|
| `crypto.rs` | AES-256-GCM + PBKDF2 unchanged |
| `zen_check.rs` | Zen process detection unchanged |
| `ntfy.rs` | Mostly unchanged; message payload changes from file ID to version number |
| `profile.rs` | File collection and WAL checkpoint unchanged |
| Push tab + Pull tab | Remain for manual one-off syncs with users on different GitHub accounts |

### New Modules

| Module | Responsibility |
|---|---|
| `github.rs` | GitHub API client: OAuth token management, repo provisioning, release asset upload / download / delete |
| `local_state.rs` | Persists `last_known_version` and `machine_name` to disk across restarts |

### Changed Modules

| Module | Change |
|---|---|
| `sync.rs` | Push/pull targets GitHub Releases instead of Litterbox |
| `daemon.rs` | Refresh loop removed; version check added to push path; state machine simplified |
| `pairing.rs` | Passphrase keychain storage removed; ntfy topic derivation changed to `SHA256(github_user_id)` |
| `lib.rs` | Updated command registration; new GitHub OAuth command |

---

## Storage Model

### GitHub Setup (auto-provisioned on first connect)

- **Auth:** GitHub OAuth App via `tauri-plugin-oauth` → access token + refresh token stored in OS keychain
- **Repo:** Private repo named `zync-sync`, created automatically under the authenticated user's account if it does not exist
- **Release:** One permanent release tagged `storage` — all assets live here, outside git history, with no accumulation over time

### Storage Layout

Two mechanisms are used deliberately:

**`metadata.json` — stored as a repo file via the GitHub Contents API**

The Contents API requires the caller to supply the current file SHA when updating (`PUT /contents/metadata.json` with `"sha": "<current>"`) and returns HTTP 409 if the SHA has changed. This is the optimistic concurrency lock that makes simultaneous-push conflict detection reliable.

**`profile-N.enc` — stored as Release assets on the `storage` release**

Release assets are outside git history and do not accumulate. Profile blobs are large and binary; storing them as release assets keeps the repo's git storage bounded regardless of sync frequency.

```
repo file:   metadata.json           ← index + optimistic lock (Contents API)
release:     storage
  assets:    profile-0.enc           ← ring buffer slot 0
             profile-1.enc
             ...
             profile-9.enc           ← ring buffer slot 9
             encryption-key.b64      ← auto-generated on first connect
```

Maximum storage for profile blobs: 10 files × ~10 MB = ~100 MB, bounded forever.

### `metadata.json` Schema

```json
{
  "version": 5,
  "current_slot": 2,
  "slots": [
    {
      "slot": 2,
      "version": 5,
      "pushed_at": "2026-05-20T14:30:00Z",
      "machine_name": "MacBook Pro",
      "size_bytes": 4521043
    },
    {
      "slot": 1,
      "version": 4,
      "pushed_at": "2026-05-20T09:15:00Z",
      "machine_name": "Mac Mini",
      "size_bytes": 4498231
    }
  ]
}
```

- `version` — monotonically incrementing integer; the single source of truth for conflict detection
- `current_slot` — which `profile-N.enc` is the live version
- `slots` — sorted newest-first, up to 10 entries; this array directly populates the rollback UI with no extra API calls

### Ring Buffer Rotation

On each push, the next slot overwrites the oldest:

```
Before:  current_slot=2, version=5, slots=[2,1,0,9,8,7,6,5,4,3]
Push:    write to slot 3 (oldest slot = (current_slot + 1) % 10)
After:   current_slot=3, version=6, slots=[3,2,1,0,9,8,7,6,5,4]
```

Slot 3's previous content is overwritten by the new push. The `slots` array always contains exactly 10 entries (or fewer until 10 pushes have occurred).

### Encryption Key Management

On first GitHub connect, Zync generates a cryptographically random 32-byte key and uploads it to the release as `encryption-key.b64` (base64-encoded). Machine B reads this file automatically on connect. The user never sees or enters an encryption key.

Security model: the key is protected by GitHub private repo access. Encryption provides defense-in-depth against accidental repo exposure; GitHub repo access is the primary access control boundary.

### ntfy Topic Derivation

`SHA256(github_user_id)` as 64-char lowercase hex — same derivation as today but sourced from the GitHub account instead of a user-supplied passphrase. Both machines on the same account derive the same topic automatically.

### Local State (persisted across restarts)

Stored at the platform config directory (e.g., `~/.config/zync/state.json` on Linux, `~/Library/Application Support/zync/state.json` on macOS):

```json
{
  "last_known_version": 5,
  "machine_name": "MacBook Pro"
}
```

`last_known_version` is the version number from the last successful push or pull. It is the missing piece that makes the current heuristics necessary — with it persisted, Zync always knows its position relative to GitHub regardless of restarts.

`machine_name` defaults to the system hostname on first run and is editable in the Sync tab.

---

## Conflict Resolution

### The Rule

**A machine may only push if its `last_known_version` equals GitHub's current `version`.** If GitHub's version is higher, the machine is behind and must pull before it can push.

### Push Flow (triggered when Zen closes)

```
Zen closes
│
├─ 1. Fetch metadata.json from GitHub
│
├─ 2. Compare github.version vs local last_known_version
│
│   ├─ EQUAL → up to date, safe to push
│   │     ├─ Encrypt and upload profile to profile-{next_slot}.enc
│   │     ├─ Upload updated metadata.json (version+1, new slot entry)
│   │     ├─ Persist last_known_version = new version to disk
│   │     └─ Publish "version:{n}" to ntfy topic
│   │
│   └─ GREATER → peer pushed while our Zen was open
│         ├─ Save local profile to a temporary local snapshot (not uploaded)
│         ├─ Download profile-{current_slot}.enc
│         ├─ Decrypt and apply to local Zen profile directory
│         ├─ Persist last_known_version = github.version to disk
│         └─ Show notification (see Notifications section)
```

### Pull Flow (triggered by ntfy or 60s poll)

```
ntfy delivers "version:6"  (or 60s poll fires)
│
├─ Fetch metadata.json from GitHub
├─ If github.version <= last_known_version → already have this, skip
│
├─ Zen is open
│     └─ Store pending_version = github.version
│        Notify: "New profile available — will sync when Zen closes"
│
└─ Zen is closed
      ├─ Download profile-{current_slot}.enc
      ├─ Decrypt and apply
      └─ Persist last_known_version = github.version to disk
```

### Notifications

**When push is blocked (Machine B missed Machine A's push):**

> **Zync**  
> MacBook Pro pushed a profile while Zen was open on this machine. MacBook Pro's profile has been applied. Your session's changes are saved as a snapshot and can be restored anytime.  
> [View Snapshots]

**When a pull succeeds normally:**

> **Zync**  
> Profile updated from MacBook Pro.

**When a push succeeds:**

> **Zync**  
> Profile synced.

### Edge Cases

| Situation | Behaviour |
|---|---|
| GitHub unreachable at push time | Queue push; retry when connectivity returns. No data loss. |
| GitHub unreachable at pull time | Retain `pending_version`; retry on next poll cycle. |
| Zen open when Restore is tapped | Block with inline message: "Close Zen before restoring." |
| Two machines push simultaneously (race within same second) | Both read `metadata.json` SHA at version 5. Machine A updates successfully (version → 6). Machine B's `PUT /contents/metadata.json` is rejected with HTTP 409 because the SHA has changed. Machine B re-fetches metadata, finds version 6 > its last_known_version 5, and handles as the "behind" case — pulls Machine A's profile instead of pushing. |
| First connect, no existing repo | Zync creates `zync-sync` repo, creates `storage` release, generates and uploads encryption key, pushes current profile as version 1. |
| First connect, repo already exists (Machine B) | Zync finds existing repo, reads `encryption-key.b64`, reads `metadata.json`, pulls current version immediately if Zen is closed. |

---

## Rollback

### How It Works

Snapshots are stored in the GitHub release ring buffer (up to 10). Restoring a snapshot:

1. Checks Zen is closed (blocks if not)
2. Downloads `profile-{slot}.enc` for the selected snapshot
3. Decrypts and applies to the local Zen profile directory (with local backup first, same as all pulls)
4. Pushes the restored profile as a **new version** to GitHub — restoring does not rewrite history
5. Other machines receive the restored version via ntfy and pull it normally

The current version is always preserved as the most recent snapshot before a restore, so no state is ever permanently lost.

### Data Available Per Snapshot (from `metadata.json`, no extra API calls)

- Machine name that pushed
- Timestamp (date + time)
- Bundle size in bytes

---

## UI

### Tabs

`Pull | Sync` (previously `Pull | Pair`)

### Sync Tab — Not Connected

```
┌─────────────────────────────────────────────┐
│  Pull  │  Sync                               │
│▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔│
│                                             │
│  Connect a GitHub account to enable         │
│  automatic sync and version history         │
│  across your machines.                      │
│                                             │
│  [Connect GitHub]                           │
│                                             │
└─────────────────────────────────────────────┘
```

### Sync Tab — Connected (Main View)

```
┌─────────────────────────────────────────────┐
│  Pull  │  Sync                               │
│▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔│
│                                             │
│  ✓ Connected as jessewallace                │
│    zync-sync · private                      │
│                                             │
│  Machine name  [MacBook Pro          ]      │
│                                             │
│  Last synced: Today at 2:30 PM              │
│  from Mac Mini                              │
│                                             │
│  [Restore Previous Version]                 │
│                                             │
│                                [Disconnect] │
└─────────────────────────────────────────────┘
```

### Sync Tab — Rollback View

Navigated to by tapping Restore Previous Version. Stays within the Sync tab (no new window or modal).

```
┌─────────────────────────────────────────────┐
│  Pull  │  Sync                               │
│▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔▔│
│  ← Back                                     │
│  ─────────────────────────────────────────  │
│  Restoring replaces your current profile.   │
│  Your current version is saved as a         │
│  snapshot first, so you can always          │
│  come back to it.                           │
│  ─────────────────────────────────────────  │
│                                             │
│  ● MacBook Pro  May 20, 2:30 PM    4.3 MB   │
│    Mac Mini     May 20, 9:15 AM    4.1 MB  [Restore]│
│    MacBook Pro  May 19, 8:45 PM    4.3 MB  [Restore]│
│    Mac Mini     May 19, 2:10 PM    4.0 MB  [Restore]│
│    MacBook Pro  May 18, 7:55 PM    4.2 MB  [Restore]│
│    MacBook Pro  May 17, 6:40 PM    4.1 MB  [Restore]│
│    Mac Mini     May 17, 1:15 PM    4.0 MB  [Restore]│
│    MacBook Pro  May 16, 9:00 PM    4.3 MB  [Restore]│
│    Mac Mini     May 16, 2:45 PM    4.1 MB  [Restore]│
│    MacBook Pro  May 15, 8:30 PM    4.0 MB  [Restore]│
└─────────────────────────────────────────────┘
```

`●` marks the currently applied version. Back returns to the main Sync view without action.

### Tray Menu

```
Open Zync
Sync now
─────────
Quit
```

Snapshots are accessed via the Sync tab, not the tray.

---

## GitHub API Operations

### OAuth
| Operation | Endpoint |
|---|---|
| OAuth exchange | `tauri-plugin-oauth` + `POST https://github.com/login/oauth/access_token` |
| Get authenticated user (for user ID + login) | `GET /user` |

### Repo + Release Setup (first connect)
| Operation | Endpoint |
|---|---|
| Create private repo | `POST /user/repos` |
| Get or create `storage` release | `GET /repos/{owner}/zync-sync/releases/tags/storage` → `POST /repos/{owner}/zync-sync/releases` if 404 |

### Metadata (Contents API — SHA-locked)
| Operation | Endpoint |
|---|---|
| Read metadata + current SHA | `GET /repos/{owner}/zync-sync/contents/metadata.json` |
| Write metadata (requires current SHA) | `PUT /repos/{owner}/zync-sync/contents/metadata.json` with `{"sha": "<current>", "content": "<base64>", "message": "..."}` |

### Profile Blobs (Release Assets)
| Operation | Endpoint |
|---|---|
| List release assets | `GET /repos/{owner}/zync-sync/releases/{id}/assets` |
| Upload profile blob | `POST https://uploads.github.com/repos/{owner}/zync-sync/releases/{id}/assets?name=profile-{n}.enc` |
| Download profile blob | `GET /repos/{owner}/zync-sync/releases/assets/{id}` with `Accept: application/octet-stream` |
| Delete old profile blob (before overwrite) | `DELETE /repos/{owner}/zync-sync/releases/assets/{id}` |

All requests authenticated with `Authorization: Bearer {token}`. Token refresh handled transparently in `github.rs` before each request using the stored refresh token.

---

## Phase 2 (Future — Not In Scope Here)

Semantic three-way merge for conflicting profiles instead of pull-wins:

- **Additive auto-merge:** bookmarks (`places.sqlite` via GUID union), containers, extensions, themes, keyboard shortcuts
- **Per-key merge with conflict flagging:** `prefs.js`, `zen-sessions.jsonlz4` (workspace names)
- **Last-write-wins:** `chrome/zen-themes.css` (compiled, not meaningfully mergeable)

When Phase 2 is implemented, the "blocked push" flow changes from "pull, discard local" to "pull, merge, push combined result." The storage model, version counter, and rollback mechanism are unchanged.

---

## What Jesse Registers Once

A **GitHub OAuth App** in GitHub Developer Settings (free, no approval process):

- Callback URL: `http://localhost` (tauri-plugin-oauth handles the port dynamically)
- Scopes needed: `repo` (to create private repos and manage releases)
- Ships a `client_id` in the app bundle; uses PKCE so no `client_secret` needs to be embedded
