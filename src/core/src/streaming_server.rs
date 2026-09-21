use std::{
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
};

use axum::serve::ListenerExt;
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use bytes::Bytes;
use dashmap::DashMap;
use reqwest::{header::HeaderMap as ReqwestHeaderMap, Client};
use std::collections::BTreeMap;
use tokio::sync::{mpsc, oneshot, Mutex};
use tower_http::cors::{Any, CorsLayer};

use crate::errors::MhResult;

/// An empty 500 — the fallback every response builder in here shares.
pub(crate) fn or_500() -> axum::http::Response<Body> {
    axum::http::Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .body(Body::empty())
        .unwrap()
}

pub enum StreamContent {
    Full(Bytes),
    Progressive(Mutex<mpsc::Receiver<Result<Bytes, String>>>),
    /// A live HLS station stream-copied to ADTS by ffmpeg, spawned fresh on each
    /// request.
    ///
    /// [`StreamContent::Progressive`] cannot do this job: it holds one receiver
    /// and is removed from the map the first time it is served, so an HLS station
    /// could be played exactly once. Respawning also happens to be the correct
    /// semantics for live audio — there is no earlier buffer worth replaying.
    Remux {
        url: String,
        headers: BTreeMap<String, String>,
    },
    Proxied {
        url: String,
        auth_headers: ReqwestHeaderMap,
    },
    SeekableDeezer {
        cdn_url: String,
        /// The song id the Blowfish key derives from, which is the one Deezer served
        /// rather than the one that was asked for.
        crypto_id: String,
        filesize: Option<u64>,
    },
    /// A background task is writing to `path`; `done` is set to `true` when it finishes.
    /// `total` is the final byte size once known (0 = unknown), enabling seek-bar Range
    /// requests before the download completes. Instant-start + seeking, no full pre-download.
    TempFile {
        path: PathBuf,
        done: Arc<AtomicBool>,
        total: Arc<AtomicU64>,
    },
    Local {
        path: PathBuf,
    },
    /// Tidal DASH FLAC: fMP4 fragments piped through `ffmpeg -c:a copy -f flac` on the
    /// fly. Segments feed ffmpeg stdin as they download; FLAC frames stream from stdout
    /// to the client. Instant-start (~1s), lossless (stream copy), no full pre-download.
    /// Not seekable (chunked, no total size) — the tradeoff for instant playback.
    TidalRemux {
        urls: Vec<String>,
        access_token: String,
    },
    AppleSeekable(Arc<crate::services::apple_music::playback::AppleSeekableStream>),
    /// An internet radio station: an endless ICY or HLS stream.
    ///
    /// Close to [`StreamContent::Proxied`] but wrong in two ways that matter for live
    /// audio. That variant resumes a dropped connection with `Range: bytes=N-`, which an
    /// ICY server has no notion of, so the retry returns a non-206 and the stream quietly
    /// ends; and it cannot see the title metadata Shoutcast interleaves into the body,
    /// which a `<audio>` element in the WebView has no way to read either.
    Radio {
        url: String,
        station_uuid: String,
        headers: BTreeMap<String, String>,
    },
}

pub struct StreamEntry {
    pub content: StreamContent,
    pub content_type: String,
    seq: AtomicU64,
}

/// Reports whether one registered stream is still alive. Handed to background
/// tasks — the radio now-playing poller — so they stop when their stream does
/// instead of needing a cancellation channel of their own.
#[derive(Clone)]
pub struct StreamLiveness {
    streams: StreamMap,
    id: String,
}

impl StreamLiveness {
    pub fn is_live(&self) -> bool {
        self.streams.contains_key(&self.id)
    }
}

/// Current track, prefetch, and a crossfade pair — doubled, because a video
/// registers two streams (picture and audio) rather than one muxed stream.
/// Sized too tightly, eviction pulls the running track's own stream out from
/// under it while the next one is being prefetched.
const MAX_LIVE_STREAMS: usize = 8;

pub type StreamMap = Arc<DashMap<String, StreamEntry>>;

#[derive(Debug, Clone)]
pub struct QobuzCaptured {
    pub user_id: String,
    pub user_auth_token: String,
    pub app_id: String,
}

#[derive(Debug, Clone)]
pub struct TidalCaptured {
    pub code: String,
}

/// Nonce-keyed one-shot slots for an OAuth redirect landing back on the local
/// server: the flow registers a nonce before opening the browser, and the
/// matching callback route fulfils it.
pub struct CaptureRegistry<T>(Arc<DashMap<String, oneshot::Sender<T>>>);

impl<T> Clone for CaptureRegistry<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Default for CaptureRegistry<T> {
    fn default() -> Self {
        Self(Arc::new(DashMap::new()))
    }
}

impl<T> CaptureRegistry<T> {
    pub fn register(&self, nonce: String) -> oneshot::Receiver<T> {
        let (tx, rx) = oneshot::channel();
        self.0.insert(nonce, tx);
        rx
    }

    pub fn unregister(&self, nonce: &str) {
        self.0.remove(nonce);
    }

    /// Hands `value` to whoever is waiting on `nonce`. False if nobody is.
    pub fn fulfil(&self, nonce: &str, value: T) -> bool {
        match self.0.remove(nonce) {
            Some((_, tx)) => tx.send(value).is_ok(),
            None => false,
        }
    }
}

type CoverResolver =
    Arc<std::sync::RwLock<Option<Arc<dyn crate::media::library::CoverMaterializer>>>>;

#[derive(Clone)]
struct AppState {
    streams: StreamMap,
    stream_seq: Arc<AtomicU64>,
    http: Client,
    covers_dir: Arc<std::sync::RwLock<Option<PathBuf>>>,
    cover_resolver: CoverResolver,
    qobuz_captures: CaptureRegistry<QobuzCaptured>,
    tidal_captures: CaptureRegistry<TidalCaptured>,
    radio_emitter: RadioEmitter,
}

type RadioEmitter = Arc<std::sync::RwLock<Option<Arc<dyn crate::EventEmitter>>>>;

pub struct StreamingServer {
    pub port: u16,
    streams: StreamMap,
    stream_seq: Arc<AtomicU64>,
    covers_dir: Arc<std::sync::RwLock<Option<PathBuf>>>,
    cover_resolver: CoverResolver,
    qobuz_captures: CaptureRegistry<QobuzCaptured>,
    tidal_captures: CaptureRegistry<TidalCaptured>,
    radio_emitter: RadioEmitter,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl StreamingServer {
    pub async fn start() -> MhResult<Self> {
        let streams: StreamMap = Arc::new(DashMap::new());
        let stream_seq = Arc::new(AtomicU64::new(0));
        let http = crate::http_client::build_audio_client()?;
        let covers_dir: Arc<std::sync::RwLock<Option<PathBuf>>> =
            Arc::new(std::sync::RwLock::new(None));
        let cover_resolver: CoverResolver = Arc::new(std::sync::RwLock::new(None));
        let qobuz_captures = CaptureRegistry::<QobuzCaptured>::default();
        let tidal_captures = CaptureRegistry::<TidalCaptured>::default();
        let radio_emitter: RadioEmitter = Arc::new(std::sync::RwLock::new(None));
        let state = AppState {
            streams: streams.clone(),
            stream_seq: stream_seq.clone(),
            http,
            covers_dir: covers_dir.clone(),
            cover_resolver: cover_resolver.clone(),
            qobuz_captures: qobuz_captures.clone(),
            tidal_captures: tidal_captures.clone(),
            radio_emitter: radio_emitter.clone(),
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/cover/{id}", get(serve_cover))
            .route("/cover-proxy/{platform}/{*key}", get(serve_cover_proxy))
            .route("/qobuz/capture", get(serve_qobuz_capture))
            .route("/tidal/capture", get(serve_tidal_capture))
            .route("/{id}", get(serve_stream))
            .with_state(state)
            .layer(cors);

        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let port = listener.local_addr()?.port();
        // Nothing sets nodelay server-side otherwise, and this server writes many
        // small chunks — the Nagle plus delayed-ACK pattern.
        let listener = listener.tap_io(|stream| {
            let _ = stream.set_nodelay(true);
        });

        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await
                .ok();
        });

        Ok(Self {
            port,
            streams,
            stream_seq,
            covers_dir,
            cover_resolver,
            qobuz_captures,
            tidal_captures,
            radio_emitter,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    pub fn set_radio_emitter(&self, emitter: Arc<dyn crate::EventEmitter>) {
        if let Ok(mut slot) = self.radio_emitter.write() {
            *slot = Some(emitter);
        }
    }

    pub fn qobuz_captures(&self) -> &CaptureRegistry<QobuzCaptured> {
        &self.qobuz_captures
    }

    pub fn tidal_captures(&self) -> &CaptureRegistry<TidalCaptured> {
        &self.tidal_captures
    }

    pub fn tidal_capture_deliverer(&self) -> impl Fn(&str, String) + Send + Sync + 'static {
        let reg = self.tidal_captures.clone();
        move |nonce: &str, code: String| {
            reg.fulfil(nonce, TidalCaptured { code });
        }
    }

    pub fn set_covers_dir(&self, dir: PathBuf) {
        if let Ok(mut w) = self.covers_dir.write() {
            *w = Some(dir);
        }
    }

    pub fn set_cover_resolver(&self, resolver: Arc<dyn crate::media::library::CoverMaterializer>) {
        if let Ok(mut w) = self.cover_resolver.write() {
            *w = Some(resolver);
        }
    }

    pub fn cover_url(&self, cover_id: &str) -> String {
        format!("http://127.0.0.1:{}/cover/{}", self.port, cover_id)
    }

    pub fn service_cover_url(&self, cover_id: &str) -> Option<String> {
        let (platform, key) = cover_id.split_once(':')?;
        Some(format!(
            "http://127.0.0.1:{}/cover-proxy/{platform}/{}",
            self.port,
            crate::services::common::http::pct_encode(key),
        ))
    }

    pub fn has_stream(&self, id: &str) -> bool {
        self.streams.contains_key(id)
    }

    fn insert_stream(&self, id: &str, content: StreamContent, content_type: String) -> String {
        let seq = self.stream_seq.fetch_add(1, Ordering::Relaxed);
        self.streams.insert(
            id.to_string(),
            StreamEntry {
                content,
                content_type,
                seq: AtomicU64::new(seq),
            },
        );
        self.evict_stale_streams();
        format!("http://127.0.0.1:{}/{}", self.port, id)
    }

    fn evict_stale_streams(&self) {
        while self.streams.len() > MAX_LIVE_STREAMS {
            let oldest = self
                .streams
                .iter()
                .min_by_key(|r| r.value().seq.load(Ordering::Relaxed))
                .map(|r| r.key().clone());
            match oldest {
                Some(id) => self.remove_stream(&id),
                None => break,
            }
        }
    }

    /// Serve `content` at `/{id}` and return its absolute URL. Callers build the
    /// `StreamContent` variant that matches how their service delivers audio.
    pub fn register(&self, id: &str, content: StreamContent, content_type: &str) -> String {
        self.insert_stream(id, content, content_type.to_string())
    }

    pub fn register_stream(&self, id: &str, data: Bytes, content_type: &str) -> String {
        self.register(id, StreamContent::Full(data), content_type)
    }

    pub fn register_radio(
        &self,
        id: &str,
        url: &str,
        station_uuid: &str,
        content_type: &str,
        headers: BTreeMap<String, String>,
    ) -> String {
        self.register(
            id,
            StreamContent::Radio {
                url: url.to_string(),
                station_uuid: station_uuid.to_string(),
                headers,
            },
            content_type,
        )
    }

    /// Serves `url` remuxed to ADTS. The ffmpeg process starts when the client
    /// first asks for the body, not here, so nothing is spawned for a station the
    /// user never actually plays.
    pub fn register_remux(
        &self,
        id: &str,
        url: &str,
        content_type: &str,
        headers: BTreeMap<String, String>,
    ) -> String {
        self.register(
            id,
            StreamContent::Remux {
                url: url.to_string(),
                headers,
            },
            content_type,
        )
    }

    pub fn register_stream_progressive(
        &self,
        id: &str,
        rx: mpsc::Receiver<Result<Bytes, String>>,
        content_type: &str,
    ) -> String {
        self.register(id, StreamContent::Progressive(Mutex::new(rx)), content_type)
    }

    pub fn remove_stream(&self, id: &str) {
        if let Some((_, entry)) = self.streams.remove(id) {
            if let StreamContent::TempFile { path, .. } = entry.content {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Caps how many streams whose id starts with `prefix` may be live at once,
    /// dropping the least recently registered first.
    ///
    /// Radio needs this because it never removed anything: each station played
    /// left an entry behind, and eight of them pushed the running track's own
    /// stream out of the shared LRU. Clearing them all is not the answer — the
    /// player prefetches the next queue entry while the current one is still
    /// being served, so the newest radio entry is not necessarily the one a
    /// listener is attached to.
    pub fn trim_streams_with_prefix(&self, prefix: &str, keep: usize) {
        let mut matching: Vec<(String, u64)> = self
            .streams
            .iter()
            .filter(|r| r.key().starts_with(prefix))
            .map(|r| (r.key().clone(), r.value().seq.load(Ordering::Relaxed)))
            .collect();
        let excess = matching.len().saturating_sub(keep);
        if excess == 0 {
            return;
        }
        matching.sort_by_key(|(_, seq)| *seq);
        for (id, _) in matching.into_iter().take(excess) {
            self.remove_stream(&id);
        }
    }

    /// A handle a background task can use to find out whether the stream it is
    /// working for is still registered, so its lifetime follows the stream's.
    pub fn liveness(&self, id: &str) -> StreamLiveness {
        StreamLiveness {
            streams: self.streams.clone(),
            id: id.to_string(),
        }
    }

    pub fn clear_streams(&self) {
        for r in self.streams.iter() {
            if let StreamContent::TempFile { path, .. } = &r.value().content {
                let _ = std::fs::remove_file(path);
            }
        }
        self.streams.clear();
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.clear_streams();
    }
}

/// A `Range: bytes=..` header, in the two forms a media client sends.
///
/// The suffix form is the one that used to be lost: `split('-').next()` on
/// `bytes=-500` yields an empty string, which parsed to nothing, so the request
/// was answered `200 OK` with the whole body. It is what a container probe uses
/// to read a trailer, and it cannot be placed until the total length is known —
/// hence `resolve` rather than a start offset falling out of the parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ByteRange {
    FromStart { start: u64, end: Option<u64> },
    Suffix { len: u64 },
}

impl ByteRange {
    /// Inclusive `(start, end)` within a body of `total` bytes, or `None` when
    /// the range lies outside it and the only honest answer is a 416.
    pub(crate) fn resolve(self, total: u64) -> Option<(u64, u64)> {
        if total == 0 {
            return None;
        }
        match self {
            ByteRange::FromStart { start, end } => {
                if start >= total {
                    return None;
                }
                Some((start, end.unwrap_or(u64::MAX).min(total - 1)))
            }
            ByteRange::Suffix { len } => {
                if len == 0 {
                    return None;
                }
                Some((total.saturating_sub(len), total - 1))
            }
        }
    }

    /// True for `bytes=start-`, the form a media element uses to mean "the rest",
    /// and the only one a branch may answer with a shorter window than asked.
    pub(crate) fn is_open_ended(self) -> bool {
        matches!(self, ByteRange::FromStart { end: None, .. })
    }
}

/// `None` for both a missing header and an unusable one — RFC 7233 says a Range
/// that cannot be understood must be ignored, and ignoring it means serving the
/// whole body, which is what every caller here does with `None`.
pub(crate) fn parse_range_header(headers: &HeaderMap) -> Option<ByteRange> {
    let spec = headers
        .get(header::RANGE)?
        .to_str()
        .ok()?
        .trim()
        .strip_prefix("bytes=")?;

    // Multi-range requests are legal, but no media element sends one and a
    // single-part response to one would misdescribe the body.
    if spec.contains(',') {
        return None;
    }

    let (start, end) = spec.split_once('-')?;
    let (start, end) = (start.trim(), end.trim());

    if start.is_empty() {
        return Some(ByteRange::Suffix {
            len: end.parse().ok()?,
        });
    }

    let start: u64 = start.parse().ok()?;
    let end = match end {
        "" => None,
        e => Some(e.parse::<u64>().ok()?),
    };
    if end.is_some_and(|e| e < start) {
        return None;
    }
    Some(ByteRange::FromStart { start, end })
}

/// Trims a decrypted chunk down to the slice the client actually asked for.
///
/// Deezer's CDN has to be fetched from a 6144-byte Blowfish boundary, so the
/// head of the first chunk is always dropped; the tail is dropped once the
/// requested end has been reached, which is also the signal to stop pulling.
pub(crate) struct RangeClip {
    pub(crate) skip: usize,
    pub(crate) remaining: Option<u64>,
}

impl RangeClip {
    pub(crate) fn take(&mut self, mut out: Vec<u8>) -> Option<Bytes> {
        if self.skip > 0 {
            let drop = self.skip.min(out.len());
            out.drain(..drop);
            self.skip -= drop;
        }
        if let Some(left) = self.remaining.as_mut() {
            let take = (*left).min(out.len() as u64) as usize;
            out.truncate(take);
            *left -= take as u64;
        }
        if out.is_empty() {
            return None;
        }
        Some(Bytes::from(out))
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.remaining == Some(0)
    }
}

enum ResolvedStream {
    Full {
        content_type: String,
        data: Bytes,
    },
    Progressive(String),
    Proxied {
        content_type: String,
        url: String,
        auth_headers: ReqwestHeaderMap,
    },
    Radio {
        content_type: String,
        url: String,
        station_uuid: String,
        headers: BTreeMap<String, String>,
    },
    Remux {
        content_type: String,
        url: String,
        headers: BTreeMap<String, String>,
    },
}

/// Stream-copies a live HLS station into ADTS so a WebKit `<audio>` element will
/// take it. WebKitGTK plays no HLS and the app ships no JavaScript player for it,
/// so this goes through the ffmpeg that is already a hard dependency — a process,
/// not a re-encode.
fn serve_remux(content_type: String, url: String, headers: BTreeMap<String, String>) -> Response {
    use std::process::Stdio;
    use tokio::io::AsyncReadExt;

    let (tx, rx) = mpsc::channel::<Result<Bytes, String>>(32);
    let ffmpeg = crate::venv_manager::resolve_ffmpeg();

    tokio::spawn(async move {
        let mut cmd = tokio::process::Command::new(&ffmpeg);
        cmd.args(["-loglevel", "error"]);
        // ffmpeg takes its own `-user_agent`; passing one through `-headers`
        // instead is ignored, so the two are split here.
        for (name, value) in &headers {
            if name.eq_ignore_ascii_case("user-agent") {
                cmd.args(["-user_agent", value]);
            }
        }
        let extra = ffmpeg_header_block(&headers);
        if !extra.is_empty() {
            cmd.args(["-headers", &extra]);
        }
        cmd.args([
            "-reconnect",
            "1",
            "-reconnect_streamed",
            "1",
            "-reconnect_delay_max",
            "10",
            "-i",
            &url,
            "-vn",
            "-c:a",
            "copy",
            "-f",
            "adts",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
        crate::subprocess::apply_no_window(&mut cmd);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = tx.send(Err(format!("ffmpeg ({ffmpeg}): {e}"))).await;
                return;
            }
        };
        let mut stderr = child.stderr.take();
        if let Some(mut stdout) = child.stdout.take() {
            let mut buf = vec![0u8; 32_768];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx
                            .send(Ok(Bytes::copy_from_slice(&buf[..n])))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string())).await;
                        break;
                    }
                }
            }
        }
        if let Some(mut err) = stderr.take() {
            let mut text = String::new();
            let _ = err.read_to_string(&mut text).await;
            if !text.trim().is_empty() {
                let _ = tx.send(Err(text)).await;
            }
        }
        // The client hanging up closes the channel, which ends the read loop
        // above; killing the child is what stops ffmpeg pulling the manifest
        // forever after the listener has gone.
        let _ = child.kill().await;
    });

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(body_from_channel(rx))
        .unwrap_or_else(|_| or_500())
}

/// Streams a radio station to the WebView, stripping Shoutcast's interleaved metadata
/// out of the audio and reporting the titles it carries.
///
/// Proxying is not optional even for a well-behaved station: the page is served from a
/// secure origin, so a plain-`http://` stream — which a large share of the directory
/// still is — would be blocked as mixed content if the element fetched it directly.
async fn serve_radio(
    state: AppState,
    content_type: String,
    url: String,
    station_uuid: String,
    station_headers: BTreeMap<String, String>,
) -> Response {
    let upstream = match icy_request(&state.http, &url, &station_headers)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    };
    if !upstream.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            format!("station returned HTTP {}", upstream.status().as_u16()),
        )
            .into_response();
    }

    let headers = upstream.headers().clone();
    let ct = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or(&content_type)
        .to_string();
    let meta_int: usize = headers
        .get("icy-metaint")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let emitter = state.radio_emitter.read().ok().and_then(|g| g.clone());
    let reader = IcyReader::new(meta_int, station_uuid, emitter);

    let body = futures_util::stream::unfold(
        (
            Some(upstream),
            reader,
            state.http.clone(),
            url,
            station_headers,
            0u32,
        ),
        |(resp, mut reader, http, url, station_headers, mut failures)| async move {
            let mut resp = resp?;
            loop {
                match resp.chunk().await {
                    Ok(Some(chunk)) => {
                        let audio = reader.take(&chunk);
                        return Some((
                            Ok::<Bytes, std::io::Error>(audio),
                            (Some(resp), reader, http, url, station_headers, 0),
                        ));
                    }
                    // A live stream has no end and no byte offsets, so a drop is
                    // reconnected from the top rather than resumed with a Range.
                    Ok(None) | Err(_) if failures < 5 => {
                        failures += 1;
                        tokio::time::sleep(std::time::Duration::from_millis(
                            500 * u64::from(failures),
                        ))
                        .await;
                        match icy_request(&http, &url, &station_headers).send().await {
                            Ok(next) if next.status().is_success() => {
                                reader.restart();
                                resp = next;
                            }
                            _ => return None,
                        }
                    }
                    _ => return None,
                }
            }
        },
    );

    axum::http::Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, ct)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from_stream(body))
        .unwrap_or_else(|_| or_500())
}

/// One upstream request for a station, carrying whatever headers it needs.
///
/// A broadcaster that checks `Referer` answers 403 without one, and the retry
/// path has to send the same set or a dropped connection never comes back.
fn icy_request(
    http: &Client,
    url: &str,
    headers: &BTreeMap<String, String>,
) -> reqwest::RequestBuilder {
    let mut req = http.get(url).header("Icy-MetaData", "1");
    for (name, value) in headers {
        req = req.header(name.as_str(), value.as_str());
    }
    req
}

/// ffmpeg takes extra headers as one CRLF-separated block. `User-Agent` is left
/// out because it has a dedicated flag that overrides anything set here.
fn ffmpeg_header_block(headers: &BTreeMap<String, String>) -> String {
    headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("user-agent"))
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect()
}

/// Splits an ICY body into audio and titles.
///
/// The server sends `icy-metaint` bytes of audio, then one length byte, then that many
/// sixteen-byte blocks of `StreamTitle='...';`, and repeats — forever. Chunk boundaries
/// fall anywhere in that pattern, so the position has to be carried between chunks.
struct IcyReader {
    meta_int: usize,
    until_meta: usize,
    meta_remaining: usize,
    expecting_length: bool,
    meta_buf: Vec<u8>,
    station_uuid: String,
    emitter: Option<Arc<dyn crate::EventEmitter>>,
    last_title: String,
}

impl IcyReader {
    fn new(
        meta_int: usize,
        station_uuid: String,
        emitter: Option<Arc<dyn crate::EventEmitter>>,
    ) -> Self {
        Self {
            meta_int,
            until_meta: meta_int,
            meta_remaining: 0,
            expecting_length: false,
            meta_buf: Vec::new(),
            station_uuid,
            emitter,
            last_title: String::new(),
        }
    }

    fn restart(&mut self) {
        self.until_meta = self.meta_int;
        self.meta_remaining = 0;
        self.expecting_length = false;
        self.meta_buf.clear();
    }

    fn take(&mut self, chunk: &[u8]) -> Bytes {
        if self.meta_int == 0 {
            return Bytes::copy_from_slice(chunk);
        }
        let mut audio = Vec::with_capacity(chunk.len());
        let mut i = 0;
        while i < chunk.len() {
            if self.expecting_length {
                self.meta_remaining = chunk[i] as usize * 16;
                self.expecting_length = false;
                self.meta_buf.clear();
                i += 1;
                if self.meta_remaining == 0 {
                    self.until_meta = self.meta_int;
                }
                continue;
            }
            if self.meta_remaining > 0 {
                let take = self.meta_remaining.min(chunk.len() - i);
                self.meta_buf.extend_from_slice(&chunk[i..i + take]);
                self.meta_remaining -= take;
                i += take;
                if self.meta_remaining == 0 {
                    self.flush_metadata();
                    self.until_meta = self.meta_int;
                }
                continue;
            }
            let take = self.until_meta.min(chunk.len() - i);
            audio.extend_from_slice(&chunk[i..i + take]);
            self.until_meta -= take;
            i += take;
            if self.until_meta == 0 {
                self.expecting_length = true;
            }
        }
        Bytes::from(audio)
    }

    fn flush_metadata(&mut self) {
        let raw = String::from_utf8_lossy(&self.meta_buf);
        let Some(title) = raw
            .split(';')
            .find_map(|part| part.trim().strip_prefix("StreamTitle="))
            .map(|v| v.trim().trim_matches('\'').trim())
            .filter(|v| !v.is_empty())
        else {
            return;
        };
        if title == self.last_title {
            return;
        }
        self.last_title = title.to_string();
        if let Some(emitter) = &self.emitter {
            emitter.emit_radio_metadata(&crate::ipc_contract::RadioMetadataEvent {
                station_uuid: self.station_uuid.clone(),
                title: title.to_string(),
            });
        }
    }
}

/// `416` with the `Content-Range: bytes */N` a client needs in order to re-ask
/// correctly. Written out at five call sites before this.
pub(crate) fn range_not_satisfiable(total: u64) -> Response {
    axum::http::Response::builder()
        .status(StatusCode::RANGE_NOT_SATISFIABLE)
        .header(header::CONTENT_RANGE, format!("bytes */{total}"))
        .body(Body::empty())
        .unwrap_or_else(|_| or_500())
}

/// A channel of chunk results as a response body. Four byte-identical copies of this
/// adapter existed inside `serve_stream`.
pub(crate) fn body_from_channel(rx: mpsc::Receiver<Result<Bytes, String>>) -> Body {
    Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Some(Ok(c)) => Some((Ok::<Bytes, std::io::Error>(c), rx)),
            Some(Err(e)) => Some((Err(std::io::Error::other(e)), rx)),
            None => None,
        }
    }))
}

async fn serve_stream(
    Path(id): Path<String>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Response {
    {
        let entry = match state.streams.get(&id) {
            Some(e) => e,
            None => return (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        };
        entry.seq.store(
            state.stream_seq.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
        if let StreamContent::AppleSeekable(stream) = &entry.content {
            let stream = stream.clone();
            let content_type = entry.content_type.clone();
            drop(entry);
            return crate::services::apple_music::http_stream::serve_seekable(
                stream,
                &headers,
                content_type,
            )
            .await;
        }
        if let StreamContent::TidalRemux { urls, access_token } = &entry.content {
            let urls = urls.clone();
            let access_token = access_token.clone();
            let content_type = entry.content_type.clone();
            drop(entry);
            return crate::services::tidal::http_stream::serve_remux(
                state.http.clone(),
                urls,
                access_token,
                content_type,
            )
            .await;
        }
        if let StreamContent::SeekableDeezer {
            cdn_url,
            crypto_id,
            filesize,
        } = &entry.content
        {
            let cdn_url = cdn_url.clone();
            let crypto_id = crypto_id.clone();
            let filesize = *filesize;
            let content_type = entry.content_type.clone();
            drop(entry);
            return crate::services::deezer::http_stream::serve_seekable(
                &state.http,
                &headers,
                cdn_url,
                crypto_id,
                filesize,
                content_type,
            )
            .await;
        } else if matches!(
            &entry.content,
            StreamContent::TempFile { .. } | StreamContent::Local { .. }
        ) {
            let (path, done_opt, total_opt) = match &entry.content {
                StreamContent::TempFile { path, done, total } => {
                    (path.clone(), Some(done.clone()), Some(total.clone()))
                }
                StreamContent::Local { path } => (path.clone(), None, None),
                _ => unreachable!(),
            };
            let content_type = entry.content_type.clone();
            drop(entry);

            let progressive = done_opt.is_some();

            let requested = parse_range_header(&headers);
            let is_range = requested.is_some();

            let known_total = total_opt
                .as_ref()
                .map(|t| t.load(Ordering::Relaxed))
                .filter(|&t| t > 0);

            let written = if progressive {
                let done = done_opt.as_ref().unwrap();
                let min_bytes = match requested {
                    Some(ByteRange::FromStart { start, .. }) => start + 1,
                    // A suffix range is measured from an end that does not exist
                    // yet, so there is nothing to wait for beyond the first byte.
                    _ => 1,
                };
                loop {
                    let size = tokio::fs::metadata(&path)
                        .await
                        .map(|m| m.len())
                        .unwrap_or(0);
                    if size >= min_bytes || done.load(Ordering::Relaxed) {
                        break size;
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            } else {
                match tokio::fs::metadata(&path).await {
                    Ok(m) => m.len(),
                    Err(e) => {
                        return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                    }
                }
            };

            let content_range_total = known_total.unwrap_or(written);

            let (range_start, range_end) = match requested {
                Some(range) => match range.resolve(content_range_total) {
                    Some(pair) => pair,
                    None => {
                        return range_not_satisfiable(content_range_total);
                    }
                },
                None => (0, content_range_total.saturating_sub(1)),
            };

            // Only bound the read when the total is settled. A progressive file
            // whose size is still unknown is longer than `written` by the time
            // the loop reaches it, and capping there would truncate the track.
            let limit = (is_range && (!progressive || known_total.is_some()))
                .then(|| range_end - range_start + 1);

            let path_task = path.clone();
            let done_task = done_opt.clone();
            let (tx, rx) = mpsc::channel::<Result<Bytes, String>>(32);
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncSeekExt};

                let mut file = match tokio::fs::File::open(&path_task).await {
                    Ok(f) => f,
                    Err(e) => {
                        let _ = tx.send(Err(e.to_string())).await;
                        return;
                    }
                };
                if range_start > 0
                    && file
                        .seek(std::io::SeekFrom::Start(range_start))
                        .await
                        .is_err()
                {
                    let _ = tx.send(Err("Seek failed".to_string())).await;
                    return;
                }
                let mut buf = vec![0u8; 65_536];
                let mut left = limit;
                loop {
                    if left == Some(0) {
                        return;
                    }
                    match file.read(&mut buf).await {
                        Ok(0) => match &done_task {
                            Some(done) if !done.load(Ordering::Relaxed) => {
                                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                            }
                            _ => break,
                        },
                        Ok(n) => {
                            let n = match left.as_mut() {
                                Some(l) => {
                                    let take = (*l).min(n as u64) as usize;
                                    *l -= take as u64;
                                    take
                                }
                                None => n,
                            };
                            if tx
                                .send(Ok(Bytes::copy_from_slice(&buf[..n])))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(e) => {
                            let _ = tx.send(Err(e.to_string())).await;
                            return;
                        }
                    }
                }
            });

            let mut builder = axum::http::Response::builder()
                .header(header::CONTENT_TYPE, content_type)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CACHE_CONTROL, "no-cache");

            if is_range {
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(
                        header::CONTENT_RANGE,
                        format!(
                            "bytes {}-{}/{}",
                            range_start, range_end, content_range_total
                        ),
                    )
                    .header(
                        header::CONTENT_LENGTH,
                        (range_end - range_start + 1).to_string(),
                    );
            } else if let Some(total) = known_total {
                let end = total.saturating_sub(1);
                builder = builder
                    .status(StatusCode::PARTIAL_CONTENT)
                    .header(header::ACCEPT_RANGES, "bytes")
                    .header(header::CONTENT_RANGE, format!("bytes 0-{}/{}", end, total))
                    .header(header::CONTENT_LENGTH, total.to_string());
            } else if progressive {
                builder = builder.status(StatusCode::OK);
            } else {
                builder = builder
                    .status(StatusCode::OK)
                    .header(header::CONTENT_LENGTH, written.to_string());
            }

            return builder
                .body(body_from_channel(rx))
                .unwrap_or_else(|_| or_500());
        }
    }

    let resolved = {
        let entry = match state.streams.get(&id) {
            Some(e) => e,
            None => return (StatusCode::NOT_FOUND, "Stream not found").into_response(),
        };
        match &entry.content {
            StreamContent::Full(b) => ResolvedStream::Full {
                content_type: entry.content_type.clone(),
                data: b.clone(),
            },
            StreamContent::Progressive(_) => {
                ResolvedStream::Progressive(entry.content_type.clone())
            }
            StreamContent::Proxied { url, auth_headers } => ResolvedStream::Proxied {
                content_type: entry.content_type.clone(),
                url: url.clone(),
                auth_headers: auth_headers.clone(),
            },
            StreamContent::Radio {
                url,
                station_uuid,
                headers,
            } => ResolvedStream::Radio {
                content_type: entry.content_type.clone(),
                url: url.clone(),
                station_uuid: station_uuid.clone(),
                headers: headers.clone(),
            },
            StreamContent::Remux { url, headers } => ResolvedStream::Remux {
                content_type: entry.content_type.clone(),
                url: url.clone(),
                headers: headers.clone(),
            },
            StreamContent::SeekableDeezer { .. }
            | StreamContent::TempFile { .. }
            | StreamContent::Local { .. }
            | StreamContent::TidalRemux { .. }
            | StreamContent::AppleSeekable(_) => unreachable!(),
        }
    };

    if let ResolvedStream::Proxied {
        content_type,
        url: cdn_url,
        auth_headers,
    } = resolved
    {
        let initial_offset: u64 = headers
            .get(header::RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("bytes="))
            .and_then(|s| s.split('-').next())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // googlevideo answers a rangeless GET with 403, so one is always sent.
        let client_asked_for_range = headers.contains_key(header::RANGE);
        let upstream_req = state
            .http
            .get(&cdn_url)
            .headers(auth_headers.clone())
            .header(
                reqwest::header::RANGE,
                headers
                    .get(header::RANGE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("bytes=0-"),
            );

        let upstream_resp = match upstream_req.send().await {
            Ok(r) => r,
            Err(e) => {
                return (StatusCode::BAD_GATEWAY, e.to_string()).into_response();
            }
        };

        let up_status = upstream_resp.status();
        let whole_body_as_partial =
            !client_asked_for_range && up_status == reqwest::StatusCode::PARTIAL_CONTENT;
        let axum_status = if whole_body_as_partial {
            StatusCode::OK
        } else {
            StatusCode::from_u16(up_status.as_u16()).unwrap_or(StatusCode::OK)
        };
        let up_headers = upstream_resp.headers().clone();

        let ct = up_headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or(&content_type)
            .to_string();

        let http = state.http.clone();

        let body_stream = futures_util::stream::unfold(
            (
                upstream_resp,
                initial_offset,
                http,
                cdn_url,
                auth_headers,
                0u32,
            ),
            |(mut resp, mut byte_pos, http, url, auth, mut consec_err)| async move {
                loop {
                    match resp.chunk().await {
                        Ok(Some(chunk)) => {
                            byte_pos += chunk.len() as u64;
                            return Some((
                                Ok::<Bytes, std::io::Error>(chunk),
                                (resp, byte_pos, http, url, auth, 0),
                            ));
                        }
                        Ok(None) => return None,
                        Err(_) if consec_err < 3 => {
                            consec_err += 1;
                            match http
                                .get(&url)
                                .headers(auth.clone())
                                .header(reqwest::header::RANGE, format!("bytes={}-", byte_pos))
                                .send()
                                .await
                            {
                                Ok(r) if r.status().as_u16() == 206 => {
                                    resp = r;
                                }
                                _ => return None,
                            }
                        }
                        Err(_) => return None,
                    }
                }
            },
        );

        let mut builder = axum::http::Response::builder()
            .status(axum_status)
            .header(header::CONTENT_TYPE, ct)
            .header(header::ACCEPT_RANGES, "bytes")
            .header(header::CACHE_CONTROL, "no-cache");

        if let Some(cl) = up_headers.get(reqwest::header::CONTENT_LENGTH) {
            builder = builder.header(header::CONTENT_LENGTH, cl.clone());
        }
        if !whole_body_as_partial {
            if let Some(cr) = up_headers.get(reqwest::header::CONTENT_RANGE) {
                builder = builder.header(header::CONTENT_RANGE, cr.clone());
            }
        }

        return builder
            .body(Body::from_stream(body_stream))
            .unwrap_or_else(|_| or_500());
    }

    if let ResolvedStream::Radio {
        content_type,
        url,
        station_uuid,
        headers,
    } = resolved
    {
        return serve_radio(state, content_type, url, station_uuid, headers).await;
    }

    if let ResolvedStream::Remux {
        content_type,
        url,
        headers,
    } = resolved
    {
        return serve_remux(content_type, url, headers);
    }

    if let ResolvedStream::Progressive(content_type) = resolved {
        let owned = match state.streams.remove(&id) {
            Some((_, e)) => e,
            None => return (StatusCode::NOT_FOUND, "Stream already consumed").into_response(),
        };
        let rx = match owned.content {
            StreamContent::Progressive(m) => m.into_inner(),
            StreamContent::Full(_)
            | StreamContent::Proxied { .. }
            | StreamContent::Radio { .. }
            | StreamContent::Remux { .. }
            | StreamContent::SeekableDeezer { .. }
            | StreamContent::TempFile { .. }
            | StreamContent::Local { .. }
            | StreamContent::TidalRemux { .. }
            | StreamContent::AppleSeekable(_) => unreachable!(),
        };

        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            body_from_channel(rx),
        )
            .into_response();
    }

    let (content_type, data) = match resolved {
        ResolvedStream::Full { content_type, data } => (content_type, data),
        _ => unreachable!(),
    };

    let total = data.len();

    if let Some(range) = parse_range_header(&headers) {
        let Some((start, end)) = range.resolve(total as u64) else {
            return range_not_satisfiable(total as u64);
        };
        let slice = data.slice(start as usize..=end as usize);
        return (
            StatusCode::PARTIAL_CONTENT,
            [
                (header::CONTENT_TYPE, content_type),
                (
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, total),
                ),
                (header::ACCEPT_RANGES, "bytes".to_string()),
                (header::CACHE_CONTROL, "no-cache".to_string()),
            ],
            slice,
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_LENGTH, total.to_string()),
            (header::ACCEPT_RANGES, "bytes".to_string()),
            (header::CACHE_CONTROL, "no-cache".to_string()),
        ],
        data,
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct QobuzCaptureQuery {
    n: String,
    u: String,
    t: String,
    a: String,
}

async fn serve_qobuz_capture(
    Query(q): Query<QobuzCaptureQuery>,
    State(state): State<AppState>,
) -> Response {
    let payload = QobuzCaptured {
        user_id: q.u,
        user_auth_token: q.t,
        app_id: q.a,
    };
    if state.qobuz_captures.fulfil(&q.n, payload) {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "Unknown nonce").into_response()
    }
}

#[derive(serde::Deserialize)]
struct TidalCaptureQuery {
    n: String,
    c: String,
}

async fn serve_tidal_capture(
    Query(q): Query<TidalCaptureQuery>,
    State(state): State<AppState>,
) -> Response {
    if state
        .tidal_captures
        .fulfil(&q.n, TidalCaptured { code: q.c })
    {
        StatusCode::NO_CONTENT.into_response()
    } else {
        (StatusCode::NOT_FOUND, "Unknown nonce").into_response()
    }
}

async fn serve_cover_proxy(
    Path((platform, key)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Response {
    use crate::services::common::library::{cover_proxy_url, ServicePlatform};
    use std::str::FromStr;
    let plat = match ServicePlatform::from_str(&platform) {
        Ok(p) => p,
        Err(_) => return (StatusCode::BAD_REQUEST, "unknown platform").into_response(),
    };
    let upstream = match cover_proxy_url(plat, &key) {
        Some(u) => u,
        None => return (StatusCode::NOT_FOUND, "no template for this service").into_response(),
    };
    let resp = match state.http.get(&upstream).send().await {
        Ok(r) => r,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream fetch failed").into_response(),
    };
    if !resp.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            format!("upstream {}", resp.status()),
        )
            .into_response();
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("image/jpeg")
        .to_string();
    let body = match resp.bytes().await {
        Ok(b) => b,
        Err(_) => return (StatusCode::BAD_GATEWAY, "upstream read failed").into_response(),
    };
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, ct),
            (
                header::CACHE_CONTROL,
                "public, max-age=604800, immutable".to_string(),
            ),
        ],
        body,
    )
        .into_response()
}

async fn serve_cover(Path(id): Path<String>, State(state): State<AppState>) -> Response {
    let dir = match state.covers_dir.read().ok().and_then(|g| g.clone()) {
        Some(d) => d,
        None => return (StatusCode::NOT_FOUND, "Covers disabled").into_response(),
    };
    let sanitized: String = id
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(32)
        .collect();
    if sanitized.is_empty() {
        return (StatusCode::BAD_REQUEST, "Bad cover id").into_response();
    }
    let path = dir.join(format!("{}.jpg", sanitized));
    let bytes = match tokio::fs::read(&path).await {
        Ok(b) => b,
        Err(_) => {
            let resolver = state.cover_resolver.read().ok().and_then(|g| g.clone());
            match resolver {
                Some(resolver) => {
                    let id = sanitized.clone();
                    let materialized =
                        tokio::task::spawn_blocking(move || resolver.ensure(&id)).await;
                    if !matches!(materialized, Ok(true)) {
                        return (StatusCode::NOT_FOUND, "Cover not found").into_response();
                    }
                    match tokio::fs::read(&path).await {
                        Ok(b) => b,
                        Err(_) => {
                            return (StatusCode::NOT_FOUND, "Cover not found").into_response()
                        }
                    }
                }
                None => return (StatusCode::NOT_FOUND, "Cover not found").into_response(),
            }
        }
    };
    let body = Bytes::from(bytes);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/jpeg".to_string()),
            (
                header::CACHE_CONTROL,
                "public, max-age=604800, immutable".to_string(),
            ),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug this guards: the caller's `Range` was passed upstream only when
    /// it had been given one, and a player's opening request carries none.
    #[tokio::test]
    async fn a_proxied_fetch_always_states_a_range() {
        use wiremock::matchers::{header_exists, method};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header_exists("Range"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 0-4/5")
                    .set_body_bytes(b"audio".to_vec()),
            )
            .expect(1)
            .mount(&upstream)
            .await;

        let server = StreamingServer::start().await.expect("server starts");
        let url = server.register(
            "proxied",
            StreamContent::Proxied {
                url: upstream.uri(),
                auth_headers: ReqwestHeaderMap::new(),
            },
            "audio/mp4",
        );

        let resp = reqwest::get(&url).await.expect("stream is reachable");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a caller that asked for no range must be answered a whole-body 200"
        );
        assert!(
            resp.headers().get(reqwest::header::CONTENT_RANGE).is_none(),
            "a 200 must not carry the Content-Range of our stand-in request"
        );
    }

    #[tokio::test]
    async fn a_proxied_fetch_forwards_the_range_it_was_given() {
        use wiremock::matchers::{header, method};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let upstream = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("Range", "bytes=10-19"))
            .respond_with(
                ResponseTemplate::new(206)
                    .insert_header("Content-Range", "bytes 10-19/100")
                    .set_body_bytes(b"0123456789".to_vec()),
            )
            .expect(1)
            .mount(&upstream)
            .await;

        let server = StreamingServer::start().await.expect("server starts");
        let url = server.register(
            "proxied-range",
            StreamContent::Proxied {
                url: upstream.uri(),
                auth_headers: ReqwestHeaderMap::new(),
            },
            "audio/mp4",
        );

        let resp = reqwest::Client::new()
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=10-19")
            .send()
            .await
            .expect("stream is reachable");
        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            Some("bytes 10-19/100"),
            "a real range request must keep its Content-Range"
        );
    }

    #[tokio::test]
    async fn a_stream_still_being_read_survives_eviction() {
        let server = StreamingServer::start().await.expect("server starts");

        // Filled from the constant rather than a fixed count, so raising the
        // limit does not silently stop testing eviction at all.
        let ids: Vec<String> = (0..MAX_LIVE_STREAMS).map(|i| format!("s{i}")).collect();
        let mut urls = Vec::new();
        for id in &ids {
            urls.push(server.register_stream(id, Bytes::from_static(b"audio"), "audio/mpeg"));
        }
        assert_eq!(server.streams.len(), MAX_LIVE_STREAMS);

        let served = reqwest::get(&urls[0]).await.expect("stream is reachable");
        assert_eq!(served.status(), StatusCode::OK);

        server.register_stream("overflow", Bytes::from_static(b"audio"), "audio/mpeg");

        assert!(
            server.has_stream(&ids[0]),
            "the stream that was just read got evicted — it would 404 mid-track"
        );
        assert!(
            !server.has_stream(&ids[1]),
            "the least recently used one should go"
        );
        assert_eq!(server.streams.len(), MAX_LIVE_STREAMS);
    }

    /// A reader that asks for a byte past the end must be told the range is
    /// unsatisfiable. Answering with the whole body instead hands back the
    /// start of the track under an offset near its end, which the player then
    /// decodes as audio — and, because the reply never runs out, forever.
    #[tokio::test]
    async fn a_range_past_the_end_is_refused_not_answered_with_the_whole_track() {
        let server = StreamingServer::start().await.expect("server starts");
        let body = Bytes::from_static(b"0123456789");
        let url = server.register_stream("eof", body.clone(), "audio/mp4");

        let resp = reqwest::Client::new()
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=10-1033")
            .send()
            .await
            .expect("reachable");

        assert_eq!(resp.status(), StatusCode::RANGE_NOT_SATISFIABLE);
        assert_eq!(
            resp.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            Some("bytes */10")
        );
        assert!(
            resp.bytes().await.expect("body").is_empty(),
            "an unsatisfiable range must not carry audio"
        );
    }

    /// `bytes=-N` means the *last* N bytes — what a container probe sends to read
    /// a trailer. The old parser took `split('-').next()`, which is empty here, so
    /// the request fell through to a whole-body `200`; the hand-rolled parser on
    /// this path was worse still and answered `206 bytes 0-N`, handing back the
    /// head of the file under a label claiming it was the tail.
    #[tokio::test]
    async fn a_suffix_range_serves_the_tail_not_the_whole_body() {
        let server = StreamingServer::start().await.expect("server starts");
        let url = server.register_stream("tail", Bytes::from_static(b"0123456789"), "audio/mp4");

        let resp = reqwest::Client::new()
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=-3")
            .send()
            .await
            .expect("reachable");

        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            Some("bytes 7-9/10")
        );
        assert_eq!(&resp.bytes().await.expect("body")[..], b"789");
    }

    /// A suffix longer than the body is satisfiable — RFC 7233 says it collapses
    /// to the whole representation rather than becoming a 416.
    #[tokio::test]
    async fn a_suffix_longer_than_the_track_collapses_to_the_whole_body() {
        let server = StreamingServer::start().await.expect("server starts");
        let url = server.register_stream("big", Bytes::from_static(b"0123456789"), "audio/mp4");

        let resp = reqwest::Client::new()
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=-4096")
            .send()
            .await
            .expect("reachable");

        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            resp.headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok()),
            Some("bytes 0-9/10")
        );
        assert_eq!(&resp.bytes().await.expect("body")[..], b"0123456789");
    }

    /// A Range header that cannot be understood must be ignored, not guessed at.
    #[tokio::test]
    async fn an_unparseable_range_is_ignored_rather_than_misread() {
        let server = StreamingServer::start().await.expect("server starts");
        let url = server.register_stream("junk", Bytes::from_static(b"0123456789"), "audio/mp4");

        for spec in ["bytes=abc-def", "items=0-4", "bytes=8-2", "bytes=0-4, 6-8"] {
            let resp = reqwest::Client::new()
                .get(&url)
                .header(reqwest::header::RANGE, spec)
                .send()
                .await
                .expect("reachable");

            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "{spec:?} is not a range this server can honour, so the whole body is the answer"
            );
            assert_eq!(&resp.bytes().await.expect("body")[..], b"0123456789");
        }
    }

    #[test]
    fn a_range_resolves_against_the_length_it_is_measured_from() {
        assert_eq!(
            ByteRange::FromStart {
                start: 4,
                end: Some(6)
            }
            .resolve(10),
            Some((4, 6))
        );
        assert_eq!(
            ByteRange::FromStart {
                start: 4,
                end: None
            }
            .resolve(10),
            Some((4, 9)),
            "an open end means the rest of the body"
        );
        assert_eq!(
            ByteRange::FromStart {
                start: 4,
                end: Some(99)
            }
            .resolve(10),
            Some((4, 9)),
            "an end past the body is clamped, not refused"
        );
        assert_eq!(ByteRange::Suffix { len: 3 }.resolve(10), Some((7, 9)));
        assert_eq!(
            ByteRange::Suffix { len: 99 }.resolve(10),
            Some((0, 9)),
            "a suffix longer than the body is the whole body"
        );
        assert_eq!(
            ByteRange::FromStart {
                start: 10,
                end: None
            }
            .resolve(10),
            None,
            "a start at or past the end has no bytes to give"
        );
        assert_eq!(ByteRange::Suffix { len: 0 }.resolve(10), None);
        assert_eq!(
            ByteRange::FromStart {
                start: 0,
                end: None
            }
            .resolve(0),
            None,
            "an empty body cannot satisfy any range"
        );
    }

    #[tokio::test]
    async fn a_range_inside_the_track_still_serves_its_slice() {
        let server = StreamingServer::start().await.expect("server starts");
        let url = server.register_stream("mid", Bytes::from_static(b"0123456789"), "audio/mp4");

        let resp = reqwest::Client::new()
            .get(&url)
            .header(reqwest::header::RANGE, "bytes=4-6")
            .send()
            .await
            .expect("reachable");

        assert_eq!(resp.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(&resp.bytes().await.expect("body")[..], b"456");
    }

    /// Radio used to add an entry per station played and remove none, so nine
    /// stations filled the whole eight-slot pool and the running track's own
    /// stream was evicted out from under it.
    #[tokio::test]
    async fn trimming_a_prefix_keeps_the_newest_and_frees_the_pool() {
        let server = StreamingServer::start().await.expect("server starts");
        server.register_stream("track-current", Bytes::from_static(b"audio"), "audio/mpeg");
        for i in 0..5 {
            server.register_radio(
                &format!("radio-{i}"),
                "https://example.invalid/stream",
                &format!("radiobrowser:{i}"),
                "audio/mpeg",
                BTreeMap::new(),
            );
        }

        server.trim_streams_with_prefix("radio-", 2);

        assert!(
            server.has_stream("radio-4"),
            "the newest station must survive"
        );
        assert!(
            server.has_stream("radio-3"),
            "the one behind it is what the player prefetched"
        );
        assert!(!server.has_stream("radio-0"), "the oldest must go");
        assert!(
            !server.has_stream("radio-2"),
            "and everything before the last two"
        );
        assert!(
            server.has_stream("track-current"),
            "trimming radio must not touch anything else"
        );
    }

    /// An HLS station was registered as `Progressive`, which is removed from the
    /// map the first time it is served — so it could be played exactly once, and
    /// pressing play again 404'd.
    #[tokio::test]
    async fn a_remuxed_station_can_be_requested_more_than_once() {
        let server = StreamingServer::start().await.expect("server starts");
        server.register_remux(
            "radio-hls",
            "https://example.invalid/live.m3u8",
            "audio/aac",
            BTreeMap::new(),
        );

        assert!(server.has_stream("radio-hls"));
        let liveness = server.liveness("radio-hls");
        assert!(liveness.is_live());

        // ffmpeg is not expected to be reachable here; what matters is that the
        // registration survives being served rather than being consumed.
        let _ = reqwest::get(format!("http://127.0.0.1:{}/radio-hls", server.port)).await;
        assert!(
            server.has_stream("radio-hls"),
            "an HLS station must still be playable after it has been played"
        );

        server.remove_stream("radio-hls");
        assert!(
            !liveness.is_live(),
            "the now-playing poller has to stop when its stream does"
        );
    }
}

#[cfg(test)]
mod icy_tests {
    use super::IcyReader;

    /// Builds a Shoutcast body: `meta_int` bytes of audio, a length byte, the padded
    /// metadata, and around again.
    fn icy_body(meta_int: usize, audio_blocks: usize, title: &str) -> (Vec<u8>, Vec<u8>) {
        let mut wire = Vec::new();
        let mut audio = Vec::new();
        for block in 0..audio_blocks {
            let chunk: Vec<u8> = (0..meta_int).map(|i| (block * 7 + i) as u8).collect();
            wire.extend_from_slice(&chunk);
            audio.extend_from_slice(&chunk);
            let meta = format!("StreamTitle='{title}';");
            let padded = meta.len().div_ceil(16) * 16;
            wire.push((padded / 16) as u8);
            wire.extend_from_slice(meta.as_bytes());
            wire.extend(std::iter::repeat_n(0u8, padded - meta.len()));
        }
        (wire, audio)
    }

    /// Chunk boundaries land anywhere — mid-audio, on the length byte, inside the
    /// metadata — so the reader is fed the same body at every chunk size that could
    /// split it.
    #[test]
    fn metadata_is_stripped_at_every_chunk_boundary() {
        let meta_int = 64;
        let (wire, expected_audio) = icy_body(meta_int, 3, "Daft Punk - Get Lucky");

        for size in [1usize, 2, 7, 16, 63, 64, 65, 100, wire.len()] {
            let mut reader = IcyReader::new(meta_int, "uuid".into(), None);
            let mut got = Vec::new();
            for part in wire.chunks(size) {
                got.extend_from_slice(&reader.take(part));
            }
            assert_eq!(got, expected_audio, "chunk size {size} corrupted the audio");
            assert_eq!(
                reader.last_title, "Daft Punk - Get Lucky",
                "chunk size {size}"
            );
        }
    }

    /// An empty metadata block means "nothing changed" and must not clear the title or
    /// shift the audio.
    #[test]
    fn an_empty_metadata_block_is_a_no_op() {
        let meta_int = 8;
        let mut wire = vec![1u8; meta_int];
        wire.push(0);
        wire.extend_from_slice(&[2u8; 8]);
        let mut reader = IcyReader::new(meta_int, "uuid".into(), None);
        let out = reader.take(&wire);
        assert_eq!(out.len(), 16);
        assert_eq!(&out[..8], &[1u8; 8]);
        assert_eq!(&out[8..], &[2u8; 8]);
    }

    /// A station that sends no `icy-metaint` is plain audio and must pass through byte
    /// for byte.
    #[test]
    fn a_stream_without_metadata_passes_through() {
        let mut reader = IcyReader::new(0, "uuid".into(), None);
        let payload: Vec<u8> = (0..255u8).collect();
        assert_eq!(reader.take(&payload).as_ref(), payload.as_slice());
    }
}
