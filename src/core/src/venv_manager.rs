use std::path::{Path, PathBuf};

use crate::{
    errors::{MhError, MhResult},
    subprocess::run_to_completion,
};

pub fn get_venv_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mediaharbor")
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

pub async fn find_system_python() -> MhResult<String> {
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
        if let Some(caps) = output
            .trim()
            .strip_prefix("Python ")
            .and_then(|v| {
                let mut parts = v.split('.');
                let major: u32 = parts.next()?.parse().ok()?;
                let minor: u32 = parts.next()?.parse().ok()?;
                Some((major, minor))
            })
        {
            if caps.0 == 3 && caps.1 >= 10 {
                return Ok(cmd.to_string());
            }
        }
    }

    Err(MhError::Subprocess(
        "No suitable Python 3.10+ installation found. Please install Python 3.10 or newer."
            .into(),
    ))
}

pub async fn ensure_venv<F>(on_progress: F) -> MhResult<()>
where
    F: Fn(u8, &str) + Send,
{
    if is_venv_ready() {
        return Ok(());
    }

    let system_python = find_system_python().await?;
    let venv_dir = get_venv_dir();

    if let Some(parent) = venv_dir.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    on_progress(5, "Creating MediaHarbor Python environment...");

    let (_, stderr, code) = run_to_completion(
        &system_python,
        &["-m", "venv", venv_dir.to_str().unwrap_or(".")],
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
    fn venv_dir_is_in_home() {
        let dir = get_venv_dir();
        let home = dirs::home_dir().unwrap();
        assert!(dir.starts_with(home));
    }

    #[test]
    fn resolve_command_returns_name_when_absent() {
        let resolved = resolve_command("nonexistent_bin_xyz", &[]);
        assert_eq!(resolved, PathBuf::from("nonexistent_bin_xyz"));
    }
}
