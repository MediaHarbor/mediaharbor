use std::collections::HashMap;

use serde::Deserialize;
use tempfile::TempDir;

use crate::errors::{MhError, MhResult};
use crate::http_client::{build_mozilla_client, download_to_file};

#[derive(Debug, Deserialize)]
struct PythonRelease {
    name: String,
    release_date: Option<String>,
}

#[cfg(target_os = "macos")]
fn compare_versions(v1: &str, v2: &str) -> std::cmp::Ordering {
    let p1: Vec<u64> = v1.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let p2: Vec<u64> = v2.split('.').map(|p| p.parse().unwrap_or(0)).collect();
    let len = p1.len().max(p2.len());
    for i in 0..len {
        let a = p1.get(i).copied().unwrap_or(0);
        let b = p2.get(i).copied().unwrap_or(0);
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
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
        return Err(MhError::Unsupported(format!(
            "Automatic Python installation is not supported on this platform. \
             Please install Python {} using your package manager.",
            full_version
        )));
    }
}

async fn url_exists(url: &str) -> bool {
    let client = match build_mozilla_client() {
        Ok(c) => c,
        Err(_) => return false,
    };
    match client.head(url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(_) => false,
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
        .map_err(|e| MhError::Network(e))?;

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

        let full_version = stripped.to_string();
        let download_url = match get_download_details(&full_version) {
            Ok((url, _)) => url,
            Err(_) => continue,
        };

        if !url_exists(&download_url).await {
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

    Ok(version_map
        .into_iter()
        .map(|(k, (v, _))| (k, v))
        .collect())
}

pub async fn download_and_install_python<F>(version: &str, on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str),
{
    on_progress(0, "Starting Python installation…");

    let (download_url, filename) = get_download_details(version)?;
    let tmp = TempDir::new()?;
    let installer_path = tmp.path().join(&filename);

    let client = build_mozilla_client()?;

    download_to_file(&client, &download_url, &installer_path, |dl, total| {
        let pct = total
            .map(|t| (dl as u128 * 40 / t as u128) as u8)
            .unwrap_or(0);
        on_progress(pct, &format!("Downloading Python… {}%", pct * 100 / 40));
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
        let status = tokio::process::Command::new(installer_path)
            .args([
                "/quiet",
                "InstallAllUsers=0",
                "PrependPath=1",
                "Include_test=0",
                "AssociateFiles=1",
            ])
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

        let status = tokio::process::Command::new("osascript")
            .args(["-e", &script])
            .status()
            .await
            .map_err(|e| MhError::Subprocess(format!("osascript failed: {}", e)))?;

        if !status.success() {
            return Err(MhError::Subprocess(
                "Python installer failed (osascript)".to_string(),
            ));
        }

        on_progress(80, "Installation completed, configuring system…");

        update_macos_path(version, on_progress)?;
        return Ok(());
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        return Err(MhError::Unsupported(
            "Unsupported platform for Python installer execution".to_string(),
        ));
    }
}

#[cfg(target_os = "macos")]
fn update_macos_path<F: Fn(u8, &str)>(version: &str, on_progress: &F) -> MhResult<()> {
    let parts: Vec<&str> = version.split('.').collect();
    let version_key = if parts.len() >= 2 {
        format!("{}.{}", parts[0], parts[1])
    } else {
        version.to_string()
    };

    let python_path = format!(
        "/Library/Frameworks/Python.framework/Versions/{}/bin",
        version_key
    );

    on_progress(90, "Updating system PATH…");

    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    let shell = std::env::var("SHELL").unwrap_or_default();

    let rc_path = if shell.contains("zsh") {
        home.join(".zshrc")
    } else {
        let bp = home.join(".bash_profile");
        if bp.exists() { bp } else { home.join(".profile") }
    };

    let existing = std::fs::read_to_string(&rc_path).unwrap_or_default();
    if !existing.contains(&python_path) {
        let line = format!(
            "\n# Added by Python installer\nexport PATH=\"{}:$PATH\"\n",
            python_path
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
