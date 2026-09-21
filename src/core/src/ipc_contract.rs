use serde::{Deserialize, Serialize};

pub fn de_string_or_int<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    use serde::de::{self, Visitor};
    struct StrOrInt;
    impl<'de> Visitor<'de> for StrOrInt {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("string or integer")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    d.deserialize_any(StrOrInt)
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchPlatform {
    #[serde(rename = "spotify")]
    Spotify,
    #[serde(rename = "tidal")]
    Tidal,
    #[serde(rename = "deezer")]
    Deezer,
    #[serde(rename = "qobuz")]
    Qobuz,
    #[serde(rename = "applemusic")]
    AppleMusic,
    #[serde(rename = "youtubemusic")]
    YoutubeMusic,
    #[serde(rename = "youtube")]
    Youtube,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    Track,
    Album,
    Artist,
    Playlist,
    Episode,
    Podcast,
    Show,
    Audiobook,
    Video,
    Song,
    Channel,
    #[serde(rename = "musicvideo")]
    MusicVideo,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetSettingsResponse {
    pub settings: crate::defaults::Settings,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SetSettingsRequest {
    pub settings: crate::defaults::Settings,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct OpResponse {
    pub success: bool,
    pub error: Option<String>,
}
pub type SetSettingsResponse = OpResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct PathResponse {
    pub path: Option<String>,
}
pub type DialogOpenFolderResponse = PathResponse;

pub type DialogOpenFileResponse = PathResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformSearchRequest {
    pub platform: SearchPlatform,
    pub query: String,
    #[serde(rename = "type")]
    pub search_type: SearchType,
    #[serde(default)]
    pub offset: Option<u32>,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchSuggestionsRequest {
    pub platform: SearchPlatform,
    pub query: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformSearchResponse {
    pub results: serde_json::Value,
    pub platform: SearchPlatform,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayMediaRequest {
    pub url: String,
    pub platform: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayMediaResponse {
    pub stream_url: String,
    pub platform: String,
    pub duration_sec: Option<f64>,
    pub media_type: Option<String>, // "audio" | "video"
    #[serde(default)]
    pub is_live: bool,
    /// Audio for a `"video"` response, when it is served separately from the
    /// picture.
    ///
    /// The player decodes audio natively for every media type; the `<video>`
    /// element only draws frames, muted. That needs the two as separate URLs,
    /// because a progressive stream is consumed once and cannot be read by both.
    /// `None` means the audio is inside `stream_url` and the element still owns
    /// it — live HLS, which is remuxed as a single transport stream.
    #[serde(default)]
    pub audio_stream_url: Option<String>,
}

impl PlayMediaResponse {
    pub fn new(stream_url: String, platform: &str, media_type: &str, is_live: bool) -> Self {
        Self {
            stream_url,
            platform: platform.to_string(),
            duration_sec: None,
            media_type: Some(media_type.to_string()),
            is_live,
            audio_stream_url: None,
        }
    }

    /// Video whose audio is served as its own stream for the native player.
    pub fn video_with_audio(stream_url: String, audio_stream_url: String, platform: &str) -> Self {
        Self {
            audio_stream_url: Some(audio_stream_url),
            ..Self::new(stream_url, platform, "video", false)
        }
    }

    pub fn audio(stream_url: String, platform: &str) -> Self {
        Self::new(stream_url, platform, "audio", false)
    }

    pub fn video(stream_url: String, platform: &str) -> Self {
        Self::new(stream_url, platform, "video", false)
    }

    pub fn live_video(stream_url: String, platform: &str) -> Self {
        Self::new(stream_url, platform, "video", true)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SuccessResponse {
    pub success: bool,
}
pub type PauseMediaResponse = SuccessResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifyOAuthLoginResponse {
    pub profile: serde_json::Value,
}

pub type SpotifyOAuthLogoutResponse = SuccessResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifyOAuthStatusResponse {
    pub logged_in: bool,
    pub profile: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifyGetTokenResponse {
    pub token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TidalStartAuthResponse {
    pub code_verifier: String,
    pub auth_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TidalExchangeCodeRequest {
    pub redirect_url: String,
    pub code_verifier: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TidalExchangeCodeResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: u64,
    pub user_id: String,
    pub country_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadMetadata {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub thumbnail: Option<String>,
    pub platform: Option<String>,
    pub quality: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartYtMusicDownloadRequest {
    pub url: String,
    pub output_dir: String,
    /// Shadows the flattened `meta.quality`, which has the same name and type
    /// and lands in the same JSON object. This field is the one that wins.
    pub quality: Option<String>,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartYtVideoDownloadRequest {
    pub url: String,
    pub output_dir: String,
    pub resolution: Option<String>,
    pub format: Option<String>,
    #[serde(default)]
    pub is_generic: bool,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartNativeDownloadRequest {
    pub url: String,
    pub output_dir: String,
    #[serde(default)]
    pub force_redownload: Option<bool>,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}
pub type StartSpotifyDownloadRequest = StartNativeDownloadRequest;

pub type StartAppleDownloadRequest = StartNativeDownloadRequest;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartStreamripDownloadRequest {
    pub url: String,
    pub output_dir: String,
    pub quality: Option<u8>,
    #[serde(default)]
    pub force_redownload: Option<bool>,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}
pub type StartQobuzDownloadRequest = StartStreamripDownloadRequest;

pub type StartDeezerDownloadRequest = StartStreamripDownloadRequest;

pub type StartTidalDownloadRequest = StartStreamripDownloadRequest;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartOrpheusDownloadRequest {
    pub url: String,
    pub output_dir: String,
    pub module_id: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBatchDownloadRequest {
    pub urls: Vec<String>,
    pub platform: String,
    pub output_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDownloadResponse {
    pub download_id: u64,
    pub success: bool,
    pub error: Option<String>,
}

impl StartDownloadResponse {
    pub fn ok(download_id: u64) -> Self {
        Self {
            download_id,
            success: true,
            error: None,
        }
    }

    pub fn failed(download_id: u64, error: impl Into<String>) -> Self {
        Self {
            download_id,
            success: false,
            error: Some(error.into()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelDownloadRequest {
    pub download_id: u64,
}

pub type CancelDownloadResponse = SuccessResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadInfoEvent {
    pub download_id: u64,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgressEvent {
    pub download_id: u64,
    pub percent: f32,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub status: String,
    pub item_index: Option<u32>,
    pub item_total: Option<u32>,
    /// What the service actually served, once it is known. Sent on progress rather
    /// than on the info event so it merges into the existing row instead of
    /// replacing the title and re-announcing the download.
    #[serde(default)]
    pub quality: Option<String>,
}

impl DownloadProgressEvent {
    /// A terminal failure on the progress channel the download row watches.
    /// The `error: ` prefix is what the frontend keys the failed state off.
    pub fn error(download_id: u64, msg: impl std::fmt::Display) -> Self {
        Self {
            download_id,
            percent: 0.0,
            speed: None,
            eta: None,
            status: format!("error: {msg}"),
            item_index: None,
            item_total: None,
            quality: None,
        }
    }

    pub fn completed(download_id: u64) -> Self {
        Self {
            download_id,
            percent: 100.0,
            speed: None,
            eta: None,
            status: "completed".into(),
            item_index: None,
            item_total: None,
            quality: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadFailure {
    pub id: String,
    pub label: String,
    pub reason: String,
}

/// Per-item outcome of a multi-track download, so a run where some tracks failed is
/// never presented as a plain success.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadSummaryEvent {
    pub download_id: u64,
    pub succeeded: u32,
    pub skipped: u32,
    pub failed: u32,
    pub total: u32,
    pub failures: Vec<DownloadFailure>,
    /// Where the files landed, so the UI's "Show in folder" action has a target.
    pub dest_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrapperProbeRequest {
    /// When set, sign in as well as probing; a `code` continues a 2FA challenge.
    #[serde(default)]
    pub sign_in: bool,
    /// When set, drop the daemon's session instead of probing or signing in.
    #[serde(default)]
    pub sign_out: bool,
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WrapperProbeResponse {
    pub reachable: bool,
    pub authenticated: bool,
    pub needs_two_factor: bool,
    /// The daemon's own login-state string, so a failed sign-in cannot read as success.
    pub state: String,
    pub playback_ready: bool,
    pub version: String,
    pub runtime: String,
    pub apple_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowItemInFolderRequest {
    pub path: String,
}

pub type ShowItemInFolderResponse = SuccessResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanDirectoryRequest {
    pub directory: String,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryRecordPlayRequest {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LibraryRadioRequest {
    pub path: String,
}

/// An edit to one file's tags. Every field is optional; an absent one keeps whatever the
/// file already carries, an empty one clears it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryWriteTagsRequest {
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub year: Option<String>,
    pub genre: Option<String>,
    pub track_no: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_no: Option<u32>,
    pub disc_total: Option<u32>,
    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub producer: Option<String>,
    pub label: Option<String>,
    pub copyright: Option<String>,
    pub isrc: Option<String>,
    pub barcode: Option<String>,
    pub comment: Option<String>,
    pub grouping: Option<String>,
    pub bpm: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioSearchRequest {
    pub name: Option<String>,
    pub tag: Option<String>,
    pub country_code: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub bitrate_min: Option<u32>,
    pub order: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Directory ids to ask. Absent means every enabled source.
    pub sources: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioFacetRequest {
    pub kind: String,
    pub limit: Option<u32>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioSuggestRequest {
    pub prefix: String,
    pub limit: Option<u32>,
}

/// A station's qualified `"{source}:{id}"` key.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStationRequest {
    pub key: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioFavoriteRequest {
    pub key: String,
    pub favorite: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioSetSourcesRequest {
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListCreateRequest {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListCreateResponse {
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListIdRequest {
    pub id: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListRenameRequest {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListAddRequest {
    pub id: i64,
    pub keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListRemoveRequest {
    pub id: i64,
    pub position: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioListReorderRequest {
    pub id: i64,
    pub from: i64,
    pub to: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioImportUrlRequest {
    pub url: String,
    pub name: Option<String>,
    /// Headers the stream needs — a broadcaster that checks `Referer` answers
    /// 403 without them, so the probe has to send them too.
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioStationEditRequest {
    pub key: String,
    pub edit: crate::services::radio::StationEdit,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioSetCoverRequest {
    pub key: String,
    /// A base64 JPEG, or `None` to go back to the directory's own icon.
    pub jpeg_base64: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioTestStreamRequest {
    pub url: String,
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
}

/// A pasted stream URL, or a whole `curl` command copied from a browser.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioParseCurlRequest {
    pub input: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioImportPlaylistRequest {
    pub path: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioImportIcecastRequest {
    pub host: String,
}

/// Stations the user picked out of an import preview. Nothing is written to the
/// library until this arrives, so a mistyped host costs nothing.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioSaveStationsRequest {
    pub stations: Vec<crate::services::radio::Station>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioForgetRequest {
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RadioMetadataEvent {
    pub station_uuid: String,
    pub title: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckDepsResponse {
    pub ffmpeg: bool,
    pub python: bool,
    pub yt_dlp: bool,
    pub votify: bool,
    pub gamdl: bool,
    pub bento4: bool,
    pub is_sandboxed: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryScanProgressEvent {
    pub directory: String,
    pub done: u64,
    pub total: u64,
    pub current_path: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryChangedEvent {
    pub directory: String,
    pub added: u64,
    pub updated: u64,
    pub removed: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct LibraryQueryRequest {
    pub kind: Option<String>,
    pub offset: Option<i64>,
    pub limit: Option<i64>,
    pub sort: Option<String>,
    pub search: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryAlbumRequest {
    pub album_key: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryCoverUrlRequest {
    pub cover_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryWatchRequest {
    pub roots: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryArtistRequest {
    pub key: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistCreateRequest {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistCreateResponse {
    pub id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistRenameRequest {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistIdRequest {
    pub id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistAddTracksRequest {
    pub id: i64,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistRemoveTrackRequest {
    pub id: i64,
    pub position: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistReorderRequest {
    pub id: i64,
    pub from: i64,
    pub to: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistImportRequest {
    pub source: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryPlaylistExportRequest {
    pub id: i64,
    pub dest: String,
}

pub type ClearDatabaseResponse = SuccessResponse;

#[derive(Debug, Serialize, Deserialize)]
pub struct GetVersionResponse {
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckUpdatesResponse {
    pub update_available: bool,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub release_notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallDepRequest {
    pub dependency: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallDepResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GetDependencyVersionsResponse {
    pub versions: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallationProgressEvent {
    pub dependency: String,
    pub percent: u8,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendLogEvent {
    pub level: String,
    pub source: String,
    pub title: String,
    pub message: String,
    pub timestamp: String,
}

impl BackendLogEvent {
    /// Build a log event with the current timestamp. Every emitter stamped this
    /// by hand, which is why the field order and the timestamp format had to be
    /// repeated at each call site.
    pub fn new(level: &str, source: &str, title: &str, message: impl Into<String>) -> Self {
        Self {
            level: level.to_string(),
            source: source.to_string(),
            title: title.to_string(),
            message: message.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }

    pub fn error(source: &str, title: &str, message: impl Into<String>) -> Self {
        Self::new("error", source, title, message)
    }

    pub fn info(source: &str, title: &str, message: impl Into<String>) -> Self {
        Self::new("info", source, title, message)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamReadyEvent {
    pub stream_url: String,
    pub platform: String,
    pub duration_sec: Option<f64>,
    pub media_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerPositionEvent {
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayerStateEvent {
    pub playing: bool,
    pub ended: bool,
    pub buffering: bool,
}

/// The decode thread has finished handing over to the track that was queued
/// behind the current one.
///
/// A crossfade is started by an `mpsc::send` that returns immediately, so the
/// frontend has no other way to learn when the swap actually happened — it used
/// to advance the queue at fade *start*, leaving MPRIS advertising the next
/// track against the previous one's clock for the whole fade.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PlayerTrackChangedEvent {
    /// Seconds of the outgoing track that had played when the swap completed,
    /// for logs — the position is reset to zero by the same handover.
    pub at_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerErrorEvent {
    pub message: String,
    /// Set when the stream opened but the player could not decode it. The
    /// frontend turns it into a report a user can file as-is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undecodable: Option<UndecodableStream>,
}

/// Everything the decoder knows about a stream it gave up on.
///
/// Fields rather than a rendered message: the frontend adds the one fact the
/// backend never has — which service and track the URL came from — and the
/// report is only worth filing if it carries both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UndecodableStream {
    /// The URL the player was handed, for matching back to the queued track.
    pub url: String,
    /// The demuxer's or decoder's own words, verbatim.
    pub detail: String,
    /// Sniffed from the first bytes, so it is known even when probing failed.
    pub container: Option<String>,
    pub codec: Option<String>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u32>,
    /// `MediaHarbor <version> · <os> <arch>`.
    pub app: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSpectrumEvent {
    pub bars: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerLoadRequest {
    pub url: String,
    pub mime_type: Option<String>,
    /// The local file this stream came from, when there is one. The ReplayGain the
    /// scanner read is looked up from it rather than threaded through every view that
    /// can start playback.
    #[serde(default)]
    pub source_path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerCrossfadeRequest {
    pub url: String,
    pub mime_type: Option<String>,
    pub duration_secs: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerSeekRequest {
    pub position_secs: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerVolumeRequest {
    pub volume: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerMutedRequest {
    pub muted: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerSpectrumRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayerDevicesResponse {
    pub devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppErrorEvent {
    pub message: String,
    pub context: Option<String>,
    pub needs_auth: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStdinPromptEvent {
    pub download_id: u64,
    pub prompt_lines: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLyricsRequest {
    pub url: String,
    pub platform: String,
    pub title: String,
    pub artist: String,
    pub duration: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetLyricsResponse {
    pub synced: Option<String>,
    pub plain: Option<String>,
    pub word_synced: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendProcessStdinRequest {
    pub download_id: u64,
    pub input: String,
}

pub type SendProcessStdinResponse = SuccessResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrpheusModuleStatus {
    pub id: String,
    pub label: String,
    pub installed: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckOrpheusDepsResponse {
    pub orpheus_installed: bool,
    pub modules: Vec<OrpheusModuleStatus>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOrpheusModuleRequest {
    pub module_id: String,
    pub custom_url: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InstallOrpheusModuleResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryQueryRequest {
    pub platform: String,
    pub kind: String,
    #[serde(default)]
    pub offset: u32,
    #[serde(default)]
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryIdRequest {
    pub platform: String,
    #[serde(deserialize_with = "de_string_or_int")]
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePlatformRequest {
    pub platform: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryCoverRequest {
    pub platform: String,
    #[serde(deserialize_with = "de_string_or_int")]
    pub id: String,
    pub jpeg_base64: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryPlaybackRequest {
    pub platform: String,
    #[serde(deserialize_with = "de_string_or_int")]
    pub id: String,
    #[serde(default)]
    pub duration_secs: u64,
    #[serde(default)]
    pub context_uri: Option<String>,
    #[serde(default)]
    pub track_index: Option<u64>,
}

/// Save/unsave a track or album, follow/unfollow an artist or playlist.
///
/// The action is data, not a command name: `kind` and `saved` pick which of the
/// eight combinations to run, and `SavedStateChangedEvent` echoes `kind` back.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibrarySetSavedRequest {
    pub platform: String,
    pub kind: crate::services::common::library::SaveKind,
    /// `true` saves or follows; `false` reverses it.
    pub saved: bool,
    pub ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibrarySavedStateRequest {
    pub platform: String,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibrarySavedStateRefreshRequest {
    pub platform: String,
    pub kinds: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryPlaylistCreateRequest {
    pub platform: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub is_collaborative: bool,
    #[serde(default)]
    pub initial_track_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryPlaylistRenameRequest {
    pub platform: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_public: Option<bool>,
    #[serde(default)]
    pub is_collaborative: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryPlaylistIdRequest {
    pub platform: String,
    pub id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryPlaylistMutateTracksRequest {
    pub platform: String,
    pub id: String,
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub positions: Option<Vec<u32>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryPlaylistReorderRequest {
    pub platform: String,
    pub id: String,
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryRadioRequest {
    pub platform: String,
    pub seed_kind: String,
    pub seed_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceLibraryRadioContinueRequest {
    pub platform: String,
    pub continuation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedStateChangedEvent {
    pub platform: String,
    pub kind: String,
    #[serde(default)]
    pub added: Vec<String>,
    #[serde(default)]
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePlaylistChangedEvent {
    pub platform: String,
    pub playlist_id: String,
    pub change: String,
    #[serde(default)]
    pub snapshot_id: Option<String>,
}
