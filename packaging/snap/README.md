# Snap packaging

## Pre-build staging

Before running `snapcraft`, stage the pre-built binary and icons into this directory:

```bash
# From project root — build the Tauri binary first
npm run tauri:build:linux

# Stage binary and icons
cp src-tauri/target/release/mediaharbor        packaging/snap/
cp src-tauri/mediaharbor/icons/icon.png        packaging/snap/
cp src-tauri/mediaharbor/icons/32x32.png       packaging/snap/
cp src-tauri/mediaharbor/icons/128x128.png     packaging/snap/
```

## Building

```bash
cd packaging/snap
snapcraft   # builds inside a Multipass VM (core24 base)
```

## Installing and testing

```bash
# devmode first (relaxed confinement — good for initial testing)
sudo snap install mediaharbor_*.snap --devmode
snap run mediaharbor

# strict confinement
sudo snap install mediaharbor_*.snap --dangerous
snap run mediaharbor
```

## Notes

- `base: core24` (Ubuntu 24.04) is required because `libwebkit2gtk-4.1` (Tauri 2
  dependency) is only available from Ubuntu 24.04 onwards. `core22` only ships
  `libwebkit2gtk-4.0`.
- The gnome extension provides GTK3, WebKitGTK 4.1, and other Tauri deps.
- The binary is staged directly (no DEB extraction) for simplicity.
