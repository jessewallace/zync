# GitHub Release & README Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship Zync v0.1.0 on GitHub with a beautiful README, macOS `.dmg`, and Windows `.msi`/`.exe` built via GitHub Actions CI.

**Architecture:** Write README + SVG mockup screenshots locally; set up a GitHub Actions release workflow that cross-builds on macOS and Windows runners when a tag is pushed; then tag v0.1.0 to trigger the first release.

**Tech Stack:** GitHub Actions, `tauri-apps/tauri-action@v0`, Rust stable, Node 20, SVG for screenshot mockups

---

## File Map

| Action | File |
|--------|------|
| Create | `README.md` |
| Create | `docs/screenshots/01-main.svg` |
| Create | `docs/screenshots/02-push-result.svg` |
| Create | `docs/screenshots/03-pull-result.svg` |
| Create | `.github/workflows/release.yml` |

---

### Task 1: Create SVG mockup screenshots

The app has three screens. We'll create SVG mockups that accurately reflect the UI for use in the README.

Design system reference:
- Background: `#f2f0e3` (parchment)
- Text: `#1a1914`
- Accent (coral): `#f76f53`
- Dark button: `#28262f`
- Muted: `#9b9781`
- Border: `#9b9781`
- Light border: `#d4d2ca`
- Success bg: `#e7f9d9`, success text: `#336d3f`
- Window size: 420×460px
- Font: system sans-serif (Bricolage Grotesque substitute in SVG)
- Radius: 10px

**Files:**
- Create: `docs/screenshots/01-main.svg`
- Create: `docs/screenshots/02-push-result.svg`
- Create: `docs/screenshots/03-pull-result.svg`

- [ ] **Step 1: Create docs/screenshots directory and main screen SVG**

Create `docs/screenshots/01-main.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="420" height="460" viewBox="0 0 420 460">
  <defs>
    <style>
      text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
    </style>
  </defs>

  <!-- Window background -->
  <rect width="420" height="460" fill="#f2f0e3" rx="12"/>

  <!-- Header / tab bar -->
  <rect x="0" y="0" width="420" height="66" fill="rgba(0,0,0,0.05)" rx="12"/>
  <rect x="0" y="34" width="420" height="32" fill="rgba(0,0,0,0.05)"/>

  <!-- Active tab "Pull" -->
  <rect x="118" y="18" width="90" height="48" fill="#f2f0e3" rx="10 10 0 0"/>
  <text x="163" y="51" font-size="22" font-weight="700" fill="#1a1914" text-anchor="middle">Pull</text>

  <!-- Inactive tab "Pair" -->
  <text x="260" y="51" font-size="22" font-weight="700" fill="#9b9781" text-anchor="middle">Pair</text>

  <!-- Upload button -->
  <rect x="32" y="90" width="356" height="60" fill="#f76f53" rx="10"/>
  <text x="210" y="128" font-size="22" font-weight="700" fill="#ffffff" text-anchor="middle">Upload</text>

  <!-- Divider -->
  <line x1="32" y1="173" x2="155" y2="173" stroke="#d4d2ca" stroke-width="1"/>
  <text x="210" y="178" font-size="11" font-weight="400" fill="#1a1914" text-anchor="middle" letter-spacing="2">or pull from another machine</text>
  <line x1="265" y1="173" x2="388" y2="173" stroke="#d4d2ca" stroke-width="1"/>

  <!-- Pull input field -->
  <rect x="32" y="192" width="356" height="60" fill="transparent" rx="10" stroke="#9b9781" stroke-width="2"/>
  <text x="210" y="230" font-size="22" font-weight="500" fill="#9b9781" text-anchor="middle" letter-spacing="2">ZEN-XXXX-XXXX</text>

  <!-- Pull button -->
  <rect x="32" y="268" width="356" height="60" fill="#28262f" rx="10"/>
  <text x="210" y="306" font-size="22" font-weight="700" fill="#ffffff" text-anchor="middle">Pull</text>
</svg>
```

- [ ] **Step 2: Create push result screen SVG**

Create `docs/screenshots/02-push-result.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="420" height="460" viewBox="0 0 420 460">
  <defs>
    <style>
      text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
    </style>
  </defs>

  <!-- Window background -->
  <rect width="420" height="460" fill="#f2f0e3" rx="12"/>

  <!-- "Sync Code" title -->
  <text x="210" y="86" font-size="32" font-weight="700" fill="#1a1914" text-anchor="middle">Sync Code</text>

  <!-- Divider under title -->
  <line x1="32" y1="108" x2="155" y2="108" stroke="#d4d2ca" stroke-width="1"/>
  <text x="210" y="113" font-size="11" font-weight="400" fill="#1a1914" text-anchor="middle" letter-spacing="2">Share this with your other machine(s)</text>
  <line x1="265" y1="108" x2="388" y2="108" stroke="#d4d2ca" stroke-width="1"/>

  <!-- Sync code box -->
  <rect x="32" y="128" width="356" height="64" fill="transparent" rx="10" stroke="#1a1914" stroke-width="2"/>
  <text x="210" y="168" font-size="22" font-weight="700" fill="#1a1914" text-anchor="middle" letter-spacing="4">ZEN-A3F9B2-K8MN4X</text>

  <!-- Copy button -->
  <rect x="32" y="208" width="173" height="60" fill="#f76f53" rx="10"/>
  <text x="119" y="246" font-size="22" font-weight="700" fill="#ffffff" text-anchor="middle">Copy</text>

  <!-- Done button -->
  <rect x="215" y="208" width="173" height="60" fill="rgba(0,0,0,0.05)" rx="10"/>
  <text x="302" y="246" font-size="22" font-weight="700" fill="#1a1914" text-anchor="middle">Done</text>

  <!-- Countdown -->
  <text x="210" y="308" font-size="18" font-weight="400" fill="#1a1914" text-anchor="middle">Expires in <tspan font-weight="800">59:42</tspan></text>
</svg>
```

- [ ] **Step 3: Create pull result screen SVG**

Create `docs/screenshots/03-pull-result.svg`:

```svg
<svg xmlns="http://www.w3.org/2000/svg" width="420" height="460" viewBox="0 0 420 460">
  <defs>
    <style>
      text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; }
    </style>
  </defs>

  <!-- Window background -->
  <rect width="420" height="460" fill="#f2f0e3" rx="12"/>

  <!-- Check circle -->
  <circle cx="210" cy="110" r="30" fill="#e7f9d9"/>
  <path d="M195 110 l10 11 l22 -22" stroke="#336d3f" stroke-width="3.5" fill="none" stroke-linecap="round" stroke-linejoin="round"/>

  <!-- "Pull successful!" -->
  <text x="210" y="168" font-size="28" font-weight="700" fill="#1a1914" text-anchor="middle">Pull successful!</text>

  <!-- Success message box -->
  <rect x="32" y="184" width="356" height="44" fill="#e7f9d9" rx="8"/>
  <text x="210" y="211" font-size="14" font-weight="600" fill="#336d3f" text-anchor="middle">Open Zen Browser to see your changes.</text>

  <!-- File list -->
  <text x="210" y="258" font-size="12" fill="#1a1914" text-anchor="middle">places.sqlite</text>
  <text x="210" y="278" font-size="12" fill="#1a1914" text-anchor="middle">zen-sessions.jsonlz4</text>
  <text x="210" y="298" font-size="12" fill="#1a1914" text-anchor="middle">containers.json</text>
  <text x="210" y="318" font-size="12" fill="#1a1914" text-anchor="middle">prefs.js · extensions.json · zen-themes.json</text>

  <!-- Done button -->
  <rect x="110" y="344" width="200" height="60" fill="rgba(0,0,0,0.05)" rx="10"/>
  <text x="210" y="382" font-size="22" font-weight="700" fill="#1a1914" text-anchor="middle">Done</text>
</svg>
```

---

### Task 2: Write the README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Write the full README**

Replace the existing `README.md` (currently just "# zync") with the following. The screenshots reference the SVG files created in Task 1 via relative paths.

```markdown
<div align="center">

<img src="src-tauri/icons/icon.png" width="96" height="96" alt="Zync icon">

# Zync

**Sync your Zen Browser profile between machines — one code, one click.**

[![GitHub release](https://img.shields.io/github/v/release/jessewallace/zync?style=flat-square&color=f76f53)](https://github.com/jessewallace/zync/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-f76f53?style=flat-square)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-28262f?style=flat-square)](#installation)

</div>

---

Zen Browser stores your workspaces, pinned tabs, themes, and keyboard shortcuts in files that Firefox Sync doesn't cover. Zync fills that gap: bundle your entire profile into an encrypted package, share a short code, and pull it onto any other machine in seconds.

No account. No server. Nothing stored after an hour.

## Screenshots

<div align="center">

| Upload your profile | Share the code | Restore on another machine |
|:---:|:---:|:---:|
| ![Main screen](docs/screenshots/01-main.svg) | ![Sync code](docs/screenshots/02-push-result.svg) | ![Pull success](docs/screenshots/03-pull-result.svg) |

</div>

## How it works

```
Machine A                           Machine B
─────────────────────               ─────────────────────
[Push]                              [Pull]
  │                                   │
  ├─ Bundle profile files             ├─ Paste sync code
  ├─ Encrypt (AES-256-GCM)           ├─ Download from Litterbox
  ├─ Upload to Litterbox             ├─ Decrypt
  └─ Show ZEN-XXXXXX-XXXXXX ──────► └─ Write to profile folder
```

1. **Upload** — Zync bundles your Zen profile, encrypts it with a random key, and uploads the encrypted blob to [Litterbox](https://litterbox.catbox.moe) (anonymous, no account required, auto-expires in 1 hour).
2. **Share the code** — A short `ZEN-XXXXXX-XXXXXX` code appears. The first part is your decryption key; the second is the Litterbox file ID. The upload URL is never sent separately — the code is all you need.
3. **Pull** — Paste the code on another machine. Zync downloads and decrypts the bundle, backs up your current profile, and writes the synced files.

## What gets synced

| File | Contents |
|------|----------|
| `places.sqlite` | Bookmarks, pinned tabs, workspaces |
| `zen-sessions.jsonlz4` | Workspace names, themes, tab assignments |
| `containers.json` | Workspace icons and colors |
| `zen-live-folders.jsonlz4` | Live folders |
| `prefs.js` | Browser preferences |
| `extensions.json` | Extensions list |
| `zen-themes.json` | Mods config |
| `chrome/zen-themes.css` | Compiled active styles |
| `zen-keyboard-shortcuts.json` | Keyboard shortcuts |

Passwords (`key4.db`, `logins.json`) are intentionally excluded for v1.

## Auto-sync (Pair mode)

Set the same passphrase on each machine and enable the toggles. Zync runs as a system-tray app that auto-pushes when Zen closes and auto-pulls when another paired machine syncs — no manual code sharing needed.

## Installation

### Download

Grab the latest release for your platform:

| Platform | Download |
|----------|----------|
| macOS (Apple Silicon & Intel) | `.dmg` from [Releases](https://github.com/jessewallace/zync/releases/latest) |
| Windows | `.msi` or `.exe` from [Releases](https://github.com/jessewallace/zync/releases/latest) |
| Linux | `.AppImage` from [Releases](https://github.com/jessewallace/zync/releases/latest) |

### macOS note

Zync is not yet notarized. After opening the `.dmg`, right-click the app and choose **Open** on first launch.

### Build from source

```bash
# Prerequisites: Rust stable, Node 20+
git clone https://github.com/jessewallace/zync.git
cd zync
npm install
npm run build
# Installer appears in src-tauri/target/release/bundle/
```

## Security

- **Encryption:** AES-256-GCM with a 100,000-round PBKDF2-derived key
- **Key space:** 2²⁴ combinations (~16M) — brute-force takes ~11h on a GPU; Litterbox expires the file in 1h
- **No relay server:** Zync calls Litterbox directly from your device. Nothing passes through any Zync infrastructure
- **Backup before pull:** Zync always backs up your current profile to `{profile}/zync-backup-{timestamp}/` before overwriting

## Roadmap

- [ ] WAL checkpoint before push (SQLite correctness fix)
- [ ] End-to-end round-trip test suite
- [ ] Password sync opt-in (`key4.db` / `logins.json`)
- [ ] Selective workspace sync
- [ ] macOS notarization

## Acknowledgments

Zync was inspired by [**arc2zen**](https://github.com/rafcabezas/arc2zen) by [@rafcabezas](https://github.com/rafcabezas) — a brilliant tool for migrating Arc Browser profiles to Zen. His work surfaced the gap that Zen Browser's cross-machine sync story needed filling, and made it clear there was a community here worth building for. Thank you.

## License

MIT
```

---

### Task 3: Set up GitHub Actions release workflow

This workflow triggers on version tags (`v*.*.*`), builds on both macOS and Windows runners using `tauri-apps/tauri-action`, and creates a GitHub release with all artifacts attached.

Note: Linux (AppImage) builds on the `ubuntu-latest` runner are included as a bonus.

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create the .github/workflows directory and release workflow**

```yaml
# .github/workflows/release.yml
name: Release

on:
  push:
    tags:
      - 'v*.*.*'

jobs:
  release:
    permissions:
      contents: write
    strategy:
      fail-fast: false
      matrix:
        include:
          - platform: macos-latest
            args: '--target aarch64-apple-darwin --target x86_64-apple-darwin'
          - platform: windows-latest
            args: ''
          - platform: ubuntu-22.04
            args: ''

    runs-on: ${{ matrix.platform }}

    steps:
      - uses: actions/checkout@v4

      - name: Setup Node
        uses: actions/setup-node@v4
        with:
          node-version: 20

      - name: Install Rust stable
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.platform == 'macos-latest' && 'aarch64-apple-darwin,x86_64-apple-darwin' || '' }}

      - name: Install Linux dependencies
        if: matrix.platform == 'ubuntu-22.04'
        run: |
          sudo apt-get update
          sudo apt-get install -y libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf

      - name: Install frontend dependencies
        run: npm install

      - name: Build and release
        uses: tauri-apps/tauri-action@v0
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          tagName: ${{ github.ref_name }}
          releaseName: 'Zync ${{ github.ref_name }}'
          releaseBody: |
            ## What's in this release

            See the [README](https://github.com/jessewallace/zync#readme) for full details on what Zync does and how it works.

            ### Downloads
            - **macOS** — `.dmg` (universal binary: Apple Silicon + Intel)
            - **Windows** — `.msi` installer or `.exe` setup
            - **Linux** — `.AppImage`

            ### First time on macOS?
            Right-click the app and choose **Open** on first launch (not yet notarized).
          releaseDraft: false
          prerelease: false
          includeUpdaterJson: false
          args: ${{ matrix.args }}
```

- [ ] **Step 2: Verify the workflow file is valid YAML**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))" && echo "YAML valid"
```

Expected: `YAML valid`

---

### Task 4: Commit, tag, and push to trigger the release

**Files:**
- No new files (all files already staged)

- [ ] **Step 1: Stage all new files**

```bash
git add README.md docs/screenshots/ .github/workflows/release.yml
git status
```

Expected: all three paths shown as new files to be committed.

- [ ] **Step 2: Commit**

```bash
git commit -m "$(cat <<'EOF'
Add README, screenshots, and GitHub Actions release workflow

Adds a fully fleshed-out README with SVG mockup screenshots and
acknowledgments; sets up a multi-platform release workflow that
builds macOS (universal), Windows, and Linux installers on tag push.

Co-Authored-By: Claude Sonnet 4.6 <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3: Push the commit**

```bash
git push origin main
```

Expected: push succeeds with no errors.

- [ ] **Step 4: Tag v0.1.0 and push the tag**

```bash
git tag v0.1.0
git push origin v0.1.0
```

Expected: tag push triggers the GitHub Actions release workflow. Watch it at:
`https://github.com/jessewallace/zync/actions`

- [ ] **Step 5: Verify the release was created**

After the Actions workflow completes (~10–15 min), verify:

```bash
gh release view v0.1.0
```

Expected: release shows with `.dmg`, `.msi`/`.exe`, and `.AppImage` assets attached.

---

## Self-Review

**Spec coverage check:**
- ✅ Beautiful README — Task 2
- ✅ Screenshots showing how it works — Task 1 (SVG mockups)
- ✅ Windows version — Task 3 (GitHub Actions `windows-latest` runner)
- ✅ macOS version — Task 3 (GitHub Actions `macos-latest` with universal binary)
- ✅ GitHub release — Task 4
- ✅ Credit to rafcabezas/arc2zen — Task 2 Acknowledgments section

**Placeholder scan:** No TBDs. All code is complete and self-contained.

**Note on Windows cross-compilation:** Windows `.msi`/`.exe` cannot be cross-compiled from macOS without a complex Wine/MSVC toolchain setup. GitHub Actions with `windows-latest` is the correct approach and is standard for all Tauri projects. The first release artifacts will appear once the tag is pushed and the workflow completes.
