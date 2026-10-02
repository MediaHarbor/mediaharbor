use std::time::{Duration, Instant};

use bytes::Bytes;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use reqwest::Client;

use crate::defaults::Settings;
use crate::downloads::yt_dlp::{common_network_args, youtube_extractor_args_with_default_client};
use crate::errors::{MhError, MhResult};
use crate::services::common::ids::now_secs;
use crate::venv_manager::ResolvedTool;

/// Fallback lifetime for a URL that carries no expiry of its own.
const CACHE_TTL: Duration = Duration::from_secs(5 * 60 * 60);

/// Retire a URL this far before its stated expiry, so a track that starts near
/// the edge of the window still has a valid URL when it finishes.
const EXPIRY_MARGIN: Duration = Duration::from_secs(10 * 60);

const YTDLP_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamInfo {
    pub url: String,
    pub mime_type: String,
    pub quality_label: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
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

/// Resolved stream URLs, held so replaying a track does not cost another
/// yt-dlp run. Entries carry the moment they stop being usable rather than the
/// moment they were stored, because a CDN URL states its own expiry.
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
    let (info, good_until) = entry.value();
    if Instant::now() < *good_until {
        Some(info.clone())
    } else {
        drop(entry);
        cache.inner.remove(key);
        None
    }
}

fn cache_set(cache: &YtAudioStreamCache, key: &str, info: StreamInfo) {
    let good_until = good_until(&info.url);
    cache.inner.insert(key.to_string(), (info, good_until));
}

/// Forget a cached URL the CDN has stopped accepting.
///
/// Nothing else can clear one: without this a URL that dies early — the address
/// it was issued for changed, the session behind it was dropped — keeps being
/// handed out until its timer runs down, and every play of that track fails.
pub fn invalidate(cache: &YtAudioStreamCache, key: &str) {
    cache.inner.remove(key);
}

/// When a resolved URL stops being valid, taken from the URL itself.
///
/// googlevideo stamps every URL with a unix `expire`; a fixed local TTL either
/// outlives it (handing out dead URLs) or falls short of it (re-running yt-dlp
/// for nothing).
fn good_until(url: &str) -> Instant {
    let now = Instant::now();

    let expire = url::Url::parse(url).ok().and_then(|u| {
        u.query_pairs()
            .find(|(k, _)| k == "expire")
            .and_then(|(_, v)| v.parse::<u64>().ok())
    });
    let Some(expire) = expire else {
        return now + CACHE_TTL;
    };

    let unix_now = now_secs();

    match expire.checked_sub(unix_now) {
        Some(left) => now + Duration::from_secs(left).saturating_sub(EXPIRY_MARGIN),
        None => now,
    }
}

/// Whether a rejection means the URL is spent rather than the request wrong.
///
/// The CDN does not distinguish these in the body, so anything that reads as
/// "this URL is no longer yours to fetch" earns one re-resolve.
fn url_is_spent(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 401 | 403 | 404 | 410)
}

fn ytdlp_candidates() -> Vec<ResolvedTool> {
    crate::venv_manager::resolve_tool_candidates("yt-dlp", "yt_dlp")
}

async fn run_ytdlp_any_candidate(extra_args: &[&str], url: &str) -> MhResult<Vec<String>> {
    let candidates = ytdlp_candidates();
    let mut last_err = MhError::Subprocess(
        "yt-dlp is not installed or not found. \
         Install it from the MediaHarbor settings page (Dependencies → yt-dlp)."
            .to_string(),
    );
    for tool in &candidates {
        match run_ytdlp(tool, extra_args, url).await {
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

pub fn find_yt_dlp_command() -> Option<ResolvedTool> {
    ytdlp_candidates().into_iter().next()
}

async fn run_with_args(
    tool: Option<&ResolvedTool>,
    extra_args: &[String],
    url: &str,
) -> MhResult<Vec<String>> {
    let refs: Vec<&str> = extra_args.iter().map(String::as_str).collect();
    if let Some(t) = tool {
        run_ytdlp(t, &refs, url).await
    } else {
        run_ytdlp_any_candidate(&refs, url).await
    }
}

async fn run_ytdlp(tool: &ResolvedTool, extra_args: &[&str], url: &str) -> MhResult<Vec<String>> {
    let mut args: Vec<&str> = tool.prefix_args.iter().map(String::as_str).collect();
    args.extend_from_slice(extra_args);
    args.push(url);

    let mut ytdlp_cmd = Command::new(&tool.program);
    ytdlp_cmd.args(&args);
    ytdlp_cmd.envs(crate::venv_manager::python_env());
    crate::subprocess::apply_no_window(&mut ytdlp_cmd);
    let output = tokio::time::timeout(YTDLP_TIMEOUT, ytdlp_cmd.output())
        .await
        .map_err(|_| MhError::Subprocess("yt-dlp timed out".to_string()))?
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                MhError::Subprocess(format!("yt-dlp not found: {}", tool.program))
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
    if crate::downloads::yt_dlp::is_login_required_error(&stderr) {
        return Err(MhError::Auth(
            "YouTube sign-in required — your cookies may be expired. Re-import them in Settings."
                .into(),
        ));
    }
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

/// Playback client.
///
/// `android_music` returns audio-only formats without authentication (134k
/// Opus, 130k AAC) and 271k with cookies. The web clients gate theirs behind a
/// GVS PO Token and otherwise fall back to itag 18 — a 360p video whose audio
/// is ~76k and sometimes 22 kHz HE-AAC. Listing another client alongside it
/// makes yt-dlp drop `android_music` entirely, so it stands alone.
const PLAYBACK_PLAYER_CLIENT: &str = "android_music";

/// Deliberately empty: yt-dlp rejects `android_music` for video formats, so the
/// client choice is left to it unless the user has named one.
const VIDEO_PLAYBACK_PLAYER_CLIENT: &str = "";

/// Audio-only formats, Opus first.
///
/// Opus is always plain Opus, while YouTube's m4a can be either AAC-LC or
/// HE-AAC depending on the upload — and HE-AAC has no pure-Rust decoder, so a
/// track encoded that way fails outright. `bestaudio` (no `ext` filter) is the
/// last audio-only rung; the final `best` only matches muxed video, which is
/// where the HE-AAC audio tracks live, so it is a genuine last resort.
const AUDIO_FORMAT_PREFERENCE: &str =
    "bestaudio[acodec=opus]/bestaudio[ext=webm]/bestaudio[ext=m4a]/bestaudio/best";

fn audio_stream_args(settings: &Settings) -> Vec<String> {
    let mut v: Vec<String> = vec![
        "-f".into(),
        AUDIO_FORMAT_PREFERENCE.into(),
        "--no-playlist".into(),
        "--print".into(),
        "%(http_headers)j".into(),
        "--get-url".into(),
    ];
    v.extend(youtube_extractor_args_with_default_client(
        settings,
        PLAYBACK_PLAYER_CLIENT,
    ));
    v.extend(common_network_args(settings));
    v
}

fn video_stream_args(settings: &Settings) -> Vec<String> {
    let mut v: Vec<String> = vec![
        "--print".into(),
        "is_live".into(),
        "--print".into(),
        "duration".into(),
        "-f".into(),
        "bestvideo[ext=mp4]+bestaudio[ext=m4a]/best[ext=mp4]/best".into(),
        "--no-playlist".into(),
        "--get-url".into(),
    ];
    v.extend(youtube_extractor_args_with_default_client(
        settings,
        VIDEO_PLAYBACK_PLAYER_CLIENT,
    ));
    v.extend(common_network_args(settings));
    v
}

fn parse_http_headers(json: &str) -> Vec<(String, String)> {
    match serde_json::from_str::<serde_json::Value>(json) {
        Ok(serde_json::Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|s| (k, s.to_string())))
            .collect(),
        _ => Vec::new(),
    }
}

pub async fn get_video_stream_info(
    video_url_or_id: &str,
    yt_dlp_cmd: Option<&ResolvedTool>,
) -> MhResult<VideoStreamInfo> {
    get_video_stream_info_with_settings(video_url_or_id, yt_dlp_cmd, &Settings::default()).await
}

/// Resolves live-stream status, duration, and CDN URL(s) in a single yt-dlp invocation.
pub async fn get_video_stream_info_with_settings(
    video_url_or_id: &str,
    yt_dlp_cmd: Option<&ResolvedTool>,
    settings: &Settings,
) -> MhResult<VideoStreamInfo> {
    let watch_url = to_watch_url(video_url_or_id);

    let args = video_stream_args(settings);
    let lines = run_with_args(yt_dlp_cmd, &args, &watch_url).await?;

    let url_lines: Vec<String> = lines
        .iter()
        .filter(|l| l.starts_with("http"))
        .cloned()
        .collect();
    let meta_lines: Vec<&String> = lines.iter().filter(|l| !l.starts_with("http")).collect();

    let is_live = meta_lines
        .first()
        .map(|s| s.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    let duration_sec = meta_lines.get(1).and_then(|s| s.parse::<f64>().ok());

    let video_url = url_lines
        .first()
        .cloned()
        .ok_or_else(|| MhError::Subprocess("yt-dlp returned no URL".to_string()))?;

    let audio_url = url_lines
        .get(1)
        .cloned()
        .unwrap_or_else(|| video_url.clone());

    let video_mime = guess_mime_from_url(&video_url, "video/mp4");
    let audio_mime = guess_mime_from_url(&audio_url, "audio/mp4");

    Ok(VideoStreamInfo {
        video: StreamInfo {
            url: video_url,
            mime_type: video_mime,
            quality_label: "video".to_string(),
            headers: Vec::new(),
        },
        audio: StreamInfo {
            url: audio_url,
            mime_type: audio_mime,
            quality_label: "audio".to_string(),
            headers: Vec::new(),
        },
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

pub async fn get_audio_stream_url_with_settings(
    video_url_or_id: &str,
    cache: &YtAudioStreamCache,
    yt_dlp_cmd: Option<&ResolvedTool>,
    settings: &Settings,
) -> MhResult<StreamInfo> {
    if let Some(cached) = cache_get(cache, video_url_or_id) {
        return Ok(cached);
    }

    let watch_url = to_watch_url(video_url_or_id);

    let args = audio_stream_args(settings);
    let lines = run_with_args(yt_dlp_cmd, &args, &watch_url).await?;

    let stream_url = lines
        .iter()
        .find(|l| l.starts_with("http"))
        .cloned()
        .ok_or_else(|| MhError::Subprocess("yt-dlp returned no URL".to_string()))?;

    let headers = lines
        .iter()
        .find(|l| l.starts_with('{'))
        .map(|l| parse_http_headers(l))
        .unwrap_or_default();

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
        headers,
    };

    cache_set(cache, video_url_or_id, info.clone());
    Ok(info)
}

/// Fetch a resolved stream with the headers it was issued for.
///
/// A googlevideo URL is minted for one client and the CDN checks the request
/// looks like that client: an `ANDROID_VR` URL fetched with a browser
/// User-Agent and a web `Referer` is answered with 403. yt-dlp reports the
/// headers each format expects, so those are sent verbatim when it supplied
/// them, and the web pair is only a fallback for when it did not.
async fn fetch_once(
    client: &Client,
    info: &StreamInfo,
    range_header: Option<&str>,
) -> MhResult<reqwest::Response> {
    let mut req = client.get(&info.url);

    if info.headers.is_empty() {
        req = req
            .header("User-Agent", crate::http_client::UA_MOZILLA)
            .header("Referer", "https://www.youtube.com/")
            .header("Origin", "https://www.youtube.com");
    } else {
        for (name, value) in &info.headers {
            req = req.header(name.as_str(), value.as_str());
        }
    }

    if let Some(range) = range_header {
        req = req.header("Range", range);
    }

    req.send().await.map_err(MhError::Network)
}

/// Fetch the audio for `video_url_or_id`, re-resolving once if the CDN has
/// stopped honouring the cached URL.
///
/// Replaying a track shortly after playing it is exactly when this matters: the
/// cache still holds the URL from the first play, and if that one has been
/// retired the fetch fails for a reason no retry of the same URL can fix. Only
/// asking yt-dlp for a new one recovers it.
pub async fn fetch_audio_stream(
    video_url_or_id: &str,
    cache: &YtAudioStreamCache,
    yt_dlp_cmd: Option<&ResolvedTool>,
    settings: &Settings,
    client: &Client,
) -> MhResult<(Bytes, String)> {
    let mut info =
        get_audio_stream_url_with_settings(video_url_or_id, cache, yt_dlp_cmd, settings).await?;
    let mut refreshed = false;

    loop {
        let resp = fetch_once(client, &info, Some("bytes=0-")).await?;
        let status = resp.status();

        if status.is_success() {
            let content_type = info.mime_type.clone();
            let data = resp.bytes().await.map_err(MhError::Network)?;
            return Ok((data, content_type));
        }

        let body = resp.text().await.unwrap_or_default();

        if !refreshed && url_is_spent(status) {
            refreshed = true;
            invalidate(cache, video_url_or_id);
            info = get_audio_stream_url_with_settings(video_url_or_id, cache, yt_dlp_cmd, settings)
                .await?;
            continue;
        }

        return Err(MhError::Other(format!(
            "YouTube refused the audio stream: HTTP {status} for {}{}",
            info.url,
            if body.trim().is_empty() {
                String::new()
            } else {
                format!("\n{}", body.trim())
            }
        )));
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_args_thread_player_client_and_color() {
        let args = audio_stream_args(&Settings::default());
        let joined = args.join(" ");
        assert!(
            joined.contains("player_client=android_music"),
            "playback must default to the client that serves audio-only \
             formats without authentication"
        );
        assert!(joined.contains("--color"));
        let opus = joined
            .find("bestaudio[acodec=opus]")
            .expect("opus preferred");
        let m4a = joined
            .find("bestaudio[ext=m4a]")
            .expect("m4a is a fallback");
        let muxed = joined
            .rfind("/best ")
            .or_else(|| joined.rfind("/best"))
            .unwrap();
        assert!(opus < m4a, "opus must come before m4a");
        assert!(m4a < muxed, "muxed video must be the last resort");

        assert!(joined.contains("%(http_headers)j"));
        assert!(!args.contains(&"--no-warnings".to_string()));

        let s = Settings {
            player_client: "default".into(),
            ..Default::default()
        };
        let joined = audio_stream_args(&s).join(" ");
        assert!(joined.contains("--extractor-args"));
        assert!(
            joined.contains("player_client=default"),
            "an explicit player_client setting must override the playback default"
        );
    }

    #[test]
    fn video_args_leave_the_client_to_yt_dlp_unless_one_is_configured() {
        let joined = video_stream_args(&Settings::default()).join(" ");
        assert!(
            !joined.contains("player_client="),
            "video playback must not pin a client of its own: {joined}"
        );

        let s = Settings {
            player_client: "tv".into(),
            ..Default::default()
        };
        let joined = video_stream_args(&s).join(" ");
        assert!(
            joined.contains("player_client=tv"),
            "an explicit player_client setting must still be honoured"
        );
    }

    fn info_for(url: &str, headers: &[(&str, &str)]) -> StreamInfo {
        StreamInfo {
            url: url.to_string(),
            mime_type: "audio/webm".to_string(),
            quality_label: "opus".to_string(),
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    fn unix_now() -> u64 {
        now_secs()
    }

    #[test]
    fn a_url_is_held_for_as_long_as_it_says_it_is_valid() {
        let url = format!(
            "https://rr1.googlevideo.com/videoplayback?expire={}",
            unix_now() + 3_600
        );
        let left = good_until(&url).saturating_duration_since(Instant::now());

        assert!(
            left <= Duration::from_secs(3_600) - EXPIRY_MARGIN + Duration::from_secs(2)
                && left >= Duration::from_secs(3_600) - EXPIRY_MARGIN - Duration::from_secs(2),
            "{left:?} of validity"
        );
    }

    #[test]
    fn an_already_expired_url_is_never_served() {
        let cache = YtAudioStreamCache::new();
        let url = format!(
            "https://rr1.googlevideo.com/videoplayback?expire={}",
            unix_now() - 30
        );
        cache_set(&cache, "abc", info_for(&url, &[]));

        assert!(
            cache_get(&cache, "abc").is_none(),
            "a URL past its own expiry must not be handed out"
        );
    }

    #[test]
    fn a_url_without_an_expiry_falls_back_to_the_fixed_lifetime() {
        let cache = YtAudioStreamCache::new();
        cache_set(
            &cache,
            "abc",
            info_for("https://example.test/audio.webm", &[]),
        );
        assert!(cache_get(&cache, "abc").is_some());
    }

    #[test]
    fn invalidating_forces_the_next_play_to_re_resolve() {
        let cache = YtAudioStreamCache::new();
        let url = format!(
            "https://rr1.googlevideo.com/videoplayback?expire={}",
            unix_now() + 3_600
        );
        cache_set(&cache, "abc", info_for(&url, &[]));
        assert!(cache_get(&cache, "abc").is_some());

        invalidate(&cache, "abc");
        assert!(
            cache_get(&cache, "abc").is_none(),
            "a rejected URL has to be gone, or every replay repeats the failure"
        );
    }

    #[test]
    fn a_rejection_is_told_apart_from_a_server_fault() {
        use reqwest::StatusCode;
        for status in [
            StatusCode::UNAUTHORIZED,
            StatusCode::FORBIDDEN,
            StatusCode::NOT_FOUND,
            StatusCode::GONE,
        ] {
            assert!(url_is_spent(status), "{status} means the URL is spent");
        }
        for status in [
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::TOO_MANY_REQUESTS,
        ] {
            assert!(
                !url_is_spent(status),
                "{status} is the CDN struggling — re-resolving does not help"
            );
        }
    }

    /// The bug this guards: the headers yt-dlp reports for the client that
    /// minted the URL were captured and then dropped at the fetch, leaving an
    /// android URL requested with a browser identity.
    #[tokio::test]
    async fn the_fetch_uses_the_headers_the_url_was_minted_for() {
        use wiremock::matchers::{header, method};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header(
                "User-Agent",
                "com.google.android.apps.youtube.vr.oculus/1.61",
            ))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let info = info_for(
            &server.uri(),
            &[(
                "User-Agent",
                "com.google.android.apps.youtube.vr.oculus/1.61",
            )],
        );
        let resp = fetch_once(&Client::new(), &info, Some("bytes=0-"))
            .await
            .expect("request sent");
        assert!(resp.status().is_success());
    }

    #[tokio::test]
    async fn a_format_without_reported_headers_falls_back_to_the_web_identity() {
        use wiremock::matchers::{header, header_regex, method};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Referer", "https://www.youtube.com/"))
            .and(header_regex("User-Agent", "^Mozilla/5"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let info = info_for(&server.uri(), &[]);
        let resp = fetch_once(&Client::new(), &info, None)
            .await
            .expect("request sent");
        assert!(resp.status().is_success());
    }

    #[test]
    fn parse_http_headers_extracts_pairs() {
        let json = r#"{"User-Agent":"UA","Referer":"https://x","X-Num":5}"#;
        let h = parse_http_headers(json);
        assert!(h.iter().any(|(k, v)| k == "User-Agent" && v == "UA"));
        assert!(h.iter().any(|(k, v)| k == "Referer" && v == "https://x"));
        assert!(!h.iter().any(|(k, _)| k == "X-Num"));
    }
}
