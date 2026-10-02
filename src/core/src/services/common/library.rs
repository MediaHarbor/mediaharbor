use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServicePlatform {
    Spotify,
    Tidal,
    Qobuz,
    Deezer,
    AppleMusic,
    YtMusic,
    Youtube,
}

impl ServicePlatform {
    /// Every service, so a sweep over "all of them" cannot silently omit one.
    pub const ALL: &'static [ServicePlatform] = &[
        ServicePlatform::Spotify,
        ServicePlatform::Tidal,
        ServicePlatform::Qobuz,
        ServicePlatform::Deezer,
        ServicePlatform::AppleMusic,
        ServicePlatform::YtMusic,
        ServicePlatform::Youtube,
    ];

    /// Services whose credentials are a cookies file. Checking them is local, so the
    /// cheap sweep covers exactly these.
    pub const COOKIE_BACKED: &'static [ServicePlatform] = &[
        ServicePlatform::Spotify,
        ServicePlatform::YtMusic,
        ServicePlatform::Youtube,
        ServicePlatform::AppleMusic,
    ];

    /// The rest, each of which costs a network round trip to verify.
    pub const TOKEN_BACKED: &'static [ServicePlatform] = &[
        ServicePlatform::Tidal,
        ServicePlatform::Qobuz,
        ServicePlatform::Deezer,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ServicePlatform::Spotify => "spotify",
            ServicePlatform::Tidal => "tidal",
            ServicePlatform::Qobuz => "qobuz",
            ServicePlatform::Deezer => "deezer",
            ServicePlatform::AppleMusic => "apple_music",
            ServicePlatform::YtMusic => "ytmusic",
            ServicePlatform::Youtube => "youtube",
        }
    }
}

/// Fold a platform string to one spelling before matching on it: lowercase, with
/// `-` and spaces as `_`. So `appleMusic`, `apple-music` and `Apple Music` all reach
/// the same arm, and a new caller cannot accidentally support a narrower set.
pub fn normalize_platform_key(s: &str) -> String {
    s.to_ascii_lowercase().replace(['-', ' '], "_")
}

impl FromStr for ServicePlatform {
    type Err = MhError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let key = normalize_platform_key(s);
        Ok(match key.as_str() {
            "spotify" => Self::Spotify,
            "tidal" => Self::Tidal,
            "qobuz" => Self::Qobuz,
            "deezer" => Self::Deezer,
            "apple_music" | "applemusic" => Self::AppleMusic,
            "ytmusic" | "yt_music" | "youtube_music" | "youtubemusic" => Self::YtMusic,
            "youtube" | "yt" => Self::Youtube,
            other => return Err(MhError::Other(format!("unknown service platform: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    Albums,
    Tracks,
    Artists,
    Playlists,
    Videos,
}

impl FromStr for ServiceKind {
    type Err = MhError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "albums" => Self::Albums,
            "tracks" => Self::Tracks,
            "artists" => Self::Artists,
            "playlists" => Self::Playlists,
            "videos" => Self::Videos,
            other => return Err(MhError::Other(format!("unknown service kind: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Page {
    #[serde(default)]
    pub offset: u32,
    #[serde(default = "default_limit")]
    pub limit: u32,
}
fn default_limit() -> u32 {
    25
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceLibraryPage<T> {
    pub items: Vec<T>,
    pub total: Option<u64>,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

impl<T> ServiceLibraryPage<T> {
    /// A page whose total the service reported (or did not).
    pub fn of(items: Vec<T>, total: Option<u64>) -> Self {
        Self {
            items,
            total,
            next_cursor: None,
        }
    }

    /// A page that is the whole list: the total is what was collected.
    pub fn counted(items: Vec<T>) -> Self {
        Self {
            total: Some(items.len() as u64),
            items,
            next_cursor: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RecommendationItem {
    Album(LibraryAlbumDto),
    Track(LibraryTrackDto),
    Artist(LibraryArtistDto),
    Playlist(LibraryPlaylistDto),
    Mix {
        id: String,
        title: String,
        subtitle: Option<String>,
        cover_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
    },
    Episode {
        id: String,
        title: String,
        show_name: Option<String>,
        cover_id: Option<String>,
        duration_ms: Option<u64>,
    },
    /// A browse/navigation tile (genre, mood, decade) — not playable content.
    /// `api_path` points at the category's own page (another `pages/*` response).
    PageLink {
        api_path: String,
        title: String,
        icon: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecommendationCategory {
    Hero,
    DailyMix,
    Discovery,
    RecentlyPlayed,
    NewReleases,
    Charts,
    Genre,
    Editorial,
    Stations,
    #[default]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationShelf {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    #[serde(default)]
    pub category: RecommendationCategory,
    pub items: Vec<RecommendationItem>,
}

impl RecommendationShelf {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        category: RecommendationCategory,
        items: Vec<RecommendationItem>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            category,
            items,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RecommendationsPage {
    pub shelves: Vec<RecommendationShelf>,
}

impl RecommendationItem {
    pub fn dedupe_key(&self) -> String {
        match self {
            RecommendationItem::Album(a) => format!("album:{}", a.album_key),
            RecommendationItem::Track(t) => format!("track:{}", t.path),
            RecommendationItem::Artist(a) => format!("artist:{}", a.key),
            RecommendationItem::Playlist(p) => {
                format!(
                    "playlist:{}",
                    p.service_id.clone().unwrap_or(p.id.to_string())
                )
            }
            RecommendationItem::Mix { id, .. } => format!("mix:{id}"),
            RecommendationItem::Episode { id, .. } => format!("episode:{id}"),
            RecommendationItem::PageLink { api_path, .. } => format!("page:{api_path}"),
        }
    }
}

pub fn merge_duplicate_shelves(shelves: Vec<RecommendationShelf>) -> Vec<RecommendationShelf> {
    let mut order: Vec<String> = Vec::new();
    let mut merged: HashMap<String, RecommendationShelf> = HashMap::new();
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();

    for shelf in shelves {
        let key = shelf.title.trim().to_ascii_lowercase();
        if key.is_empty() {
            let unique = format!("\u{0}{}", order.len());
            order.push(unique.clone());
            seen.insert(unique.clone(), HashSet::new());
            merged.insert(unique, shelf);
            continue;
        }
        match merged.get_mut(&key) {
            None => {
                let mut keys = HashSet::new();
                for it in &shelf.items {
                    keys.insert(it.dedupe_key());
                }
                order.push(key.clone());
                seen.insert(key.clone(), keys);
                merged.insert(key, shelf);
            }
            Some(existing) => {
                let keys = seen.entry(key).or_default();
                for it in shelf.items {
                    if keys.insert(it.dedupe_key()) {
                        existing.items.push(it);
                    }
                }
            }
        }
    }

    order
        .into_iter()
        .filter_map(|k| merged.remove(&k))
        .filter(|s| !s.items.is_empty())
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub struct FeedTrim {
    pub head_keep: usize,
    pub min_tail_items: usize,
    pub max_shelves: usize,
}

impl Default for FeedTrim {
    fn default() -> Self {
        Self {
            head_keep: 8,
            min_tail_items: 3,
            max_shelves: 20,
        }
    }
}

pub fn trim_feed_tail(
    shelves: Vec<RecommendationShelf>,
    cfg: FeedTrim,
) -> Vec<RecommendationShelf> {
    if shelves.len() <= cfg.head_keep {
        return shelves;
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<RecommendationShelf> = Vec::with_capacity(shelves.len());

    for (idx, shelf) in shelves.into_iter().enumerate() {
        if idx < cfg.head_keep {
            seen.extend(shelf.items.iter().map(|i| i.dedupe_key()));
            out.push(shelf);
            continue;
        }
        if out.len() >= cfg.max_shelves {
            break;
        }
        if shelf.items.len() < cfg.min_tail_items {
            continue;
        }
        if shelf.items.iter().all(|i| seen.contains(&i.dedupe_key())) {
            continue;
        }
        seen.extend(shelf.items.iter().map(|i| i.dedupe_key()));
        out.push(shelf);
    }

    out
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ServiceCapabilities {
    pub albums: bool,
    pub tracks: bool,
    pub artists: bool,
    pub playlists: bool,
    pub videos: bool,
    pub recommendations: bool,
    #[serde(default)]
    pub followers: bool,
    #[serde(default)]
    pub activity_feed: bool,
    #[serde(default)]
    pub episode_bookmarks: bool,
    #[serde(default)]
    pub mutations: MutationCapabilities,
}

impl ServiceCapabilities {
    pub const fn all() -> Self {
        Self {
            albums: true,
            tracks: true,
            artists: true,
            playlists: true,
            videos: true,
            recommendations: true,
            followers: false,
            activity_feed: false,
            episode_bookmarks: false,
            mutations: MutationCapabilities::none(),
        }
    }
    pub const fn audio_only() -> Self {
        Self {
            albums: true,
            tracks: true,
            artists: true,
            playlists: true,
            videos: false,
            recommendations: true,
            followers: false,
            activity_feed: false,
            episode_bookmarks: false,
            mutations: MutationCapabilities::none(),
        }
    }
    pub const fn videos_and_playlists() -> Self {
        Self {
            albums: false,
            tracks: false,
            artists: false,
            playlists: true,
            videos: true,
            recommendations: true,
            followers: false,
            activity_feed: false,
            episode_bookmarks: false,
            mutations: MutationCapabilities::none(),
        }
    }
    pub const fn with_mutations(self, mutations: MutationCapabilities) -> Self {
        Self { mutations, ..self }
    }
    pub const fn with_followers(self) -> Self {
        Self {
            followers: true,
            ..self
        }
    }
    pub const fn with_activity_feed(self) -> Self {
        Self {
            activity_feed: true,
            ..self
        }
    }
    pub const fn with_episode_bookmarks(self) -> Self {
        Self {
            episode_bookmarks: true,
            ..self
        }
    }
}

/// One normalized entry in a service activity feed, so a single view renders every
/// service's feed regardless of the upstream payload shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityFeedItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub cover_id: Option<String>,
    pub service_id: Option<String>,
    pub occurred_at: Option<String>,
    pub seen: bool,
}

pub fn activity_feed_page(items: Vec<ActivityFeedItem>) -> serde_json::Value {
    serde_json::json!({ "activities": items })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct MutationCapabilities {
    #[serde(default)]
    pub save_tracks: bool,
    #[serde(default)]
    pub save_albums: bool,
    #[serde(default)]
    pub follow_artists: bool,
    #[serde(default)]
    pub follow_playlists: bool,
    #[serde(default)]
    pub create_playlists: bool,
    #[serde(default)]
    pub edit_playlists: bool,
    #[serde(default)]
    pub reorder_playlists: bool,
    #[serde(default)]
    pub radio: bool,
}

impl MutationCapabilities {
    pub const fn none() -> Self {
        Self {
            save_tracks: false,
            save_albums: false,
            follow_artists: false,
            follow_playlists: false,
            create_playlists: false,
            edit_playlists: false,
            reorder_playlists: false,
            radio: false,
        }
    }
    pub const fn full_audio() -> Self {
        Self {
            save_tracks: true,
            save_albums: true,
            follow_artists: true,
            follow_playlists: true,
            create_playlists: true,
            edit_playlists: true,
            reorder_playlists: true,
            radio: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveKind {
    Track,
    Album,
    Artist,
    Playlist,
}

impl SaveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SaveKind::Track => "track",
            SaveKind::Album => "album",
            SaveKind::Artist => "artist",
            SaveKind::Playlist => "playlist",
        }
    }
}

/// The bulk-favourite collections a service exposes. Wider than [`SaveKind`]
/// because Qobuz also has labels, which never reach the saved-state DB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavKind {
    Track,
    Album,
    Artist,
    Label,
}

impl FavKind {
    pub fn as_str(self) -> &'static str {
        match self {
            FavKind::Track => "track",
            FavKind::Album => "album",
            FavKind::Artist => "artist",
            FavKind::Label => "label",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FavOp {
    Add,
    Remove,
}

impl SavedIdSet {
    /// Puts `ids` in the field for `kind`, so callers match on kind once (to pick
    /// the wire key) instead of twice.
    pub fn set(&mut self, kind: SaveKind, ids: Vec<String>) {
        match kind {
            SaveKind::Track => self.track_ids = ids,
            SaveKind::Album => self.album_ids = ids,
            SaveKind::Artist => self.artist_ids = ids,
            SaveKind::Playlist => self.playlist_ids = ids,
        }
    }

    /// As [`SavedIdSet::set`], plus the per-item library ids that Apple Music
    /// returns alongside the catalog ids. Only tracks and albums carry them.
    pub fn set_with_library_ids(
        &mut self,
        kind: SaveKind,
        ids: Vec<String>,
        library_ids: Vec<Option<String>>,
    ) {
        self.set(kind, ids);
        match kind {
            SaveKind::Track => self.track_library_ids = Some(library_ids),
            SaveKind::Album => self.album_library_ids = Some(library_ids),
            SaveKind::Artist | SaveKind::Playlist => {}
        }
    }

    /// The ids recorded for `kind`, paired with their library ids where the
    /// service supplied them.
    pub fn for_kind(&self, kind: SaveKind) -> (&[String], &[Option<String>]) {
        let none: &[Option<String>] = &[];
        match kind {
            SaveKind::Track => (
                &self.track_ids,
                self.track_library_ids.as_deref().unwrap_or(none),
            ),
            SaveKind::Album => (
                &self.album_ids,
                self.album_library_ids.as_deref().unwrap_or(none),
            ),
            SaveKind::Artist => (&self.artist_ids, none),
            SaveKind::Playlist => (&self.playlist_ids, none),
        }
    }
}

impl FromStr for SaveKind {
    type Err = MhError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "track" | "tracks" => Self::Track,
            "album" | "albums" => Self::Album,
            "artist" | "artists" => Self::Artist,
            "playlist" | "playlists" => Self::Playlist,
            other => return Err(MhError::Other(format!("unknown save kind: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RadioSeedKind {
    Track,
    Album,
    Artist,
    Playlist,
}

impl FromStr for RadioSeedKind {
    type Err = MhError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "track" => Self::Track,
            "album" => Self::Album,
            "artist" => Self::Artist,
            "playlist" => Self::Playlist,
            other => return Err(MhError::Other(format!("unknown radio seed kind: {other}"))),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadioResult {
    pub seed_kind: RadioSeedKind,
    pub seed_id: String,
    pub title: String,
    pub tracks: Vec<LibraryTrackDto>,
    #[serde(default)]
    pub station_id: Option<String>,
    #[serde(default)]
    pub video_urls: std::collections::HashMap<String, String>,
    /// Opaque token for the next page of an endless station. `None` means the service
    /// handed over everything it intends to.
    #[serde(default)]
    pub continuation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistCreateInput {
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlaylistMutateResult {
    pub playlist_id: String,
    #[serde(default)]
    pub library_id: Option<String>,
    #[serde(default)]
    pub snapshot_id: Option<String>,
}

impl PlaylistMutateResult {
    pub fn id(playlist_id: impl Into<String>) -> Self {
        Self {
            playlist_id: playlist_id.into(),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedIdSet {
    #[serde(default)]
    pub track_ids: Vec<String>,
    #[serde(default)]
    pub album_ids: Vec<String>,
    #[serde(default)]
    pub artist_ids: Vec<String>,
    #[serde(default)]
    pub playlist_ids: Vec<String>,
    #[serde(default)]
    pub track_library_ids: Option<Vec<Option<String>>>,
    #[serde(default)]
    pub album_library_ids: Option<Vec<Option<String>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OwnedPlaylistRow {
    pub platform: String,
    pub service_id: String,
    pub name: String,
    #[serde(default)]
    pub cover_id: Option<String>,
    #[serde(default)]
    pub track_count: i64,
    pub updated_at: i64,
}

#[async_trait]
pub trait ServiceLibrary: Send + Sync {
    fn platform(&self) -> ServicePlatform;
    fn capabilities(&self) -> ServiceCapabilities;

    /// This same object, viewed as its mutation surface, or `None` for a read-only
    /// service.
    ///
    /// A service opts in by overriding. `build_mutations` was previously a second
    /// seven-arm match that rebuilt exactly what `build` had just built — two lists
    /// that had to agree, with nothing making them.
    fn as_mutations(self: Arc<Self>) -> Option<Arc<dyn ServiceLibraryMutations>> {
        None
    }

    async fn albums(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        Ok(empty())
    }
    async fn tracks(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        Ok(empty())
    }
    async fn artists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        Ok(empty())
    }
    async fn playlists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        Ok(empty())
    }
    async fn videos(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        Ok(empty())
    }
    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        Ok(RecommendationsPage::default())
    }

    /// Curated Explore/browse surface (genres, editorial). Default: empty (unsupported).
    async fn explore(&self) -> MhResult<RecommendationsPage> {
        Ok(RecommendationsPage::default())
    }

    /// Drill into a browse category (a page path from an explore PageLink, e.g.
    /// "pages/genre_hip_hop") -> that category's own shelves. Default: empty.
    async fn explore_page(&self, _path: &str) -> MhResult<RecommendationsPage> {
        Ok(RecommendationsPage::default())
    }

    /// "New from artists you follow" activity feed, as `{ "activities": [ActivityFeedItem] }`.
    /// Default: empty (unsupported).
    async fn activity_feed(&self, _limit: u32) -> MhResult<serde_json::Value> {
        Ok(activity_feed_page(Vec::new()))
    }

    /// Editorial album page (credits, related). Default: empty (unsupported).
    async fn album_page(&self, _id: &str) -> MhResult<serde_json::Value> {
        Ok(serde_json::json!({ "shelves": [], "raw": null }))
    }

    /// Rich artist page (biography, top tracks, discography, videos, similar,
    /// credits, appears-on). Default: empty (unsupported).
    async fn artist_page(&self, _id: &str) -> MhResult<serde_json::Value> {
        Ok(serde_json::json!({ "shelves": [], "raw": null }))
    }

    /// Report a played track to the service (scrobble / listening history).
    /// Gated on a per-service opt-in inside the impl. Default: no-op.
    /// `context_uri` / `track_index` identify the album/playlist the play came from.
    async fn report_playback(
        &self,
        _service_track_id: &str,
        _duration_secs: u64,
        _context_uri: Option<&str>,
        _track_index: Option<u64>,
    ) -> MhResult<()> {
        Ok(())
    }

    /// Looping background video (Spotify Canvas) for a track, if one exists.
    /// Returns a raw CDN url; the caller proxies it. Default: none.
    async fn canvas_url(&self, _service_track_id: &str) -> MhResult<Option<String>> {
        Ok(None)
    }

    /// Set a custom playlist cover from a square JPEG (base64). Default: unsupported.
    async fn set_playlist_cover(&self, _playlist_id: &str, _jpeg_base64: &str) -> MhResult<()> {
        Err(MhError::Unsupported(
            "Custom playlist covers are not supported for this service".into(),
        ))
    }

    /// Time-synced transcript for a podcast episode, if one exists.
    /// Returns `{ episodeName, showName, lines: [{ startMs, speaker?, text }] }` or null.
    async fn podcast_transcript(&self, _episode_id: &str) -> MhResult<serde_json::Value> {
        Ok(serde_json::Value::Null)
    }

    /// Saved podcast-episode resume positions (read-only). Default: empty.
    async fn episode_bookmarks(&self) -> MhResult<Vec<LibraryTrackDto>> {
        Ok(Vec::new())
    }

    /// Users this account follows / is followed by. Default: empty (unsupported).
    async fn followers(&self) -> MhResult<Vec<LibraryArtistDto>> {
        Ok(Vec::new())
    }
    async fn following(&self) -> MhResult<Vec<LibraryArtistDto>> {
        Ok(Vec::new())
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail>;
    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail>;
    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail>;

    async fn resolve_cover(&self, key: &str) -> MhResult<String> {
        cover_proxy_url(self.platform(), key)
            .ok_or_else(|| MhError::Other(format!("no cover url for key: {key}")))
    }
}

fn empty<T>() -> ServiceLibraryPage<T> {
    ServiceLibraryPage::of(Vec::new(), Some(0))
}

pub fn cover_id(platform: ServicePlatform, key: &str) -> String {
    format!("{}:{}", platform.as_str(), key)
}

pub fn stable_hash_i64(s: &str) -> i64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish() as i64
}

pub fn json_id(v: &serde_json::Value) -> Option<String> {
    v.as_i64()
        .map(|i| i.to_string())
        .or_else(|| v.as_str().map(str::to_string))
}

pub fn json_id_at(node: &serde_json::Value, key: &str) -> Option<String> {
    json_id(node.get(key)?)
}

pub fn json_id_ptr(node: &serde_json::Value, ptr: &str) -> Option<String> {
    json_id(node.pointer(ptr)?)
}

/// Resolves one `path` against `node`. A leading `/` means a JSON Pointer
/// (`/artist/name`), anything else is a direct key lookup (`title`).
fn at<'a>(node: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    if path.starts_with('/') {
        node.pointer(path)
    } else {
        node.get(path)
    }
}

/// First `paths` entry that resolves to something. Service payloads spell the
/// same field several ways across endpoints, so callers pass every known
/// spelling in preference order rather than writing an `.or_else` chain.
pub fn any_at<'a>(node: &'a serde_json::Value, paths: &[&str]) -> Option<&'a serde_json::Value> {
    paths.iter().find_map(|p| at(node, p))
}

pub fn str_at<'a>(node: &'a serde_json::Value, paths: &[&str]) -> Option<&'a str> {
    paths.iter().find_map(|p| at(node, p)?.as_str())
}

/// Like [`str_at`] but treats a present-but-empty string as absent.
pub fn nonempty_at(node: &serde_json::Value, paths: &[&str]) -> Option<String> {
    str_at(node, paths)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `""` when nothing matches — for DTO fields that are plain `String`.
pub fn string_at(node: &serde_json::Value, paths: &[&str]) -> String {
    str_at(node, paths).unwrap_or_default().to_string()
}

pub fn i64_at(node: &serde_json::Value, paths: &[&str]) -> Option<i64> {
    paths.iter().find_map(|p| {
        let v = at(node, p)?;
        v.as_i64().or_else(|| v.as_str()?.parse().ok())
    })
}

pub fn f64_at(node: &serde_json::Value, paths: &[&str]) -> Option<f64> {
    paths.iter().find_map(|p| {
        let v = at(node, p)?;
        v.as_f64().or_else(|| v.as_str()?.parse().ok())
    })
}

pub fn u32_at(node: &serde_json::Value, paths: &[&str]) -> Option<u32> {
    i64_at(node, paths).and_then(|n| u32::try_from(n).ok())
}

pub fn bool_at(node: &serde_json::Value, paths: &[&str]) -> Option<bool> {
    paths.iter().find_map(|p| at(node, p)?.as_bool())
}

/// An id that may arrive as a string or a bare integer.
pub fn id_at(node: &serde_json::Value, paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|p| json_id(at(node, p)?))
}

/// Leading 4 digits of a date field — services return `2019`, `2019-04-26` and
/// `2019-04-26T00:00:00Z` for the same concept.
pub fn year4_at(node: &serde_json::Value, paths: &[&str]) -> Option<String> {
    let raw = str_at(node, paths)
        .map(str::to_string)
        .or_else(|| i64_at(node, paths).map(|n| n.to_string()))?;
    let year: String = raw.chars().take(4).collect();
    (year.len() == 4 && year.chars().all(|c| c.is_ascii_digit())).then_some(year)
}

pub async fn paginate_all<T, F, Fut>(limit: u32, mut pull: F) -> MhResult<Vec<T>>
where
    F: FnMut(Page) -> Fut,
    Fut: std::future::Future<Output = MhResult<ServiceLibraryPage<T>>>,
{
    let mut out: Vec<T> = Vec::new();
    let mut offset = 0u32;
    for _ in 0..200 {
        let page = pull(Page { offset, limit }).await?;
        let count = page.items.len() as u32;
        if count == 0 {
            break;
        }
        offset += count;
        out.extend(page.items);
        if let Some(total) = page.total {
            if u64::from(offset) >= total {
                break;
            }
        }
        if count < limit {
            break;
        }
    }
    Ok(out)
}

/// `H:M:S`, `M:S` or a bare seconds count, in seconds.
///
/// Components parse as `f64` so a fractional seconds field — which TTML lyric
/// timings carry, e.g. `00:01:23.456` — survives. Strict: any unparseable
/// component makes the whole string `None`.
pub fn parse_hms_seconds(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.trim().split(':').collect();
    let nums: Result<Vec<f64>, _> = parts.iter().map(|p| p.trim().parse::<f64>()).collect();
    let nums = nums.ok()?;
    match nums.len() {
        1 => Some(nums[0]),
        2 => Some(nums[0] * 60.0 + nums[1]),
        3 => Some(nums[0] * 3600.0 + nums[1] * 60.0 + nums[2]),
        _ => None,
    }
}

pub fn read_cookies(path: &str) -> MhResult<std::collections::HashMap<String, String>> {
    let content = std::fs::read_to_string(path).map_err(MhError::Io)?;
    let mut out = std::collections::HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 7 {
            continue;
        }
        let domain = parts[0];
        if !(domain.contains("youtube.com") || domain.contains("google.com")) {
            continue;
        }
        out.insert(parts[5].to_string(), parts[6].trim().to_string());
    }
    Ok(out)
}

pub fn collect_set_cookies(headers: &reqwest::header::HeaderMap) -> Vec<String> {
    headers
        .get_all(reqwest::header::SET_COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_string))
        .collect()
}

fn is_allowed_cover_host(url: &str) -> bool {
    let rest = match url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        Some(r) => r,
        None => return false,
    };
    let host = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    const ALLOWED_SUFFIXES: &[&str] = &[
        "resources.tidal.com",
        "images.tidal.com",
        "music.youtube.com",
        ".qobuz.com",
        ".dzcdn.net",
        ".mzstatic.com",
        ".scdn.co",
        ".spotifycdn.com",
        ".ytimg.com",
        ".ggpht.com",
        ".googleusercontent.com",
        ".gstatic.com",
    ];
    ALLOWED_SUFFIXES
        .iter()
        .any(|suf| match suf.strip_prefix('.') {
            Some(bare) => host == bare || host.ends_with(suf),
            None => host == *suf,
        })
}

pub fn cover_proxy_url(platform: ServicePlatform, key: &str) -> Option<String> {
    let passthrough = |k: &str| -> Option<String> {
        if k.starts_with("http") {
            if is_allowed_cover_host(k) {
                Some(k.to_string())
            } else {
                None
            }
        } else {
            None
        }
    };

    match platform {
        ServicePlatform::Tidal => {
            if key.starts_with("http") {
                return passthrough(key);
            }
            let path = key.replace('-', "/");
            Some(format!(
                "https://resources.tidal.com/images/{path}/640x640.jpg"
            ))
        }
        ServicePlatform::Qobuz => passthrough(key),
        ServicePlatform::Deezer => {
            if key.starts_with("http") {
                return passthrough(key);
            }
            let (kind, hash) = key.split_once(':')?;
            Some(format!(
                "https://e-cdns-images.dzcdn.net/images/{kind}/{hash}/500x500-000000-80-0-0.jpg"
            ))
        }
        ServicePlatform::AppleMusic => passthrough(key).map(|k| {
            k.replace("{w}", "640")
                .replace("{h}", "640")
                .replace("{f}", "jpg")
                .replace("{c}", "bb")
        }),
        ServicePlatform::Youtube | ServicePlatform::YtMusic => passthrough(key),
        ServicePlatform::Spotify => passthrough(key),
    }
}

#[async_trait]
pub trait ServiceLibraryMutations: Send + Sync {
    fn platform(&self) -> ServicePlatform;

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities::none()
    }

    /// The one bulk-favourite primitive a service implements; the eight
    /// `save_*`/`unsave_*`/`follow_*` methods below are defaults over it.
    async fn set_favorites(&self, kind: FavKind, _op: FavOp, _ids: &[String]) -> MhResult<()> {
        Err(MhError::Unsupported(format!(
            "{} favourites are not supported on {}",
            kind.as_str(),
            self.platform().as_str()
        )))
    }

    async fn save_tracks(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Track, FavOp::Add, ids).await
    }
    async fn unsave_tracks(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Track, FavOp::Remove, ids).await
    }
    async fn save_albums(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Album, FavOp::Add, ids).await
    }
    async fn unsave_albums(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Album, FavOp::Remove, ids).await
    }
    async fn follow_artists(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Artist, FavOp::Add, ids).await
    }
    async fn unfollow_artists(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Artist, FavOp::Remove, ids)
            .await
    }
    async fn save_labels(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Label, FavOp::Add, ids).await
    }
    async fn unsave_labels(&self, ids: &[String]) -> MhResult<()> {
        self.set_favorites(FavKind::Label, FavOp::Remove, ids).await
    }
    async fn follow_playlist(&self, _id: &str) -> MhResult<()> {
        Err(MhError::Other("follow_playlist not supported".into()))
    }
    async fn unfollow_playlist(&self, _id: &str) -> MhResult<()> {
        Err(MhError::Other("unfollow_playlist not supported".into()))
    }
    async fn follow_user(&self, _id: &str) -> MhResult<()> {
        Err(MhError::Other("follow_user not supported".into()))
    }
    async fn unfollow_user(&self, _id: &str) -> MhResult<()> {
        Err(MhError::Other("unfollow_user not supported".into()))
    }

    async fn create_playlist(&self, _input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        Err(MhError::Other("create_playlist not supported".into()))
    }
    async fn rename_playlist(
        &self,
        _id: &str,
        _new_name: &str,
        _new_description: Option<&str>,
        _new_is_public: Option<bool>,
        _new_is_collaborative: Option<bool>,
    ) -> MhResult<()> {
        Err(MhError::Other("rename_playlist not supported".into()))
    }
    async fn delete_playlist(&self, _id: &str) -> MhResult<()> {
        Err(MhError::Other("delete_playlist not supported".into()))
    }
    async fn add_playlist_tracks(
        &self,
        _id: &str,
        _track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        Err(MhError::Other("add_playlist_tracks not supported".into()))
    }
    async fn remove_playlist_tracks(
        &self,
        _id: &str,
        _track_ids: &[String],
        _positions: Option<&[u32]>,
    ) -> MhResult<PlaylistMutateResult> {
        Err(MhError::Other(
            "remove_playlist_tracks not supported".into(),
        ))
    }
    async fn reorder_playlist(
        &self,
        _id: &str,
        _from: u32,
        _to: u32,
    ) -> MhResult<PlaylistMutateResult> {
        Err(MhError::Other("reorder_playlist not supported".into()))
    }

    async fn radio_for(&self, _seed_kind: RadioSeedKind, _seed_id: &str) -> MhResult<RadioResult> {
        Err(MhError::Other("radio_for not supported".into()))
    }

    /// The next page of a station started by `radio_for`, keyed by the token that call
    /// returned. Services that hand over a finite station never produce a token, so the
    /// default is to report that there is no more.
    async fn radio_continue(&self, _token: &str) -> MhResult<RadioResult> {
        Err(MhError::Other("radio_continue not supported".into()))
    }

    async fn fetch_saved_ids(&self, _kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        Ok(SavedIdSet::default())
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        Ok(Vec::new())
    }
}

pub async fn build(
    platform: ServicePlatform,
    state: &crate::BackendState,
) -> MhResult<Arc<dyn ServiceLibrary>> {
    let settings = state.settings.read().await.clone();
    match platform {
        ServicePlatform::Tidal => Ok(Arc::new(
            crate::services::tidal::library::TidalLibrary::from_state(state, &settings)?,
        )),
        ServicePlatform::Qobuz => Ok(Arc::new(
            crate::services::qobuz::library::QobuzLibrary::from_state(state, &settings).await?,
        )),
        ServicePlatform::Spotify => Ok(Arc::new(
            crate::services::spotify::library::SpotifyLibrary::from_state(state)?,
        )),
        ServicePlatform::Deezer => Ok(Arc::new(
            crate::services::deezer::library::DeezerLibrary::from_state(state, &settings)?,
        )),
        ServicePlatform::AppleMusic => Ok(Arc::new(
            crate::services::apple_music::library::AppleMusicLibrary::from_settings(&settings)?,
        )),
        ServicePlatform::Youtube => Ok(Arc::new(
            crate::services::youtube::library::YoutubeLibrary::from_settings(&settings)?,
        )),
        ServicePlatform::YtMusic => Ok(Arc::new(
            crate::services::ytmusic::library::YtMusicLibrary::from_settings(&settings)?,
        )),
    }
}

pub async fn build_mutations(
    platform: ServicePlatform,
    state: &crate::BackendState,
) -> MhResult<Option<Arc<dyn ServiceLibraryMutations>>> {
    Ok(build(platform, state).await?.as_mutations())
}

#[cfg(test)]
mod tests {

    #[test]
    fn parse_hms_seconds_handles_every_shape() {
        assert_eq!(super::parse_hms_seconds("90"), Some(90.0));
        assert_eq!(super::parse_hms_seconds("1:30"), Some(90.0));
        assert_eq!(super::parse_hms_seconds("1:01:30"), Some(3690.0));
        assert_eq!(super::parse_hms_seconds("00:01:23.456"), Some(83.456));
        assert_eq!(super::parse_hms_seconds("1:xx:30"), None);
        assert_eq!(super::parse_hms_seconds("1:2:3:4"), None);
    }
    use super::*;
    use std::str::FromStr;

    #[test]
    fn platform_from_str_accepts_all_vocabularies() {
        for s in [
            "ytmusic",
            "yt_music",
            "yt-music",
            "youtube_music",
            "youtubeMusic",
            "youtubemusic",
        ] {
            assert_eq!(
                ServicePlatform::from_str(s).unwrap(),
                ServicePlatform::YtMusic,
                "{s}"
            );
        }
        for s in ["apple_music", "apple-music", "applemusic", "appleMusic"] {
            assert_eq!(
                ServicePlatform::from_str(s).unwrap(),
                ServicePlatform::AppleMusic,
                "{s}"
            );
        }
        assert_eq!(
            ServicePlatform::from_str("Spotify").unwrap(),
            ServicePlatform::Spotify
        );
        assert!(ServicePlatform::from_str("nope").is_err());
    }

    /// `RecommendationItem` is internally tagged on `kind`, so serde writes the tag
    /// and then the payload's own fields into the same JSON map. A payload field
    /// named `kind` silently overwrites the tag and the whole shelf item becomes
    /// unidentifiable to the frontend — `LibraryAlbumDto::kind` therefore ships as
    /// `album_kind`.
    #[test]
    fn every_shelf_item_keeps_its_kind_tag() {
        let cases = [
            (
                "album",
                RecommendationItem::Album(LibraryAlbumDto {
                    kind: "collection".into(),
                    ..Default::default()
                }),
            ),
            ("track", RecommendationItem::Track(Default::default())),
            (
                "artist",
                RecommendationItem::Artist(LibraryArtistDto {
                    key: String::new(),
                    display: String::new(),
                    album_count: 0,
                    track_count: 0,
                    cover_id: None,
                }),
            ),
            (
                "playlist",
                RecommendationItem::Playlist(LibraryPlaylistDto {
                    id: 0,
                    name: String::new(),
                    created_at: 0,
                    updated_at: 0,
                    track_count: 0,
                    cover_ids: Vec::new(),
                    service_id: None,
                    description: None,
                    owner: None,
                }),
            ),
        ];

        for (expected, item) in cases {
            let v = serde_json::to_value(&item).unwrap();
            assert_eq!(v["kind"], expected, "{expected} lost its tag");
        }

        let album = serde_json::to_value(RecommendationItem::Album(LibraryAlbumDto {
            kind: "collection".into(),
            ..Default::default()
        }))
        .unwrap();
        assert_eq!(album["album_kind"], "collection");
    }
}

#[cfg(test)]
mod shelf_merge_tests {
    use super::*;

    fn artist(key: &str) -> RecommendationItem {
        RecommendationItem::Artist(LibraryArtistDto {
            key: key.to_string(),
            display: key.to_string(),
            album_count: 0,
            track_count: 0,
            cover_id: None,
        })
    }

    fn shelf(title: &str, items: Vec<RecommendationItem>) -> RecommendationShelf {
        RecommendationShelf {
            id: format!("id-{title}-{}", items.len()),
            title: title.to_string(),
            subtitle: None,
            category: RecommendationCategory::Other,
            items,
        }
    }

    #[test]
    fn folds_repeated_titles_into_one_shelf() {
        let out = merge_duplicate_shelves(vec![
            shelf("More like Daft Punk", vec![artist("a")]),
            shelf("Recently played", vec![artist("r")]),
            shelf("More like Daft Punk", vec![artist("b")]),
            shelf("more like daft punk", vec![artist("c")]),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "More like Daft Punk");
        assert_eq!(out[0].items.len(), 3);
        assert_eq!(out[1].title, "Recently played");
    }

    #[test]
    fn drops_items_already_present_in_the_merged_shelf() {
        let out = merge_duplicate_shelves(vec![
            shelf("Made for you", vec![artist("x"), artist("y")]),
            shelf("Made for you", vec![artist("y"), artist("z")]),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].items.len(), 3);
    }

    #[test]
    fn keeps_untitled_shelves_separate() {
        let out = merge_duplicate_shelves(vec![
            shelf("", vec![artist("a")]),
            shelf("", vec![artist("b")]),
        ]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn preserves_first_appearance_order() {
        let out = merge_duplicate_shelves(vec![
            shelf("Zeta", vec![artist("z")]),
            shelf("Alpha", vec![artist("a")]),
            shelf("Zeta", vec![artist("z2")]),
        ]);
        assert_eq!(out[0].title, "Zeta");
        assert_eq!(out[1].title, "Alpha");
    }
}

#[cfg(test)]
mod feed_trim_tests {
    use super::*;

    fn artist(key: &str) -> RecommendationItem {
        RecommendationItem::Artist(LibraryArtistDto {
            key: key.to_string(),
            display: key.to_string(),
            album_count: 0,
            track_count: 0,
            cover_id: None,
        })
    }

    fn shelf(title: &str, n: usize, prefix: &str) -> RecommendationShelf {
        RecommendationShelf {
            id: title.to_string(),
            title: title.to_string(),
            subtitle: None,
            category: RecommendationCategory::Other,
            items: (0..n).map(|i| artist(&format!("{prefix}{i}"))).collect(),
        }
    }

    fn feed(n: usize, items_each: usize) -> Vec<RecommendationShelf> {
        (0..n)
            .map(|i| shelf(&format!("s{i}"), items_each, &format!("p{i}-")))
            .collect()
    }

    #[test]
    fn short_feeds_are_never_touched() {
        let cfg = FeedTrim::default();
        for n in 0..=cfg.head_keep {
            let before = feed(n, 1);
            let after = trim_feed_tail(before.clone(), cfg);
            assert_eq!(after.len(), before.len(), "n={n}");
        }
    }

    #[test]
    fn head_survives_even_when_thin() {
        let mut f = feed(3, 1);
        f.extend(feed(10, 5));
        let out = trim_feed_tail(f, FeedTrim::default());
        assert_eq!(out[0].items.len(), 1);
        assert_eq!(out[1].items.len(), 1);
        assert_eq!(out[2].items.len(), 1);
    }

    #[test]
    fn thin_tail_shelves_are_dropped() {
        let mut f = feed(8, 5);
        f.extend((0..12).map(|i| shelf(&format!("tail{i}"), 1, &format!("t{i}-"))));
        let out = trim_feed_tail(f, FeedTrim::default());
        assert_eq!(out.len(), 8, "every 1-item tail shelf should go");
    }

    #[test]
    fn a_tail_shelf_of_only_already_seen_items_is_dropped() {
        let mut f = feed(8, 5);
        let mut echo = shelf("echo", 0, "x");
        echo.items = vec![artist("p0-0"), artist("p0-1"), artist("p1-0")];
        f.push(echo);
        let out = trim_feed_tail(f, FeedTrim::default());
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn a_tail_shelf_with_one_new_item_survives() {
        let mut f = feed(8, 5);
        let mut mixed = shelf("mixed", 0, "x");
        mixed.items = vec![artist("p0-0"), artist("p0-1"), artist("brand-new")];
        f.push(mixed);
        let out = trim_feed_tail(f, FeedTrim::default());
        assert_eq!(out.len(), 9);
    }

    #[test]
    fn total_is_capped() {
        let out = trim_feed_tail(feed(60, 5), FeedTrim::default());
        assert_eq!(out.len(), FeedTrim::default().max_shelves);
    }

    #[test]
    fn trimming_never_inspects_any_text() {
        let en = feed(8, 5);
        let mut fr: Vec<RecommendationShelf> = en.clone();
        for (i, sh) in fr.iter_mut().enumerate() {
            sh.title = format!("Fait pour vous {i}");
        }
        let a = trim_feed_tail(en, FeedTrim::default()).len();
        let b = trim_feed_tail(fr, FeedTrim::default()).len();
        assert_eq!(a, b);
    }
}

#[cfg(test)]
mod mix_id_tests {
    use super::*;

    fn mix(id: &str, url: Option<&str>) -> RecommendationItem {
        RecommendationItem::Mix {
            id: id.into(),
            title: "M".into(),
            subtitle: None,
            cover_id: None,
            url: url.map(str::to_string),
        }
    }

    /// Each service emits the id its own `playlist_detail` accepts. Callers must
    /// pass it through verbatim — a caller-side prefix double-qualifies Deezer
    /// (`mix::stl::…`) and sends Apple station ids to the playlists endpoint.
    #[test]
    fn mix_ids_arrive_fully_qualified() {
        for (id, prefix) in [("mix::123", "mix::"), ("stl::456", "stl::")] {
            match mix(id, None) {
                RecommendationItem::Mix { id: got, .. } => {
                    assert!(got.starts_with(prefix), "{got} should start with {prefix}")
                }
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn a_mix_with_a_url_is_one_the_service_cannot_enumerate() {
        match mix(
            "ra.1228739600",
            Some("https://music.apple.com/tr/station/x/ra.1228739600"),
        ) {
            RecommendationItem::Mix { url, .. } => assert!(url.is_some()),
            _ => unreachable!(),
        }
    }

    #[test]
    fn dedupe_still_distinguishes_mixes() {
        assert_ne!(
            mix("mix::1", None).dedupe_key(),
            mix("mix::2", None).dedupe_key()
        );
    }
}

#[cfg(test)]
mod accessor_tests {
    use super::*;
    use serde_json::json;

    fn sample() -> serde_json::Value {
        json!({
            "id": 12345,
            "sid": "abc",
            "title": "  Get Lucky  ",
            "blank": "",
            "count": "42",
            "n": 7,
            "dur": "3.5",
            "flag": true,
            "artist": { "name": "Daft Punk", "id": 99 },
            "artists": [{ "id": 111, "name": "Fallback Artist" }],
            "released": "2019-04-26T00:00:00Z",
            "yearnum": 1983
        })
    }

    #[test]
    fn a_pointer_path_and_a_key_path_both_resolve() {
        let v = sample();
        assert_eq!(str_at(&v, &["title"]), Some("  Get Lucky  "));
        assert_eq!(str_at(&v, &["/artist/name"]), Some("Daft Punk"));
    }

    #[test]
    fn the_first_matching_path_wins_and_misses_fall_through() {
        let v = sample();
        assert_eq!(str_at(&v, &["nope", "/artist/name"]), Some("Daft Punk"));
        assert_eq!(
            id_at(&v, &["/artist/id", "/artists/0/id"]),
            Some("99".into())
        );
        assert_eq!(
            id_at(&v, &["/missing/id", "/artists/0/id"]),
            Some("111".into()),
            "falls through to the alternate spelling"
        );
        assert_eq!(str_at(&v, &["a", "b"]), None);
    }

    #[test]
    fn ids_accept_both_ints_and_strings() {
        let v = sample();
        assert_eq!(id_at(&v, &["id"]), Some("12345".into()));
        assert_eq!(id_at(&v, &["sid"]), Some("abc".into()));
    }

    #[test]
    fn numbers_parse_out_of_strings_too() {
        let v = sample();
        assert_eq!(i64_at(&v, &["n"]), Some(7));
        assert_eq!(i64_at(&v, &["count"]), Some(42), "numeric string");
        assert_eq!(f64_at(&v, &["dur"]), Some(3.5));
        assert_eq!(u32_at(&v, &["n"]), Some(7));
        assert_eq!(bool_at(&v, &["flag"]), Some(true));
        assert_eq!(
            i64_at(&v, &["title"]),
            None,
            "non-numeric string is not a number"
        );
    }

    #[test]
    fn empty_strings_count_as_absent_only_for_nonempty_at() {
        let v = sample();
        assert_eq!(str_at(&v, &["blank"]), Some(""));
        assert_eq!(nonempty_at(&v, &["blank"]), None);
        assert_eq!(
            nonempty_at(&v, &["blank", "title"]),
            None,
            "blank still matched first"
        );
        assert_eq!(
            nonempty_at(&v, &["title"]),
            Some("Get Lucky".into()),
            "trimmed"
        );
        assert_eq!(string_at(&v, &["missing"]), "");
    }

    #[test]
    fn a_year_is_the_leading_four_digits_of_any_date_shape() {
        let v = sample();
        assert_eq!(year4_at(&v, &["released"]), Some("2019".into()));
        assert_eq!(year4_at(&v, &["yearnum"]), Some("1983".into()));
        assert_eq!(year4_at(&v, &["title"]), None, "not a date");
        assert_eq!(year4_at(&v, &["missing"]), None);
    }
}
