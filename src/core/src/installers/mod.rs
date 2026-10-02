pub mod aria2c;
pub mod bento4;
pub mod deno;
pub mod ffmpeg;
pub mod nm3u8dlre;
pub mod python;

use std::path::Path;

use crate::errors::{MhError, MhResult};

/// Leading bytes of a ZIP archive (local-file-header / empty-archive end-record).
pub(crate) const ZIP_MAGICS: &[&[u8]] = &[b"PK\x03\x04", b"PK\x05\x06"];

/// Leading bytes of a gzip archive.
pub(crate) const GZIP_MAGICS: &[&[u8]] = &[&[0x1F, 0x8B]];

/// Leading bytes of an xz archive.
pub(crate) const XZ_MAGICS: &[&[u8]] = &[&[0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00]];

/// Reject a download that is too small or whose leading bytes match no accepted
/// archive magic. `label` names the tool for the size-error message (e.g. "FFmpeg").
pub(crate) fn validate_download(
    path: &Path,
    min_bytes: u64,
    label: &str,
    magics: &[&[u8]],
) -> MhResult<()> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if len < min_bytes {
        return Err(MhError::Other(format!(
            "Downloaded {label} archive is too small ({len} bytes) — the mirror likely returned an error page. Please try again later."
        )));
    }
    let mut head = [0u8; 8];
    let n = file.read(&mut head)?;
    let head = &head[..n];
    if magics.iter().any(|m| head.starts_with(m)) {
        Ok(())
    } else {
        Err(MhError::Other(
            "Downloaded file is not a valid archive (the mirror may have returned an error page). Please try again later.".to_string(),
        ))
    }
}

/// Mark a file as executable (`0o755`).
#[cfg(unix)]
pub(crate) fn set_executable(path: &Path) -> MhResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

/// Extract a single named binary out of a zip archive into `dest_dir`.
pub(crate) fn extract_zip_single_binary(
    zip_path: &Path,
    dest_dir: &Path,
    binary_name: &str,
) -> MhResult<()> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip_path)?)
        .map_err(|e| MhError::Other(format!("zip open: {}", e)))?;

    let mut found = false;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| MhError::Other(format!("zip entry: {}", e)))?;
        let name = entry.name().to_string();
        let file_name = Path::new(&name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if file_name != binary_name {
            continue;
        }
        let out_path = dest_dir.join(&file_name);
        let mut out = std::fs::File::create(&out_path)?;
        std::io::copy(&mut entry, &mut out)?;
        found = true;
    }

    if !found {
        return Err(MhError::Other(format!(
            "The downloaded archive did not contain a {} binary.",
            binary_name
        )));
    }

    Ok(())
}

/// Stream-extract every entry whose file name matches one of `binary_names` into
/// `dest_dir`, marking each executable. `reader` is a decompressed tar stream (wrap
/// the archive in the appropriate decoder). Returns how many binaries were extracted.
pub(crate) fn extract_tar_binaries<R: std::io::Read>(
    reader: R,
    dest_dir: &Path,
    binary_names: &[&str],
) -> MhResult<usize> {
    let mut archive = tar::Archive::new(reader);
    let mut found = 0;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if binary_names.contains(&file_name) {
            let dest = dest_dir.join(file_name);
            let mut out = std::fs::File::create(&dest)?;
            std::io::copy(&mut entry, &mut out)?;
            #[cfg(unix)]
            set_executable(&dest)?;
            found += 1;
        }
    }
    Ok(found)
}

/// Extract every name in `binary_names` out of a compressed tar into `dest_dir`,
/// erroring unless all of them were found.
fn extract_tar_all<R: std::io::Read>(
    reader: R,
    dest_dir: &Path,
    binary_names: &[&str],
) -> MhResult<()> {
    if extract_tar_binaries(reader, dest_dir, binary_names)? < binary_names.len() {
        return Err(MhError::Other(format!(
            "The downloaded archive did not contain {}.",
            binary_names.join(" and ")
        )));
    }
    Ok(())
}

/// Archive layout of a managed binary download.
pub(crate) enum ArchiveKind {
    TarGz,
    TarXz,
    Zip,
}

impl ArchiveKind {
    fn ext(&self) -> &'static str {
        match self {
            ArchiveKind::TarGz => "tar.gz",
            ArchiveKind::TarXz => "tar.xz",
            ArchiveKind::Zip => "zip",
        }
    }

    fn magics(&self) -> &'static [&'static [u8]] {
        match self {
            ArchiveKind::TarGz => GZIP_MAGICS,
            ArchiveKind::TarXz => XZ_MAGICS,
            ArchiveKind::Zip => ZIP_MAGICS,
        }
    }
}

/// What a managed-binary installer does inside a Flatpak/Snap sandbox. Flatpak
/// always installs into the app's data dir, as v2 did.
pub(crate) enum SandboxPolicy {
    /// Refuse inside the Snap with the given message.
    RefuseInSnap(&'static str),
    /// Attempt anyway — the runtime may stage the binary onto PATH.
    Attempt,
}

impl SandboxPolicy {
    /// The message to refuse with in the current runtime, or `None` to go ahead.
    fn refusal(&self) -> Option<&'static str> {
        match self {
            SandboxPolicy::RefuseInSnap(msg) if crate::sandbox::is_snap() => Some(msg),
            _ => None,
        }
    }
}

/// One downloadable build, matched against `std::env::consts::{OS, ARCH}` at run
/// time. Listing targets in a table rather than behind `#[cfg]` keeps the whole
/// matrix visible in one place and makes "no build for this platform" an
/// ordinary `None` instead of a second, cfg-gated copy of every function.
pub(crate) struct Target {
    pub os: &'static str,
    pub arch: &'static str,
    /// Mirrors, tried in order; the first that downloads, validates and extracts wins.
    pub urls: &'static [&'static str],
    pub archive: ArchiveKind,
    /// Every binary this one archive yields — ffmpeg ships ffprobe alongside it.
    pub binary_names: &'static [&'static str],
}

impl Target {
    fn extract(&self, archive_path: &Path, dest_dir: &Path) -> MhResult<()> {
        match self.archive {
            ArchiveKind::TarGz => extract_tar_all(
                flate2::read::GzDecoder::new(std::fs::File::open(archive_path)?),
                dest_dir,
                self.binary_names,
            ),
            ArchiveKind::TarXz => extract_tar_all(
                lzma_rust2::XzReader::new(std::fs::File::open(archive_path)?, true),
                dest_dir,
                self.binary_names,
            ),
            ArchiveKind::Zip => {
                for name in self.binary_names {
                    extract_zip_single_binary(archive_path, dest_dir, name)?;
                    #[cfg(unix)]
                    set_executable(&dest_dir.join(name))?;
                }
                Ok(())
            }
        }
    }
}

/// Everything the generic installer driver needs to fetch a single managed binary.
pub(crate) struct BinarySpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub targets: &'static [Target],
    pub min_bytes: u64,
    pub sandbox: SandboxPolicy,
    /// Returns the name of an already-installed satisfying binary, if any.
    pub detect: fn() -> Option<&'static str>,
    /// Manual-install hint appended to the "does not run" error message.
    pub install_hint: &'static str,
    /// Shown when no `targets` entry matches this machine.
    pub unsupported_hint: &'static str,
}

impl BinarySpec {
    fn target(&self) -> Option<&Target> {
        self.targets
            .iter()
            .find(|t| t.os == std::env::consts::OS && t.arch == std::env::consts::ARCH)
    }
}

/// Download → validate → extract → verify a single managed binary described by `spec`.
pub(crate) async fn install_single_binary<F: Fn(u8, &str)>(
    spec: &BinarySpec,
    on_progress: &F,
    force: bool,
) -> MhResult<()> {
    on_progress(
        0,
        &format!("Checking for an existing {}…", spec.display_name),
    );

    if !force {
        if let Some(found) = (spec.detect)() {
            on_progress(
                100,
                &format!("{} already available — no download needed.", found),
            );
            return Ok(());
        }
    }

    if let Some(msg) = spec.sandbox.refusal() {
        return Err(MhError::Unsupported(msg.to_string()));
    }

    let target = spec.target().ok_or_else(|| {
        MhError::Unsupported(format!(
            "MediaHarbor does not provide a managed {} build for {}-{}. {}",
            spec.display_name,
            std::env::consts::OS,
            std::env::consts::ARCH,
            spec.unsupported_hint
        ))
    })?;

    let bin_dir = crate::venv_manager::managed_bin_dir()
        .ok_or_else(|| MhError::Other("Cannot determine data directory".to_string()))?;
    std::fs::create_dir_all(&bin_dir)?;

    if force {
        for name in target.binary_names {
            let _ = std::fs::remove_file(bin_dir.join(name));
        }
    }

    let client = crate::http_client::build_download_client()?;
    let downloading = format!("Downloading {}…", spec.display_name);
    let mut last_err: Option<MhError> = None;
    let mut installed_from = None;

    for url in target.urls {
        let tmp = tempfile::TempDir::new()?;
        let archive_path = tmp
            .path()
            .join(format!("{}.{}", spec.id, target.archive.ext()));

        on_progress(5, &downloading);
        if let Err(e) =
            crate::http_client::download_to_file(&client, url, &archive_path, |dl, total| {
                let pct = total.map(|t| 5 + dl * 80 / t).unwrap_or(5);
                on_progress(pct.min(85) as u8, &downloading);
            })
            .await
        {
            last_err = Some(e);
            continue;
        }

        if let Err(e) = validate_download(
            &archive_path,
            spec.min_bytes,
            spec.display_name,
            target.archive.magics(),
        ) {
            last_err = Some(e);
            continue;
        }

        on_progress(88, &format!("Extracting {}…", spec.display_name));
        match target.extract(&archive_path, &bin_dir) {
            Ok(()) => {
                installed_from = Some(url);
                break;
            }
            Err(e) => last_err = Some(e),
        }
    }

    if installed_from.is_none() {
        return Err(last_err.unwrap_or_else(|| {
            MhError::Other(format!(
                "{} could not be downloaded from any mirror.",
                spec.display_name
            ))
        }));
    }

    crate::venv_manager::invalidate_binary_cache();

    on_progress(95, &format!("Verifying {}…", spec.display_name));
    for name in target.binary_names {
        let installed = bin_dir.join(name);
        if !crate::venv_manager::verify_binary(&installed).await {
            let _ = std::fs::remove_file(&installed);
            crate::venv_manager::invalidate_binary_cache();
            return Err(MhError::Other(format!(
                "The downloaded {} does not run on this machine (wrong CPU architecture or \
                 corrupt download). {}",
                name, spec.install_hint
            )));
        }
    }

    on_progress(
        100,
        &format!("{} installed successfully!", spec.display_name),
    );
    Ok(())
}

/// Managed binaries whose install status is reported to the frontend by dep id.
/// Single source of truth shared by `install_dep` and `check_deps`.
pub(crate) const MANAGED_BINARIES: &[&BinarySpec] =
    &[&aria2c::SPEC, &deno::SPEC, &ffmpeg::SPEC, &nm3u8dlre::SPEC];

/// Installs the managed binary registered under `id`, if there is one.
pub(crate) async fn install_managed<F: Fn(u8, &str)>(
    id: &str,
    on_progress: &F,
    force: bool,
) -> Option<MhResult<()>> {
    let spec = MANAGED_BINARIES.iter().find(|s| s.id == id)?;
    Some(install_single_binary(spec, on_progress, force).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every registered binary must resolve by the id the frontend sends.
    #[test]
    fn managed_binary_ids_are_unique() {
        let mut ids: Vec<&str> = MANAGED_BINARIES.iter().map(|s| s.id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate dependency id");
    }
}
