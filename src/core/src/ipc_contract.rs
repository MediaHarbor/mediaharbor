use serde::{Deserialize, Serialize};

pub fn de_string_or_int<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    use serde::de::{self, Visitor};
    struct StrOrInt;
    impl<'de> Visitor<'de> for StrOrInt {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("string or integer")
        }
        fn visit_str<E: de::Error>(self, v: &str) -> Result<String, E> { Ok(v.to_string()) }
        fn visit_string<E: de::Error>(self, v: String) -> Result<String, E> { Ok(v) }
        fn visit_i64<E: de::Error>(self, v: i64) -> Result<String, E> { Ok(v.to_string()) }
        fn visit_u64<E: de::Error>(self, v: u64) -> Result<String, E> { Ok(v.to_string()) }
    }
    d.deserialize_any(StrOrInt)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
pub struct SetSettingsResponse {
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DialogOpenFolderResponse {
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DialogOpenFileResponse {
    pub path: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PerformSearchRequest {
    pub platform: SearchPlatform,
    pub query: String,
    #[serde(rename = "type")]
    pub search_type: SearchType,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PauseMediaResponse {
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifyOAuthLoginResponse {
    pub profile: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SpotifyOAuthLogoutResponse {
    pub success: bool,
}

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArtistDetailsRequest {
    #[serde(deserialize_with = "de_string_or_int")]
    pub artist_id: String,
    pub platform: SearchPlatform,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetAlbumDetailsRequest {
    #[serde(deserialize_with = "de_string_or_int")]
    pub album_id: String,
    pub platform: SearchPlatform,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetPlaylistDetailsRequest {
    #[serde(deserialize_with = "de_string_or_int")]
    pub playlist_id: String,
    pub platform: SearchPlatform,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MediaDetailsResponse {
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct DownloadMetadata {
    pub title:     Option<String>,
    pub artist:    Option<String>,
    pub album:     Option<String>,
    pub thumbnail: Option<String>,
    pub platform:  Option<String>,
    pub quality:   Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartYtMusicDownloadRequest {
    pub url: String,
    pub output_dir: String,
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
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSpotifyDownloadRequest {
    pub url: String,
    pub output_dir: String,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartAppleDownloadRequest {
    pub url: String,
    pub output_dir: String,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartQobuzDownloadRequest {
    pub url: String,
    pub output_dir: String,
    pub quality: Option<u8>,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartDeezerDownloadRequest {
    pub url: String,
    pub output_dir: String,
    pub quality: Option<u8>,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartTidalDownloadRequest {
    pub url: String,
    pub output_dir: String,
    #[serde(flatten)]
    pub meta: DownloadMetadata,
}

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelDownloadRequest {
    pub download_id: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CancelDownloadResponse {
    pub success: bool,
}

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowItemInFolderRequest {
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShowItemInFolderResponse {
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanDirectoryRequest {
    pub directory: String,
    pub force: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClearDatabaseResponse {
    pub success: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckDepsResponse {
    pub ffmpeg: bool,
    pub python: bool,
    pub git: bool,
    pub yt_dlp: bool,
    pub votify: bool,
    pub gamdl: bool,
    pub bento4: bool,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamReadyEvent {
    pub stream_url: String,
    pub platform: String,
    pub duration_sec: Option<f64>,
    pub media_type: Option<String>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SendProcessStdinResponse {
    pub success: bool,
}

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
