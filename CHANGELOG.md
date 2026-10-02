# Changelog

## [3.0.0] - 2026-10-02

_If you are upgrading: settings are migrated on first launch, and the 2.x file is kept as `mh-settings.json.bak`._

### Changed

- **Breaking:** store settings in a nested, versioned layout that 2.x builds cannot read ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- **Breaking:** play Spotify as AAC through Widevine instead of Ogg Vorbis through PlayPlay, which needs a `.wvd` file and the Spotify dependency ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Play audio through a native engine instead of the WebView, with crossfade, replay gain and output device selection ([`72d4183`](https://github.com/MediaHarbor/mediaharbor/commit/72d4183782329492cbd9edd3879973ba6f147faf))
- Index local music in SQLite instead of rescanning folders on every visit ([`fb24ac6`](https://github.com/MediaHarbor/mediaharbor/commit/fb24ac69a08cee97876e27ea1c8699ea8d0ba1ec))
- Open search results as full album, artist and playlist pages instead of a pop-up ([`fb24ac6`](https://github.com/MediaHarbor/mediaharbor/commit/fb24ac69a08cee97876e27ea1c8699ea8d0ba1ec))
- Start Apple Music playback without Python and gamdl by handling the Widevine license in Rust ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Show track details in OS media controls (MPRIS, Windows, macOS) ([`72d4183`](https://github.com/MediaHarbor/mediaharbor/commit/72d4183782329492cbd9edd3879973ba6f147faf))
- Show speed, ETA, per-item progress and a summary for batch downloads ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Stop editing shell profiles and the Windows user `PATH` when installing tools ([`3b8f086`](https://github.com/MediaHarbor/mediaharbor/commit/3b8f086ca0fc9d0ee18fffd61d4fae82a04e4823))
- Run every music service through shared search, download and playback providers ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Replace the logo and app icons ([`183e82a`](https://github.com/MediaHarbor/mediaharbor/commit/183e82a636d7921eaadba1a9133e42f7743131cc))
- Require Node.js 24 or later to build from source ([`58b7c70`](https://github.com/MediaHarbor/mediaharbor/commit/58b7c705ef1df8a61355b22f9c51791dcbcc4ef1), [`4d6fce3`](https://github.com/MediaHarbor/mediaharbor/commit/4d6fce314e7cdaac7f3a1c44cda544ffa88dc093))

### Added

- Add native Spotify and Apple Music downloaders and use them by default; votify and gamdl remain available ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Add internet radio: search Radio Browser, SomaFM and the Icecast directory, with favourites, custom lists, imports and a station editor ([`67bcba2`](https://github.com/MediaHarbor/mediaharbor/commit/67bcba20f42813d17f9e4c9c9d19cd636e1971cd))
- Add streaming-service libraries with saved albums, artists, playlists and home shelves, and write saves, follows and playlist edits back to the service ([`fb24ac6`](https://github.com/MediaHarbor/mediaharbor/commit/fb24ac69a08cee97876e27ea1c8699ea8d0ba1ec))
- Add in-app updates for AppImage, deb, rpm, NSIS, MSI and macOS builds ([`f85240c`](https://github.com/MediaHarbor/mediaharbor/commit/f85240ce826d81cdabd9156b735836b447f8867b))
- Add local playlists with M3U import and export ([`fb24ac6`](https://github.com/MediaHarbor/mediaharbor/commit/fb24ac69a08cee97876e27ea1c8699ea8d0ba1ec))
- Add a tag editor, play history and a start-radio action to the local library ([`fb24ac6`](https://github.com/MediaHarbor/mediaharbor/commit/fb24ac69a08cee97876e27ea1c8699ea8d0ba1ec))
- Add autoplay, which queues similar tracks from the same service when the queue runs out ([`72d4183`](https://github.com/MediaHarbor/mediaharbor/commit/72d4183782329492cbd9edd3879973ba6f147faf))
- Add playback of APE, Musepack, Speex and TTA files ([`72d4183`](https://github.com/MediaHarbor/mediaharbor/commit/72d4183782329492cbd9edd3879973ba6f147faf))
- Add credential health checks with a status pill on each service's settings tab ([`9e819a8`](https://github.com/MediaHarbor/mediaharbor/commit/9e819a843b33105a0c81a42b8888c2866d58a1f5))
- Add in-app sign-in for Qobuz and Tidal, and Tidal token import ([`9e819a8`](https://github.com/MediaHarbor/mediaharbor/commit/9e819a843b33105a0c81a42b8888c2866d58a1f5))
- Add a service picker to onboarding, and hide the services you don't use ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b), [`3b8f086`](https://github.com/MediaHarbor/mediaharbor/commit/3b8f086ca0fc9d0ee18fffd61d4fae82a04e4823))
- Add an Audio/Video choice for pasted links, so any yt-dlp site can be saved as audio ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Skip tracks already downloaded from the same service ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Add installers for aria2c, Deno and N_m3u8DL-RE ([`3b8f086`](https://github.com/MediaHarbor/mediaharbor/commit/3b8f086ca0fc9d0ee18fffd61d4fae82a04e4823))
- Add action buttons to notifications, and replace repeated ones instead of stacking them ([`bff223f`](https://github.com/MediaHarbor/mediaharbor/commit/bff223f892cc777a0086be509b4bc525150b8b02))
- Sign release checksums and the RPM with GPG, and attest build provenance ([`8b4111c`](https://github.com/MediaHarbor/mediaharbor/commit/8b4111cb9288a05b05210e5bfeccfc2013816edc), [`e714448`](https://github.com/MediaHarbor/mediaharbor/commit/e714448e54a6df334c463b9f0a273c176289abd6))
- Document how to verify a release ([`5e091b8`](https://github.com/MediaHarbor/mediaharbor/commit/5e091b8961bf6fd3fa84821d44930667a487accd))

### Fixed

- Sort album tracks by disc and track number ([#41](https://github.com/MediaHarbor/mediaharbor/issues/41)) ([`fb24ac6`](https://github.com/MediaHarbor/mediaharbor/commit/fb24ac69a08cee97876e27ea1c8699ea8d0ba1ec))
- Install a native FFmpeg build on Apple Silicon Macs ([#38](https://github.com/MediaHarbor/mediaharbor/issues/38)) ([`3b8f086`](https://github.com/MediaHarbor/mediaharbor/commit/3b8f086ca0fc9d0ee18fffd61d4fae82a04e4823))
- Detect yt-dlp installed outside MediaHarbor's Python environment ([#30](https://github.com/MediaHarbor/mediaharbor/issues/30)) ([`3b8f086`](https://github.com/MediaHarbor/mediaharbor/commit/3b8f086ca0fc9d0ee18fffd61d4fae82a04e4823))
- Report the real app version on the Dependencies page and to the update check ([#44](https://github.com/MediaHarbor/mediaharbor/issues/44)) ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))
- Bundle FFmpeg and Python in the Snap, so downloads work there ([`d81d3ad`](https://github.com/MediaHarbor/mediaharbor/commit/d81d3ad7c647fad1c74daa4f3117e381b8825c35))
- Fix Copy link doing nothing on Linux ([`a109276`](https://github.com/MediaHarbor/mediaharbor/commit/a10927626353fd46ddd595c19d2be353f344250b))

## [2.2.0] - 2026-05-08

_Changes in this and earlier releases are listed on [GitHub Releases](https://github.com/MediaHarbor/mediaharbor/releases)._

[3.0.0]: https://github.com/MediaHarbor/mediaharbor/releases/tag/v3.0.0
[2.2.0]: https://github.com/MediaHarbor/mediaharbor/releases/tag/v2.2.0
