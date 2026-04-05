#[cfg(target_os = "windows")]
use serde::Deserialize;

use crate::errors::{MhError, MhResult};
#[cfg(target_os = "windows")]
use crate::http_client::{build_mozilla_client, download_to_file};
#[cfg(target_os = "windows")]
use tempfile::TempDir;

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
struct GhRelease {
    assets: Vec<GhAsset>,
}

#[cfg(target_os = "windows")]
#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
}

#[cfg(target_os = "windows")]
async fn fetch_git_windows_url() -> MhResult<String> {
    let client = build_mozilla_client()?;

    let release: GhRelease = client
        .get("https://api.github.com/repos/git-for-windows/git/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await?
        .json()
        .await
        .map_err(|e| MhError::Network(e))?;

    #[cfg(target_arch = "aarch64")]
    let suffix = "-arm64.exe";
    #[cfg(not(target_arch = "aarch64"))]
    let suffix = "-64-bit.exe";

    release
        .assets
        .iter()
        .find(|a| a.name.ends_with(suffix))
        .map(|a| a.browser_download_url.clone())
        .ok_or_else(|| MhError::NotFound("Git installer asset not found".to_string()))
}

async fn run_cmd(program: &str, args: &[&str]) -> MhResult<String> {
    let output = tokio::process::Command::new(program)
        .args(args)
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("Failed to run `{}`: {}", program, e)))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(MhError::Subprocess(format!(
            "`{}` failed: {}",
            program,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

#[cfg(target_os = "macos")]
async fn run_shell(cmd: &str) -> MhResult<String> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", cmd])
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("Shell command failed: {}", e)))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(MhError::Subprocess(format!(
            "Shell command `{}` failed: {}",
            cmd,
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}

pub async fn download_and_install_git<F>(on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    #[cfg(target_os = "windows")]
    {
        on_progress(0, "Fetching latest Git release info…");
        let download_url = fetch_git_windows_url().await?;

        let client = build_mozilla_client()?;
        let tmp = TempDir::new()?;
        let installer_path = tmp.path().join("git-installer.exe");

        on_progress(5, "Starting Git download…");

        download_to_file(&client, &download_url, &installer_path, |dl, total: Option<u64>| {
            let pct = total
                .map(|t| (dl as u128 * 40 / t as u128) as u8)
                .unwrap_or(0);
            on_progress(pct, &format!("Downloading Git… {}%", pct * 100 / 40));
        })
        .await?;

        on_progress(50, "Installing Git…");

        let status = tokio::process::Command::new(&installer_path)
            .args(["/VERYSILENT", "/NORESTART", "/NOCANCEL", "/SP-",
                   "/CLOSEAPPLICATIONS", "/RESTARTAPPLICATIONS"])
            .status()
            .await
            .map_err(|e| MhError::Subprocess(format!("Failed to run Git installer: {}", e)))?;

        if !status.success() {
            return Err(MhError::Subprocess(format!(
                "Git installer exited with code {:?}",
                status.code()
            )));
        }

        on_progress(90, "Verifying installation…");
        let version = run_cmd("git", &["--version"]).await?;
        on_progress(100, &format!("Git {} installed successfully", version));

        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        on_progress(0, "Checking Homebrew installation…");

        let brew_ok = run_cmd("brew", &["--version"]).await.is_ok();

        if brew_ok {
            on_progress(20, "Updating Homebrew…");
            run_cmd("brew", &["update"]).await?;
        } else {
            on_progress(20, "Installing Homebrew…");
            run_shell(
                r#"/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)""#,
            )
            .await?;
        }

        on_progress(50, "Installing Git via Homebrew…");
        run_cmd("brew", &["install", "git"]).await?;

        on_progress(90, "Verifying installation…");
        let version = run_cmd("git", &["--version"]).await?;
        on_progress(100, &format!("Git {} installed successfully", version));

        return Ok(());
    }

    #[cfg(target_os = "linux")]
    {
        let os_release = tokio::fs::read_to_string("/etc/os-release")
            .await
            .unwrap_or_default();

        let is_debian = os_release.contains("ID=debian")
            || os_release.contains("ID=ubuntu")
            || os_release.contains("ID_LIKE=debian")
            || os_release.contains("ID_LIKE=ubuntu");

        let is_redhat = os_release.contains("ID=rhel")
            || os_release.contains("ID=fedora")
            || os_release.contains("ID=centos")
            || os_release.contains("ID_LIKE=rhel")
            || os_release.contains("ID_LIKE=fedora");

        on_progress(20, "Updating package manager…");

        if is_debian {
            run_cmd("sudo", &["apt-get", "update"]).await?;
            on_progress(50, "Installing Git…");
            run_cmd("sudo", &["apt-get", "install", "-y", "git"]).await?;
        } else if is_redhat {
            let has_dnf = run_cmd("which", &["dnf"]).await.is_ok();
            on_progress(50, "Installing Git…");
            if has_dnf {
                run_cmd("sudo", &["dnf", "install", "-y", "git"]).await?;
            } else {
                run_cmd("sudo", &["yum", "install", "-y", "git"]).await?;
            }
        } else {
            return Err(MhError::Unsupported(
                "Unsupported Linux distribution".to_string(),
            ));
        }

        on_progress(90, "Verifying installation…");
        let version = run_cmd("git", &["--version"]).await?;
        on_progress(100, &format!("Git {} installed successfully", version));

        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        return Err(MhError::Unsupported(
            "Git installation is not supported on this platform".to_string(),
        ));
    }
}
