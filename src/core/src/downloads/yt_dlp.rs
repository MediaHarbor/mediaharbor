use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use tokio::sync::mpsc;

/// `--flag value` pairs, which yt-dlp arg building is almost entirely made of.
trait YtArgs {
    fn opt(&mut self, flag: &str, value: impl Into<String>);
    /// Skips the flag entirely when the value is blank after trimming.
    fn opt_trimmed(&mut self, flag: &str, value: &str);
    /// Skips the flag when the number is zero.
    fn num(&mut self, flag: &str, n: u32);
}

impl YtArgs for Vec<String> {
    fn opt(&mut self, flag: &str, value: impl Into<String>) {
        self.push(flag.to_string());
        self.push(value.into());
    }

    fn opt_trimmed(&mut self, flag: &str, value: &str) {
        let t = value.trim();
        if !t.is_empty() {
            self.opt(flag, t);
        }
    }

    fn num(&mut self, flag: &str, n: u32) {
        if n > 0 {
            self.opt(flag, n.to_string());
        }
    }
}

use crate::{
    defaults::Settings,
    errors::{MhError, MhResult},
    services::common::cli::driver::{drive, IdleAction, LineEffect, ProcessDriver, Running},
    subprocess::{spawn_with_output, LineSource},
    venv_manager::ResolvedTool,
};

pub const PROGRESS_PREFIX: &str = "MHPROG";

#[derive(Debug, Clone)]
pub struct YtDlpMusicArgs {
    pub url: String,
    pub output_template: String,
    pub format: String,
    pub download_path: String,
    pub quality: String,
    pub no_playlist: bool,
    pub continue_download: bool,
    pub speed_limit: Option<String>, // e.g. "5M"
    pub use_aria2: bool,
    pub aria2c_active: bool,
    pub embed_thumbnail: bool,
    pub add_metadata: bool,
    pub use_authentication: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub is_playlist: bool,
    pub settings: Settings,
}

#[derive(Debug, Clone)]
pub struct YtDlpVideoArgs {
    pub url: String,
    pub format: String,
    pub output_template: String,
    pub download_path: String,
    pub no_playlist: bool,
    pub continue_download: bool,
    pub speed_limit: Option<String>,
    pub use_aria2: bool,
    pub aria2c_active: bool,
    pub merge_output_format: Option<String>,
    pub embed_thumbnail: bool,
    pub add_metadata: bool,
    pub embed_chapters: bool,
    pub add_subtitles: bool,
    pub use_authentication: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub is_playlist: bool,
    pub is_generic: bool,
    pub settings: Settings,
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

pub fn find_yt_dlp_command() -> ResolvedTool {
    crate::venv_manager::resolve_tool("yt-dlp", "yt_dlp")
}

pub(crate) fn youtube_extractor_args(settings: &Settings) -> Vec<String> {
    youtube_extractor_args_with_default_client(settings, "")
}

pub(crate) fn youtube_extractor_args_with_default_client(
    settings: &Settings,
    default_client: &str,
) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();

    let mut parts: Vec<String> = Vec::new();
    let client = settings.player_client.trim();
    if !client.is_empty() {
        parts.push(format!("player_client={}", client));
    } else if !default_client.is_empty() {
        parts.push(format!("player_client={}", default_client));
    }
    if settings.pot_provider_enabled {
        parts.push("fetch_pot=auto".to_string());
        if settings.pot_trace {
            parts.push("pot_trace=true".to_string());
        }
        let token = settings.po_token.trim();
        if !token.is_empty() {
            parts.push(format!("po_token={}", token));
        }
    }
    if !parts.is_empty() {
        v.opt("--extractor-args", format!("youtube:{}", parts.join(";")));
    }

    if let Some(deno) = crate::venv_manager::find_managed_or_path("deno") {
        v.opt("--js-runtimes", format!("deno:{}", deno.display()));
    }

    let ejs = settings.ejs_remote_components.trim();
    if !ejs.is_empty() {
        v.opt("--remote-components", ejs.to_string());
    }

    v
}

pub(crate) fn common_network_args(settings: &Settings) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();

    let cookies = if settings.use_cookies {
        [
            settings.ytmusic_cookies_path.trim(),
            settings.ytdlp_cookies_path.trim(),
            settings.cookies.trim(),
        ]
        .into_iter()
        .find(|c| !c.is_empty())
    } else {
        Some(settings.ytmusic_cookies_path.trim()).filter(|c| !c.is_empty())
    };

    if let Some(cookies) = cookies {
        v.opt("--cookies", cookies.to_string());
    }
    v.opt_trimmed("--cookies-from-browser", &settings.cookies_from_browser);

    if settings.use_proxy && !settings.proxy_url.trim().is_empty() {
        v.opt("--proxy", settings.proxy_url.trim().to_string());
    }

    v.num("--socket-timeout", settings.socket_timeout);

    let retries = if settings.max_retries > 0 {
        settings.max_retries.to_string()
    } else {
        "10".to_string()
    };
    v.opt("--retries", retries);

    v.opt_trimmed("--extractor-retries", &settings.extractor_retries);

    v.opt("--color", "never".to_string());

    v
}

fn progress_template_args() -> Vec<String> {
    vec![
        "--newline".to_string(),
        "--progress-delta".to_string(),
        "0.5".to_string(),
        "--progress-template".to_string(),
        format!(
            "download:{}|%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s|%(progress._total_bytes_str)s|%(info.playlist_index)s|%(info.n_entries)s",
            PROGRESS_PREFIX
        ),
    ]
}

fn sponsorblock_args(settings: &Settings) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if settings.no_sponsorblock {
        return v;
    }
    v.opt_trimmed("--sponsorblock-mark", &settings.sponsorblock_mark);
    v.opt_trimmed("--sponsorblock-remove", &settings.sponsorblock_remove);
    v.opt_trimmed(
        "--sponsorblock-chapter-title",
        &settings.sponsorblock_chapter_title,
    );
    v.opt_trimmed("--sponsorblock-api", &settings.sponsorblock_api_url);
    v
}

fn aria2c_downloader_args(settings: &Settings, active: bool) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    if active {
        v.opt("--downloader", "aria2c".to_string());
        v.opt("--downloader", "dash,m3u8:native".to_string());
        if !settings.aria2c_args.trim().is_empty() {
            v.opt(
                "--downloader-args",
                format!("aria2c:{}", settings.aria2c_args.trim()),
            );
        }
    }
    v
}

fn push_download_resilience(v: &mut Vec<String>, settings: &Settings) {
    if settings.concurrent_fragments > 0 {
        v.push("-N".to_string());
        v.push(settings.concurrent_fragments.to_string());
    }
    v.opt_trimmed("--fragment-retries", &settings.fragment_retries);
    v.opt_trimmed("--file-access-retries", &settings.file_access_retries);
    v.opt_trimmed("--throttled-rate", &settings.throttled_rate);
    v.opt_trimmed("--sleep-requests", &settings.sleep_requests);
    v.opt_trimmed("--sleep-interval", &settings.sleep_interval);
    v.opt_trimmed("--max-sleep-interval", &settings.max_sleep_interval);
    v.num("--trim-filenames", settings.trim_filenames);
    if settings.restrict_filenames {
        v.push("--restrict-filenames".to_string());
    }
    if settings.windows_filenames {
        v.push("--windows-filenames".to_string());
    }
    v.opt_trimmed("--geo-bypass-country", &settings.geo_bypass_country);
    v.num("--max-downloads", settings.max_downloads);
    if settings.set_mtime {
        v.push("--mtime".to_string());
    } else {
        v.push("--no-mtime".to_string());
    }
}

fn push_overwrite_continue(v: &mut Vec<String>, settings: &Settings, continue_download: bool) {
    if settings.no_overwrites {
        v.push("--no-overwrites".to_string());
        if continue_download {
            v.push("--continue".to_string());
        } else {
            v.push("--no-continue".to_string());
        }
    } else {
        v.push("--force-overwrites".to_string());
    }
}

fn normalize_audio_quality(quality: &str) -> String {
    let q = quality.trim();
    if q.is_empty() {
        return "0".to_string();
    }
    if q.ends_with('K') || q.ends_with('k') {
        return q.to_string();
    }
    if let Ok(n) = q.parse::<u32>() {
        if n <= 10 {
            return n.to_string();
        }
        return format!("{}K", n);
    }
    q.to_string()
}

fn split_args(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut has_token = false;

    for c in input.chars() {
        match c {
            '\'' if !in_double => {
                in_single = !in_single;
                has_token = true;
            }
            '"' if !in_single => {
                in_double = !in_double;
                has_token = true;
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

fn push_thumbnail_args(v: &mut Vec<String>, embed_thumbnail: bool, settings: &Settings) {
    if embed_thumbnail {
        if settings.convert_thumbnails_jpg {
            v.opt("--convert-thumbnails", "jpg".to_string());
        }
        v.push("--embed-thumbnail".to_string());
    }
}

pub fn build_music_args(args: &YtDlpMusicArgs) -> Vec<String> {
    let s = &args.settings;
    let mut v: Vec<String> = vec!["-x".to_string()];

    if s.ytm_override_download_extension {
        v.opt("--audio-format", args.format.clone());
        v.opt("--audio-quality", normalize_audio_quality(&args.quality));
    }

    v.push("-P".to_string());
    v.push(format!("home:{}", args.download_path));
    v.opt("--output", args.output_template.clone());

    v.extend(youtube_extractor_args(s));
    v.extend(common_network_args(s));
    v.extend(progress_template_args());

    if args.no_playlist {
        v.push("--no-playlist".to_string());
    }

    push_download_resilience(&mut v, s);
    push_overwrite_continue(&mut v, s, args.continue_download);

    if let Some(ref limit) = args.speed_limit {
        v.push("-r".to_string());
        v.push(limit.clone());
    }

    v.extend(aria2c_downloader_args(s, args.aria2c_active));

    if args.use_authentication {
        if let (Some(user), Some(pass)) = (&args.username, &args.password) {
            v.opt("--username", user.clone());
            v.opt("--password", pass.clone());
        }
    }

    v.extend(sponsorblock_args(s));

    push_thumbnail_args(&mut v, args.embed_thumbnail, s);
    if args.add_metadata {
        v.push("--embed-metadata".to_string());
    }

    v.opt_trimmed("--postprocessor-args", &s.postprocessor_args);

    v.extend(split_args(&s.extra_args));

    v.push(args.url.clone());
    v
}

pub fn build_video_args(args: &YtDlpVideoArgs) -> Vec<String> {
    let s = &args.settings;
    let mut v: Vec<String> = Vec::new();

    v.push("-f".to_string());
    v.push(args.format.clone());

    if !args.is_generic && s.youtube_quality > 0 {
        v.push("-S".to_string());
        v.push(format!("res:{},vcodec:h264,acodec:aac", s.youtube_quality));
    }

    v.push("-P".to_string());
    v.push(format!("home:{}", args.download_path));
    v.opt("--output", args.output_template.clone());

    if !args.is_generic {
        v.extend(youtube_extractor_args(s));
    }
    v.extend(common_network_args(s));
    v.extend(progress_template_args());

    if !args.is_generic {
        if let Some(ref fmt) = args.merge_output_format {
            v.opt("--merge-output-format", fmt.clone());
        }
        v.opt_trimmed("--remux-video", &s.remux_format);
    }

    if args.no_playlist || args.is_generic {
        v.push("--no-playlist".to_string());
    }

    push_download_resilience(&mut v, s);
    push_overwrite_continue(&mut v, s, args.continue_download);

    if let Some(ref limit) = args.speed_limit {
        v.push("-r".to_string());
        v.push(limit.clone());
    }

    v.extend(aria2c_downloader_args(s, args.aria2c_active));

    if args.use_authentication {
        if let (Some(user), Some(pass)) = (&args.username, &args.password) {
            v.opt("--username", user.clone());
            v.opt("--password", pass.clone());
        }
    }

    v.extend(sponsorblock_args(s));

    push_thumbnail_args(&mut v, args.embed_thumbnail, s);
    if args.add_metadata {
        v.push("--embed-metadata".to_string());
    }
    if args.embed_chapters {
        v.push("--embed-chapters".to_string());
    }

    if args.add_subtitles {
        v.push("--embed-subs".to_string());
        let langs = if s.sub_langs.trim().is_empty() {
            "all,-live_chat".to_string()
        } else {
            s.sub_langs.trim().to_string()
        };
        v.opt("--sub-langs", langs);
        if s.write_auto_subs {
            v.push("--write-auto-subs".to_string());
        }
        if s.convert_subs_srt {
            v.opt("--convert-subs", "srt".to_string());
        }
    }

    if !args.is_generic && s.faststart_mp4 {
        v.opt(
            "--postprocessor-args",
            "Merger+ffmpeg_o:-movflags +faststart".to_string(),
        );
    }
    v.opt_trimmed("--postprocessor-args", &s.postprocessor_args);

    v.extend(split_args(&s.extra_args));

    v.push(args.url.clone());
    v
}

/// Whether a URL addresses a playlist as a whole rather than one item inside one.
///
/// `list=` alone is not enough: `watch?v=X&list=Y` is a single track that happens to
/// carry its container, and yt-dlp's default for that URL is to take the whole list.
pub fn url_is_playlist(url: &str) -> bool {
    let (path, query) = match url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (url, ""),
    };

    if path.contains("/playlist") {
        return true;
    }

    let params = || query.split('&').filter_map(|p| p.split_once('='));
    let has = |key: &str| params().any(|(k, v)| k == key && !v.is_empty());

    has("list") && !has("v")
}

pub fn music_args_from_settings(url: &str, quality: &str, settings: &Settings) -> YtDlpMusicArgs {
    let is_playlist = url_is_playlist(url);
    let speed_limit = if settings.download_speed_limit && settings.speed_limit_value > 0 {
        Some(format!(
            "{}{}",
            settings.speed_limit_value, settings.speed_limit_type
        ))
    } else {
        None
    };

    YtDlpMusicArgs {
        url: url.to_string(),
        output_template: settings.download_output_template.clone(),
        format: settings.youtube_audio_extensions.clone(),
        download_path: settings.download_location.clone(),
        quality: quality.to_string(),
        no_playlist: !is_playlist,
        continue_download: settings.continue_download,
        speed_limit,
        use_aria2: settings.use_aria2,
        aria2c_active: settings.use_aria2
            && crate::venv_manager::find_managed_or_path("aria2c").is_some(),
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
        settings: settings.clone(),
    }
}

pub fn video_args_from_settings(
    url: &str,
    quality: &str,
    settings: &Settings,
    is_generic: bool,
) -> YtDlpVideoArgs {
    let is_playlist = !is_generic && url_is_playlist(url);
    let speed_limit = if settings.download_speed_limit && settings.speed_limit_value > 0 {
        Some(format!(
            "{}{}",
            settings.speed_limit_value, settings.speed_limit_type
        ))
    } else {
        None
    };

    let merge_output_format = if !is_generic
        && settings.yt_override_download_extension
        && !settings.youtube_video_extensions.is_empty()
    {
        Some(settings.youtube_video_extensions.clone())
    } else {
        None
    };

    YtDlpVideoArgs {
        url: url.to_string(),
        format: quality.to_string(),
        output_template: settings.download_output_template.clone(),
        download_path: settings.download_location.clone(),
        no_playlist: !is_playlist,
        continue_download: settings.continue_download,
        speed_limit,
        use_aria2: settings.use_aria2,
        aria2c_active: settings.use_aria2
            && crate::venv_manager::find_managed_or_path("aria2c").is_some(),
        merge_output_format,
        embed_thumbnail: settings.add_metadata,
        add_metadata: settings.add_metadata,
        embed_chapters: settings.embed_chapters,
        add_subtitles: settings.add_subtitle_to_file,
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
        settings: settings.clone(),
    }
}

pub async fn prefetch_metadata(url: &str, yt_dlp: &ResolvedTool) -> MhResult<MediaMetadata> {
    let args = yt_dlp.argv(&[
        "--print",
        "%(title)s",
        "--print",
        "%(uploader)s",
        "--print",
        "%(duration_string)s",
        "--print",
        "%(thumbnail)s",
        "--no-download",
        url,
    ]);

    let mut cmd = tokio::process::Command::new(&yt_dlp.program);
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .envs(crate::venv_manager::python_env());
    crate::subprocess::apply_no_window(&mut cmd);

    let output = cmd
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("Failed to run yt-dlp prefetch: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut meta = MediaMetadata::default();
    if let Some(title) = lines.first() {
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

pub async fn prefetch_playlist_metadata(
    url: &str,
    yt_dlp: &ResolvedTool,
) -> MhResult<MediaMetadata> {
    let args = yt_dlp.argv(&[
        "--flat-playlist",
        "--print",
        "%(playlist)s",
        "--print",
        "%(playlist_uploader)s",
        "--print",
        "%(playlist_thumbnail)s",
        "--print",
        "%(playlist_count)s",
        "--no-download",
        url,
    ]);

    let mut cmd = tokio::process::Command::new(&yt_dlp.program);
    cmd.args(&args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .envs(crate::venv_manager::python_env());
    crate::subprocess::apply_no_window(&mut cmd);

    let output = cmd.output().await.map_err(|e| {
        MhError::Subprocess(format!("Failed to run yt-dlp playlist prefetch: {}", e))
    })?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    let mut meta = MediaMetadata {
        is_playlist: true,
        ..Default::default()
    };

    if let Some(title) = lines.first() {
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

fn clean_field(field: Option<&&str>) -> String {
    match field {
        Some(s) => {
            let t = s.trim();
            if t.is_empty() || t == "NA" {
                String::new()
            } else {
                t.to_string()
            }
        }
        None => String::new(),
    }
}

fn parse_percent_field(field: Option<&&str>) -> f32 {
    match field {
        Some(s) => s
            .trim()
            .trim_end_matches('%')
            .trim()
            .parse::<f32>()
            .unwrap_or(0.0),
        None => 0.0,
    }
}

fn parse_u32_field(field: Option<&&str>) -> Option<u32> {
    field.and_then(|s| {
        let t = s.trim();
        if t.is_empty() || t == "NA" {
            None
        } else {
            t.parse::<u32>().ok()
        }
    })
}

const POSTPROCESS_MARKERS: &[&str] = &[
    "[ExtractAudio]",
    "[Merger]",
    "[VideoConvertor]",
    "[VideoRemuxer]",
    "[Metadata]",
    "[EmbedThumbnail]",
    "[EmbedSubtitle]",
    "[SponsorBlock]",
    "[ModifyChapters]",
    "[ThumbnailsConvertor]",
    "[SubtitlesConvertor]",
    "[Fixup",
    "Deleting original file",
];

pub fn is_postprocessing_line(line: &str) -> bool {
    POSTPROCESS_MARKERS.iter().any(|m| line.contains(m))
}

pub fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    let trimmed = line.trim();

    if let Some(rest) = trimmed
        .strip_prefix(PROGRESS_PREFIX)
        .and_then(|r| r.strip_prefix('|'))
    {
        let fields: Vec<&str> = rest.split('|').collect();
        let percent = parse_percent_field(fields.first());
        let speed = clean_field(fields.get(1));
        let eta = clean_field(fields.get(2));
        let item_index = parse_u32_field(fields.get(4));
        let item_total = parse_u32_field(fields.get(5));

        return Some(DownloadProgress {
            percent,
            speed,
            eta,
            status: "downloading".to_string(),
            item_index,
            item_total,
        });
    }

    if trimmed.contains("has already been downloaded") {
        return Some(DownloadProgress {
            percent: 100.0,
            speed: String::new(),
            eta: String::new(),
            status: "already_downloaded".to_string(),
            item_index: None,
            item_total: None,
        });
    }

    if is_postprocessing_line(trimmed) {
        return Some(DownloadProgress {
            percent: 100.0,
            speed: String::new(),
            eta: String::new(),
            status: "processing".to_string(),
            item_index: None,
            item_total: None,
        });
    }

    None
}

pub async fn generate_m3u(dir: &Path, playlist_title: &str) -> MhResult<PathBuf> {
    let entries = read_dir_sorted(dir, &["mp3"]).await?;

    let mut content = "#EXTM3U\n".to_string();
    for file in &entries {
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
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
        let name = file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
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

const LOGIN_REQUIRED_MARKERS: &[&str] = &[
    "Sign in to confirm",
    "login_required",
    "This video is only available to",
    "Use --cookies",
    "Please sign in",
    "account associated with this",
    "members-only",
];

pub fn is_login_required_error(text: &str) -> bool {
    LOGIN_REQUIRED_MARKERS.iter().any(|m| text.contains(m))
}

fn stall_ceiling(base: Duration, aria2c_active: bool, postprocessing: bool) -> Duration {
    if aria2c_active {
        base * 10
    } else if postprocessing {
        base * 5
    } else {
        base
    }
}

/// Warn once via `on_log` when the user requested aria2c but no binary is available
/// (yt-dlp falls back to its built-in downloader).
fn warn_aria2c_fallback(use_aria2: bool, active: bool, on_log: &impl Fn(String)) {
    if use_aria2 && !active {
        on_log("aria2c is not installed; falling back to the built-in downloader.".to_string());
    }
}

/// The sidecar written next to a finished playlist, per download kind.
#[derive(Clone, Copy)]
enum PlaylistSidecar {
    M3u,
    M3u8,
}

impl PlaylistSidecar {
    async fn write(self, dir: &Path, title: &str) -> MhResult<PathBuf> {
        match self {
            Self::M3u => generate_m3u(dir, title).await,
            Self::M3u8 => generate_m3u8(dir, title).await,
        }
    }
}

/// The fields every yt-dlp run has, whatever it is downloading.
struct JobCommon<'a> {
    url: &'a str,
    download_path: &'a str,
    is_playlist: bool,
    use_aria2: bool,
    aria2c_active: bool,
}

/// The per-kind half of a yt-dlp run. Music and video differ only in how their argv is
/// built and which playlist file is written; everything else about driving yt-dlp is
/// the same, so it lives once in [`run_yt_dlp`].
trait YtDlpJob: Clone + Send {
    fn common(&self) -> JobCommon<'_>;
    fn sidecar() -> PlaylistSidecar;
    /// Point this run at `dir` and number its files by playlist position.
    fn retarget_to(&mut self, dir: &Path);
    fn build(&self) -> Vec<String>;
}

/// Playlist output template shared by both kinds: index first so the folder sorts in
/// playlist order rather than alphabetically.
const PLAYLIST_TEMPLATE: &str = "%(playlist_index)s - %(title)s.%(ext)s";

impl YtDlpJob for YtDlpMusicArgs {
    fn common(&self) -> JobCommon<'_> {
        JobCommon {
            url: &self.url,
            download_path: &self.download_path,
            is_playlist: self.is_playlist,
            use_aria2: self.use_aria2,
            aria2c_active: self.aria2c_active,
        }
    }
    fn sidecar() -> PlaylistSidecar {
        PlaylistSidecar::M3u
    }
    fn retarget_to(&mut self, dir: &Path) {
        self.download_path = dir.to_string_lossy().to_string();
        self.output_template = PLAYLIST_TEMPLATE.to_string();
        self.no_playlist = false;
        self.is_playlist = false;
    }
    fn build(&self) -> Vec<String> {
        build_music_args(self)
    }
}

impl YtDlpJob for YtDlpVideoArgs {
    fn common(&self) -> JobCommon<'_> {
        JobCommon {
            url: &self.url,
            download_path: &self.download_path,
            is_playlist: self.is_playlist,
            use_aria2: self.use_aria2,
            aria2c_active: self.aria2c_active,
        }
    }
    fn sidecar() -> PlaylistSidecar {
        PlaylistSidecar::M3u8
    }
    fn retarget_to(&mut self, dir: &Path) {
        self.download_path = dir.to_string_lossy().to_string();
        self.output_template = PLAYLIST_TEMPLATE.to_string();
        self.no_playlist = false;
        self.is_playlist = false;
    }
    fn build(&self) -> Vec<String> {
        build_video_args(self)
    }
}

/// What yt-dlp's output means. The loop that feeds it lives in
/// `services/common/cli/driver.rs` and is shared with every other spawned downloader.
struct YtDlpDriver {
    base_timeout: Duration,
    is_playlist: bool,
    aria2c_active: bool,
    has_started: bool,
    postprocessing: bool,
    login_required: bool,
    /// Only the last stderr line is ever reported, so only it is kept.
    last_error: Option<String>,
}

impl YtDlpDriver {
    fn new(is_playlist: bool, aria2c_active: bool) -> Self {
        Self {
            // A playlist spends longer between items than a single track does.
            base_timeout: if is_playlist {
                Duration::from_secs(60)
            } else {
                Duration::from_secs(30)
            },
            is_playlist,
            aria2c_active,
            has_started: false,
            postprocessing: false,
            login_required: false,
            last_error: None,
        }
    }
}

impl ProcessDriver for YtDlpDriver {
    type Progress = DownloadProgress;

    fn feed(&mut self, source: LineSource, line: &str) -> LineEffect<DownloadProgress> {
        if is_postprocessing_line(line) {
            self.postprocessing = true;
        } else if line.trim().starts_with(PROGRESS_PREFIX) {
            self.postprocessing = false;
        }

        match source {
            LineSource::Stdout => {
                if let Some(progress) = parse_progress_line(line) {
                    self.has_started = true;
                    // yt-dlp already rate-limits its own progress template, so every
                    // line it does print is worth forwarding.
                    return LineEffect::important(progress);
                }
            }
            LineSource::Stderr => {
                if is_login_required_error(line) {
                    self.login_required = true;
                }
                self.last_error = Some(line.to_string());
            }
        }
        LineEffect::None
    }

    fn on_idle(&mut self, idle: Duration) -> IdleAction {
        // Before the first byte of a single download there is nothing to stall on —
        // yt-dlp may still be resolving formats.
        if !(self.is_playlist || self.has_started) {
            return IdleAction::Wait;
        }
        let ceiling = stall_ceiling(self.base_timeout, self.aria2c_active, self.postprocessing);
        if idle > ceiling {
            IdleAction::Stall(format!(
                "Download stalled (no output for {}s)",
                ceiling.as_secs()
            ))
        } else {
            IdleAction::Wait
        }
    }

    fn finish(&mut self, exit_code: i32) -> MhResult<Option<DownloadProgress>> {
        if exit_code == 0 {
            return Ok(None);
        }
        if self.login_required {
            return Err(MhError::Auth(
                "YouTube sign-in required — your cookies may be expired. Re-import them in Settings.".into(),
            ));
        }
        Err(MhError::Subprocess(format!(
            "yt-dlp exited with code {}: {}",
            exit_code,
            self.last_error.as_deref().unwrap_or("no error output")
        )))
    }
}

async fn run_yt_dlp<J: YtDlpJob>(
    args: J,
    yt_dlp: &ResolvedTool,
    on_progress: impl Fn(DownloadProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    let JobCommon {
        url,
        download_path,
        is_playlist,
        use_aria2,
        aria2c_active,
    } = args.common();
    let (url, download_path) = (url.to_string(), download_path.to_string());
    warn_aria2c_fallback(use_aria2, aria2c_active, &on_log);

    let (effective_args, playlist_dir) = if is_playlist {
        let meta = prefetch_playlist_metadata(&url, yt_dlp).await?;
        let dir = PathBuf::from(&download_path).join(sanitize_filename(&meta.title));
        tokio::fs::create_dir_all(&dir).await?;

        let mut modified = args.clone();
        modified.retarget_to(&dir);
        let mut cli_args = modified.build();
        cli_args.retain(|a| a != "--no-playlist");
        cli_args.push("--yes-playlist".to_string());
        (cli_args, Some((dir, meta.title)))
    } else {
        (args.build(), None)
    };

    let (tx, mut rx) = mpsc::channel::<(LineSource, String)>(512);
    let extra: Vec<&str> = effective_args.iter().map(|s| s.as_str()).collect();
    let arg_refs = yt_dlp.argv(&extra);
    let mut handle = spawn_with_output(&yt_dlp.program, &arg_refs, None, None, tx).await?;

    drive(
        &mut YtDlpDriver::new(is_playlist, aria2c_active),
        Running::new(&mut handle, &mut rx, Duration::from_millis(500)),
        &cancel_flag,
        on_progress,
        on_log,
    )
    .await?;

    if let Some((dir, title)) = playlist_dir {
        let _ = J::sidecar().write(&dir, &title).await;
    }

    Ok(())
}

pub async fn download_music(
    args: YtDlpMusicArgs,
    yt_dlp: &ResolvedTool,
    on_progress: impl Fn(DownloadProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    run_yt_dlp(args, yt_dlp, on_progress, on_log, cancel_flag).await
}

pub async fn download_video(
    args: YtDlpVideoArgs,
    yt_dlp: &ResolvedTool,
    on_progress: impl Fn(DownloadProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + 'static,
    cancel_flag: Arc<AtomicBool>,
) -> MhResult<()> {
    run_yt_dlp(args, yt_dlp, on_progress, on_log, cancel_flag).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn music_fixture() -> YtDlpMusicArgs {
        YtDlpMusicArgs {
            url: "https://example.com/watch?v=test".into(),
            output_template: "%(title)s.%(ext)s".into(),
            format: "mp3".into(),
            download_path: std::env::temp_dir().to_string_lossy().to_string(),
            quality: "320".into(),
            no_playlist: false,
            continue_download: true,
            speed_limit: None,
            use_aria2: false,
            aria2c_active: false,
            embed_thumbnail: true,
            add_metadata: true,
            use_authentication: false,
            username: None,
            password: None,
            is_playlist: false,
            settings: Settings::default(),
        }
    }

    fn video_fixture() -> YtDlpVideoArgs {
        let settings = Settings {
            sponsorblock_mark: "sponsor".into(),
            ..Settings::default()
        };
        YtDlpVideoArgs {
            url: "https://example.com/v".into(),
            format: "bestvideo+bestaudio".into(),
            output_template: "%(title)s.%(ext)s".into(),
            download_path: std::env::temp_dir().to_string_lossy().to_string(),
            no_playlist: false,
            continue_download: true,
            speed_limit: None,
            use_aria2: false,
            aria2c_active: false,
            merge_output_format: None,
            embed_thumbnail: false,
            add_metadata: false,
            embed_chapters: true,
            add_subtitles: false,
            use_authentication: false,
            username: None,
            password: None,
            is_playlist: false,
            is_generic: false,
            settings,
        }
    }

    fn pair(cli: &[String], flag: &str) -> Option<String> {
        cli.iter()
            .position(|a| a.as_str() == flag)
            .and_then(|i| cli.get(i + 1).cloned())
    }

    #[test]
    fn playlist_detection_from_url() {
        assert!(url_is_playlist(
            "https://www.youtube.com/playlist?list=PL123"
        ));
        assert!(url_is_playlist(
            "https://music.youtube.com/playlist?list=OLAK5uy_abc&si=x"
        ));
        assert!(!url_is_playlist(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123"
        ));
        assert!(!url_is_playlist(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        ));
        assert!(!url_is_playlist("https://youtu.be/dQw4w9WgXcQ"));
        assert!(!url_is_playlist(
            "https://www.youtube.com/watch?v=abc&list="
        ));
    }

    #[test]
    fn single_track_url_is_pinned_to_one_item() {
        let s = Settings {
            download_location: std::env::temp_dir().to_string_lossy().to_string(),
            ..Settings::default()
        };

        let track = music_args_from_settings(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123",
            "320",
            &s,
        );
        assert!(!track.is_playlist);
        assert!(build_music_args(&track).contains(&"--no-playlist".to_string()));

        let list =
            music_args_from_settings("https://www.youtube.com/playlist?list=PL123", "320", &s);
        assert!(list.is_playlist);
        assert!(!build_music_args(&list).contains(&"--no-playlist".to_string()));

        let video = video_args_from_settings(
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&list=PL123",
            "best",
            &s,
            false,
        );
        assert!(!video.is_playlist);
        assert!(build_video_args(&video).contains(&"--no-playlist".to_string()));
    }

    #[test]
    fn parse_progress_template_line() {
        let line = "MHPROG| 50.5%|1.23MiB/s|00:05|10.00MiB|3|10";
        let p = parse_progress_line(line).unwrap();
        assert!((p.percent - 50.5).abs() < 0.01);
        assert_eq!(p.speed, "1.23MiB/s");
        assert_eq!(p.eta, "00:05");
        assert_eq!(p.item_index, Some(3));
        assert_eq!(p.item_total, Some(10));
        assert_eq!(p.status, "downloading");
    }

    #[test]
    fn parse_progress_template_na_fields() {
        let line = "MHPROG|100%|NA|NA|10.00MiB|NA|NA";
        let p = parse_progress_line(line).unwrap();
        assert!((p.percent - 100.0).abs() < 0.01);
        assert_eq!(p.speed, "");
        assert_eq!(p.item_index, None);
        assert_eq!(p.item_total, None);
    }

    #[test]
    fn parse_progress_postprocessing() {
        let p = parse_progress_line("[ExtractAudio] Destination: foo.mp3").unwrap();
        assert_eq!(p.status, "processing");
    }

    #[test]
    fn sanitize_filename_replaces_bad_chars() {
        assert_eq!(sanitize_filename("foo:bar/baz?"), "foo_bar_baz_");
    }

    #[test]
    fn music_audio_format_gated_on_override() {
        let mut args = music_fixture();
        args.settings.ytm_override_download_extension = false;
        let cli = build_music_args(&args);
        assert!(!cli.contains(&"--audio-format".to_string()));

        args.settings.ytm_override_download_extension = true;
        let cli = build_music_args(&args);
        assert!(cli.contains(&"--audio-format".to_string()));
        assert_eq!(pair(&cli, "--audio-quality"), Some("320K".to_string()));
    }

    #[test]
    fn music_uses_relative_output_and_typed_paths() {
        let args = music_fixture();
        let cli = build_music_args(&args);
        assert!(cli.iter().any(|a| a.starts_with("home:")));
        assert_eq!(
            pair(&cli, "--output"),
            Some("%(title)s.%(ext)s".to_string())
        );
    }

    #[test]
    fn music_emits_player_client_only_when_set_and_progress_template() {
        let mut args = music_fixture();
        let cli = build_music_args(&args);
        assert!(
            !cli.iter().any(|a| a.contains("player_client=")),
            "default (empty) player_client must not force a client list"
        );
        assert!(cli.contains(&"--progress-template".to_string()));
        assert!(cli.iter().any(|a| a.starts_with("download:MHPROG|")));
        assert!(cli.contains(&"--color".to_string()));
        assert!(cli.contains(&"never".to_string()));
        assert_eq!(
            pair(&cli, "--remote-components"),
            Some("ejs:github".to_string())
        );

        args.settings.player_client = "default".into();
        let cli = build_music_args(&args);
        let ea = pair(&cli, "--extractor-args").unwrap();
        assert!(ea.contains("player_client=default"));
    }

    #[test]
    fn music_thumbnail_conversion_and_metadata() {
        let args = music_fixture();
        let cli = build_music_args(&args);
        assert!(cli.contains(&"-x".to_string()));
        assert!(cli.contains(&"--convert-thumbnails".to_string()));
        assert!(cli.contains(&"--embed-thumbnail".to_string()));
        assert!(cli.contains(&"--embed-metadata".to_string()));
    }

    #[test]
    fn video_no_write_thumbnail_and_embed_metadata() {
        let mut args = video_fixture();
        args.add_metadata = true;
        args.embed_thumbnail = true;
        let cli = build_video_args(&args);
        assert!(!cli.contains(&"--write-thumbnail".to_string()));
        assert!(!cli.contains(&"--add-metadata".to_string()));
        assert!(cli.contains(&"--embed-metadata".to_string()));
        assert!(cli.contains(&"--embed-thumbnail".to_string()));
    }

    #[test]
    fn video_sponsorblock_and_chapters() {
        let cli = build_video_args(&video_fixture());
        assert!(cli.contains(&"--sponsorblock-mark".to_string()));
        assert!(cli.contains(&"sponsor".to_string()));
        assert!(cli.contains(&"--embed-chapters".to_string()));
    }

    #[test]
    fn video_format_sort_resolution_cap() {
        let mut args = video_fixture();
        args.settings.youtube_quality = 1080;
        let cli = build_video_args(&args);
        let sort = pair(&cli, "-S").unwrap();
        assert!(sort.contains("res:1080"));
    }

    #[test]
    fn video_remux_and_faststart() {
        let args = video_fixture();
        let cli = build_video_args(&args);
        assert_eq!(pair(&cli, "--remux-video"), Some("mp4".to_string()));
        assert!(cli.iter().any(|a| a.contains("faststart")));
    }

    #[test]
    fn max_downloads_emitted_when_positive() {
        let mut args = music_fixture();
        args.settings.max_downloads = 5;
        let cli = build_music_args(&args);
        assert_eq!(pair(&cli, "--max-downloads"), Some("5".to_string()));
    }

    #[test]
    fn cookies_from_browser_decoupled_from_use_cookies() {
        let mut args = music_fixture();
        args.settings.use_cookies = false;
        args.settings.cookies_from_browser = "firefox".into();
        let cli = build_music_args(&args);
        assert_eq!(
            pair(&cli, "--cookies-from-browser"),
            Some("firefox".to_string())
        );
    }

    #[test]
    fn split_args_handles_quotes() {
        let out = split_args("--foo bar \"baz qux\" 'a b'");
        assert_eq!(out, vec!["--foo", "bar", "baz qux", "a b"]);
    }

    #[test]
    fn normalize_audio_quality_variants() {
        assert_eq!(normalize_audio_quality("320"), "320K");
        assert_eq!(normalize_audio_quality("320K"), "320K");
        assert_eq!(normalize_audio_quality("5"), "5");
        assert_eq!(normalize_audio_quality(""), "0");
    }
}
