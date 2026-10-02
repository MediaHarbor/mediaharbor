use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::fs;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_download_client, download_to_file};

const BENTO4_VERSION: &str = "1-6-0-641";
const BENTO4_BASE_URL: &str = "https://www.bok.net/Bento4/binaries";

fn get_download_source() -> MhResult<(String, &'static str)> {
    #[cfg(target_os = "windows")]
    {
        let url = format!(
            "{}/Bento4-SDK-{}.x86_64-microsoft-win32.zip",
            BENTO4_BASE_URL, BENTO4_VERSION
        );
        Ok((
            url,
            "6916a390f75878872594be74554b8b54ab220bb29812424441a8e1ecc9a6ac5e",
        ))
    }

    #[cfg(target_os = "macos")]
    {
        let url = format!(
            "{}/Bento4-SDK-{}.universal-apple-macosx.zip",
            BENTO4_BASE_URL, BENTO4_VERSION
        );
        Ok((
            url,
            "0570cf0dd59f362904d6f1cb472cbf4cdd37928fb0fe28e4c7f98c460e8e0ced",
        ))
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        let url = format!(
            "{}/Bento4-SDK-{}.x86_64-unknown-linux.zip",
            BENTO4_BASE_URL, BENTO4_VERSION
        );
        Ok((
            url,
            "d48dc6b164941212e5614237b4d9aeff81d4d111ee8b1508892764078a0870e8",
        ))
    }

    #[cfg(all(target_os = "linux", not(target_arch = "x86_64")))]
    {
        Err(MhError::Unsupported(
            "Bento4 (mp4decrypt) has no prebuilt binary for this Linux CPU architecture. \
             Use MediaHarbor's native Spotify/Apple Music backend, which decrypts without Bento4."
                .to_string(),
        ))
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        Err(MhError::Unsupported(
            "Bento4 installation is not supported on this platform".to_string(),
        ))
    }
}

fn verify_sha256(path: &Path, expected: &str) -> MhResult<()> {
    use sha2::{Digest, Sha256};
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual = hex::encode(hasher.finalize());
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(MhError::Other(format!(
            "Bento4 download failed its integrity check (expected {}, got {}). Please try again later.",
            expected, actual
        )))
    }
}

fn default_install_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Applications/Bento4");

    #[cfg(not(target_os = "macos"))]
    {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mediaharbor")
            .join("bento4")
    }
}

fn find_bin_dir_in(root: &Path) -> Option<PathBuf> {
    if !root.exists() {
        return None;
    }

    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(_) => return None,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.eq_ignore_ascii_case("bin") && path.is_dir() {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = find_bin_dir_in(&path) {
                return Some(found);
            }
        }
    }
    None
}

fn extract_zip_to(zip_path: &Path, dest: &Path) -> MhResult<()> {
    let bytes = std::fs::read(zip_path)?;
    let cursor = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|e| MhError::Other(format!("zip open: {}", e)))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| MhError::Other(format!("zip entry {}: {}", i, e)))?;

        let out_path = match entry.enclosed_name() {
            Some(p) => dest.join(p),
            None => continue,
        };

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out)?;
            #[cfg(unix)]
            if let Some(mode) = entry.unix_mode() {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode));
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn chmod_dir(dir: &Path) -> MhResult<()> {
    use std::os::unix::fs::PermissionsExt;
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        let mut perms = std::fs::metadata(&path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&path, perms)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn symlink_to_usr_local_bin(bin_dir: &Path) -> MhResult<()> {
    let local_bin = Path::new("/usr/local/bin");
    if !local_bin.exists() {
        std::fs::create_dir_all(local_bin)?;
    }

    for entry in std::fs::read_dir(bin_dir)?.flatten() {
        let src = entry.path();
        let dst = local_bin.join(entry.file_name());
        if dst.exists() || dst.is_symlink() {
            let _ = std::fs::remove_file(&dst);
        }
        std::os::unix::fs::symlink(&src, &dst)?;
    }
    Ok(())
}

pub fn get_bento4_bin_dir() -> PathBuf {
    default_install_dir().join("bin")
}

/// Whether a usable mp4decrypt is already reachable. One detector shared by
/// `check_deps` (what the UI badge shows) and the installer (whether to
/// download), so the badge and the guard cannot disagree.
pub fn detect() -> Option<&'static str> {
    crate::venv_manager::find_managed_or_path("mp4decrypt").map(|_| "mp4decrypt")
}

fn binary_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    }
}

/// mp4decrypt has no `--version`: it prints usage and exits non-zero, so
/// `venv_manager::verify_binary` would reject a perfectly good build and delete
/// it. What is worth checking is that it `exec`s at all — a wrong-architecture or
/// truncated binary fails before it ever has an exit code to report.
async fn mp4decrypt_runs(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let mut cmd = tokio::process::Command::new(path);
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::subprocess::apply_no_window(&mut cmd);
    cmd.status().await.is_ok()
}

/// Replace `dest` with `staged`, keeping the previous copy until the swap lands.
/// Both live under the same parent, so each rename is a same-filesystem move —
/// staging in a `TempDir` instead would risk `EXDEV` between `/var/folders` and
/// `/Applications`.
async fn swap_in(staged: &Path, dest: &Path) -> MhResult<()> {
    let previous = dest.with_extension("previous");
    fs::remove_dir_all(&previous).await.ok();

    let had_previous = dest.exists();
    if had_previous {
        fs::rename(dest, &previous).await?;
    }

    match fs::rename(staged, dest).await {
        Ok(()) => {
            fs::remove_dir_all(&previous).await.ok();
            Ok(())
        }
        Err(e) => {
            if had_previous {
                fs::rename(&previous, dest).await.ok();
            }
            Err(e.into())
        }
    }
}

pub async fn download_and_install_bento4<F>(on_progress: F, force: bool) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    on_progress(0, "Checking for an existing mp4decrypt…");

    if !force && detect().is_some() {
        on_progress(100, "mp4decrypt is already available — no download needed.");
        return Ok(());
    }

    let (download_url, expected_sha) = get_download_source()?;
    let install_dir = default_install_dir();

    std::fs::create_dir_all(&install_dir)?;

    let client = build_download_client()?;
    let tmp = TempDir::new()?;
    let zip_path = tmp.path().join("Bento4-SDK.zip");

    download_to_file(&client, &download_url, &zip_path, |dl, total| {
        let pct = total
            .map(|t| (dl as u128 * 40 / t as u128) as u8)
            .unwrap_or(0);
        on_progress(
            pct,
            &format!("Downloading Bento4… {}%", (pct as u32) * 100 / 40),
        );
    })
    .await?;

    verify_sha256(&zip_path, expected_sha)?;

    on_progress(40, "Extracting Bento4…");
    extract_zip_to(&zip_path, tmp.path())?;
    on_progress(75, "Extraction completed");

    let bin_dir_src = find_bin_dir_in(tmp.path()).ok_or_else(|| {
        MhError::Other("Bento4 bin directory not found after extraction".to_string())
    })?;

    // Stage beside the live directory and swap only once the payload is known
    // good. Deleting `bin/` up front meant a mirror outage, a truncated download
    // or a wrong-architecture build left the user with no mp4decrypt at all —
    // worse off than before they pressed Install.
    let final_bin_dir = install_dir.join("bin");
    let staged = install_dir.join("bin.incoming");
    fs::remove_dir_all(&staged).await.ok();
    fs::create_dir_all(&staged).await?;

    let mut rd = fs::read_dir(&bin_dir_src).await?;
    while let Some(entry) = rd.next_entry().await? {
        fs::copy(entry.path(), staged.join(entry.file_name())).await?;
    }

    #[cfg(unix)]
    chmod_dir(&staged)?;

    on_progress(85, "Verifying Bento4…");
    if !mp4decrypt_runs(&staged.join(binary_name("mp4decrypt"))).await {
        fs::remove_dir_all(&staged).await.ok();
        return Err(MhError::Other(
            "The downloaded mp4decrypt does not run on this machine (wrong CPU architecture \
             or corrupt download). Please install Bento4 manually from \
             https://www.bok.net/Bento4/ instead."
                .to_string(),
        ));
    }

    swap_in(&staged, &final_bin_dir).await?;

    #[cfg(target_os = "macos")]
    if !crate::sandbox::is_sandboxed() {
        let _ = symlink_to_usr_local_bin(&final_bin_dir);
    }

    // Bento4's prefix outranks `$PATH` in `ffmpeg_search_dirs`, so a copy cached
    // from further down the search order has to be dropped for this one to win.
    crate::venv_manager::invalidate_binary_cache();

    on_progress(100, "Bento4 installed successfully!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_bin_dir_in_finds_the_sdks_nested_bin() {
        let tmp = TempDir::new().unwrap();
        let bin = tmp
            .path()
            .join("Bento4-SDK-1-6-0-641.universal-apple-macosx");
        std::fs::create_dir_all(bin.join("bin")).unwrap();
        std::fs::create_dir_all(bin.join("docs")).unwrap();

        assert_eq!(find_bin_dir_in(tmp.path()), Some(bin.join("bin")));
    }

    #[test]
    fn find_bin_dir_in_returns_none_without_one() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("Bento4-SDK/docs")).unwrap();

        assert_eq!(find_bin_dir_in(tmp.path()), None);
    }

    /// A failed swap must leave the mp4decrypt the user already had.
    #[tokio::test]
    async fn swap_in_keeps_the_live_dir_when_there_is_nothing_to_swap() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("bin");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("mp4decrypt"), b"old").unwrap();

        assert!(swap_in(&tmp.path().join("bin.incoming"), &dest)
            .await
            .is_err());
        assert_eq!(std::fs::read(dest.join("mp4decrypt")).unwrap(), b"old");
    }

    #[tokio::test]
    async fn swap_in_replaces_the_live_dir_and_cleans_up() {
        let tmp = TempDir::new().unwrap();
        let dest = tmp.path().join("bin");
        let staged = tmp.path().join("bin.incoming");
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(dest.join("mp4decrypt"), b"old").unwrap();
        std::fs::write(staged.join("mp4decrypt"), b"new").unwrap();

        swap_in(&staged, &dest).await.unwrap();

        assert_eq!(std::fs::read(dest.join("mp4decrypt")).unwrap(), b"new");
        assert!(!staged.exists());
        assert!(!tmp.path().join("bin.previous").exists());
    }
}
