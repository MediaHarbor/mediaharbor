
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use once_cell::sync::Lazy;
use regex::Regex;
use tokio::{
    process::Command,
    sync::mpsc,
};

use crate::{
    defaults::Settings,
    errors::{MhError, MhResult},
    subprocess::LineSource,
    venv_manager::resolve_command,
};

#[derive(Debug, Clone)]
pub struct BatchProgress {
    pub completed: u32,
    pub total: u32,
    pub current_track: String,
    pub percent: f32,
}

#[derive(Debug, Clone)]
pub enum InteractivePromptType {
    CookiesDialog,
    WvdDialog,
    OAuthUrl(String),
    InteractiveSelection(String),
}

static ANSI_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());

static PROGRESS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[download\]\s+(\d+(?:\.\d+)?)%").unwrap());

static TRACK_COUNT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\[Track\s+(\d+)/(\d+)\]").unwrap());

static FOUND_TRACKS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Found\s+(\d+)\s+tracks?").unwrap());

static DOWNLOADING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"Downloading\s+"(.+)""#).unwrap());

static SKIPPING_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"Skipping\s+"(.+?)":\s*(.+)"#).unwrap());

static FINISHED_ERRORS_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"Finished with (\d+) error").unwrap());

static OAUTH_URL_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"https://accounts\.spotify\.com/authorize[^\s]+").unwrap()
});

static DOES_NOT_EXIST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"does not exist").unwrap()
});

pub fn detect_interactive_prompt(line: &str) -> Option<InteractivePromptType> {
    if DOES_NOT_EXIST_RE.is_match(line)
        && line.to_lowercase().contains("press enter to continue")
    {
        if line.to_lowercase().contains("cookies") {
            return Some(InteractivePromptType::CookiesDialog);
        } else {
            return Some(InteractivePromptType::WvdDialog);
        }
    }

    if line.contains("Click on the following link to login:") {
        return None; // The URL will be on a following line
    }
    if let Some(caps) = OAUTH_URL_RE.captures(line) {
        return Some(InteractivePromptType::OAuthUrl(caps[0].to_string()));
    }

    if line.contains("Select which") && (line.contains("to download") || line.contains("codec")) {
        return Some(InteractivePromptType::InteractiveSelection(line.to_string()));
    }

    None
}

pub fn build_gamdl_args(settings: &Settings, url: &str, config_path: &Path) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    args.push("--config-path".into());
    args.push(config_path.to_string_lossy().to_string());

    if settings.create_platform_subfolders {
        let output_path = std::path::Path::new(&settings.download_location)
            .join("Apple Music")
            .to_string_lossy()
            .to_string();
        args.push("-o".into());
        args.push(output_path);
    }

    if !url.is_empty() {
        args.push(url.to_string());
    }
    args
}

pub fn build_votify_args(settings: &Settings, url: &str, config_path: &Path) -> Vec<String> {
    build_votify_args_inner(settings, config_path, false, Some(url), None, None)
}

pub fn build_votify_batch_args(
    settings: &Settings,
    file_path: &Path,
    quality: Option<&str>,
    config_path: &Path,
) -> Vec<String> {
    build_votify_args_inner(settings, config_path, true, None, Some(file_path), quality)
}

fn build_votify_args_inner(
    settings: &Settings,
    config_path: &Path,
    is_batch: bool,
    url: Option<&str>,
    batch_file: Option<&Path>,
    _quality_override: Option<&str>,
) -> Vec<String> {
    let mut args: Vec<String> = Vec::new();

    args.push("--config-path".into());
    args.push(config_path.to_string_lossy().to_string());

    if settings.create_platform_subfolders {
        let output_path = std::path::Path::new(&settings.download_location)
            .join("Spotify")
            .to_string_lossy()
            .to_string();
        args.push("-o".into());
        args.push(output_path);
    }

    if is_batch {
        args.push("-r".into());
        if let Some(fp) = batch_file {
            args.push(fp.to_string_lossy().to_string());
        }
    } else if let Some(u) = url {
        args.push(u.to_string());
    }

    args
}

#[allow(dead_code)]
struct ProcessHandle {
    stdin: tokio::process::ChildStdin,
    stdout_rx: mpsc::Receiver<(LineSource, String)>,
    child: tokio::process::Child,
}

async fn spawn_gamdl_process(
    command: &str,
    args: &[String],
) -> MhResult<ProcessHandle> {
    use std::process::Stdio;

    let resolved = resolve_command(command, &[]);
    let resolved_str = resolved.to_string_lossy().to_string();

    let mut cmd = Command::new(&resolved_str);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("LANG", "en_US.UTF-8")
        .env("LC_ALL", "en_US.UTF-8");

    let mut child = cmd.spawn().map_err(|e| {
        MhError::Subprocess(format!("Failed to spawn `{}`: {}", command, e))
    })?;

    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let stdin = child.stdin.take().unwrap();

    let (tx, rx) = mpsc::channel::<(LineSource, String)>(1024);

    let tx_out = tx.clone();
    let tx_err = tx.clone();

    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = stdout;
        let mut pending = String::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let mut start = 0;
                    for (i, c) in pending.char_indices() {
                        if c == '\r' || c == '\n' {
                            let line = pending[start..i].trim().to_string();
                            if !line.is_empty() && tx_out.send((LineSource::Stdout, line)).await.is_err() {
                                return;
                            }
                            start = i + c.len_utf8();
                        }
                    }
                    pending = pending[start..].to_string();
                }
            }
        }
    });

    tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut reader = stderr;
        let mut pending = String::new();
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    pending.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let mut start = 0;
                    for (i, c) in pending.char_indices() {
                        if c == '\r' || c == '\n' {
                            let line = pending[start..i].trim().to_string();
                            if !line.is_empty() && tx_err.send((LineSource::Stderr, line)).await.is_err() {
                                return;
                            }
                            start = i + c.len_utf8();
                        }
                    }
                    pending = pending[start..].to_string();
                }
            }
        }
    });

    Ok(ProcessHandle {
        stdin,
        stdout_rx: rx,
        child,
    })
}

pub async fn download_with_gamdl(
    settings: &Settings,
    url: &str,
    config_path: &Path,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_prompt: impl Fn(InteractivePromptType) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let args = build_gamdl_args(settings, url, config_path);

    let mut handle = spawn_gamdl_process("gamdl", &args).await?;

    run_gamrip_loop(
        &mut handle,
        cancel_flag,
        on_progress,
        on_prompt,
    )
    .await
}

pub async fn download_with_votify(
    settings: &Settings,
    url: &str,
    config_path: &Path,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_prompt: impl Fn(InteractivePromptType) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let args = build_votify_args(settings, url, config_path);

    let mut handle = spawn_gamdl_process("votify", &args).await?;

    run_gamrip_loop(
        &mut handle,
        cancel_flag,
        on_progress,
        on_prompt,
    )
    .await
}

pub async fn download_with_votify_batch(
    settings: &Settings,
    batch_file: &Path,
    quality: Option<&str>,
    config_path: &Path,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_prompt: impl Fn(InteractivePromptType) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let args = build_votify_batch_args(settings, batch_file, quality, config_path);

    let mut handle = spawn_gamdl_process("votify", &args).await?;

    run_gamrip_loop(
        &mut handle,
        cancel_flag,
        on_progress,
        on_prompt,
    )
    .await
}

pub async fn download_with_gamdl_batch(
    settings: &Settings,
    batch_file: &Path,
    config_path: &Path,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_prompt: impl Fn(InteractivePromptType) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let mut args = build_gamdl_args(settings, "", config_path);
    args.pop(); // remove empty url
    args.push("-r".into());
    args.push(batch_file.to_string_lossy().to_string());

    let mut handle = spawn_gamdl_process("gamdl", &args).await?;

    run_gamrip_loop(
        &mut handle,
        cancel_flag,
        on_progress,
        on_prompt,
    )
    .await
}

async fn run_gamrip_loop(
    handle: &mut ProcessHandle,
    cancel_flag: Arc<AtomicBool>,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_prompt: impl Fn(InteractivePromptType) + Send + 'static,
) -> MhResult<()> {
    let mut total_tracks: u32 = 0;
    let mut completed_tracks: u32 = 0;
    let mut current_track = String::from("Unknown Track");
    let mut full_output = String::new();
    let mut skipped_tracks: Vec<(String, String)> = Vec::new();
    let mut last_progress_emit = Instant::now();
    let mut track_progress: std::collections::HashMap<String, f32> =
        std::collections::HashMap::new();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            let _ = handle.child.kill().await;
            return Err(MhError::Cancelled);
        }

        match tokio::time::timeout(Duration::from_millis(200), handle.stdout_rx.recv()).await {
            Ok(Some((_source, line))) => {
                full_output.push_str(&line);
                full_output.push('\n');

                let stripped = ANSI_RE.replace_all(&line, "");
                let clean_line = stripped.trim_start_matches("Error: ").trim().to_string();

                if let Some(prompt) = detect_interactive_prompt(&clean_line) {
                    match &prompt {
                        InteractivePromptType::InteractiveSelection(_) => {
                            let _ = handle.child.kill().await;
                            on_prompt(prompt);
                            return Err(MhError::Subprocess(
                                "Interactive selection not supported".into(),
                            ));
                        }
                        _ => {
                            on_prompt(prompt);
                        }
                    }
                    continue;
                }

                if let Some(caps) = TRACK_COUNT_RE.captures(&clean_line) {
                    total_tracks = caps[2].parse().unwrap_or(total_tracks);
                    let current_idx: u32 = caps[1].parse().unwrap_or(1);
                    completed_tracks = current_idx.saturating_sub(1);

                    let pct = if total_tracks > 0 {
                        (completed_tracks as f32 / total_tracks as f32 * 100.0).min(99.0)
                    } else {
                        0.0
                    };
                    on_progress(BatchProgress {
                        completed: completed_tracks,
                        total: total_tracks,
                        current_track: current_track.clone(),
                        percent: pct,
                    });
                    last_progress_emit = Instant::now();
                }

                if let Some(caps) = FOUND_TRACKS_RE.captures(&clean_line) {
                    total_tracks = caps[1].parse().unwrap_or(total_tracks);
                }

                if let Some(caps) = DOWNLOADING_RE.captures(&clean_line) {
                    current_track = caps[1].trim().to_string();
                }

                if let Some(caps) = SKIPPING_RE.captures(&clean_line) {
                    skipped_tracks.push((
                        caps[1].trim().to_string(),
                        caps[2].trim().to_string(),
                    ));
                    completed_tracks = completed_tracks.saturating_add(1).min(total_tracks);
                    track_progress.remove(&current_track);
                }

                if clean_line.contains("Finished with") {
                    completed_tracks = total_tracks;
                    track_progress.clear();
                }

                if let Some(caps) = PROGRESS_RE.captures(&clean_line) {
                    let pct: f32 = caps[1].parse().unwrap_or(0.0);
                    track_progress.insert(current_track.clone(), pct);

                    let total_progress: f32 = track_progress.values().sum();
                    let overall = if total_tracks > 0 {
                        (total_progress / (total_tracks as f32 * 100.0) * 100.0).min(100.0)
                    } else {
                        pct
                    };

                    if last_progress_emit.elapsed() >= Duration::from_millis(250) {
                        last_progress_emit = Instant::now();
                        on_progress(BatchProgress {
                            completed: completed_tracks,
                            total: total_tracks,
                            current_track: current_track.clone(),
                            percent: overall,
                        });
                    }
                }
            }
            Ok(None) => {
                break;
            }
            Err(_timeout) => {
                if cancel_flag.load(Ordering::Relaxed) {
                    let _ = handle.child.kill().await;
                    return Err(MhError::Cancelled);
                }
            }
        }
    }

    let status = handle.child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);

    if exit_code != 0 {
        if let Some(caps) = FINISHED_ERRORS_RE.captures(&full_output) {
            let n: u32 = caps[1].parse().unwrap_or(1);
            if n > 0 {
                let skip_msg = if !skipped_tracks.is_empty() {
                    skipped_tracks
                        .iter()
                        .map(|(t, r)| format!("\"{}\" skipped: {}", t, r))
                        .collect::<Vec<_>>()
                        .join("; ")
                } else {
                    format!("Finished with {} error(s)", n)
                };
                return Err(MhError::Subprocess(skip_msg));
            }
        }

        return Err(MhError::Subprocess(format!(
            "Process exited with code {}: {}",
            exit_code,
            full_output.lines().rev().take(5).collect::<Vec<_>>().join(" | ")
        )));
    }

    if !skipped_tracks.is_empty() && completed_tracks == 0 {
        let skip_msg = skipped_tracks
            .iter()
            .map(|(t, r)| format!("\"{}\" skipped: {}", t, r))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(MhError::Subprocess(format!("All tracks skipped: {}", skip_msg)));
    }

    on_progress(BatchProgress {
        completed: total_tracks.max(completed_tracks),
        total: total_tracks.max(1),
        current_track,
        percent: 100.0,
    });

    Ok(())
}

pub async fn write_votify_config(settings: &Settings, config_path: &Path) -> MhResult<()> {
    crate::settings::save_service_config(config_path, settings, "spotify").await
}

pub async fn write_gamdl_config(settings: &Settings, config_path: &Path) -> MhResult<()> {
    crate::settings::save_service_config(config_path, settings, "apple").await
}

pub fn clear_spotify_credentials(mut settings: Settings) -> Settings {
    settings.spotify_cookies_path = String::new();
    settings.spotify_wvd_path = String::new();
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_cookies_dialog() {
        let line = "cookies file does not exist, press enter to continue";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::CookiesDialog)));
    }

    #[test]
    fn detect_wvd_dialog() {
        let line = "WVD file does not exist, press enter to continue";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::WvdDialog)));
    }

    #[test]
    fn detect_oauth_url() {
        let line = "https://accounts.spotify.com/authorize?client_id=abc&response_type=code";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::OAuthUrl(_))));
    }

    #[test]
    fn detect_interactive_selection() {
        let line = "Select which codec to download:";
        let result = detect_interactive_prompt(line);
        assert!(matches!(result, Some(InteractivePromptType::InteractiveSelection(_))));
    }

    #[test]
    fn no_prompt_for_normal_line() {
        let line = "[download]  50.3% of 10.00MiB at 1.23MiB/s ETA 00:04";
        assert!(detect_interactive_prompt(line).is_none());
    }

    #[test]
    fn gamdl_args_has_config_and_output() {
        let settings = Settings::default();
        let tmp = std::env::temp_dir();
        let config = tmp.join("gamdl_config.ini");
        let config_str = config.to_string_lossy().to_string();
        let args = build_gamdl_args(&settings, "https://music.apple.com/album/123", &config);
        assert!(args.contains(&"--config-path".to_string()));
        assert!(args.contains(&config_str));
        assert_eq!(args.last().unwrap(), "https://music.apple.com/album/123");
    }

    #[test]
    fn votify_args_has_output() {
        let settings = Settings::default();
        let tmp = std::env::temp_dir();
        let config = tmp.join("votify_config.ini");
        let args = build_votify_args(&settings, "https://open.spotify.com/track/abc", &config);
        assert!(args.contains(&"--config-path".to_string()));
        assert_eq!(args.last().unwrap(), "https://open.spotify.com/track/abc");
    }
}
