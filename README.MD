![MediaHarbor Banner](assets/MediaHarborBanner.svg)

# MediaHarbor

[![Tauri](https://img.shields.io/badge/Tauri-%2324C8DB.svg?logo=tauri&logoColor=white)](https://tauri.app/)[![Rust](https://img.shields.io/badge/Rust-%23000000.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)[![TypeScript](https://img.shields.io/badge/TypeScript-%23007ACC.svg?logo=typescript&logoColor=white)](https://www.typescriptlang.org/)
![License](https://img.shields.io/github/license/MediaHarbor/MediaHarbor) ![Build Status](https://img.shields.io/github/actions/workflow/status/MediaHarbor/MediaHarbor/node.js.yml) ![Downloads](https://img.shields.io/github/downloads/MediaHarbor/MediaHarbor/latest/total)
![Static Badge](https://img.shields.io/badge/website-mediaharbor?style=for-the-badge&logo=github&label=MediaHarbor&link=https%3A%2F%2Fmediaharbor.org) [![Discord](https://img.shields.io/discord/1410501161449951334?logo=discord&logoColor=white&label=Discord&color=%235865F2&style=for-the-badge)](https://discord.gg/Kc97D86TeZ)


MediaHarbor is an all-in-one media downloader and player. Search, stream, and download audio/video from YouTube, Spotify, Tidal, Deezer, Qobuz, Apple Music, and more.

---

## Table of Contents

- [Features](#features)
- [Supported Platforms](#supported-platforms)
- [Prerequisites](#prerequisites)
- [Installation](#installation)
- [Building from Source](#building-from-source)
- [In-App Dependency Setup](#in-app-dependency-setup)
- [Verifying a Release](#verifying-a-release)
- [Known Issues](#known-issues)
- [Contributing](#contributing)
- [License](#license)

---

## Features

- **Search** across all supported platforms from a single interface
- **Download** tracks, albums, playlists, and videos at your chosen quality
- **Stream** directly inside the app with a built-in audio/video player
- **Lyrics** — synced (LRC), plain text, and word-by-word display where available
- **Local library** — scan a folder and browse your downloaded media by album/artist
- **Automatic dependency management** installs yt-dlp, ffmpeg, Python, votify, gamdl, and more from the Dependencies page
- **OrpheusDL integration** optional backend for platform downloads via modular support
- **In-app settings editor** — configure all platform credentials without touching config files

---

## Prerequisites

Before building, make sure you have:

- **Node.js** ≥ 18 and **npm**
- **Rust** (stable toolchain) — install from [rustup.rs](https://rustup.rs/)
- Tauri system dependencies for your OS — see the [Tauri v2 prerequisites guide](https://tauri.app/start/prerequisites/)

On Linux you also need: `libwebkit2gtk-4.1-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`

---

## Building from Source

### 1. Clone the repository

```bash
git clone https://github.com/MediaHarbor/MediaHarbor.git
cd MediaHarbor
git checkout tauri-migration
```

### 2. Install dependencies

```bash
npm install
```

### 3. Run in development mode

```bash
npm run tauri:dev
```

This starts the Vite dev server and the Tauri process concurrently.

### 4. Build a production binary

```bash
# Linux only (.deb, .rpm, .AppImage)
npm run tauri:build:linux

# Windows only (.msi, .exe)
npm run tauri:build:win

# macOS (universal or targeted)
npm run tauri:build:mac
npm run tauri:build:mac:x64
npm run tauri:build:mac:arm64
```

The output is placed in `src/app/target/release/bundle/`.

---

## In-App Dependency Setup

MediaHarbor manages its own tool dependencies through the **Dependencies** page inside the app. On first launch, open the Updates tab and install what you need:

- **Python / yt-dlp** required for YouTube and YouTube Music downloads
- **ffmpeg** required for muxing and format conversion
- **votify + Bento4** required for Spotify downloads (cookies also needed in Settings)
- **gamdl + Bento4** required for Apple Music downloads (cookies also needed in Settings)
- **OrpheusDL + modules** optional alternative backend for platform downloads

Platform credentials (Tidal OAuth, Deezer ARL, Qobuz login, Spotify/Apple cookies) are all configured from the **Settings** page.

---

## Verifying a Release

Every release artifact is hashed in `SHA256SUMS.txt`, and that manifest is GPG-signed as
`SHA256SUMS.txt.asc`. Verifying the manifest therefore verifies every file listed in it.

```sh
# one-time: import the release signing key
curl -fsSL https://raw.githubusercontent.com/MediaHarbor/mediaharbor/main/packaging/keys/mediaharbor-release.asc | gpg --import

# check the manifest signature, then the downloaded files against it
gpg --verify SHA256SUMS.txt.asc SHA256SUMS.txt
sha256sum --ignore-missing -c SHA256SUMS.txt
```

Release signing key fingerprint:

```
4E7E D764 DFB4 F1B6 59C1  C570 DD7A E5F9 C20A 4047
MediaHarbor Release Signing <info@mediaharbor.org>
```

The RPM additionally carries an embedded signature:

```sh
sudo rpm --import packaging/keys/mediaharbor-release.asc
rpm --checksig MediaHarbor-*.rpm
```

Every artifact also has a [Sigstore build-provenance attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations/using-artifact-attestations-to-establish-provenance-for-builds),
proving it was produced by this repository's release workflow and not rebuilt elsewhere:

```sh
gh attestation verify <file> --repo MediaHarbor/mediaharbor
```

### macOS: first launch

MediaHarbor is not notarized — that requires a paid Apple Developer ID, which the project
does not have yet. macOS quarantines the app on first launch. After dragging it to
Applications:

```sh
xattr -dr com.apple.quarantine /Applications/MediaHarbor.app
```

This is only needed once. Updates installed from inside the app are not re-quarantined.

### Windows

The installers are not Authenticode-signed, so SmartScreen shows an "unknown publisher"
warning; choose **More info -> Run anyway**. Verify the download against `SHA256SUMS.txt`
above if you want certainty about what you are running.

---

## Known Issues

- Library page may not work properly for big folders
- Apple Music streamings haven't added yet
- Synced lyrics may get out of sync due on Windows and some tracks

---

## Contributing

Contributions are welcome. Please open an issue before submitting a pull request for larger changes. For smaller fixes, a PR is fine directly.

---

## License

GPL-3.0 — see [LICENSE](LICENSE) for details.