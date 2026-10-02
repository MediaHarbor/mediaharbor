use std::sync::{Arc, Mutex};

use mediaharbor_core::ipc_contract;
use serde::Serialize;
use tauri::utils::{config::BundleType, platform::bundle_type};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// The checked update, plus its installer payload once staged. `Arc` so the recheck
/// timer can carry the payload forward without copying it.
type StagedUpdate = (Update, Option<Arc<Vec<u8>>>);

#[derive(Default)]
pub struct PendingUpdate(pub Mutex<Option<StagedUpdate>>);

// Snap must be excluded before `bundle_type()` is consulted: it unpacks the DEB and
// so inherits the DEB marker, which would send the updater off to run `pkexec dpkg -i`
// inside the sandbox. Flatpak and the AUR package carry no marker at all.
fn channel() -> &'static str {
    if mediaharbor_core::sandbox::is_flatpak() {
        return "flatpak";
    }
    if mediaharbor_core::sandbox::is_snap() {
        return "snap";
    }
    match bundle_type() {
        Some(BundleType::AppImage) => "appimage",
        Some(BundleType::Deb) => "deb",
        Some(BundleType::Rpm) => "rpm",
        Some(BundleType::Nsis) => "nsis",
        Some(BundleType::Msi) => "msi",
        Some(BundleType::App | BundleType::Dmg) => "app",
        _ => "source",
    }
}

// An allow-list, so a channel added to `channel()` without being considered here
// fails closed rather than silently gaining the ability to overwrite itself.
pub fn self_update_supported() -> bool {
    !cfg!(debug_assertions)
        && matches!(
            channel(),
            "appimage" | "deb" | "rpm" | "nsis" | "msi" | "app"
        )
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    supported: bool,
    channel: &'static str,
    current_version: String,
    available: bool,
    version: Option<String>,
    notes: Option<String>,
    date: Option<String>,
    release_url: Option<String>,
    staged: bool,
}

impl UpdateStatus {
    fn none(current_version: String) -> Self {
        Self {
            supported: true,
            channel: channel(),
            current_version,
            available: false,
            version: None,
            notes: None,
            date: None,
            release_url: None,
            staged: false,
        }
    }
}

fn iso_date(update: &Update) -> Option<String> {
    update
        .date
        .map(|d| format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day()))
}

#[tauri::command]
pub async fn updater_check(
    app: AppHandle,
    state: State<'_, crate::AppState>,
    pending: State<'_, PendingUpdate>,
) -> Result<UpdateStatus, String> {
    let current_version = state.0.get_version().version;

    if !self_update_supported() {
        let found: ipc_contract::CheckUpdatesResponse =
            state.0.check_updates().await.map_err(|e| e.to_string())?;
        return Ok(UpdateStatus {
            supported: false,
            channel: channel(),
            current_version,
            available: found.update_available,
            version: found.latest_version,
            notes: found.release_notes,
            date: None,
            release_url: found.release_url,
            staged: false,
        });
    }

    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;

    let mut guard = pending.0.lock().map_err(|e| e.to_string())?;

    // Carry an already-downloaded payload across the recheck, or a session left
    // running re-fetches the same bytes every time the interval fires.
    let staged = match (&update, guard.as_ref()) {
        (Some(new), Some((prev, bytes))) if prev.version == new.version => bytes.clone(),
        _ => None,
    };

    let status = match &update {
        Some(u) => UpdateStatus {
            supported: true,
            channel: channel(),
            current_version,
            available: true,
            version: Some(u.version.clone()),
            notes: u.body.clone(),
            date: iso_date(u),
            release_url: None,
            staged: staged.is_some(),
        },
        None => UpdateStatus::none(current_version),
    };

    *guard = update.map(|u| (u, staged));
    Ok(status)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateProgress {
    downloaded: u64,
    content_length: Option<u64>,
    percent: u8,
}

async fn download_with_progress(app: &AppHandle, update: &Update) -> Result<Vec<u8>, String> {
    let mut downloaded: u64 = 0;
    let emitter = app.clone();

    update
        .download(
            move |chunk, content_length| {
                downloaded += chunk as u64;
                let percent = content_length
                    .filter(|total| *total > 0)
                    .map(|total| ((downloaded * 100) / total).min(100) as u8)
                    .unwrap_or(0);
                let _ = emitter.emit(
                    "update-progress",
                    UpdateProgress {
                        downloaded,
                        content_length,
                        percent,
                    },
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn updater_download(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let update = {
        let guard = pending.0.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some((_, Some(_))) => return Ok(()),
            Some((u, None)) => u.clone(),
            None => return Err("No update has been checked for".into()),
        }
    };

    let bytes = download_with_progress(&app, &update).await?;

    if let Some(slot) = pending.0.lock().map_err(|e| e.to_string())?.as_mut() {
        slot.1 = Some(Arc::new(bytes));
    }
    Ok(())
}

// On Windows `install` hands off to the NSIS/MSI installer and terminates this process,
// so the restart below is never reached there: flush anything that must survive before
// invoking this command.
#[tauri::command]
pub async fn updater_apply(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let (update, staged) = {
        let guard = pending.0.lock().map_err(|e| e.to_string())?;
        match guard.as_ref() {
            Some((u, bytes)) => (u.clone(), bytes.clone()),
            None => return Err("No update has been checked for".into()),
        }
    };

    let bytes = match staged {
        // The guard still holds a reference, so this unwraps to a copy rather than a move.
        // The win is upstream: the interval-driven recheck now clones a pointer, not the
        // whole installer payload.
        Some(b) => Arc::try_unwrap(b).unwrap_or_else(|a| a.as_ref().clone()),
        None => download_with_progress(&app, &update).await?,
    };

    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

#[cfg(test)]
mod tests {
    use tauri_plugin_updater::RemoteRelease;

    /// Mirrors what `.github/workflows/tauri-build.yml` writes to latest.json. A
    /// malformed `pub_date` or version rejects the whole manifest rather than the
    /// one field, which would silently strand every install on its current build.
    const MANIFEST: &str = r#"{
      "version": "2.3.1",
      "notes": "- feat: something\n",
      "pub_date": "2026-09-04T12:41:48Z",
      "platforms": {
        "linux-x86_64-appimage": { "url": "https://example.invalid/a.AppImage", "signature": "s" },
        "linux-x86_64-deb":      { "url": "https://example.invalid/a.deb",      "signature": "s" },
        "linux-x86_64-rpm":      { "url": "https://example.invalid/a.rpm",      "signature": "s" },
        "windows-x86_64-nsis":   { "url": "https://example.invalid/a.exe",      "signature": "s" },
        "windows-x86_64-msi":    { "url": "https://example.invalid/a.msi",      "signature": "s" },
        "darwin-x86_64":         { "url": "https://example.invalid/x.tar.gz",   "signature": "s" },
        "darwin-aarch64":        { "url": "https://example.invalid/a.tar.gz",   "signature": "s" }
      }
    }"#;

    #[test]
    fn ci_manifest_deserializes_and_resolves_every_shipped_installer() {
        let release: RemoteRelease = serde_json::from_str(MANIFEST).expect("manifest must parse");
        assert_eq!(release.version.to_string(), "2.3.1");
        assert!(release.pub_date.is_some());

        for target in [
            "linux-x86_64-appimage",
            "linux-x86_64-deb",
            "linux-x86_64-rpm",
            "windows-x86_64-nsis",
            "windows-x86_64-msi",
            "darwin-x86_64",
            "darwin-aarch64",
        ] {
            assert!(
                release.download_url(target).is_ok(),
                "manifest is missing target {target}"
            );
        }
    }

    /// Flatpak, Snap and the AUR package report no installer, so the
    /// plugin only ever looks up the bare `{os}-{arch}` key. Leaving those keys
    /// out of the manifest is the backstop behind `self_update_supported`.
    #[test]
    fn unmarked_linux_and_windows_builds_resolve_nothing() {
        let release: RemoteRelease = serde_json::from_str(MANIFEST).expect("manifest must parse");
        assert!(release.download_url("linux-x86_64").is_err());
        assert!(release.download_url("windows-x86_64").is_err());
    }
}
