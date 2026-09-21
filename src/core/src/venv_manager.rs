use std::path::{Path, PathBuf};

use crate::{
    errors::{MhError, MhResult},
    subprocess::run_to_completion,
};

pub fn get_venv_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("mediaharbor")
        .join("venv")
}

pub fn get_venv_python() -> PathBuf {
    let venv = get_venv_dir();
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

pub fn get_venv_pip() -> (PathBuf, Vec<String>) {
    (get_venv_python(), vec!["-m".into(), "pip".into()])
}

pub fn is_venv_ready() -> bool {
    get_venv_python().exists()
}

pub fn get_venv_bin(name: &str) -> PathBuf {
    let venv = get_venv_dir();
    let bin_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    let dir = if cfg!(windows) { "Scripts" } else { "bin" };
    venv.join(dir).join(bin_name)
}

pub fn resolve_command(name: &str, fallback_paths: &[&Path]) -> PathBuf {
    let venv_bin = get_venv_bin(name);
    if venv_bin.exists() {
        return venv_bin;
    }
    for &p in fallback_paths {
        if p.exists() {
            return p.to_path_buf();
        }
    }
    PathBuf::from(name)
}

/// Resolve the ffmpeg binary, checking venv and common install locations.
pub fn resolve_ffmpeg() -> String {
    let venv_bin = get_venv_bin("ffmpeg");
    if venv_bin.exists() {
        return venv_bin.to_string_lossy().to_string();
    }

    #[cfg(target_os = "windows")]
    {
        let app_data = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."));
        let local = app_data.join("Microsoft").join("WinGet").join("Links").join("ffmpeg.exe");
        if local.exists() {
            return local.to_string_lossy().to_string();
        }
        let mh_ffmpeg = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mediaharbor")
            .join("bin")
            .join("ffmpeg.exe");
        if mh_ffmpeg.exists() {
            return mh_ffmpeg.to_string_lossy().to_string();
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        for p in &["/app/bin/ffmpeg", "/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg", "/opt/homebrew/bin/ffmpeg"] {
            if Path::new(p).exists() {
                return p.to_string();
            }
        }
        let mh_bin = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("mediaharbor")
            .join("bin")
            .join("ffmpeg");
        if mh_bin.exists() {
            return mh_bin.to_string_lossy().to_string();
        }
    }

    "ffmpeg".to_string()
}

pub async fn find_system_python() -> MhResult<String> {
    // On Unix, probe absolute versioned paths from newest to oldest first.
    // Returning an absolute versioned path (e.g. /usr/bin/python3.13) rather
    // than the bare "python3" symlink means the venv stays pinned to one minor
    // version and is not broken by system upgrades or pyenv changes.
    #[cfg(not(target_os = "windows"))]
    {
        let prefixes = ["/app/bin", "/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];
        for minor in (10u32..=20).rev() {
            for &prefix in &prefixes {
                let path = format!("{}/python3.{}", prefix, minor);
                if !Path::new(&path).exists() {
                    continue;
                }
                let (stdout, stderr, code) =
                    run_to_completion(&path, &["--version"], None, None)
                        .await
                        .unwrap_or_default();
                if code != 0 {
                    continue;
                }
                let output = if stdout.is_empty() { &stderr } else { &stdout };
                if python_version_ok(output.trim()) {
                    return Ok(path);
                }
            }
        }
    }

    let candidates: &[&str] = if cfg!(windows) {
        &["py", "python3", "python"]
    } else {
        &["python3", "python"]
    };

    for &cmd in candidates {
        let (stdout, stderr, code) =
            run_to_completion(cmd, &["--version"], None, None)
                .await
                .unwrap_or_default();

        if code != 0 {
            continue;
        }

        let output = if stdout.is_empty() { &stderr } else { &stdout };
        if python_version_ok(output.trim()) {
            return Ok(cmd.to_string());
        }
    }

    if let Some(path) = scan_filesystem_python().await {
        return Ok(path);
    }

    Err(MhError::Subprocess(
        "No suitable Python 3.10+ installation found. Please install Python 3.10 or newer."
            .into(),
    ))
}

fn python_version_ok(output: &str) -> bool {
    output
        .strip_prefix("Python ")
        .and_then(|v| {
            let mut parts = v.split('.');
            let major: u32 = parts.next()?.parse().ok()?;
            let minor: u32 = parts.next()?.parse().ok()?;
            Some(major == 3 && minor >= 10)
        })
        .unwrap_or(false)
}

fn read_venv_version() -> Option<(u32, u32)> {
    let content = std::fs::read_to_string(get_venv_dir().join("pyvenv.cfg")).ok()?;
    for line in content.lines() {
        if let Some(val) = line.strip_prefix("version = ") {
            let mut parts = val.trim().split('.');
            let major: u32 = parts.next()?.parse().ok()?;
            let minor: u32 = parts.next()?.parse().ok()?;
            return Some((major, minor));
        }
    }
    None
}

async fn get_python_minor_version(python: &str) -> Option<(u32, u32)> {
    let (stdout, stderr, code) =
        run_to_completion(python, &["--version"], None, None)
            .await
            .ok()?;
    if code != 0 {
        return None;
    }
    let output = if stdout.is_empty() { &stderr } else { &stdout };
    output.trim().strip_prefix("Python ").and_then(|v| {
        let mut parts = v.split('.');
        let major: u32 = parts.next()?.parse().ok()?;
        let minor: u32 = parts.next()?.parse().ok()?;
        Some((major, minor))
    })
}

async fn scan_filesystem_python() -> Option<String> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(target_os = "windows")]
    {
        if let Some(local_app_data) = dirs::data_local_dir() {
            let python_root = local_app_data.join("Programs").join("Python");
            if let Ok(entries) = std::fs::read_dir(&python_root) {
                let mut dir_entries: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.is_dir()
                            && p.file_name()
                                .and_then(|n| n.to_str())
                                .map(|n| {
                                    if let Some(digits) = n.strip_prefix("Python") {
                                        if digits.len() >= 3 {
                                            let mut chars = digits.chars();
                                            let major: u32 =
                                                chars.next().and_then(|c| c.to_digit(10)).unwrap_or(0);
                                            let minor: u32 = chars.as_str().parse().unwrap_or(0);
                                            return major == 3 && minor >= 10;
                                        }
                                    }
                                    false
                                })
                                .unwrap_or(false)
                    })
                    .collect();
                dir_entries.sort_by(|a, b| b.cmp(a));
                for dir in dir_entries {
                    candidates.push(dir.join("python.exe"));
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        let fw_root = PathBuf::from("/Library/Frameworks/Python.framework/Versions");
        if let Ok(entries) = std::fs::read_dir(&fw_root) {
            let mut ver_dirs: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| {
                                let parts: Vec<&str> = n.split('.').collect();
                                if parts.len() == 2 {
                                    let major: u32 = parts[0].parse().unwrap_or(0);
                                    let minor: u32 = parts[1].parse().unwrap_or(0);
                                    return major == 3 && minor >= 10;
                                }
                                false
                            })
                            .unwrap_or(false)
                })
                .collect();
            ver_dirs.sort_by(|a, b| b.cmp(a));
            for dir in ver_dirs {
                candidates.push(dir.join("bin").join("python3"));
            }
        }
    }

    for path in candidates {
        if !path.exists() {
            continue;
        }
        let path_str = path.to_string_lossy().to_string();
        let Ok((stdout, stderr, code)) =
            run_to_completion(&path_str, &["--version"], None, None).await
        else {
            continue;
        };
        if code != 0 {
            continue;
        }
        let output = if stdout.is_empty() { &stderr } else { &stdout };
        if python_version_ok(output.trim()) {
            return Some(path_str);
        }
    }

    None
}

pub async fn venv_pip_works() -> bool {
    let py = get_venv_python();
    if !py.exists() {
        return false;
    }
    let py_str = py.to_string_lossy().to_string();
    matches!(
        run_to_completion(&py_str, &["-m", "pip", "--version"], None, None).await,
        Ok((_, _, 0))
    )
}

pub async fn ensure_venv<F>(on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str) + Send,
{
    let venv_dir = get_venv_dir();
    // Resolve the system Python once upfront — needed in both the healthy
    // fast-path check and the recreation path.
    let system_python = find_system_python().await?;

    if is_venv_ready() {
        // Compare the venv's recorded Python minor version against the current
        // system Python. A mismatch (e.g. runtime upgrade) means we must
        // rebuild even if the binary still exists and pip appears to run.
        let venv_ver = read_venv_version();
        let sys_ver = get_python_minor_version(&system_python).await;
        let version_ok = matches!((venv_ver, sys_ver), (Some(v), Some(s)) if v == s);

        if version_ok && venv_pip_works().await {
            return Ok(());
        }

        on_progress(2, "Rebuilding Python environment after system update...");
        tokio::fs::remove_dir_all(&venv_dir).await.ok();
    }

    if let Some(parent) = venv_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    on_progress(5, "Creating MediaHarbor Python environment...");

    // --copies makes the venv use a real copy of the Python binary instead of
    // a symlink. This pins the venv to the exact Python minor version it was
    // created with, so changes to the system python3 symlink (upgrades, pyenv,
    // Flatpak runtime bumps) cannot silently break the installed packages.
    let (_, stderr, code) = run_to_completion(
        &system_python,
        &["-m", "venv", "--copies", venv_dir.to_str().unwrap_or(".")],
        None,
        None,
    )
    .await?;

    if code != 0 || !is_venv_ready() {
        return Err(MhError::Subprocess(format!(
            "Failed to create Python virtual environment (exit {code}): {stderr}"
        )));
    }

    on_progress(15, "Python environment created");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn venv_dir_is_in_data_local() {
        let dir = get_venv_dir();
        let data = dirs::data_local_dir().unwrap();
        assert!(dir.starts_with(data));
    }

    #[test]
    fn resolve_command_returns_name_when_absent() {
        let resolved = resolve_command("nonexistent_bin_xyz", &[]);
        assert_eq!(resolved, PathBuf::from("nonexistent_bin_xyz"));
    }
}
