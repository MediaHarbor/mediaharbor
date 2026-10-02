use std::collections::HashMap;

use serde::Deserialize;
use tempfile::TempDir;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_download_client, build_mozilla_client, download_to_file};
#[cfg(target_os = "macos")]
use crate::update_checker::compare_versions;

#[derive(Debug, Deserialize)]
struct PythonRelease {
    name: String,
    release_date: Option<String>,
}

fn get_download_details(full_version: &str) -> MhResult<(String, String)> {
    #[cfg(target_os = "windows")]
    {
        let filename = format!("python-{}-amd64.exe", full_version);
        let url = format!(
            "https://www.python.org/ftp/python/{}/{}",
            full_version, filename
        );
        return Ok((url, filename));
    }

    #[cfg(target_os = "macos")]
    {
        let suffix = if compare_versions(full_version, "3.9.13") >= std::cmp::Ordering::Equal {
            "macos11"
        } else {
            "macosx10.9"
        };
        let filename = format!("python-{}-{}.pkg", full_version, suffix);
        let url = format!(
            "https://www.python.org/ftp/python/{}/{}",
            full_version, filename
        );
        return Ok((url, filename));
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(MhError::Unsupported(format!(
            "Automatic Python installation is not supported on this platform. \
             Please install Python {} using your package manager.",
            full_version
        )))
    }
}

pub async fn fetch_python_versions() -> MhResult<HashMap<String, String>> {
    let client = build_mozilla_client()?;

    let releases: Vec<PythonRelease> = client
        .get("https://www.python.org/api/v2/downloads/release/?is_published=true&format=json")
        .send()
        .await?
        .json()
        .await
        .map_err(MhError::Network)?;

    let mut version_map: HashMap<String, (String, String)> = HashMap::new();

    for release in &releases {
        let stripped = match release.name.strip_prefix("Python ") {
            Some(s) => s,
            None => continue,
        };

        let parts: Vec<&str> = stripped.split('.').collect();
        if parts.len() < 3 {
            continue;
        }

        let major: u64 = parts[0].parse().unwrap_or(0);
        let minor: u64 = parts[1].parse().unwrap_or(0);

        if major != 3 || minor < 10 {
            continue;
        }

        if !parts[2].chars().all(|c| c.is_ascii_digit()) {
            continue;
        }

        let full_version = stripped.to_string();

        if get_download_details(&full_version).is_err() {
            continue;
        }

        let major_minor = format!("{}.{}", major, minor);
        let date = release.release_date.clone().unwrap_or_default();

        let insert = match version_map.get(&major_minor) {
            None => true,
            Some((_, existing_date)) => date > *existing_date,
        };

        if insert {
            version_map.insert(major_minor, (full_version, date));
        }
    }

    Ok(version_map.into_iter().map(|(k, (v, _))| (k, v)).collect())
}

pub async fn download_and_install_python<F>(version: &str, on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    on_progress(0, "Starting Python installation…");

    let (download_url, filename) = get_download_details(version)?;
    let tmp = TempDir::new()?;
    let installer_path = tmp.path().join(&filename);

    let client = build_download_client()?;

    download_to_file(&client, &download_url, &installer_path, |dl, total| {
        let pct = total
            .map(|t| (dl as u128 * 40 / t as u128) as u8)
            .unwrap_or(0);
        on_progress(
            pct,
            &format!("Downloading Python… {}%", (pct as u32) * 100 / 40),
        );
    })
    .await?;

    on_progress(40, "Running Python installer…");

    run_installer(&installer_path, version, &on_progress).await?;

    on_progress(100, "Python installation completed successfully!");

    Ok(())
}

#[allow(unused_variables)]
async fn run_installer<F: Fn(u8, &str)>(
    installer_path: &std::path::Path,
    version: &str,
    on_progress: &F,
) -> MhResult<()> {
    #[cfg(target_os = "windows")]
    {
        let mut cmd = tokio::process::Command::new(installer_path);
        cmd.args([
            "/quiet",
            "InstallAllUsers=0",
            "PrependPath=1",
            "Include_test=0",
            "AssociateFiles=1",
        ]);
        crate::subprocess::apply_no_window(&mut cmd);
        let status = cmd
            .status()
            .await
            .map_err(|e| MhError::Subprocess(format!("Failed to run Python installer: {}", e)))?;

        if !status.success() {
            return Err(MhError::Subprocess(format!(
                "Python installer exited with code {:?}",
                status.code()
            )));
        }

        on_progress(80, "Installation completed, configuring system…");
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        let pkg_path = installer_path.to_string_lossy();
        let command = format!("installer -pkg \"{}\" -target /", pkg_path);
        let script = format!(
            "do shell script \"{}\" with administrator privileges",
            command.replace('"', "\\\"")
        );

        let output = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .output()
            .await
            .map_err(|e| MhError::Subprocess(format!("osascript failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // osascript reports a dismissed authorisation dialog as -128, which is a
            // user choice rather than a fault worth reporting as an installer failure.
            if stderr.contains("-128") || stderr.contains("User canceled") {
                return Err(MhError::Subprocess(
                    "Python installation was cancelled at the administrator password prompt."
                        .to_string(),
                ));
            }
            return Err(MhError::Subprocess(format!(
                "Python installer failed: {}",
                stderr.trim()
            )));
        }

        on_progress(100, "Installation completed.");
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Err(MhError::Unsupported(
            "Unsupported platform for Python installer execution".to_string(),
        ))
    }
}
