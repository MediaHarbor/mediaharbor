use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};

use crate::services::common::library::{
    any_at, cover_id, f64_at, i64_at, id_at, nonempty_at, str_at, string_at, u32_at, year4_at,
    FavKind, FavOp, MutationCapabilities, OwnedPlaylistRow, Page, PlaylistCreateInput,
    PlaylistMutateResult, RadioResult, RadioSeedKind, RecommendationCategory, RecommendationItem,
    RecommendationShelf, RecommendationsPage, SaveKind, SavedIdSet, ServiceCapabilities,
    ServiceLibrary, ServiceLibraryMutations, ServiceLibraryPage, ServicePlatform,
};

const BASE: &str = "https://www.qobuz.com/api.json/0.2";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";

pub struct QobuzLibrary {
    client: Client,
    token: String,
    app_id: String,
    secret: String,
    quality: u8,
    sync_playback_history: bool,
    telemetry_enabled: bool,
}

fn qobuz_format_id(quality: u8) -> u32 {
    match quality {
        5 => 5,
        6 => 6,
        7 => 7,
        _ => 27,
    }
}

impl QobuzLibrary {
    /// Resolves the app credentials first if they are not stored yet, so that
    /// browsing works on a freshly pasted token rather than only after a
    /// download has run.
    pub async fn from_state(state: &crate::BackendState, s: &Settings) -> MhResult<Self> {
        if crate::services::qobuz::app_credentials::configured_pair(s).is_none() {
            let client = state.cached_qobuz_client(s).await?;
            let mut resolved = s.clone();
            resolved.qobuz_app_id = client.app_id.clone();
            resolved.qobuz_secrets = client.secret.clone();
            return Self::from_settings(&resolved);
        }
        Self::from_settings(s)
    }

    pub fn from_settings(s: &Settings) -> MhResult<Self> {
        if s.qobuz_password_or_token.is_empty() {
            return Err(MhError::Auth("Qobuz not connected (no user token)".into()));
        }
        let pair =
            crate::services::qobuz::app_credentials::configured_pair(s).ok_or_else(|| {
                MhError::Auth("Qobuz app credentials have not been resolved yet".into())
            })?;
        let (app_id, secret) = (pair.app_id, pair.secret);
        let client = crate::http_client::ua_client(UA)?;
        Ok(Self {
            client,
            token: s.qobuz_password_or_token.clone(),
            app_id,
            secret,
            quality: s.qobuz_quality,
            sync_playback_history: s.sync_playback_history(ServicePlatform::Qobuz),
            telemetry_enabled: s.telemetry_enabled(ServicePlatform::Qobuz),
        })
    }

    /// The official client opens with a `session/start` handshake. Sending it makes
    /// MediaHarbor's traffic start the way a real client's does; it writes nothing
    /// to the account. Fired at most once per run, only off a genuine browse, and
    /// never allowed to block or fail the response.
    async fn maybe_fire_telemetry(&self) {
        static SENT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !self.telemetry_enabled || self.secret.is_empty() {
            return;
        }
        if SENT.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let (client, app_id, token, secret) = (
            self.client.clone(),
            self.app_id.clone(),
            self.token.clone(),
            self.secret.clone(),
        );
        tokio::spawn(async move {
            crate::services::qobuz::telemetry::report_session_start(client, app_id, token, secret)
                .await;
        });
    }

    async fn get(&self, endpoint: &str, params: &[(&str, &str)]) -> MhResult<Value> {
        let mut url = format!("{BASE}/{endpoint}?app_id={}", self.app_id);
        for (k, v) in params {
            url.push('&');
            url.push_str(k);
            url.push('=');
            url.push_str(&crate::services::common::http::pct_encode(v));
        }
        let resp = self
            .client
            .get(&url)
            .header("X-App-Id", &self.app_id)
            .header("X-User-Auth-Token", &self.token)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(MhError::Network)?;
        crate::services::common::http::read_json(&format!("Qobuz {endpoint}"), resp).await
    }

    async fn discover_index(&self) -> MhResult<RecommendationsPage> {
        let body = self.get("discover/index", &[("genre_ids", "")]).await?;
        let containers = body
            .get("containers")
            .ok_or_else(|| MhError::Parse("Qobuz discover/index: no containers".into()))?;
        const SECTIONS: &[(&str, &str, bool)] = &[
            ("new_releases", "New Releases", false),
            ("recent_releases", "Recent Releases", false),
            ("album_of_the_week", "Album of the Week", false),
            ("qobuzissims", "Qobuzissims", false),
            ("ideal_discography", "Ideal Discography", false),
            ("most_streamed", "Most Streamed", false),
            ("press_awards", "Press Awards", false),
            ("playlists", "Playlists", true),
        ];
        let mut shelves = Vec::new();
        for (key, label, is_playlist) in SECTIONS {
            let Some(items_v) = containers
                .pointer(&format!("/{key}/data/items"))
                .and_then(|x| x.as_array())
            else {
                continue;
            };
            let items: Vec<RecommendationItem> = if *is_playlist {
                items_v
                    .iter()
                    .filter_map(map_playlist)
                    .map(RecommendationItem::Playlist)
                    .collect()
            } else {
                items_v
                    .iter()
                    .filter_map(map_album)
                    .map(RecommendationItem::Album)
                    .collect()
            };
            if items.is_empty() {
                continue;
            }
            shelves.push(RecommendationShelf {
                id: format!("discover-{key}"),
                title: (*label).into(),
                subtitle: None,
                category: if *is_playlist {
                    RecommendationCategory::Editorial
                } else {
                    RecommendationCategory::NewReleases
                },
                items,
            });
        }
        Ok(RecommendationsPage { shelves })
    }
}

fn map_album(v: &Value) -> Option<LibraryAlbumDto> {
    Some(LibraryAlbumDto {
        album_key: id_at(v, &["id"])?,
        title: string_at(v, &["title"]),
        artist: string_at(v, &["/artist/name", "/artists/0/name"]),
        year: year4_at(
            v,
            &["release_date_original", "released_at", "/dates/original"],
        ),
        cover_id: str_at(v, &["/image/large", "/image/small", "/image/thumbnail"])
            .map(|s| cover_id(ServicePlatform::Qobuz, s)),
        track_count: i64_at(v, &["tracks_count", "nb_tracks", "track_count"]).unwrap_or(0),
        artist_id: None,
        ..Default::default()
    })
}

fn map_track(v: &Value) -> Option<LibraryTrackDto> {
    let id = id_at(v, &["id"])?;
    let performer = string_at(v, &["/performer/name", "/album/artist/name"]);
    Some(LibraryTrackDto {
        path: format!("https://play.qobuz.com/track/{id}"),
        title: Some(string_at(v, &["title"])),
        artist: Some(performer.clone()),
        album: nonempty_at(v, &["/album/title"]),
        album_key: id_at(v, &["/album/id"]),
        duration_secs: f64_at(v, &["duration"]),
        track_no: u32_at(v, &["track_number"]),
        disc_no: u32_at(v, &["media_number"]),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: str_at(v, &["/album/image/large", "/album/image/small"])
            .map(|s| cover_id(ServicePlatform::Qobuz, s)),
        primary_artist: Some(performer),
        artist_id: id_at(v, &["/performer/id", "/album/artist/id"]),
        ..Default::default()
    })
}

fn map_artist(v: &Value) -> Option<LibraryArtistDto> {
    Some(LibraryArtistDto {
        key: id_at(v, &["id"])?,
        display: string_at(v, &["name", "/name/display"]),
        album_count: i64_at(v, &["albums_count"]).unwrap_or(0),
        track_count: 0,
        cover_id: str_at(v, &["/image/large", "picture"])
            .map(|s| cover_id(ServicePlatform::Qobuz, s)),
    })
}

fn map_playlist(v: &Value) -> Option<LibraryPlaylistDto> {
    let id = i64_at(v, &["id"])?;
    let covers: Vec<String> = any_at(v, &["images300", "/image/covers"])
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|u| u.as_str())
                .take(4)
                .map(|u| cover_id(ServicePlatform::Qobuz, u))
                .collect()
        })
        .or_else(|| {
            str_at(v, &["/image/rectangle"]).map(|u| vec![cover_id(ServicePlatform::Qobuz, u)])
        })
        .unwrap_or_default();
    Some(LibraryPlaylistDto {
        id,
        name: string_at(v, &["name"]),
        created_at: i64_at(v, &["created_at"]).unwrap_or(0),
        updated_at: i64_at(v, &["updated_at"]).unwrap_or(0),
        track_count: i64_at(v, &["tracks_count"]).unwrap_or(0),
        cover_ids: covers,
        service_id: Some(id.to_string()),
        description: nonempty_at(v, &["description"]),
        owner: nonempty_at(v, &["/owner/name"]),
    })
}

fn items_at<'a>(body: &'a Value, key: &str) -> &'a [Value] {
    body.pointer(&format!("/{key}/items"))
        .and_then(|v| v.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

fn total_at(body: &Value, key: &str) -> Option<u64> {
    body.pointer(&format!("/{key}/total"))
        .and_then(|v| v.as_u64())
}

impl QobuzLibrary {
    /// One page of `favorite/getUserFavorites`. The `kind` doubles as the request's
    /// `type` and the key the items and total are nested under in the response.
    async fn favorites_page<T, F>(
        &self,
        kind: &str,
        page: Page,
        mapper: F,
    ) -> MhResult<ServiceLibraryPage<T>>
    where
        F: Fn(&Value) -> Option<T> + Send + Sync,
    {
        let limit = page.limit.to_string();
        let offset = page.offset.to_string();
        let body = self
            .get(
                "favorite/getUserFavorites",
                &[("type", kind), ("limit", &limit), ("offset", &offset)],
            )
            .await?;
        let items: Vec<T> = items_at(&body, kind).iter().filter_map(mapper).collect();
        Ok(ServiceLibraryPage::of(items, total_at(&body, kind)))
    }
}

#[async_trait]
impl ServiceLibrary for QobuzLibrary {
    fn as_mutations(
        self: std::sync::Arc<Self>,
    ) -> Option<std::sync::Arc<dyn ServiceLibraryMutations>> {
        Some(self)
    }

    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Qobuz
    }
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::audio_only().with_mutations(self.mutation_capabilities())
    }

    async fn albums(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        self.favorites_page("albums", page, map_album).await
    }

    async fn tracks(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        self.favorites_page("tracks", page, map_track).await
    }

    async fn artists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        self.favorites_page("artists", page, map_artist).await
    }

    async fn playlists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        let limit = page.limit.to_string();
        let offset = page.offset.to_string();
        let body = self
            .get(
                "playlist/getUserPlaylists",
                &[("limit", &limit), ("offset", &offset)],
            )
            .await?;
        let items: Vec<LibraryPlaylistDto> = items_at(&body, "playlists")
            .iter()
            .filter_map(map_playlist)
            .collect();
        Ok(ServiceLibraryPage::of(items, total_at(&body, "playlists")))
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        self.maybe_fire_telemetry().await;
        if let Ok(page) = self.discover_index().await {
            if !page.shelves.is_empty() {
                return Ok(page);
            }
        }
        let mut shelves = Vec::new();
        const ALBUM_SHELVES: [(&str, &str); 11] = [
            ("New Releases", "new-releases"),
            ("New Releases Full", "new-releases-full"),
            ("Recent Releases", "recent-releases"),
            ("Editor Picks", "editor-picks"),
            ("Press Awards", "press-awards"),
            ("Most Streamed", "most-streamed"),
            ("Best Sellers", "best-sellers"),
            ("Qobuzissims", "qobuzissims"),
            ("Ideal Discography", "ideal-discography"),
            ("Re-releases", "re-releases"),
            ("Still Trending", "still-trending"),
        ];
        const PLAYLIST_SHELVES: [(&str, &str); 3] = [
            ("Editor Playlists", "editor-picks"),
            ("Last Created Playlists", "last-created"),
            ("Focus Playlists", "focus"),
        ];
        let (album_bodies, playlist_bodies, discover_body, popular_body, weekly_body) = tokio::join!(
            futures_util::future::join_all(ALBUM_SHELVES.iter().map(|(_, kind)| async move {
                self.get("album/getFeatured", &[("type", kind), ("limit", "50")])
                    .await
            })),
            futures_util::future::join_all(PLAYLIST_SHELVES.iter().map(|(_, kind)| async move {
                self.get("playlist/getFeatured", &[("type", kind), ("limit", "50")])
                    .await
            })),
            self.get("discover/playlists", &[("limit", "30")]),
            self.get("most-popular/get", &[("query", "album"), ("limit", "30")]),
            self.get("dynamic-tracks/get", &[("type", "weekly"), ("limit", "50")]),
        );

        for (&(label, kind), body) in ALBUM_SHELVES.iter().zip(album_bodies) {
            if let Ok(body) = body {
                let items: Vec<RecommendationItem> = items_at(&body, "albums")
                    .iter()
                    .filter_map(map_album)
                    .map(RecommendationItem::Album)
                    .collect();
                if !items.is_empty() {
                    shelves.push(RecommendationShelf::new(
                        format!("album-getFeatured-{kind}"),
                        label,
                        RecommendationCategory::Editorial,
                        items,
                    ));
                }
            }
        }
        for (&(label, kind), body) in PLAYLIST_SHELVES.iter().zip(playlist_bodies) {
            if let Ok(body) = body {
                let items: Vec<RecommendationItem> = items_at(&body, "playlists")
                    .iter()
                    .filter_map(map_playlist)
                    .map(RecommendationItem::Playlist)
                    .collect();
                if !items.is_empty() {
                    shelves.push(RecommendationShelf::new(
                        format!("playlist-getFeatured-{kind}"),
                        label,
                        RecommendationCategory::Editorial,
                        items,
                    ));
                }
            }
        }
        if let Ok(body) = discover_body {
            let items: Vec<RecommendationItem> = body
                .get("items")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(map_playlist)
                        .map(RecommendationItem::Playlist)
                        .collect()
                })
                .unwrap_or_default();
            if !items.is_empty() {
                shelves.push(RecommendationShelf::new(
                    "discover-playlists",
                    "Discover Playlists",
                    RecommendationCategory::Discovery,
                    items,
                ));
            }
        }
        if let Ok(body) = popular_body {
            let items: Vec<RecommendationItem> = items_at(&body, "most_popular")
                .iter()
                .filter_map(|e| e.pointer("/content/album").or_else(|| e.get("content")))
                .filter_map(map_album)
                .map(RecommendationItem::Album)
                .collect();
            if !items.is_empty() {
                shelves.push(RecommendationShelf::new(
                    "most-popular",
                    "Most Popular",
                    RecommendationCategory::Charts,
                    items,
                ));
            }
        }
        if let Ok(body) = weekly_body {
            let items: Vec<RecommendationItem> = items_at(&body, "tracks")
                .iter()
                .filter_map(map_track)
                .map(RecommendationItem::Track)
                .collect();
            if !items.is_empty() {
                shelves.insert(
                    0,
                    RecommendationShelf {
                        id: "weekly-q".into(),
                        title: "Weekly Q".into(),
                        subtitle: Some("Your personalised mix this week".into()),
                        category: RecommendationCategory::Discovery,
                        items,
                    },
                );
            }
        }
        Ok(RecommendationsPage { shelves })
    }

    async fn explore(&self) -> MhResult<RecommendationsPage> {
        self.maybe_fire_telemetry().await;
        let body = self.get("genre/list", &[]).await?;
        let tiles: Vec<RecommendationItem> = items_at(&body, "genres")
            .iter()
            .filter_map(|g| {
                let id = g.get("id").and_then(|v| v.as_i64())?;
                let name = str_at(g, &["name"])?.to_string();
                Some(RecommendationItem::PageLink {
                    api_path: format!("genre::{id}"),
                    title: name,
                    icon: None,
                })
            })
            .collect();
        let shelves = if tiles.is_empty() {
            Vec::new()
        } else {
            vec![RecommendationShelf::new(
                "genres",
                "Genres",
                RecommendationCategory::Genre,
                tiles,
            )]
        };
        Ok(RecommendationsPage { shelves })
    }

    async fn explore_page(&self, path: &str) -> MhResult<RecommendationsPage> {
        let genre_id = path.strip_prefix("genre::").unwrap_or(path);
        let mut shelves = Vec::new();

        for (label, kind) in [
            ("New Releases", "new-releases"),
            ("Press Awards", "press-awards"),
            ("Most Streamed", "most-streamed"),
            ("Best Sellers", "best-sellers"),
        ] {
            if let Ok(body) = self
                .get(
                    "album/getFeatured",
                    &[("type", kind), ("genre_ids", genre_id), ("limit", "50")],
                )
                .await
            {
                let items: Vec<RecommendationItem> = items_at(&body, "albums")
                    .iter()
                    .filter_map(map_album)
                    .map(RecommendationItem::Album)
                    .collect();
                if !items.is_empty() {
                    shelves.push(RecommendationShelf::new(
                        format!("genre-{genre_id}-album-{kind}"),
                        label,
                        RecommendationCategory::Editorial,
                        items,
                    ));
                }
            }
        }

        if let Ok(body) = self
            .get(
                "playlist/getFeatured",
                &[
                    ("type", "editor-picks"),
                    ("genre_ids", genre_id),
                    ("limit", "50"),
                ],
            )
            .await
        {
            let items: Vec<RecommendationItem> = items_at(&body, "playlists")
                .iter()
                .filter_map(map_playlist)
                .map(RecommendationItem::Playlist)
                .collect();
            if !items.is_empty() {
                shelves.push(RecommendationShelf::new(
                    format!("genre-{genre_id}-playlists"),
                    "Playlists",
                    RecommendationCategory::Editorial,
                    items,
                ));
            }
        }

        Ok(RecommendationsPage { shelves })
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let body = self
            .get("album/get", &[("album_id", id), ("extra", "track_ids")])
            .await?;
        let album =
            map_album(&body).ok_or_else(|| MhError::NotFound(format!("Qobuz album {id}")))?;
        let tracks: Vec<LibraryTrackDto> = body.pointer("/tracks/items")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|t| {
                if t.get("album").is_some() {
                    return map_track(t);
                }
                let mut t = t.clone();
                if let Value::Object(ref mut m) = t {
                    m.insert("album".into(), serde_json::json!({
                        "id": album.album_key,
                        "title": album.title,
                        "image": { "large": album.cover_id.as_deref().and_then(|s| s.strip_prefix("qobuz:")).unwrap_or("") },
                        "artist": { "name": album.artist },
                    }));
                }
                map_track(&t)
            }).collect())
            .unwrap_or_default();
        Ok(LibraryAlbumDetail { album, tracks })
    }

    async fn album_page(&self, id: &str) -> MhResult<Value> {
        let body = self.get("album/suggest", &[("album_id", id)]).await?;
        let suggestions: Vec<RecommendationItem> = items_at(&body, "albums")
            .iter()
            .filter(|a| str_at(a, &["id"]) != Some(id))
            .filter_map(map_album)
            .map(RecommendationItem::Album)
            .collect();
        let shelves: Vec<RecommendationShelf> = if suggestions.is_empty() {
            Vec::new()
        } else {
            vec![RecommendationShelf::new(
                "album_suggest",
                "You might also like",
                RecommendationCategory::Discovery,
                suggestions,
            )]
        };
        Ok(serde_json::json!({ "shelves": shelves, "raw": body }))
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let body = self
            .get("artist/page", &[("artist_id", id), ("limit", "100")])
            .await?;
        let display = str_at(&body, &["name"])
            .or_else(|| str_at(&body, &["/name/display"]))
            .unwrap_or("")
            .to_string();

        let mut seen_albums = std::collections::HashSet::new();
        let mut albums: Vec<LibraryAlbumDto> = Vec::new();
        if let Some(groups) = body.get("releases").and_then(|v| v.as_array()) {
            for group in groups {
                if let Some(items) = group.pointer("/items").and_then(|v| v.as_array()) {
                    for it in items {
                        if let Some(a) = map_album(it) {
                            if seen_albums.insert(a.album_key.clone()) {
                                albums.push(a);
                            }
                        }
                    }
                }
            }
        }

        let tracks: Vec<LibraryTrackDto> = body
            .pointer("/top_tracks/items")
            .or_else(|| body.get("top_tracks"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(map_track).collect())
            .unwrap_or_default();

        Ok(LibraryArtistDetail {
            key: id.into(),
            display,
            albums,
            tracks,
        })
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        const PAGE: &str = "500";
        let rows_of = |b: &Value| -> Vec<LibraryTrackDto> {
            b.pointer("/tracks/items")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(map_track).collect())
                .unwrap_or_default()
        };
        let body = self
            .get(
                "playlist/get",
                &[
                    ("playlist_id", id),
                    ("extra", "tracks"),
                    ("limit", PAGE),
                    ("offset", "0"),
                ],
            )
            .await?;
        let playlist =
            map_playlist(&body).ok_or_else(|| MhError::NotFound(format!("Qobuz playlist {id}")))?;
        let total = body
            .pointer("/tracks/total")
            .and_then(|v| v.as_u64())
            .unwrap_or(0) as usize;
        let mut tracks = rows_of(&body);
        let mut offset = tracks.len();
        let mut guard = 0;
        while offset < total {
            guard += 1;
            if guard > 40 {
                break;
            }
            let off = offset.to_string();
            let page = self
                .get(
                    "playlist/get",
                    &[
                        ("playlist_id", id),
                        ("extra", "tracks"),
                        ("limit", PAGE),
                        ("offset", &off),
                    ],
                )
                .await?;
            let rows = rows_of(&page);
            if rows.is_empty() {
                break;
            }
            offset += rows.len();
            tracks.extend(rows);
        }
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }

    async fn report_playback(
        &self,
        service_track_id: &str,
        duration_secs: u64,
        _context_uri: Option<&str>,
        _track_index: Option<u64>,
    ) -> MhResult<()> {
        if !self.sync_playback_history || self.secret.is_empty() {
            return Ok(());
        }
        let client = self.client.clone();
        let app_id = self.app_id.clone();
        let token = self.token.clone();
        let secret = self.secret.clone();
        let track_id = service_track_id.to_string();
        let format_id = qobuz_format_id(self.quality);
        tokio::spawn(async move {
            crate::services::qobuz::telemetry::report_playback(
                client,
                app_id,
                token,
                secret,
                track_id,
                format_id,
                duration_secs,
                crate::services::qobuz::telemetry::QobuzContext::default(),
            )
            .await;
        });
        Ok(())
    }
}

impl QobuzLibrary {
    async fn post_form(&self, endpoint: &str, form: &[(&str, &str)]) -> MhResult<Value> {
        let url = format!("{BASE}/{endpoint}?app_id={}", self.app_id);
        let resp = self
            .client
            .post(&url)
            .header("X-App-Id", &self.app_id)
            .header("X-User-Auth-Token", &self.token)
            .header("accept", "application/json")
            .form(form)
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let body: Value = resp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(MhError::Other(format!(
                "Qobuz {endpoint} returned {}: {body}",
                status.as_u16(),
            )));
        }
        Ok(body)
    }

    async fn favorite_mutate(&self, op: &str, key: &str, ids: &[String]) -> MhResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.get(op, &[(key, ids.join(",").as_str())]).await?;
        Ok(())
    }
}

#[async_trait]
impl ServiceLibraryMutations for QobuzLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Qobuz
    }

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities {
            reorder_playlists: false,
            ..MutationCapabilities::full_audio()
        }
    }

    async fn set_favorites(&self, kind: FavKind, op: FavOp, ids: &[String]) -> MhResult<()> {
        let endpoint = match op {
            FavOp::Add => "favorite/create",
            FavOp::Remove => "favorite/delete",
        };
        let key = match kind {
            FavKind::Track => "track_ids",
            FavKind::Album => "album_ids",
            FavKind::Artist => "artist_ids",
            FavKind::Label => "label_ids",
        };
        self.favorite_mutate(endpoint, key, ids).await
    }

    async fn follow_playlist(&self, id: &str) -> MhResult<()> {
        self.post_form("playlist/subscribe", &[("playlist_id", id)])
            .await?;
        Ok(())
    }
    async fn unfollow_playlist(&self, id: &str) -> MhResult<()> {
        self.post_form("playlist/unsubscribe", &[("playlist_id", id)])
            .await?;
        Ok(())
    }

    async fn create_playlist(&self, input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        let desc = input.description.clone().unwrap_or_default();
        let is_public = if input.is_public { "true" } else { "false" };
        let is_collaborative = if input.is_collaborative {
            "true"
        } else {
            "false"
        };
        let body = self
            .post_form(
                "playlist/create",
                &[
                    ("name", input.name.as_str()),
                    ("description", desc.as_str()),
                    ("is_public", is_public),
                    ("is_collaborative", is_collaborative),
                ],
            )
            .await?;
        let id = body
            .get("id")
            .and_then(|v| {
                v.as_i64()
                    .map(|i| i.to_string())
                    .or_else(|| v.as_str().map(str::to_string))
            })
            .ok_or_else(|| MhError::Other(format!("Qobuz createPlaylist: no id in {body}")))?;
        if !input.initial_track_ids.is_empty() {
            let joined = input.initial_track_ids.join(",");
            self.post_form(
                "playlist/addTracks",
                &[
                    ("playlist_id", id.as_str()),
                    ("track_ids", joined.as_str()),
                    ("no_duplicate", "true"),
                ],
            )
            .await?;
        }
        Ok(PlaylistMutateResult::id(id))
    }

    async fn rename_playlist(
        &self,
        id: &str,
        new_name: &str,
        new_description: Option<&str>,
        new_is_public: Option<bool>,
        new_is_collaborative: Option<bool>,
    ) -> MhResult<()> {
        let desc = new_description.unwrap_or("");
        let mut form: Vec<(&str, &str)> = vec![
            ("playlist_id", id),
            ("name", new_name),
            ("description", desc),
        ];
        if let Some(p) = new_is_public {
            form.push(("is_public", if p { "true" } else { "false" }));
        }
        if let Some(c) = new_is_collaborative {
            form.push(("is_collaborative", if c { "true" } else { "false" }));
        }
        self.post_form("playlist/update", &form).await?;
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> MhResult<()> {
        self.post_form("playlist/delete", &[("playlist_id", id)])
            .await?;
        Ok(())
    }

    async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        let joined = track_ids.join(",");
        self.post_form(
            "playlist/addTracks",
            &[
                ("playlist_id", id),
                ("track_ids", joined.as_str()),
                ("no_duplicate", "true"),
            ],
        )
        .await?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn remove_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
        _positions: Option<&[u32]>,
    ) -> MhResult<PlaylistMutateResult> {
        let body = self
            .get(
                "playlist/get",
                &[("playlist_id", id), ("extra", "tracks"), ("limit", "1000")],
            )
            .await?;
        let items = body
            .pointer("/tracks/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut pti = Vec::new();
        for item in &items {
            let track_id = item
                .get("id")
                .and_then(|v| {
                    v.as_i64()
                        .map(|i| i.to_string())
                        .or_else(|| v.as_str().map(str::to_string))
                })
                .unwrap_or_default();
            if track_ids.iter().any(|t| t == &track_id) {
                if let Some(p) = item.get("playlist_track_id").and_then(|v| v.as_i64()) {
                    pti.push(p.to_string());
                } else if let Some(p) = str_at(item, &["playlist_track_id"]) {
                    pti.push(p.to_string());
                }
            }
        }
        if pti.is_empty() {
            return Err(MhError::Other(
                "Qobuz remove_playlist_tracks: no matching playlist_track_ids found".into(),
            ));
        }
        self.post_form(
            "playlist/deleteTracks",
            &[
                ("playlist_id", id),
                ("playlist_track_ids", pti.join(",").as_str()),
            ],
        )
        .await?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn radio_for(&self, seed_kind: RadioSeedKind, seed_id: &str) -> MhResult<RadioResult> {
        let (endpoint, id_param, station_id) = match seed_kind {
            RadioSeedKind::Artist => ("radio/artist", "artist_id", seed_id.to_string()),
            RadioSeedKind::Album => ("radio/album", "album_id", seed_id.to_string()),
            RadioSeedKind::Track => {
                let body = self.get("track/get", &[("track_id", seed_id)]).await?;
                let album_id = body
                    .pointer("/album/id")
                    .and_then(|v| v.as_str().map(str::to_string));
                match album_id {
                    Some(aid) => ("radio/album", "album_id", aid),
                    None => {
                        let artist_id = body
                            .pointer("/album/artist/id")
                            .or_else(|| body.pointer("/performer/id"))
                            .and_then(|v| {
                                v.as_i64()
                                    .map(|i| i.to_string())
                                    .or_else(|| v.as_str().map(str::to_string))
                            })
                            .ok_or_else(|| {
                                MhError::Other("Qobuz: no album/artist id on seed track".into())
                            })?;
                        ("radio/artist", "artist_id", artist_id)
                    }
                }
            }
            RadioSeedKind::Playlist => {
                return Err(MhError::Other(
                    "Qobuz radio supports track / album / artist seeds only".into(),
                ))
            }
        };

        let body = self
            .get(endpoint, &[(id_param, station_id.as_str())])
            .await?;
        let title = str_at(&body, &["title"])
            .map(str::to_string)
            .unwrap_or_else(|| format!("Qobuz radio · {seed_id}"));
        let mut seen = std::collections::HashSet::new();
        let tracks: Vec<LibraryTrackDto> = body
            .pointer("/tracks/items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(map_track)
                    .filter(|t| seen.insert(t.path.clone()))
                    .collect()
            })
            .unwrap_or_default();

        Ok(RadioResult {
            seed_kind,
            seed_id: seed_id.to_string(),
            title,
            tracks,
            station_id: Some(station_id),
            video_urls: Default::default(),
            continuation: None,
        })
    }

    async fn fetch_saved_ids(&self, kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        let mut out = SavedIdSet::default();
        for kind in kinds {
            let type_param = match kind {
                SaveKind::Track => "tracks",
                SaveKind::Album => "albums",
                SaveKind::Artist => "artists",
                SaveKind::Playlist => continue,
            };
            let body = self
                .get(
                    "favorite/getUserFavorites",
                    &[("type", type_param), ("limit", "1000"), ("offset", "0")],
                )
                .await?;
            let items = body
                .get(type_param)
                .and_then(|v| v.get("items"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let ids: Vec<String> = items
                .iter()
                .filter_map(|x| crate::services::common::library::json_id_at(x, "id"))
                .collect();
            out.set(*kind, ids);
        }
        Ok(out)
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        let body = self
            .get(
                "playlist/getUserPlaylists",
                &[("limit", "500"), ("offset", "0")],
            )
            .await?;
        let items = body
            .get("playlists")
            .and_then(|v| v.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut rows = Vec::new();
        for item in items {
            let id = crate::services::common::library::json_id_at(&item, "id").unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            rows.push(OwnedPlaylistRow {
                platform: ServicePlatform::Qobuz.as_str().into(),
                service_id: id,
                name: str_at(&item, &["name"]).unwrap_or("").into(),
                cover_id: str_at(&item, &["/images300/0"])
                    .or_else(|| str_at(&item, &["/image_rectangle/0"]))
                    .map(|s| cover_id(ServicePlatform::Qobuz, s)),
                track_count: item
                    .get("tracks_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0),
                updated_at: item.get("updated_at").and_then(|v| v.as_i64()).unwrap_or(0),
            });
        }
        Ok(rows)
    }
}
