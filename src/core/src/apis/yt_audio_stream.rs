
use std::path::Path;
use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use reqwest::Client;

use crate::errors::{MhError, MhResult};
use crate::venv_manager::{get_venv_python, is_venv_ready};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60 * 60); // 5 hours
const YTDLP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub url: String,
    pub mime_type: String,
    pub quality_label: String,
}

/// All information about a YouTube video stream resolved in a single yt-dlp call.
#[derive(Debug)]
pub struct VideoStreamInfo {
    /// Primary (or combined) stream. For live streams this is the HLS manifest URL.
    pub video: StreamInfo,
    /// Secondary audio stream. Identical to `video` when a combined URL is returned.
    pub audio: StreamInfo,
    pub is_live: bool,
    pub duration_sec: Option<f64>,
}

pub struct YtAudioStreamCache {
    inner: DashMap<String, (StreamInfo, Instant)>,
}

impl YtAudioStreamCache {
    pub fn new() -> Self {
        Self {
            inner: DashMap::new(),
        }
    }
}

impl Default for YtAudioStreamCache {
    fn default() -> Self {
        Self::new()
    }
}

fn cache_get(cache: &YtAudioStreamCache, key: &str) -> Option<StreamInfo> {
    let entry = cache.inner.get(key)?;
    let (info, inserted_at) = entry.value();
    if inserted_at.elapsed() < CACHE_TTL {
        Some(info.clone())
    } else {
        drop(entry);
        cache.inner.remove(key);
        None
    }
}

fn cache_set(cache: &YtAudioStreamCache, key: &str, info: StreamInfo) {
    cache.inner.insert(key.to_string(), (info, Instant::now()));
}

fn ytdlp_candidates(venv_python: Option<&Path>) -> Vec<String> {
    let mut candidates: Vec<String> = if cfg!(windows) {
        vec!["yt-dlp.exe".to_string(), "yt-dlp".to_string()]
    } else {
        vec!["yt-dlp".to_string()]
    };

    let venv = venv_python
        .map(|p| p.to_string_lossy().to_string())
        .or_else(|| {
            if is_venv_ready() {
                Some(get_venv_python().to_string_lossy().to_string())
            } else {
                None
            }
        });

    if let Some(py) = venv {
        candidates.push(format!("{} -m yt_dlp", py));
    }

    let python_names: &[&str] = if cfg!(windows) {
        &["py", "python", "python3"]
    } else {
        &["python", "python3", "py"]
    };
    for py in python_names {
        candidates.push(format!("{} -m yt_dlp", py));
    }

    candidates
}

async fn run_ytdlp_any_candidate(
    extra_args: &[&str],
    url: &str,
    venv_python: Option<&Path>,
) -> MhResult<Vec<String>> {
    let candidates = ytdlp_candidates(venv_python);
    let mut last_err = MhError::Subprocess(
        "yt-dlp is not installed or not found. \
         Install it from the MediaHarbor settings page (Dependencies → yt-dlp)."
            .to_string(),
    );
    for cmd in &candidates {
        match run_ytdlp(cmd, extra_args, url).await {
            Ok(lines) => return Ok(lines),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("yt-dlp not found")
                    || msg.contains("No such file")
                    || msg.contains("no module named")
                {
                    last_err = e;
                    continue;
                }
                return Err(e);
            }
        }
    }
    Err(last_err)
}

pub fn find_yt_dlp_command(venv_python: Option<&Path>) -> Option<String> {
    ytdlp_candidates(venv_python).into_iter().next()
}

async fn run_ytdlp(cmd_str: &str, extra_args: &[&str], url: &str) -> MhResult<Vec<String>> {
    let parts: Vec<&str> = cmd_str.splitn(3, ' ').collect();
    let (program, prefix_args): (&str, Vec<&str>) = match parts.as_slice() {
        [p] => (p, vec![]),
        [p, "-m", module] => (p, vec!["-m", module]),
        [p, rest @ ..] => (p, rest.to_vec()),
        _ => return Err(MhError::Subprocess("Empty yt-dlp command".to_string())),
    };

    let mut args: Vec<&str> = prefix_args;
    args.extend_from_slice(extra_args);
    args.push(url);

    let output = tokio::time::timeout(
        YTDLP_TIMEOUT,
        Command::new(program)
            .args(&args)
            .output(),
    )
    .await
    .map_err(|_| MhError::Subprocess("yt-dlp timed out".to_string()))?
    .map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            MhError::Subprocess(format!("yt-dlp not found: {}", cmd_str))
        } else {
            MhError::Subprocess(e.to_string())
        }
    })?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<String> = stdout
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() {
            return Ok(lines);
        }
    }

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let friendly = categorise_ytdlp_error(&stderr);
    Err(MhError::Subprocess(friendly))
}

fn categorise_ytdlp_error(stderr: &str) -> String {
    if stderr.contains("No video formats found")
        || stderr.contains("is not a valid URL")
        || stderr.contains("Unsupported URL")
    {
        return format!("Unsupported or invalid URL: {}", stderr.trim());
    }
    if stderr.contains("HTTP Error 403") || stderr.contains("HTTP Error 429") {
        return "Blocked by server — try again later".to_string();
    }
    if stderr.contains("getaddrinfo")
        || stderr.contains("network")
        || stderr.contains("Connection refused")
    {
        return "No internet connection or server unreachable".to_string();
    }
    if stderr.is_empty() {
        return "yt-dlp failed (no output)".to_string();
    }
    stderr.trim().to_string()
}

const AUDIO_FLAGS: &[&str] = &[
    "-f",
    "bestaudio[ext=m4a]/bestaudio/best",
    "--get-url",
    "--no-playlist",
    "--no-warnings",
    "--quiet",
];

const VIDEO_FLAGS: &[&str] = &[
    "-f",
    "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
    "--get-url",
    "--no-playlist",
    "--no-warnings",
    "--quiet",
];

/// Like VIDEO_FLAGS but prepends `--print is_live` and `--print duration` so that
/// live-stream detection, duration, and CDN URLs are all resolved in a single yt-dlp call.
const VIDEO_STREAM_FLAGS: &[&str] = &[
    "--print", "is_live",
    "--print", "duration",
    "-f", "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best",
    "--get-url",
    "--no-playlist",
    "--no-warnings",
    "--quiet",
];

/// Resolves live-stream status, duration, and CDN URL(s) in a single yt-dlp invocation.
///
/// Output lines are split into:
/// - `http`-prefixed lines → CDN URL(s) (`--get-url` output)
/// - everything else → metadata (`--print is_live` / `--print duration`)
///
/// Degrades gracefully on older yt-dlp that doesn't support `--print`: metadata lines will
/// simply be absent, `is_live` defaults to `false` and `duration_sec` to `None`.
pub async fn get_video_stream_info(
    video_url_or_id: &str,
    yt_dlp_cmd: Option<&str>,
) -> MhResult<VideoStreamInfo> {
    let watch_url = to_watch_url(video_url_or_id);

    let lines = if let Some(cmd) = yt_dlp_cmd {
        run_ytdlp(cmd, VIDEO_STREAM_FLAGS, &watch_url).await?
    } else {
        run_ytdlp_any_candidate(VIDEO_STREAM_FLAGS, &watch_url, None).await?
    };

    // Separate URL lines from metadata lines (order-independent and version-agnostic).
    let url_lines: Vec<String> = lines.iter().filter(|l| l.starts_with("http")).cloned().collect();
    let meta_lines: Vec<&String> = lines.iter().filter(|l| !l.starts_with("http")).collect();

    let is_live = meta_lines.first().map(|s| s.eq_ignore_ascii_case("true")).unwrap_or(false);
    let duration_sec = meta_lines.get(1).and_then(|s| s.parse::<f64>().ok());

    let video_url = url_lines
        .first()
        .cloned()
        .ok_or_else(|| MhError::Subprocess("yt-dlp returned no URL".to_string()))?;

    // For a combined/live single-URL format, audio == video.
    let audio_url = url_lines.get(1).cloned().unwrap_or_else(|| video_url.clone());

    let video_mime = guess_mime_from_url(&video_url, "video/mp4");
    let audio_mime = guess_mime_from_url(&audio_url, "audio/mp4");

    Ok(VideoStreamInfo {
        video: StreamInfo { url: video_url, mime_type: video_mime, quality_label: "video".to_string() },
        audio: StreamInfo { url: audio_url, mime_type: audio_mime, quality_label: "audio".to_string() },
        is_live,
        duration_sec,
    })
}

fn mime_to_quality_label(mime: &str) -> String {
    if mime.contains("opus") || mime.contains("webm") {
        "opus".to_string()
    } else if mime.contains("mp4a") || mime.contains("mp4") {
        "mp4a".to_string()
    } else {
        "audio".to_string()
    }
}

fn normalise_watch_url(url: &str) -> String {
    if let Ok(mut u) = url::Url::parse(url) {
        if u.host_str() == Some("music.youtube.com") || u.host_str() == Some("m.youtube.com") {
            let _ = u.set_host(Some("www.youtube.com"));
        }
        return u.to_string();
    }
    url.to_string()
}

fn to_watch_url(url_or_id: &str) -> String {
    if url_or_id.starts_with("http") {
        normalise_watch_url(url_or_id)
    } else {
        format!("https://www.youtube.com/watch?v={}", url_or_id)
    }
}

pub async fn get_audio_stream_url(
    video_url_or_id: &str,
    cache: &YtAudioStreamCache,
    yt_dlp_cmd: Option<&str>,
) -> MhResult<StreamInfo> {
    if let Some(cached) = cache_get(cache, video_url_or_id) {
        return Ok(cached);
    }

    let watch_url = to_watch_url(video_url_or_id);

    let lines = if let Some(cmd) = yt_dlp_cmd {
        run_ytdlp(cmd, AUDIO_FLAGS, &watch_url).await?
    } else {
        run_ytdlp_any_candidate(AUDIO_FLAGS, &watch_url, None).await?
    };
    let stream_url = lines
        .into_iter()
        .next()
        .ok_or_else(|| MhError::Subprocess("yt-dlp returned no URL".to_string()))?;

    let mime = url::Url::parse(&stream_url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "mime")
                .map(|(_, v)| v.to_string())
        })
        .unwrap_or_else(|| "audio/webm".to_string());

    let quality = mime_to_quality_label(&mime);
    let info = StreamInfo {
        url: stream_url,
        mime_type: mime,
        quality_label: quality,
    };

    cache_set(cache, video_url_or_id, info.clone());
    Ok(info)
}

pub async fn get_video_stream_url(
    video_url_or_id: &str,
    yt_dlp_cmd: Option<&str>,
) -> MhResult<(StreamInfo, StreamInfo)> {
    let watch_url = to_watch_url(video_url_or_id);

    let lines = if let Some(cmd) = yt_dlp_cmd {
        run_ytdlp(cmd, VIDEO_FLAGS, &watch_url).await?
    } else {
        run_ytdlp_any_candidate(VIDEO_FLAGS, &watch_url, None).await?
    };

    let video_url = lines
        .first()
        .cloned()
        .ok_or_else(|| MhError::Subprocess("yt-dlp returned no video URL".to_string()))?;

    let audio_url = lines.get(1).cloned().unwrap_or_else(|| video_url.clone());

    let video_mime = guess_mime_from_url(&video_url, "video/mp4");
    let audio_mime = guess_mime_from_url(&audio_url, "audio/mp4");

    Ok((
        StreamInfo {
            url: video_url,
            mime_type: video_mime.clone(),
            quality_label: "video".to_string(),
        },
        StreamInfo {
            url: audio_url,
            mime_type: audio_mime,
            quality_label: "audio".to_string(),
        },
    ))
}

pub async fn proxy_audio_stream(
    url: &str,
    range_header: Option<&str>,
    client: &Client,
) -> MhResult<(Bytes, String)> {
    let mut req = client.get(url).header("User-Agent", crate::http_client::UA_MOZILLA);

    if let Some(range) = range_header {
        req = req.header("Range", range);
    }

    let resp = req.send().await.map_err(MhError::Network)?;

    if resp.status().as_u16() >= 400 {
        return Err(MhError::Other(format!(
            "Upstream stream fetch failed: HTTP {}",
            resp.status()
        )));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("audio/webm")
        .to_string();

    let data = resp.bytes().await.map_err(MhError::Network)?;
    Ok((data, content_type))
}


fn guess_mime_from_url(url: &str, default: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.query_pairs()
                .find(|(k, _)| k == "mime")
                .map(|(_, v)| v.to_string())
        })
        .unwrap_or_else(|| default.to_string())
}

