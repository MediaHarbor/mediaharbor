use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tokio::fs;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_mozilla_client, download_to_file};

const BENTO4_VERSION: &str = "1-6-0-641";
const BENTO4_BASE_URL: &str = "https://www.bok.net/Bento4/binaries";

fn get_download_url() -> MhResult<String> {
    let versioned = format!("Bento4-SDK-{}", BENTO4_VERSION);

    #[cfg(target_os = "windows")]
    {
        #[cfg(target_arch = "x86_64")]
        let arch_suffix = "x86_64-microsoft-win32";
        #[cfg(not(target_arch = "x86_64"))]
        let arch_suffix = "x86-microsoft-win32";

        return Ok(format!(
            "{}/{}.{}.zip",
            BENTO4_BASE_URL, versioned, arch_suffix
        ));
    }

    #[cfg(target_os = "macos")]
    {
        return Ok(format!(
            "{}/{}.universal-apple-macosx.zip",
            BENTO4_BASE_URL, versioned
        ));
    }

    #[cfg(target_os = "linux")]
    {
        #[cfg(target_arch = "x86_64")]
        let arch_suffix = "x86_64-unknown-linux";
        #[cfg(not(target_arch = "x86_64"))]
        let arch_suffix = "x86_64-unknown-linux"; // default; real port would map aarch64

        return Ok(format!(
            "{}/{}.{}.zip",
            BENTO4_BASE_URL, versioned, arch_suffix
        ));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        return Err(MhError::Unsupported(
            "Bento4 installation is not supported on this platform".to_string(),
        ));
    }
}

fn default_install_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    return PathBuf::from("/Applications/Bento4");

    #[cfg(not(target_os = "macos"))]
    {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
        home.join("Bento4")
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
        if name.to_ascii_lowercase() == "bin" && path.is_dir() {
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
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
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

fn update_shell_path(bin_dir: &Path) -> MhResult<()> {
    let bin_dir_str = bin_dir.to_string_lossy().to_string();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));

    #[cfg(target_os = "macos")]
    {
        for rc_name in &[".zshrc", ".bash_profile"] {
            let rc_path = home.join(rc_name);
            let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();
            if !existing.contains(&bin_dir_str) {
                let line = format!(
                    "\n# Added by Bento4 installer\nexport PATH=\"{}:$PATH\"\n",
                    bin_dir_str
                );
                use std::io::Write;
                let mut f = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&rc_path)?;
                f.write_all(line.as_bytes())?;
            }
        }
        return Ok(());
    }

    #[cfg(target_os = "windows")]
    {
        let ps_cmd = format!(
            r#"
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$binDir = '{dir}'
if ($userPath -split ';' -notcontains $binDir) {{
    $newPath = $userPath + ';' + $binDir
    [Environment]::SetEnvironmentVariable('Path', $newPath, 'User')
    Write-Output "Added Bento4 to PATH"
}} else {{
    Write-Output "Bento4 already in PATH"
}}
"#,
            dir = bin_dir_str.replace('\'', "''")
        );

        std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_cmd])
            .status()?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let shell = std::env::var("SHELL").unwrap_or_default();
        let rc_path = if shell.contains("zsh") {
            home.join(".zshrc")
        } else if shell.contains("bash") {
            home.join(".bashrc")
        } else {
            home.join(".profile")
        };

        let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();
        if !existing.contains(&bin_dir_str) {
            let line = format!(
                "\n# Added by Bento4 installer\nexport PATH=\"{}:$PATH\"\n",
                bin_dir_str
            );
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&rc_path)?;
            f.write_all(line.as_bytes())?;
        }
        Ok(())
    }
}

pub fn get_bento4_bin_dir() -> PathBuf {
    default_install_dir().join("bin")
}

pub fn get_bento4_tool_path(tool_name: &str) -> PathBuf {
    let bin_dir = get_bento4_bin_dir();

    #[cfg(target_os = "windows")]
    let binary = format!("{}.exe", tool_name);
    #[cfg(not(target_os = "windows"))]
    let binary = tool_name.to_string();

    let full = bin_dir.join(&binary);
    if full.exists() { full } else { PathBuf::from(tool_name) }
}

pub async fn download_and_install_bento4<F>(on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    on_progress(0, "Starting Bento4 installation…");

    let download_url = get_download_url()?;
    let install_dir = default_install_dir();

    std::fs::create_dir_all(&install_dir)?;

    let client = build_mozilla_client()?;
    let tmp = TempDir::new()?;
    let zip_path = tmp.path().join("Bento4-SDK.zip");

    download_to_file(&client, &download_url, &zip_path, |dl, total| {
        let pct = total
            .map(|t| (dl as u128 * 40 / t as u128) as u8)
            .unwrap_or(0);
        on_progress(pct, &format!("Downloading Bento4… {}%", (pct as u32) * 100 / 40));
    })
    .await?;

    on_progress(40, "Extracting Bento4…");
    extract_zip_to(&zip_path, tmp.path())?;
    on_progress(75, "Extraction completed");

    let bin_dir_src = find_bin_dir_in(tmp.path())
        .ok_or_else(|| MhError::Other("Bento4 bin directory not found after extraction".to_string()))?;

    let final_bin_dir = install_dir.join("bin");
    if final_bin_dir.exists() {
        fs::remove_dir_all(&final_bin_dir).await?;
    }
    fs::create_dir_all(&final_bin_dir).await?;

    let mut rd = fs::read_dir(&bin_dir_src).await?;
    while let Some(entry) = rd.next_entry().await? {
        let dest = final_bin_dir.join(entry.file_name());
        fs::copy(entry.path(), &dest).await?;
    }

    #[cfg(target_os = "macos")]
    {
        chmod_dir(&final_bin_dir)?;
        let _ = symlink_to_usr_local_bin(&final_bin_dir);
    }

    on_progress(80, "Configuring system PATH…");
    update_shell_path(&final_bin_dir)?;

    on_progress(
        100,
        "Bento4 installed successfully! Please restart your terminal to use Bento4.",
    );

    Ok(())
}
