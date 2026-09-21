use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, ORIGIN, REFERER};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};
use crate::services::apple_music::playback::get_media_user_token;

use crate::services::common::library::{
    cover_id, f64_at, i64_at, nonempty_at, stable_hash_i64, str_at, string_at, u32_at, year4_at,
    FavKind, FavOp, MutationCapabilities, OwnedPlaylistRow, Page, PlaylistCreateInput,
    PlaylistMutateResult, RadioResult, RadioSeedKind, RecommendationCategory, RecommendationItem,
    RecommendationShelf, RecommendationsPage, SaveKind, SavedIdSet, ServiceCapabilities,
    ServiceLibrary, ServiceLibraryMutations, ServiceLibraryPage, ServicePlatform,
};

use crate::services::apple_music::APPLE_MUSIC_HOMEPAGE as HOMEPAGE;
const AMP_API: &str = "https://amp-api.music.apple.com";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0 Safari/537.36";

#[derive(Clone)]
struct CachedToken {
    token: String,
    expires_at: Instant,
}

pub struct AppleMusicLibrary {
    client: Client,
    media_user_token: String,
    dev_token: Arc<RwLock<Option<CachedToken>>>,
    storefront: Arc<RwLock<Option<String>>>,
    locale: String,
    sync_playback_history: bool,
}

impl AppleMusicLibrary {
    pub fn from_settings(s: &Settings) -> MhResult<Self> {
        if s.apple_cookies_path.is_empty() {
            return Err(MhError::Auth(
                "Apple Music not connected (no cookies path)".into(),
            ));
        }
        let mut_token = get_media_user_token(&s.apple_cookies_path)
            .map_err(|e| MhError::Auth(format!("Apple media-user-token: {e}")))?;
        let client = crate::http_client::ua_client(UA)?;
        let locale = if s.apple_language.trim().is_empty() {
            "en-US".to_string()
        } else {
            s.apple_language.trim().to_string()
        };
        Ok(Self {
            client,
            media_user_token: mut_token,
            dev_token: Arc::new(RwLock::new(None)),
            storefront: Arc::new(RwLock::new(None)),
            locale,
            sync_playback_history: s.sync_playback_history(ServicePlatform::AppleMusic),
        })
    }

    async fn dev_token(&self) -> MhResult<String> {
        {
            let g = self.dev_token.read().await;
            if let Some(c) = g.as_ref() {
                if Instant::now() < c.expires_at {
                    return Ok(c.token.clone());
                }
            }
        }
        let token = crate::services::apple_music::playback::get_developer_token().await?;
        let cached = CachedToken {
            token: token.clone(),
            expires_at: Instant::now() + Duration::from_secs(3600),
        };
        *self.dev_token.write().await = Some(cached);
        Ok(token)
    }

    pub async fn probe_session(&self) -> MhResult<bool> {
        match self.get(&format!("{AMP_API}/v1/me/storefront"), &[]).await {
            Ok(_) => Ok(true),
            Err(MhError::Auth(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn storefront(&self) -> MhResult<String> {
        {
            let g = self.storefront.read().await;
            if let Some(s) = g.as_ref() {
                return Ok(s.clone());
            }
        }
        let body = self
            .get(&format!("{AMP_API}/v1/me/storefront"), &[])
            .await?;
        let sf = str_at(&body, &["/data/0/id"]).unwrap_or("us").to_string();
        *self.storefront.write().await = Some(sf.clone());
        Ok(sf)
    }

    fn headers(&self, dev_token: &str) -> MhResult<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {dev_token}"))?,
        );
        h.insert(
            "Media-User-Token",
            HeaderValue::from_str(&self.media_user_token)?,
        );
        h.insert(ORIGIN, HeaderValue::from_static(HOMEPAGE));
        h.insert(REFERER, HeaderValue::from_static(HOMEPAGE));
        h.insert("accept", HeaderValue::from_static("*/*"));
        h.insert(
            "accept-language",
            HeaderValue::from_str(&self.locale)
                .unwrap_or_else(|_| HeaderValue::from_static("en-US")),
        );
        Ok(h)
    }

    /// One page of a `/v1/me/library/{segment}` collection. Apple's five library
    /// endpoints differ only in that segment and the `include` list.
    async fn library_page<T, F>(
        &self,
        segment: &str,
        include: &str,
        page: Page,
        mapper: F,
    ) -> MhResult<ServiceLibraryPage<T>>
    where
        F: Fn(&Value) -> Option<T> + Send + Sync,
    {
        let limit = page.limit.min(100).to_string();
        let offset = page.offset.to_string();
        let body = self
            .get(
                &format!("{AMP_API}/v1/me/library/{segment}"),
                &[
                    ("limit", &limit),
                    ("offset", &offset),
                    ("include", include),
                    ("platform", "web"),
                    ("l", &self.locale),
                ],
            )
            .await?;
        let items: Vec<T> = data_array(&body).iter().filter_map(mapper).collect();
        Ok(ServiceLibraryPage::of(items, meta_total(&body)))
    }

    /// `GET /v1/me/library/{what}` → its `data` array, empty on any failure —
    /// a missing library section is normal, not an error worth propagating.
    async fn library_data(&self, what: &str, extra: &[(&str, &str)]) -> Vec<Value> {
        let mut params: Vec<(&str, &str)> = vec![("limit", "100")];
        params.extend_from_slice(extra);
        let body = self
            .get(&format!("{AMP_API}/v1/me/library/{what}"), &params)
            .await
            .unwrap_or(Value::Null);
        data_array(&body)
    }

    async fn get(&self, url: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let token = self.dev_token().await?;
        let h = self.headers(&token)?;
        let resp = self
            .client
            .get(url)
            .headers(h)
            .query(params)
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(MhError::Network)?;
        if !status.is_success() {
            if matches!(status.as_u16(), 401 | 403) {
                return Err(MhError::Auth(
                    "Apple Music session expired — reconnect Apple Music or check that your \
                     subscription is active."
                        .to_string(),
                ));
            }
            let body_str = body.to_string();
            if status.as_u16() == 400
                && (body_str.contains("40015")
                    || body_str.contains("CloudLibrary")
                    || body_str.contains("Insufficient Privileges"))
            {
                return Err(MhError::Unsupported(
                    "no-subscription: This Apple Music account has no active subscription, so your \
                     cloud library is unavailable."
                        .to_string(),
                ));
            }
            return Err(MhError::Other(format!(
                "Apple Music {url} returned {}: {body}",
                status.as_u16()
            )));
        }
        Ok(body)
    }
}

const AM_ARTWORK_PATHS: &[&str] = &[
    "/attributes/artwork/url",
    "/attributes/editorialArtwork/subscriptionHero/url",
    "/attributes/editorialArtwork/storeFlowcase/url",
    "/attributes/editorialArtwork/originalFlowcase/url",
    "/attributes/editorialArtwork/subscriptionCover/url",
    "/attributes/editorialArtwork/subscriptionHeroWithTitleAlt/url",
    "/attributes/editorialArtwork/superHeroTall/url",
    "/attributes/editorialArtwork/storeFlowcaseBrick/url",
    "/attributes/editorialArtwork/brandLogo/url",
    "/attributes/editorialVideo/motionDetailSquare/previewFrame/url",
    "/attributes/editorialVideo/motionSquareVideo1x1/previewFrame/url",
];

fn artwork_cover(v: &Value) -> Option<String> {
    for p in AM_ARTWORK_PATHS {
        if let Some(url) = v.pointer(p).and_then(|x| x.as_str()) {
            return Some(cover_id(ServicePlatform::AppleMusic, url));
        }
    }
    None
}

fn artwork_cover_at(v: &Value, path: &str) -> Option<String> {
    if let Some(url) = v.pointer(&format!("{path}/url")).and_then(|x| x.as_str()) {
        return Some(cover_id(ServicePlatform::AppleMusic, url));
    }
    artwork_cover(v)
}

fn library_catalog_id(v: &Value) -> Option<String> {
    str_at(v, &["/relationships/catalog/data/0/id"]).map(str::to_string)
}

/// Catalog id of a related resource. Library resources nest the catalog
/// relationships one level deeper, so both shapes are tried.
fn am_related_id(v: &Value, rel: &str) -> Option<String> {
    for p in [
        format!("/relationships/{rel}/data/0/id"),
        format!("/relationships/catalog/data/0/relationships/{rel}/data/0/id"),
    ] {
        if let Some(s) = v.pointer(&p).and_then(|x| x.as_str()) {
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

/// Apple song/album URLs end in the numeric album id:
/// `…/album/{slug}/{albumId}` and `…/album/{slug}/{albumId}?i={songId}`.
fn album_id_from_url(url: &str) -> Option<String> {
    let path = url.split('?').next()?;
    let last = path.rsplit('/').next()?;
    if !last.is_empty() && last.bytes().all(|b| b.is_ascii_digit()) {
        Some(last.to_string())
    } else {
        None
    }
}

fn am_album_key(v: &Value, attrs: &Value) -> Option<String> {
    am_related_id(v, "albums").or_else(|| str_at(attrs, &["url"]).and_then(album_id_from_url))
}

fn map_library_song(v: &Value) -> Option<LibraryTrackDto> {
    let id = str_at(v, &["id"])?.to_string();
    v.get("attributes")?;
    let artist = string_at(v, &["/attributes/artistName"]);
    let catalog_id = library_catalog_id(v);
    Some(LibraryTrackDto {
        path: match &catalog_id {
            Some(cid) => format!("https://music.apple.com/song/{cid}"),
            None => format!("https://music.apple.com/library/song/{id}"),
        },
        title: Some(string_at(v, &["/attributes/name"])),
        artist: Some(artist.clone()),
        album: nonempty_at(v, &["/attributes/albumName"]),
        album_key: am_album_key(v, v.get("attributes")?),
        year: year4_at(v, &["/attributes/releaseDate"]),
        genre: nonempty_at(v, &["/attributes/genreNames/0"]),
        duration_secs: f64_at(v, &["/attributes/durationInMillis"]).map(|m| m / 1000.0),
        track_no: u32_at(v, &["/attributes/trackNumber"]),
        disc_no: u32_at(v, &["/attributes/discNumber"]),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: artwork_cover(v),
        primary_artist: Some(artist),
        artist_id: am_related_id(v, "artists"),
        ..Default::default()
    })
}

fn map_library_video(v: &Value) -> Option<LibraryTrackDto> {
    let mut t = map_library_song(v)?;
    t.is_video = true;
    Some(t)
}

/// Apple wraps every entity as `{ id, attributes: { … } }`. A *library* row also
/// carries the catalog id of the same release, which is the key the rest of the
/// app uses — so library rows prefer it and catalog rows already are it.
fn map_album_node(v: &Value, prefer_catalog_id: bool) -> Option<LibraryAlbumDto> {
    let id = str_at(v, &["id"])?.to_string();
    v.get("attributes")?;
    Some(LibraryAlbumDto {
        album_key: if prefer_catalog_id {
            library_catalog_id(v).unwrap_or(id)
        } else {
            id
        },
        title: string_at(v, &["/attributes/name"]),
        artist: string_at(v, &["/attributes/artistName"]),
        year: year4_at(v, &["/attributes/releaseDate"]),
        cover_id: artwork_cover(v),
        track_count: i64_at(v, &["/attributes/trackCount"]).unwrap_or(0),
        artist_id: am_related_id(v, "artists"),
        ..Default::default()
    })
}

fn map_library_album(v: &Value) -> Option<LibraryAlbumDto> {
    map_album_node(v, true)
}

fn map_catalog_album(v: &Value) -> Option<LibraryAlbumDto> {
    map_album_node(v, false)
}

/// Library and catalog playlists carry the same fields, so one mapper serves both.
fn map_playlist(v: &Value) -> Option<LibraryPlaylistDto> {
    let id_str = str_at(v, &["id"])?.to_string();
    v.get("attributes")?;
    Some(LibraryPlaylistDto {
        id: stable_hash_i64(&id_str),
        name: string_at(v, &["/attributes/name"]),
        created_at: 0,
        updated_at: 0,
        track_count: 0,
        cover_ids: artwork_cover(v).map(|s| vec![s]).unwrap_or_default(),
        service_id: Some(id_str),
        description: nonempty_at(
            v,
            &[
                "/attributes/description/standard",
                "/attributes/description/short",
            ],
        ),
        owner: nonempty_at(v, &["/attributes/curatorName"]),
    })
}

fn map_artist_node(v: &Value, prefer_catalog_id: bool) -> Option<LibraryArtistDto> {
    let id = str_at(v, &["id"])?.to_string();
    v.get("attributes")?;
    Some(LibraryArtistDto {
        key: if prefer_catalog_id {
            library_catalog_id(v).unwrap_or(id)
        } else {
            id
        },
        display: string_at(v, &["/attributes/name"]),
        album_count: 0,
        track_count: 0,
        cover_id: artwork_cover(v),
    })
}

fn map_library_artist(v: &Value) -> Option<LibraryArtistDto> {
    map_artist_node(v, true)
}

fn map_catalog_artist(v: &Value) -> Option<LibraryArtistDto> {
    map_artist_node(v, false)
}

fn view_category(key: &str) -> RecommendationCategory {
    match key {
        "top-songs" | "top-music-videos" => RecommendationCategory::Charts,
        "latest-release" => RecommendationCategory::NewReleases,
        "similar-artists" | "you-might-also-like" => RecommendationCategory::Discovery,
        "featured-albums" | "featured-playlists" => RecommendationCategory::Editorial,
        _ => RecommendationCategory::Other,
    }
}

fn composition_category(items: &[RecommendationItem]) -> RecommendationCategory {
    if items.is_empty() {
        return RecommendationCategory::Other;
    }
    if items
        .iter()
        .all(|i| matches!(i, RecommendationItem::Mix { .. }))
    {
        return RecommendationCategory::Stations;
    }
    if items
        .iter()
        .all(|i| matches!(i, RecommendationItem::Track(_)))
    {
        return RecommendationCategory::Charts;
    }
    RecommendationCategory::Other
}

fn shelf_from_view(
    name: &str,
    view: &Value,
    category: RecommendationCategory,
) -> Option<RecommendationShelf> {
    let title = str_at(view, &["/attributes/title/stringForDisplay"])
        .unwrap_or(name)
        .to_string();
    let data = view
        .pointer("/data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let mut items = Vec::new();
    for it in data {
        match str_at(&it, &["type"]).unwrap_or("") {
            "albums" => {
                if let Some(a) = map_catalog_album(&it) {
                    items.push(RecommendationItem::Album(a));
                }
            }
            "songs" | "music-videos" => {
                if let Some(t) = map_library_song(&it) {
                    items.push(RecommendationItem::Track(t));
                }
            }
            "artists" => {
                if let Some(a) = map_catalog_artist(&it) {
                    items.push(RecommendationItem::Artist(a));
                }
            }
            "playlists" => {
                if let Some(p) = map_playlist(&it) {
                    items.push(RecommendationItem::Playlist(p));
                }
            }
            _ => {}
        }
    }
    if items.is_empty() {
        return None;
    }
    let category = match category {
        RecommendationCategory::Other => composition_category(&items),
        explicit => explicit,
    };
    Some(RecommendationShelf::new(
        name.to_string(),
        title,
        category,
        items,
    ))
}

fn shelves_from_views(entry: &Value, order: &[&str]) -> Vec<RecommendationShelf> {
    let views = match entry.get("views") {
        Some(Value::Object(m)) => m,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for name in order {
        if let Some(view) = views.get(*name) {
            if let Some(shelf) = shelf_from_view(name, view, view_category(name)) {
                out.push(shelf);
            }
        }
    }
    out
}

fn data_array(body: &Value) -> Vec<Value> {
    body.get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn meta_total(body: &Value) -> Option<u64> {
    body.pointer("/meta/total").and_then(|v| v.as_u64())
}

#[async_trait]
impl ServiceLibrary for AppleMusicLibrary {
    fn as_mutations(self: Arc<Self>) -> Option<Arc<dyn ServiceLibraryMutations>> {
        Some(self)
    }

    fn platform(&self) -> ServicePlatform {
        ServicePlatform::AppleMusic
    }
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::all().with_mutations(self.mutation_capabilities())
    }

    async fn albums(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        self.library_page("albums", "catalog,artists", page, map_library_album)
            .await
    }

    async fn tracks(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        self.library_page("songs", "catalog,artists,albums", page, map_library_song)
            .await
    }

    async fn artists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        self.library_page("artists", "catalog", page, map_library_artist)
            .await
    }

    async fn playlists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        self.library_page("playlists", "catalog", page, map_playlist)
            .await
    }

    async fn videos(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        self.library_page(
            "music-videos",
            "catalog,artists,albums",
            page,
            map_library_video,
        )
        .await
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        let mut shelves = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        let timezone = chrono::Local::now().format("%:z").to_string();
        const PAGE: usize = 30;
        const MAX_PAGES: usize = 6;
        for page in 0..MAX_PAGES {
            let offset = (page * PAGE).to_string();
            let body = match self
                .get(
                    &format!("{AMP_API}/v1/me/recommendations"),
                    &[
                        ("platform", "web"),
                        ("l", &self.locale),
                        ("timezone", &timezone),
                        ("types", "albums,playlists,stations,songs,artists"),
                        ("extend", "editorialArtwork"),
                        ("limit", "30"),
                        ("offset", &offset),
                    ],
                )
                .await
            {
                Ok(b) => b,
                Err(_) => break,
            };
            let entries = data_array(&body);
            if entries.is_empty() {
                break;
            }
            let before = shelves.len();
            for entry in entries {
                let rec_id = str_at(&entry, &["id"]).unwrap_or("").to_string();
                if !rec_id.is_empty() && !seen_ids.insert(rec_id.clone()) {
                    continue;
                }
                let title = str_at(&entry, &["/attributes/title/stringForDisplay"]).unwrap_or("");
                let contents = entry
                    .pointer("/relationships/contents/data")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut items = Vec::new();
                for it in contents {
                    let ty = str_at(&it, &["type"]).unwrap_or("");
                    match ty {
                        "albums" => {
                            if let Some(a) = map_catalog_album(&it) {
                                items.push(RecommendationItem::Album(a));
                            }
                        }
                        "playlists" => {
                            if let Some(p) = map_playlist(&it) {
                                items.push(RecommendationItem::Playlist(p));
                            }
                        }
                        "stations" => {
                            let id = str_at(&it, &["id"]).unwrap_or("").to_string();
                            let name = str_at(&it, &["/attributes/name"]).unwrap_or("").to_string();
                            let cover = artwork_cover_at(&it, "/attributes/artwork");
                            let url = str_at(&it, &["/attributes/url"]).map(str::to_string);
                            items.push(RecommendationItem::Mix {
                                id,
                                title: name,
                                subtitle: None,
                                cover_id: cover,
                                url,
                            });
                        }
                        _ => {}
                    }
                }
                if items.is_empty() {
                    continue;
                }
                let category = composition_category(&items);
                shelves.push(RecommendationShelf::new(
                    rec_id,
                    title.to_string(),
                    category,
                    items,
                ));
            }
            if shelves.len() == before {
                break;
            }
        }
        if shelves.is_empty() {
            shelves = self.catalog_charts_shelves(None).await;
        }
        let shelves = crate::services::common::library::trim_feed_tail(
            crate::services::common::library::merge_duplicate_shelves(shelves),
            crate::services::common::library::FeedTrim::default(),
        );
        Ok(RecommendationsPage { shelves })
    }

    async fn explore(&self) -> MhResult<RecommendationsPage> {
        let sf = self.storefront().await?;
        let mut shelves = Vec::new();
        if let Ok(body) = self
            .get(
                &format!("{AMP_API}/v1/catalog/{sf}/genres"),
                &[("l", &self.locale)],
            )
            .await
        {
            let tiles: Vec<RecommendationItem> = data_array(&body)
                .iter()
                .filter_map(|g| {
                    let id = str_at(g, &["id"])?;
                    let name = str_at(g, &["/attributes/name"])?;
                    Some(RecommendationItem::PageLink {
                        api_path: format!("genre::{id}"),
                        title: name.to_string(),
                        icon: None,
                    })
                })
                .collect();
            if !tiles.is_empty() {
                shelves.push(RecommendationShelf::new(
                    "genres",
                    "Genres",
                    RecommendationCategory::Genre,
                    tiles,
                ));
            }
        }
        shelves.extend(self.catalog_charts_shelves(None).await);
        Ok(RecommendationsPage { shelves })
    }

    async fn explore_page(&self, path: &str) -> MhResult<RecommendationsPage> {
        let genre = path.strip_prefix("genre::").unwrap_or(path);
        Ok(RecommendationsPage {
            shelves: self.catalog_charts_shelves(Some(genre)).await,
        })
    }

    async fn report_playback(
        &self,
        service_track_id: &str,
        duration_secs: u64,
        _context_uri: Option<&str>,
        _track_index: Option<u64>,
    ) -> MhResult<()> {
        if !self.sync_playback_history {
            return Ok(());
        }
        let adam_id = service_track_id
            .rsplit(['/', '=', '?', '&'])
            .find(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(service_track_id);
        if adam_id.is_empty() || !adam_id.chars().all(|c| c.is_ascii_digit()) {
            return Ok(());
        }
        let dev_token = self.dev_token().await?;
        let store_front = self.storefront().await.unwrap_or_else(|_| "us".into());
        let _ = crate::services::apple_music::telemetry::report_playback(
            &self.client,
            &dev_token,
            &self.media_user_token,
            &store_front,
            adam_id,
            duration_secs.saturating_mul(1000),
            "library",
        )
        .await;
        Ok(())
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let sf = self.storefront().await?;
        let body = self
            .get(
                &format!("{AMP_API}/v1/catalog/{sf}/albums/{id}"),
                &[
                    ("include", "tracks,artists"),
                    ("include[songs]", "artists,albums"),
                    ("l", &self.locale),
                ],
            )
            .await?;
        let entry = body
            .pointer("/data/0")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Apple album {id}")))?;
        let album = map_catalog_album(&entry)
            .ok_or_else(|| MhError::NotFound(format!("Apple album {id}")))?;
        let tracks = self.all_relationship_tracks(&entry).await;
        Ok(LibraryAlbumDetail { album, tracks })
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let sf = self.storefront().await?;
        let body = self
            .get(
                &format!("{AMP_API}/v1/catalog/{sf}/artists/{id}"),
                &[
                    ("include", "albums"),
                    ("l", &self.locale),
                    ("views", "top-songs"),
                ],
            )
            .await?;
        let entry = body
            .pointer("/data/0")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Apple artist {id}")))?;
        let display = str_at(&entry, &["/attributes/name"])
            .unwrap_or("")
            .to_string();
        let albums_arr = entry
            .pointer("/relationships/albums/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let albums: Vec<LibraryAlbumDto> =
            albums_arr.iter().filter_map(map_catalog_album).collect();
        let top_arr = entry
            .pointer("/views/top-songs/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = top_arr.iter().filter_map(map_library_song).collect();
        Ok(LibraryArtistDetail {
            key: id.into(),
            display,
            albums,
            tracks,
        })
    }

    async fn album_page(&self, id: &str) -> MhResult<Value> {
        let sf = self.storefront().await?;
        let body = self
            .get(
                &format!("{AMP_API}/v1/catalog/{sf}/albums/{id}"),
                &[
                    ("l", &self.locale),
                    ("extend", "extendedAssetUrls,offers"),
                    (
                        "views",
                        "other-versions,more-by-artist,you-might-also-like,appears-on",
                    ),
                ],
            )
            .await?;
        let entry = body
            .pointer("/data/0")
            .ok_or_else(|| MhError::NotFound(format!("Apple album {id}")))?;
        let shelves = shelves_from_views(
            entry,
            &[
                "other-versions",
                "more-by-artist",
                "you-might-also-like",
                "appears-on",
            ],
        );
        Ok(serde_json::json!({ "shelves": shelves, "raw": Value::Null }))
    }

    async fn artist_page(&self, id: &str) -> MhResult<Value> {
        let sf = self.storefront().await?;
        let body = self
            .get(
                &format!("{AMP_API}/v1/catalog/{sf}/artists/{id}"),
                &[
                    ("l", &self.locale),
                    (
                        "views",
                        "latest-release,top-songs,full-albums,featured-albums,top-music-videos,similar-artists",
                    ),
                ],
            )
            .await?;
        let entry = body
            .pointer("/data/0")
            .ok_or_else(|| MhError::NotFound(format!("Apple artist {id}")))?;
        let shelves = shelves_from_views(
            entry,
            &[
                "latest-release",
                "top-songs",
                "full-albums",
                "featured-albums",
                "top-music-videos",
                "similar-artists",
            ],
        );
        Ok(serde_json::json!({ "shelves": shelves, "raw": Value::Null }))
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        let id = id.strip_prefix("mix::").unwrap_or(id);
        if id.starts_with("ra.") {
            return Err(MhError::Unsupported(format!(
                "Apple Music station {id} is a live radio stream, not a playlist — it has no                  track list to show. Open it in Apple Music to listen."
            )));
        }
        let is_library = id.starts_with('p') && !id.starts_with("pl.");
        let url = if is_library {
            format!(
                "{AMP_API}/v1/me/library/playlists/{id}?include=tracks\
                 &include[library-songs]=catalog,artists,albums&platform=web&l=en-US"
            )
        } else {
            let sf = self.storefront().await?;
            format!(
                "{AMP_API}/v1/catalog/{sf}/playlists/{id}?include=tracks\
                 &include[songs]=artists,albums&l=en-US"
            )
        };
        let body = self.get(&url, &[]).await?;
        let entry = body
            .pointer("/data/0")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Apple playlist {id}")))?;
        let playlist = map_playlist(&entry)
            .ok_or_else(|| MhError::NotFound(format!("Apple playlist {id}")))?;
        let tracks = self.all_relationship_tracks(&entry).await;
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }
}

impl AppleMusicLibrary {
    /// Every row of an AMP `relationships.tracks`, following its `next` link.
    ///
    /// `?include=tracks` returns at most 100 rows and hands back a `next` path for the
    /// rest, so a longer album or playlist arrived cut off at exactly 100 with no error.
    async fn all_relationship_tracks(&self, entry: &Value) -> Vec<LibraryTrackDto> {
        let node = entry.pointer("/relationships/tracks");
        let mut out: Vec<LibraryTrackDto> = node
            .and_then(|n| n.get("data"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(map_library_song).collect())
            .unwrap_or_default();
        let mut next = node
            .and_then(|n| n.get("next"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let mut guard = 0;
        while let Some(path) = next.take() {
            guard += 1;
            if guard > 50 {
                break;
            }
            let url = if path.starts_with("http") {
                path
            } else {
                format!("{AMP_API}{path}")
            };
            let Ok(page) = self.get(&url, &[]).await else {
                break;
            };
            let rows: Vec<LibraryTrackDto> = page
                .get("data")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(map_library_song).collect())
                .unwrap_or_default();
            if rows.is_empty() {
                break;
            }
            out.extend(rows);
            next = str_at(&page, &["next"]).map(str::to_string);
        }
        out
    }

    async fn catalog_charts_shelves(&self, genre: Option<&str>) -> Vec<RecommendationShelf> {
        let sf = match self.storefront().await {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let mut params: Vec<(&str, &str)> = vec![
            ("types", "albums,playlists,songs"),
            ("limit", "20"),
            ("l", &self.locale),
        ];
        if let Some(g) = genre {
            params.push(("genre", g));
        }
        let body = match self
            .get(&format!("{AMP_API}/v1/catalog/{sf}/charts"), &params)
            .await
        {
            Ok(b) => b,
            Err(_) => return Vec::new(),
        };
        let results = match body.get("results") {
            Some(Value::Object(m)) => m,
            _ => return Vec::new(),
        };
        let mut shelves = Vec::new();
        for (_kind, arr) in results {
            let charts = match arr.as_array() {
                Some(a) => a,
                None => continue,
            };
            for chart in charts {
                if let Some(shelf) = shelf_from_view(
                    str_at(chart, &["name"]).unwrap_or("Top"),
                    chart,
                    RecommendationCategory::Charts,
                ) {
                    shelves.push(shelf);
                }
            }
        }
        shelves
    }

    async fn write(
        &self,
        method: reqwest::Method,
        url: &str,
        query: &[(&str, &str)],
        body: Option<&Value>,
    ) -> MhResult<Value> {
        let token = self.dev_token().await?;
        let h = self.headers(&token)?;
        let mut req = self
            .client
            .request(method.clone(), url)
            .headers(h)
            .query(query);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(MhError::Network)?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(MhError::Other(format!(
                "Apple Music {method} {url} returned {}: {body}",
                status.as_u16()
            )));
        }
        Ok(body)
    }

    /// The library row id for a catalog id, which is what DELETE addresses.
    /// `segment` is "songs" or "albums".
    async fn library_id_for(&self, segment: &str, catalog_id: &str) -> MhResult<Option<String>> {
        let url = format!("{AMP_API}/v1/me/library/{segment}");
        let key = format!("catalog:{catalog_id}");
        let body = self
            .get(&url, &[("filter[equivalents-with]", key.as_str())])
            .await?;
        Ok(str_at(&body, &["/data/0/id"]).map(str::to_string))
    }

    async fn paged_library_catalog_ids(&self, segment: &str) -> (Vec<String>, Vec<Option<String>>) {
        let entries = crate::services::common::library::paginate_all(100, |page| async move {
            let body = self
                .get(
                    &format!("{AMP_API}/v1/me/library/{segment}"),
                    &[
                        ("limit", "100"),
                        ("offset", &page.offset.to_string()),
                        ("include", "catalog"),
                    ],
                )
                .await
                .unwrap_or(Value::Null);
            Ok(ServiceLibraryPage::of(
                body.get("data")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default(),
                None,
            ))
        })
        .await
        .unwrap_or_default();

        let mut catalog_ids = Vec::new();
        let mut library_ids: Vec<Option<String>> = Vec::new();
        for entry in &entries {
            if let Some(cat) = library_catalog_id(entry) {
                catalog_ids.push(cat);
                library_ids.push(str_at(entry, &["id"]).map(String::from));
            }
        }
        (catalog_ids, library_ids)
    }
}

#[async_trait]
impl ServiceLibraryMutations for AppleMusicLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::AppleMusic
    }

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities {
            reorder_playlists: false,
            radio: true,
            ..MutationCapabilities::full_audio()
        }
    }

    async fn radio_for(&self, seed_kind: RadioSeedKind, seed_id: &str) -> MhResult<RadioResult> {
        let seed_type = match seed_kind {
            RadioSeedKind::Track => "songs",
            RadioSeedKind::Album => "albums",
            RadioSeedKind::Artist => "artists",
            RadioSeedKind::Playlist => "playlists",
        };
        let mut title = String::from("Station");
        let mut station_id = None;
        let mut tracks: Vec<LibraryTrackDto> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut next_seed = serde_json::json!({ "id": seed_id, "type": seed_type });

        for round in 0..5 {
            let body = self
                .write(
                    reqwest::Method::POST,
                    &format!("{AMP_API}/v1/me/stations/continuous"),
                    &[
                        ("with", "tracks"),
                        ("limit[results:tracks]", "10"),
                        ("include[songs]", "albums"),
                        ("l", &self.locale),
                    ],
                    Some(&serde_json::json!({ "data": [next_seed.clone()] })),
                )
                .await;
            let body = match body {
                Ok(b) => b,
                Err(e) if round == 0 => return Err(e),
                Err(_) => break,
            };
            if round == 0 {
                title = str_at(&body, &["/results/station/attributes/name"])
                    .unwrap_or("Station")
                    .to_string();
                station_id = str_at(&body, &["/results/station/id"]).map(str::to_string);
            }
            let batch: Vec<Value> = body
                .pointer("/results/tracks/data")
                .or_else(|| body.pointer("/results/tracks"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let mut last_song: Option<String> = None;
            let before = tracks.len();
            for raw in &batch {
                if let Some(t) = map_library_song(raw) {
                    if seen.insert(t.path.clone()) {
                        if str_at(raw, &["type"]) == Some("songs") {
                            last_song = str_at(raw, &["id"]).map(str::to_string);
                        }
                        tracks.push(t);
                    }
                }
            }
            if tracks.len() == before {
                break;
            }
            match last_song {
                Some(id) => next_seed = serde_json::json!({ "id": id, "type": "songs" }),
                None => break,
            }
        }
        Ok(RadioResult {
            seed_kind,
            seed_id: seed_id.to_string(),
            title,
            tracks,
            station_id,
            video_urls: std::collections::HashMap::new(),
            continuation: None,
        })
    }

    async fn set_favorites(&self, kind: FavKind, op: FavOp, ids: &[String]) -> MhResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let segment = match kind {
            FavKind::Track => "songs",
            FavKind::Album => "albums",
            FavKind::Artist => "artists",
            FavKind::Label => return Err(MhError::Unsupported("Apple Music has no labels".into())),
        };
        match op {
            FavOp::Add => {
                let joined = ids.join(",");
                self.write(
                    reqwest::Method::POST,
                    &format!("{AMP_API}/v1/me/library"),
                    &[(&format!("ids[{segment}]"), joined.as_str())],
                    None,
                )
                .await?;
                Ok(())
            }
            FavOp::Remove => {
                if kind == FavKind::Artist {
                    return Err(MhError::Unsupported(
                        "Apple Music does not expose an artist-unfollow endpoint".into(),
                    ));
                }
                futures_util::future::try_join_all(ids.iter().map(|id| async move {
                    let lib_id = self.library_id_for(segment, id).await?.ok_or_else(|| {
                        MhError::NotFound(format!(
                            "Apple Music: no library id for catalog {} {id}",
                            kind.as_str()
                        ))
                    })?;
                    let url = format!("{AMP_API}/v1/me/library/{segment}/{lib_id}");
                    self.write(reqwest::Method::DELETE, &url, &[], None).await
                }))
                .await?;
                Ok(())
            }
        }
    }

    async fn follow_playlist(&self, id: &str) -> MhResult<()> {
        let url = format!("{AMP_API}/v1/me/library");
        self.write(reqwest::Method::POST, &url, &[("ids[playlists]", id)], None)
            .await?;
        Ok(())
    }
    async fn unfollow_playlist(&self, id: &str) -> MhResult<()> {
        let url = format!("{AMP_API}/v1/me/library/playlists/{id}");
        self.write(reqwest::Method::DELETE, &url, &[], None).await?;
        Ok(())
    }

    async fn create_playlist(&self, input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        let tracks: Vec<Value> = input
            .initial_track_ids
            .iter()
            .map(|id| json!({ "id": id, "type": "songs" }))
            .collect();
        let mut attrs = serde_json::Map::new();
        attrs.insert("name".into(), Value::String(input.name.clone()));
        if let Some(d) = &input.description {
            attrs.insert("description".into(), Value::String(d.clone()));
        }
        let body = json!({
            "attributes": Value::Object(attrs),
            "relationships": { "tracks": { "data": tracks } },
        });
        let url = format!("{AMP_API}/v1/me/library/playlists");
        let resp = self
            .write(reqwest::Method::POST, &url, &[], Some(&body))
            .await?;
        let lib_id = str_at(&resp, &["/data/0/id"]).ok_or_else(|| {
            MhError::Other(format!(
                "Apple Music createPlaylist: no library id in {resp}"
            ))
        })?;
        Ok(PlaylistMutateResult {
            playlist_id: lib_id.to_string(),
            library_id: Some(lib_id.to_string()),
            snapshot_id: None,
        })
    }

    async fn rename_playlist(
        &self,
        id: &str,
        new_name: &str,
        new_description: Option<&str>,
        _new_is_public: Option<bool>,
        _new_is_collaborative: Option<bool>,
    ) -> MhResult<()> {
        let mut attrs = serde_json::Map::new();
        attrs.insert("name".into(), Value::String(new_name.into()));
        if let Some(d) = new_description {
            attrs.insert("description".into(), Value::String(d.into()));
        }
        let body = json!({ "attributes": Value::Object(attrs) });
        let url = format!("{AMP_API}/v1/me/library/playlists/{id}");
        self.write(reqwest::Method::PATCH, &url, &[], Some(&body))
            .await?;
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> MhResult<()> {
        let url = format!("{AMP_API}/v1/me/library/playlists/{id}");
        self.write(reqwest::Method::DELETE, &url, &[], None).await?;
        Ok(())
    }

    async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        let tracks: Vec<Value> = track_ids
            .iter()
            .map(|tid| json!({ "id": tid, "type": "songs" }))
            .collect();
        let body = json!({ "data": tracks });
        let url = format!("{AMP_API}/v1/me/library/playlists/{id}/tracks");
        self.write(reqwest::Method::POST, &url, &[], Some(&body))
            .await?;
        Ok(PlaylistMutateResult {
            playlist_id: id.into(),
            library_id: Some(id.into()),
            snapshot_id: None,
        })
    }

    async fn fetch_saved_ids(&self, kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        let mut out = SavedIdSet::default();
        for kind in kinds {
            match kind {
                SaveKind::Track | SaveKind::Album => {
                    let endpoint = if *kind == SaveKind::Track {
                        "songs"
                    } else {
                        "albums"
                    };
                    let (catalog_ids, library_ids) = self.paged_library_catalog_ids(endpoint).await;
                    out.set_with_library_ids(*kind, catalog_ids, library_ids);
                }
                SaveKind::Artist => {
                    let data = self
                        .library_data("artists", &[("include", "catalog")])
                        .await;
                    out.set(*kind, data.iter().filter_map(library_catalog_id).collect());
                }
                SaveKind::Playlist => {
                    let data = self.library_data("playlists", &[]).await;
                    out.set(
                        *kind,
                        data.iter()
                            .filter_map(|p| str_at(p, &["id"]).map(String::from))
                            .collect(),
                    );
                }
            }
        }
        Ok(out)
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        let body = self
            .get(
                &format!("{AMP_API}/v1/me/library/playlists"),
                &[("limit", "100")],
            )
            .await?;
        let data = body
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut rows = Vec::new();
        for entry in data {
            let id = str_at(&entry, &["id"]).unwrap_or("");
            if id.is_empty() {
                continue;
            }
            let attrs = entry.get("attributes").cloned().unwrap_or(Value::Null);
            let owned = attrs
                .get("canEdit")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !owned {
                continue;
            }
            rows.push(OwnedPlaylistRow {
                platform: ServicePlatform::AppleMusic.as_str().into(),
                service_id: id.to_string(),
                name: str_at(&attrs, &["name"]).unwrap_or("").into(),
                cover_id: artwork_cover(&entry),
                track_count: 0,
                updated_at: 0,
            });
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod am_category_tests {
    use super::{
        album_id_from_url, am_album_key, am_related_id, composition_category, view_category,
    };
    use crate::media::library::{LibraryAlbumDto, LibraryTrackDto};
    use crate::services::common::library::{RecommendationCategory as C, RecommendationItem};
    use serde_json::json;

    fn station(id: &str) -> RecommendationItem {
        RecommendationItem::Mix {
            id: id.into(),
            title: id.into(),
            subtitle: None,
            cover_id: None,
            url: None,
        }
    }

    fn album() -> RecommendationItem {
        RecommendationItem::Album(LibraryAlbumDto {
            album_key: "a".into(),
            title: "A".into(),
            artist: "B".into(),
            year: None,
            cover_id: None,
            track_count: 0,
            artist_id: None,
            ..Default::default()
        })
    }

    fn track() -> RecommendationItem {
        RecommendationItem::Track(LibraryTrackDto {
            path: "https://music.apple.com/song/1".into(),
            size: 0,
            mtime_ns: 0,
            is_video: false,
            ..Default::default()
        })
    }

    #[test]
    fn apple_view_keys_drive_the_category() {
        for (key, want) in [
            ("top-songs", C::Charts),
            ("top-music-videos", C::Charts),
            ("latest-release", C::NewReleases),
            ("similar-artists", C::Discovery),
            ("you-might-also-like", C::Discovery),
            ("featured-albums", C::Editorial),
            ("more-by-artist", C::Other),
            ("other-versions", C::Other),
            ("appears-on", C::Other),
        ] {
            assert_eq!(view_category(key), want, "key: {key}");
        }
    }

    #[test]
    fn view_keys_are_wire_constants_not_display_text() {
        assert_eq!(view_category("Top Songs"), C::Other);
        assert_eq!(view_category("Meilleurs titres"), C::Other);
    }

    #[test]
    fn an_all_station_shelf_is_a_station_shelf() {
        assert_eq!(
            composition_category(&[station("ra.1"), station("ra.2")]),
            C::Stations,
        );
    }

    #[test]
    fn a_shelf_mixing_stations_with_albums_is_not() {
        assert_eq!(composition_category(&[station("ra.1"), album()]), C::Other);
    }

    #[test]
    fn an_all_track_shelf_reads_as_a_list() {
        assert_eq!(composition_category(&[track(), track()]), C::Charts);
    }

    #[test]
    fn empty_never_claims_a_category() {
        assert_eq!(composition_category(&[]), C::Other);
    }

    /// Catalog songs (playlist / album tracks) carry direct relationships.
    #[test]
    fn catalog_song_yields_both_ids() {
        let v = json!({
            "id": "1",
            "attributes": {"url": "https://music.apple.com/gb/album/some-song/1631983005?i=1631983010"},
            "relationships": {
                "artists": {"data": [{"id": "320569549"}]},
                "albums": {"data": [{"id": "1631983005"}]}
            }
        });
        assert_eq!(am_related_id(&v, "artists").as_deref(), Some("320569549"));
        assert_eq!(
            am_album_key(&v, v.get("attributes").unwrap()).as_deref(),
            Some("1631983005"),
        );
    }

    /// Library songs nest the catalog relationships one level deeper.
    #[test]
    fn library_song_reads_through_the_catalog_relationship() {
        let v = json!({
            "id": "i.abc",
            "relationships": {
                "catalog": {"data": [{
                    "id": "1631983010",
                    "relationships": {
                        "artists": {"data": [{"id": "320569549"}]},
                        "albums": {"data": [{"id": "1631983005"}]}
                    }
                }]}
            }
        });
        assert_eq!(am_related_id(&v, "artists").as_deref(), Some("320569549"));
        assert_eq!(am_related_id(&v, "albums").as_deref(), Some("1631983005"));
    }

    /// With no relationships at all the song URL still carries the album id.
    #[test]
    fn album_id_falls_back_to_the_song_url() {
        let attrs = json!({"url": "https://music.apple.com/gb/album/slug/1631983005?i=1631983010"});
        assert_eq!(
            am_album_key(&json!({}), &attrs).as_deref(),
            Some("1631983005")
        );
    }

    #[test]
    fn url_parsing_rejects_non_numeric_tails() {
        assert_eq!(
            album_id_from_url("https://music.apple.com/gb/artist/slug"),
            None
        );
        assert_eq!(album_id_from_url("https://music.apple.com/"), None);
        assert_eq!(
            album_id_from_url("https://music.apple.com/gb/album/x/9?i=1").as_deref(),
            Some("9"),
        );
    }

    #[test]
    fn missing_ids_stay_none_rather_than_guessing() {
        assert_eq!(am_related_id(&json!({"id": "1"}), "artists"), None);
        assert_eq!(am_album_key(&json!({}), &json!({})), None);
    }
}
