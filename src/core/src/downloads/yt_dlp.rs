
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::{sync::mpsc, time::Instant};

use crate::{
    defaults::Settings,
    errors::{MhError, MhResult},
    subprocess::{spawn_with_output, LineSource},
    venv_manager::get_venv_bin,
};

#[derive(Debug, Clone)]
pub struct YtDlpMusicArgs {
    pub url: String,
    pub output_template: String,
    pub format: String,
    pub download_path: String,
    pub quality: String,
    pub cookies_path: Option<String>,
    pub cookies_from_browser: Option<String>,
    pub proxy: Option<String>,
    pub no_playlist: bool,
    pub max_retries: u32,
    pub continue_download: bool,
    pub speed_limit: Option<String>, // e.g. "5M"
    pub use_aria2: bool,
    pub embed_thumbnail: bool,
    pub add_metadata: bool,
    pub use_authentication: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub is_playlist: bool,
}

#[derive(Debug, Clone)]
pub struct YtDlpVideoArgs {
    pub url: String,
    pub format: String,
    pub output_template: String,
    pub download_path: String,
    pub cookies_path: Option<String>,
    pub cookies_from_browser: Option<String>,
    pub proxy: Option<String>,
    pub no_playlist: bool,
    pub max_retries: u32,
    pub continue_download: bool,
    pub speed_limit: Option<String>,
    pub use_aria2: bool,
    pub merge_output_format: Option<String>,
    pub add_metadata: bool,
    pub embed_chapters: bool,
    pub add_subtitles: bool,
    pub no_sponsorblock: bool,
    pub sponsorblock_mark: Option<String>,
    pub sponsorblock_remove: Option<String>,
    pub sponsorblock_chapter_title: Option<String>,
    pub sponsorblock_api_url: Option<String>,
    pub use_authentication: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub is_playlist: bool,
    pub is_generic: bool,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub percent: f32,
    pub speed: String,
    pub eta: String,
    pub status: String,
    pub item_index: Option<u32>,
    pub item_total: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaMetadata {
    pub title: String,
    pub uploader: String,
    pub duration: String,
    pub thumbnail: String,
    pub is_playlist: bool,
    pub entries_count: Option<u32>,
}

pub fn find_yt_dlp_command(venv_python: Option<&Path>) -> String {
    let venv_bin = get_venv_bin("yt-dlp");
    if venv_bin.exists() {
        return venv_bin.to_string_lossy().to_string();
    }

    if let Some(py) = venv_python {
        let parent = py.parent().unwrap_or(Path::new("."));
        let bin = if cfg!(windows) {
            parent.join("yt-dlp.exe")
        } else {
            parent.join("yt-dlp")
        };
        if bin.exists() {
            return bin.to_string_lossy().to_string();
        }
    }

    "yt-dlp".to_string()
}

pub fn build_music_args(args: &YtDlpMusicArgs) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();

    v.push("-x".into());
    v.push("--audio-format".into());
    v.push(args.format.clone());
    v.push("--audio-quality".into());
    v.push(args.quality.clone());

    let out_path = format!(
        "{}/{}",
        args.download_path,
        args.output_template
    );
    v.push("--output".into());
    v.push(out_path);

    v.push("--no-warnings".into());
    v.push("--ignore-errors".into());
    v.push("--no-abort-on-error".into());
    v.push("--newline".into());
    v.push("--progress".into());

    if args.no_playlist {
        v.push("--no-playlist".into());
    }

    v.push("--retries".into());
    v.push(args.max_retries.to_string());

    if args.continue_download {
        v.push("--continue".into());
    } else {
        v.push("--no-continue".into());
    }

    if let Some(ref limit) = args.speed_limit {
        v.push("-r".into());
        v.push(limit.clone());
    }

    if args.use_aria2 {
        v.push("--downloader".into());
        v.push("aria2c".into());
    }

    if let Some(ref proxy) = args.proxy {
        v.push("--proxy".into());
        v.push(proxy.clone());
    }

    if args.use_authentication {
        if let (Some(user), Some(pass)) = (&args.username, &args.password) {
            v.push("--username".into());
            v.push(user.clone());
            v.push("--password".into());
            v.push(pass.clone());
        }
    }

    if let Some(ref cookies) = args.cookies_path {
        v.push("--cookies".into());
        v.push(cookies.clone());
    } else if let Some(ref browser) = args.cookies_from_browser {
        v.push("--cookies-from-browser".into());
        v.push(browser.clone());
    }

    if args.embed_thumbnail {
        v.push("--embed-thumbnail".into());
    }
    if args.add_metadata {
        v.push("--embed-metadata".into());
    }

    v.push(args.url.clone());
    v
}

pub fn build_video_args(args: &YtDlpVideoArgs) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();

    v.push("-f".into());
    v.push(args.format.clone());

    let out_path = format!("{}/{}", args.download_path, args.output_template);
    v.push("--output".into());
    v.push(out_path);

    v.push("--no-warnings".into());
    v.push("--ignore-errors".into());
    v.push("--no-abort-on-error".into());
    v.push("--newline".into());
    v.push("--progress".into());

    if !args.is_generic {
        if let Some(ref fmt) = args.merge_output_format {
            v.push("--merge-output-format".into());
            v.push(fmt.clone());
        }
    }

    if args.no_playlist || args.is_generic {
        v.push("--no-playlist".into());
    }

    v.push("--retries".into());
    v.push(args.max_retries.to_string());

    if args.continue_download {
        v.push("--continue".into());
    } else {
        v.push("--no-continue".into());
    }

    if let Some(ref limit) = args.speed_limit {
        v.push("-r".into());
        v.push(limit.clone());
    }

    if args.use_aria2 {
        v.push("--downloader".into());
        v.push("aria2c".into());
    }

    if let Some(ref proxy) = args.proxy {
        v.push("--proxy".into());
        v.push(proxy.clone());
    }

    if args.use_authentication {
        if let (Some(user), Some(pass)) = (&args.username, &args.password) {
            v.push("--username".into());
            v.push(user.clone());
            v.push("--password".into());
            v.push(pass.clone());
        }
    }

    if !args.no_sponsorblock {
        if let Some(ref mark) = args.sponsorblock_mark {
            v.push("--sponsorblock-mark".into());
            v.push(mark.clone());
        }
        if let Some(ref remove) = args.sponsorblock_remove {
            v.push("--sponsorblock-remove".into());
            v.push(remove.clone());
        }
        if let Some(ref title) = args.sponsorblock_chapter_title {
            v.push("--sponsorblock-chapter-title".into());
            v.push(title.clone());
        }
        if let Some(ref api) = args.sponsorblock_api_url {
            v.push("--sponsorblock-api".into());
            v.push(api.clone());
        }
    }

    if let Some(ref cookies) = args.cookies_path {
        v.push("--cookies".into());
        v.push(cookies.clone());
    } else if let Some(ref browser) = args.cookies_from_browser {
        v.push("--cookies-from-browser".into());
        v.push(browser.clone());
    }

    if args.add_metadata {
        v.push("--write-thumbnail".into());
        v.push("--embed-thumbnail".into());
        v.push("--add-metadata".into());
    }

    if args.embed_chapters {
        v.push("--embed-chapters".into());
    }

    if args.add_subtitles {
        v.push("--embed-subs".into());
        v.push("--sub-langs".into());
        v.push("all".into());
    }

    v.push(args.url.clone());
    v
}

pub fn music_args_from_settings(
    url: &str,
    quality: &str,
    settings: &Settings,
    is_playlist: bool,
) -> YtDlpMusicArgs {
    let speed_limit = if settings.download_speed_limit && settings.speed_limit_value > 0 {
        Some(format!("{}{}", settings.speed_limit_value, settings.speed_limit_type))
    } else {
        None
    };

    let cookies_path = if settings.use_cookies && !settings.cookies.is_empty() {
        Some(settings.cookies.clone())
    } else {
        None
    };
    let cookies_from_browser = if settings.use_cookies
        && cookies_path.is_none()
        && !settings.cookies_from_browser.is_empty()
    {
        Some(settings.cookies_from_browser.clone())
    } else {
        None
    };

    YtDlpMusicArgs {
        url: url.to_string(),
        output_template: settings.download_output_template.clone(),
        format: settings.youtube_audio_extensions.clone(),
        download_path: settings.download_location.clone(),
        quality: quality.to_string(),
        cookies_path,
        cookies_from_browser,
        proxy: if settings.use_proxy && !settings.proxy_url.is_empty() {
            Some(settings.proxy_url.clone())
        } else {
            None
        },
        no_playlist: false,
        max_retries: if settings.max_retries > 0 { settings.max_retries } else { 10 },
        continue_download: settings.continue_download,
        speed_limit,
        use_aria2: settings.use_aria2,
        embed_thumbnail: settings.add_metadata,
        add_metadata: settings.add_metadata,
        use_authentication: settings.use_authentication,
        username: if settings.use_authentication && !settings.username.is_empty() {
            Some(settings.username.clone())
        } else {
            None
        },
        password: if settings.use_authentication && !settings.password.is_empty() {
            Some(settings.password.clone())
        } else {
            None
        },
        is_playlist,
    }
}

pub fn video_args_from_settings(
    url: &str,
    quality: &str,
    settings: &Settings,
    is_playlist: bool,
    is_generic: bool,
) -> YtDlpVideoArgs {
    let speed_limit = if settings.download_speed_limit && settings.speed_limit_value > 0 {
        Some(format!("{}{}", settings.speed_limit_value, settings.speed_limit_type))
    } else {
        None
    };

    let cookies_path = if settings.use_cookies && !settings.cookies.is_empty() {
        Some(settings.cookies.clone())
    } else {
        None
    };
    let cookies_from_browser = if settings.use_cookies
        && cookies_path.is_none()
        && !settings.cookies_from_browser.is_empty()
    {
        Some(settings.cookies_from_browser.clone())
    } else {
        None
    };

    let merge_output_format =
        if !is_generic && settings.yt_override_download_extension && !settings.youtube_video_extensions.is_empty() {
            Some(settings.youtube_video_extensions.clone())
        } else {
            None
        };

    YtDlpVideoArgs {
        url: url.to_string(),
        format: quality.to_string(),
        output_template: settings.download_output_template.clone(),
        download_path: settings.download_location.clone(),
        cookies_path,
        cookies_from_browser,
        proxy: if settings.use_proxy && !settings.proxy_url.is_empty() {
            Some(settings.proxy_url.clone())
        } else {
            None
        },
        no_playlist: false,
        max_retries: if settings.max_retries > 0 { settings.max_retries } else { 10 },
        continue_download: settings.continue_download,
        speed_limit,
        use_aria2: settings.use_aria2,
        merge_output_format,
        add_metadata: settings.add_metadata,
        embed_chapters: settings.embed_chapters,
        add_subtitles: settings.add_subtitle_to_file,
        no_sponsorblock: settings.no_sponsorblock,
        sponsorblock_mark: if !settings.no_sponsorblock && !settings.sponsorblock_mark.is_empty() {
            Some(settings.sponsorblock_mark.clone())
        } else {
            None
        },
        sponsorblock_remove: if !settings.no_sponsorblock && !settings.sponsorblock_remove.is_empty() {
            Some(settings.sponsorblock_remove.clone())
        } else {
            None
        },
        sponsorblock_chapter_title: if !settings.no_sponsorblock
            && !settings.sponsorblock_chapter_title.is_empty()
        {
            Some(settings.sponsorblock_chapter_title.clone())
        } else {
            None
        },
        sponsorblock_api_url: if !settings.no_sponsorblock && !settings.sponsorblock_api_url.is_empty() {
            Some(settings.sponsorblock_api_url.clone())
        } else {
            None
        },
        use_authentication: settings.use_authentication,
        username: if settings.use_authentication && !settings.username.is_empty() {
            Some(settings.username.clone())
        } else {
            None
        },
        password: if settings.use_authentication && !settings.password.is_empty() {
            Some(settings.password.clone())
        } else {
            None
        },
        is_playlist,
        is_generic,
    }
}

pub async fn prefetch_metadata(url: &str, yt_dlp_cmd: &str) -> MhResult<MediaMetadata> {
    let args: Vec<&str> = vec![
        "--print", "%(title)s",
        "--print", "%(uploader)s",
        "--print", "%(duration_string)s",
        "--print", "%(thumbnail)s",
        "--no-download",
        url,
    ];

    let mut cmd = tokio::process::Command::new(yt_dlp_cmd);
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .env("PYTHONIOENCODING", "utf-8");

    let output = cmd.output().await.map_err(|e| {
        MhError::Subprocess(format!("Failed to run yt-dlp prefetch: {}", e))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut meta = MediaMetadata::default();
    if let Some(title) = lines.get(0) {
        meta.title = title.trim().to_string();
    }
    if let Some(uploader) = lines.get(1) {
        meta.uploader = uploader.trim().to_string();
    }
    if let Some(duration) = lines.get(2) {
        meta.duration = duration.trim().to_string();
    }
    if let Some(thumbnail) = lines.get(3) {
        let mut thumb = thumbnail.trim().to_string();
        if !thumb.starts_with("http") && !thumb.is_empty() {
            thumb = format!("https:{}", thumb);
        }
        meta.thumbnail = thumb;
    }

    Ok(meta)
}

pub async fn prefetch_playlist_metadata(url: &str, yt_dlp_cmd: &str) -> MhResult<MediaMetadata> {
    let args: Vec<&str> = vec![
        "--flat-playlist",
        "--print", "%(playlist)s",
        "--print", "%(playlist_uploader)s",
        "--print", "%(playlist_thumbnail)s",
        "--print", "%(playlist_count)s",
        "--no-download",
        url,
    ];

    let mut cmd = tokio::process::Command::new(yt_dlp_cmd);
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .env("PYTHONIOENCODING", "utf-8");

    let output = cmd.output().await.map_err(|e| {
        MhError::Subprocess(format!("Failed to run yt-dlp playlist prefetch: {}", e))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut meta = MediaMetadata::default();
    meta.is_playlist = true;

    if let Some(title) = lines.get(0) {
        meta.title = title.trim().to_string();
    }
    if let Some(uploader) = lines.get(1) {
        meta.uploader = uploader.trim().to_string();
    }
    if let Some(thumbnail) = lines.get(2) {
        let mut thumb = thumbnail.trim().to_string();
        if !thumb.starts_with("http") && !thumb.is_empty() {
            thumb = format!("https:{}", thumb);
        }
        meta.thumbnail = thumb;
    }
    if let Some(count_str) = lines.get(3) {
        meta.entries_count = count_str.trim().parse().ok();
    }

    Ok(meta)
}

mod _regexes {
    use once_cell::sync::Lazy;
    use regex::Regex;

    pub static PERCENT_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\[download\]\s+(\d+\.?\d*)%\s+of[^|]*\|\s*([^\s]+)\s*\|\s*ETA\s+([^\s]+)")
            .unwrap()
    });
    pub static PERCENT_SIMPLE_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(\d+\.?\d*)%").unwrap());
    pub static ITEM_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\[download\]\s+Downloading\s+(?:item|video)\s+(\d+)\s+of\s+(\d+)").unwrap()
    });
    pub static FRAGMENT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)Fragment\s+(\d+)\s+of\s+(\d+)").unwrap());
    pub static EXTRACT_AUDIO_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\[ExtractAudio\] Destination:").unwrap());
}

pub fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    if let Some(caps) = _regexes::PERCENT_RE.captures(line) {
        let percent: f32 = caps[1].parse().unwrap_or(0.0);
        let speed = caps[2].to_string();
        let eta = caps[3].to_string();

        let (item_index, item_total) = extract_item_counts(line);

        return Some(DownloadProgress {
            percent,
            speed,
            eta,
            status: "downloading".into(),
            item_index,
            item_total,
        });
    }

    if let Some(caps) = _regexes::PERCENT_SIMPLE_RE.captures(line) {
        let percent: f32 = caps[1].parse().unwrap_or(0.0);
        let (item_index, item_total) = extract_item_counts(line);
        return Some(DownloadProgress {
            percent,
            speed: String::new(),
            eta: String::new(),
            status: "downloading".into(),
            item_index,
            item_total,
        });
    }

    if let Some(caps) = _regexes::FRAGMENT_RE.captures(line) {
        let current: f32 = caps[1].parse().unwrap_or(0.0);
        let total: f32 = caps[2].parse().unwrap_or(1.0);
        let percent = if total > 0.0 { (current / total) * 100.0 } else { 0.0 };
        let (item_index, item_total) = extract_item_counts(line);
        return Some(DownloadProgress {
            percent,
            speed: String::new(),
            eta: String::new(),
            status: "downloading".into(),
            item_index,
            item_total,
        });
    }

    if let Some(caps) = _regexes::ITEM_RE.captures(line) {
        let idx: u32 = caps[1].parse().unwrap_or(1);
        let total: u32 = caps[2].parse().unwrap_or(1);
        return Some(DownloadProgress {
            percent: 0.0,
            speed: String::new(),
            eta: String::new(),
            status: "downloading".into(),
            item_index: Some(idx),
            item_total: Some(total),
        });
    }

    if _regexes::EXTRACT_AUDIO_RE.is_match(line) {
        return Some(DownloadProgress {
            percent: 100.0,
            speed: String::new(),
            eta: String::new(),
            status: "extracting".into(),
            item_index: None,
            item_total: None,
        });
    }

    if line.contains("has already been downloaded") {
        return Some(DownloadProgress {
            percent: 100.0,
            speed: String::new(),
            eta: String::new(),
            status: "already_downloaded".into(),
            item_index: None,
            item_total: None,
        });
    }

    None
}

fn extract_item_counts(line: &str) -> (Option<u32>, Option<u32>) {
    if let Some(caps) = _regexes::ITEM_RE.captures(line) {
        let idx: u32 = caps[1].parse().unwrap_or(1);
        let total: u32 = caps[2].parse().unwrap_or(1);
        return (Some(idx), Some(total));
    }
    (None, None)
}

pub fn calculate_playlist_progress(current_item: u32, total_items: u32, file_progress: f32) -> f32 {
    if total_items == 0 {
        return 0.0;
    }
    let completed = (current_item.saturating_sub(1) as f32) * 100.0;
    (completed + file_progress) / total_items as f32
}

pub async fn generate_m3u(dir: &Path, playlist_title: &str) -> MhResult<PathBuf> {
    let entries = read_dir_sorted(dir, &["mp3"]).await?;

    let mut content = "#EXTM3U\n".to_string();
    for file in &entries {
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let title = strip_index_and_ext(&name);
        content.push_str(&format!("#EXTINF:-1,{}\n{}\n", title, name));
    }

    let safe_name = sanitize_filename(playlist_title);
    let out_path = dir.join(format!("{}.m3u", safe_name));
    tokio::fs::write(&out_path, content).await?;
    Ok(out_path)
}

pub async fn generate_m3u8(dir: &Path, playlist_title: &str) -> MhResult<PathBuf> {
    let entries = read_dir_sorted(dir, &["mp4", "mkv", "webm"]).await?;

    let mut content = "#EXTM3U\n".to_string();
    for file in &entries {
        let name = file.file_name().unwrap_or_default().to_string_lossy().to_string();
        let title = strip_index_and_ext(&name);
        content.push_str(&format!("#EXTINF:-1,{}\n{}\n", title, name));
    }

    let safe_name = sanitize_filename(playlist_title);
    let out_path = dir.join(format!("{}.m3u8", safe_name));
    tokio::fs::write(&out_path, content).await?;
    Ok(out_path)
}

async fn read_dir_sorted(dir: &Path, extensions: &[&str]) -> MhResult<Vec<PathBuf>> {
    let mut rd = tokio::fs::read_dir(dir).await?;
    let mut files: Vec<PathBuf> = Vec::new();

    while let Some(entry) = rd.next_entry().await? {
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if extensions.contains(&ext) {
                files.push(path);
            }
        }
    }

    files.sort_by(|a, b| {
        let na = leading_number(a);
        let nb = leading_number(b);
        na.cmp(&nb).then_with(|| a.cmp(b))
    });

    Ok(files)
}

fn leading_number(path: &Path) -> u64 {
    path.file_name()
        .and_then(|n| n.to_str())
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .unwrap_or(u64::MAX)
}

fn strip_index_and_ext(name: &str) -> &str {
    let no_ext = name.rfind('.').map(|i| &name[..i]).unwrap_or(name);
    if let Some(idx) = no_ext.find(" - ") {
        &no_ext[idx + 3..]
    } else {
        no_ext
    }
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c => c,
        })
        .collect()
}

pub async fn download_music(
    args: YtDlpMusicArgs,
    yt_dlp_cmd: &str,
    on_progress: impl Fn(DownloadProgress) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let is_playlist = args.is_playlist;

    let (effective_args, playlist_dir) = if is_playlist {
        let meta = prefetch_playlist_metadata(&args.url, yt_dlp_cmd).await?;
        let safe_title = sanitize_filename(&meta.title);
        let playlist_dir = PathBuf::from(&args.download_path).join(&safe_title);
        tokio::fs::create_dir_all(&playlist_dir).await?;

        let mut modified = args.clone();
        modified.download_path = playlist_dir.to_string_lossy().to_string();
        modified.output_template = "%(playlist_index)s - %(title)s.%(ext)s".to_string();
        modified.no_playlist = false;
        let mut cli_args = build_music_args(&modified);
        cli_args.retain(|a| a != "--no-playlist");
        cli_args.push("--yes-playlist".to_string());
        (cli_args, Some((playlist_dir, meta.title)))
    } else {
        (build_music_args(&args), None)
    };

    let (tx, mut rx) = mpsc::channel::<(LineSource, String)>(512);

    let arg_refs: Vec<&str> = effective_args.iter().map(|s| s.as_str()).collect();
    let env = vec![("PYTHONIOENCODING".to_string(), "utf-8".to_string())];
    let mut handle =
        spawn_with_output(yt_dlp_cmd, &arg_refs, Some(env), None, tx).await?;

    let stall_timeout = if is_playlist {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(30)
    };

    let mut last_activity = Instant::now();
    let mut current_item: u32 = 1;
    let mut total_items: u32 = 1;
    let mut has_started = false;
    let mut full_output = String::new();
    let mut error_log = String::new();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            handle.kill().await;
            return Err(MhError::Cancelled);
        }

        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some((source, line))) => {
                last_activity = Instant::now();

                match source {
                    LineSource::Stdout => {
                        full_output.push_str(&line);
                        full_output.push('\n');

                        if let Some(mut progress) = parse_progress_line(&line) {
                            has_started = true;

                            if let (Some(idx), Some(tot)) = (progress.item_index, progress.item_total) {
                                current_item = idx;
                                total_items = tot;
                            } else if is_playlist {
                                let file_progress = progress.percent;
                                progress.percent = calculate_playlist_progress(
                                    current_item,
                                    total_items,
                                    file_progress,
                                );
                                progress.item_index = Some(current_item);
                                progress.item_total = Some(total_items);
                            }

                            on_progress(progress);
                        }
                    }
                    LineSource::Stderr => {
                        error_log.push_str(&line);
                        error_log.push('\n');
                    }
                }
            }
            Ok(None) => {
                break;
            }
            Err(_timeout) => {
                let should_check_stall = if is_playlist {
                    true
                } else {
                    has_started
                };

                if should_check_stall && last_activity.elapsed() > stall_timeout {
                    handle.kill().await;
                    return Err(MhError::Subprocess(format!(
                        "Download stalled (no output for {}s)",
                        stall_timeout.as_secs()
                    )));
                }
            }
        }
    }

    let status = handle.child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);

    if exit_code != 0 {
        return Err(MhError::Subprocess(format!(
            "yt-dlp exited with code {}: {}",
            exit_code,
            error_log.lines().last().unwrap_or("no error output")
        )));
    }

    if let Some((dir, title)) = playlist_dir {
        let _ = generate_m3u(&dir, &title).await;
    }

    Ok(())
}

pub async fn download_video(
    args: YtDlpVideoArgs,
    yt_dlp_cmd: &str,
    on_progress: impl Fn(DownloadProgress) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let is_playlist = args.is_playlist;

    let (effective_args, playlist_dir) = if is_playlist {
        let meta = prefetch_playlist_metadata(&args.url, yt_dlp_cmd).await?;
        let safe_title = sanitize_filename(&meta.title);
        let playlist_dir = PathBuf::from(&args.download_path).join(&safe_title);
        tokio::fs::create_dir_all(&playlist_dir).await?;

        let mut modified = args.clone();
        modified.download_path = playlist_dir.to_string_lossy().to_string();
        modified.output_template = "%(playlist_index)s - %(title)s.%(ext)s".to_string();
        modified.no_playlist = false;
        modified.is_playlist = false; // prevent re-entering this branch

        let mut cli_args = build_video_args(&modified);
        cli_args.retain(|a| a != "--no-playlist");
        cli_args.push("--yes-playlist".to_string());
        (cli_args, Some((playlist_dir, meta.title)))
    } else {
        (build_video_args(&args), None)
    };

    let (tx, mut rx) = mpsc::channel::<(LineSource, String)>(512);

    let arg_refs: Vec<&str> = effective_args.iter().map(|s| s.as_str()).collect();
    let env = vec![("PYTHONIOENCODING".to_string(), "utf-8".to_string())];
    let mut handle =
        spawn_with_output(yt_dlp_cmd, &arg_refs, Some(env), None, tx).await?;

    let stall_timeout = if is_playlist {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(30)
    };

    let mut last_activity = Instant::now();
    let mut current_item: u32 = 1;
    let mut total_items: u32 = 1;
    let mut full_output = String::new();
    let mut error_log = String::new();

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            handle.kill().await;
            return Err(MhError::Cancelled);
        }

        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some((source, line))) => {
                last_activity = Instant::now();

                match source {
                    LineSource::Stdout => {
                        full_output.push_str(&line);
                        full_output.push('\n');

                        if let Some(mut progress) = parse_progress_line(&line) {
                            if let (Some(idx), Some(tot)) = (progress.item_index, progress.item_total) {
                                current_item = idx;
                                total_items = tot;
                            } else if is_playlist {
                                let file_progress = progress.percent;
                                progress.percent = calculate_playlist_progress(
                                    current_item,
                                    total_items,
                                    file_progress,
                                );
                                progress.item_index = Some(current_item);
                                progress.item_total = Some(total_items);
                            }

                            on_progress(progress);
                        }
                    }
                    LineSource::Stderr => {
                        error_log.push_str(&line);
                        error_log.push('\n');
                    }
                }
            }
            Ok(None) => break,
            Err(_) => {
                if last_activity.elapsed() > stall_timeout {
                    handle.kill().await;
                    return Err(MhError::Subprocess(format!(
                        "Download stalled (no output for {}s)",
                        stall_timeout.as_secs()
                    )));
                }
            }
        }
    }

    let status = handle.child.wait().await?;
    let exit_code = status.code().unwrap_or(-1);

    if exit_code != 0 {
        return Err(MhError::Subprocess(format!(
            "yt-dlp exited with code {}: {}",
            exit_code,
            error_log.lines().last().unwrap_or("no error output")
        )));
    }

    if let Some((dir, title)) = playlist_dir {
        let _ = generate_m3u8(&dir, &title).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_progress_line_percent() {
        let line = "[download]  50.5% of   10.00MiB at    1.23MiB/s ETA 00:05";
        let p = parse_progress_line(line).unwrap();
        assert!((p.percent - 50.5).abs() < 0.01);
    }

    #[test]
    fn parse_progress_line_item() {
        let line = "[download] Downloading item 3 of 10";
        let p = parse_progress_line(line).unwrap();
        assert_eq!(p.item_index, Some(3));
        assert_eq!(p.item_total, Some(10));
    }

    #[test]
    fn parse_progress_line_fragment() {
        let line = "Fragment 4 of 10";
        let p = parse_progress_line(line).unwrap();
        assert!((p.percent - 40.0).abs() < 0.01);
    }

    #[test]
    fn playlist_progress_calc() {
        let p = calculate_playlist_progress(2, 4, 50.0);
        assert!((p - 37.5).abs() < 0.01);
    }

    #[test]
    fn sanitize_filename_replaces_bad_chars() {
        assert_eq!(sanitize_filename("foo:bar/baz?"), "foo_bar_baz_");
    }

    #[test]
    fn build_music_args_contains_required_flags() {
        let args = YtDlpMusicArgs {
            url: "https://example.com/watch?v=test".into(),
            output_template: "%(title)s.%(ext)s".into(),
            format: "mp3".into(),
            download_path: std::env::temp_dir().to_string_lossy().to_string(),
            quality: "0".into(),
            cookies_path: None,
            cookies_from_browser: None,
            proxy: None,
            no_playlist: false,
            max_retries: 10,
            continue_download: true,
            speed_limit: None,
            use_aria2: false,
            embed_thumbnail: true,
            add_metadata: true,
            use_authentication: false,
            username: None,
            password: None,
            is_playlist: false,
        };
        let cli = build_music_args(&args);
        assert!(cli.contains(&"-x".to_string()));
        assert!(cli.contains(&"--audio-format".to_string()));
        assert!(cli.contains(&"mp3".to_string()));
        assert!(cli.contains(&"--embed-thumbnail".to_string()));
        assert!(cli.contains(&"--embed-metadata".to_string()));
    }

    #[test]
    fn build_video_args_sponsorblock() {
        let args = YtDlpVideoArgs {
            url: "https://example.com/v".into(),
            format: "bestvideo+bestaudio".into(),
            output_template: "%(title)s.%(ext)s".into(),
            download_path: std::env::temp_dir().to_string_lossy().to_string(),
            cookies_path: None,
            cookies_from_browser: None,
            proxy: None,
            no_playlist: false,
            max_retries: 5,
            continue_download: true,
            speed_limit: None,
            use_aria2: false,
            merge_output_format: None,
            add_metadata: false,
            embed_chapters: true,
            add_subtitles: false,
            no_sponsorblock: false,
            sponsorblock_mark: Some("sponsor".into()),
            sponsorblock_remove: None,
            sponsorblock_chapter_title: None,
            sponsorblock_api_url: None,
            use_authentication: false,
            username: None,
            password: None,
            is_playlist: false,
            is_generic: false,
        };
        let cli = build_video_args(&args);
        assert!(cli.contains(&"--sponsorblock-mark".to_string()));
        assert!(cli.contains(&"sponsor".to_string()));
        assert!(cli.contains(&"--embed-chapters".to_string()));
    }
}
