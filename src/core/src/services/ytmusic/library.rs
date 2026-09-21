use std::collections::HashSet;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};

use crate::services::common::innertube::{self, collect_renderers as collect, Innertube};
use crate::services::common::library::{
    cover_id, parse_hms_seconds, stable_hash_i64, str_at, FavKind, FavOp, MutationCapabilities,
    OwnedPlaylistRow, Page, PlaylistCreateInput, PlaylistMutateResult, RadioResult, RadioSeedKind,
    RecommendationCategory, RecommendationItem, RecommendationShelf, RecommendationsPage, SaveKind,
    SavedIdSet, ServiceCapabilities, ServiceLibrary, ServiceLibraryMutations, ServiceLibraryPage,
    ServicePlatform,
};

const BROWSE_URL: &str = "https://music.youtube.com/youtubei/v1/browse?prettyPrint=false";

pub struct YtMusicLibrary {
    it: Innertube,
    sync_playback_history: bool,
}

impl YtMusicLibrary {
    pub fn from_settings(s: &Settings) -> MhResult<Self> {
        let cookies_path = if !s.ytmusic_cookies_path.trim().is_empty() {
            s.ytmusic_cookies_path.clone()
        } else {
            s.cookies.clone()
        };
        if cookies_path.is_empty() {
            return Err(MhError::Auth(
                "YouTube Music not connected: settings.cookies path empty".into(),
            ));
        }
        Ok(Self {
            it: Innertube::from_cookies(
                &cookies_path,
                innertube::YTMUSIC,
                "Cookies file has no SAPISID/__Secure-3PAPISID",
            )?,
            sync_playback_history: s.sync_playback_history(ServicePlatform::YtMusic),
        })
    }

    async fn browse(&self, browse_id: &str) -> MhResult<Value> {
        self.it.browse(BROWSE_URL, browse_id, None).await
    }

    async fn browse_with_params(&self, browse_id: &str, params: &str) -> MhResult<Value> {
        self.it.browse(BROWSE_URL, browse_id, Some(params)).await
    }

    async fn browse_continuation(&self, token: &str) -> MhResult<Value> {
        self.it.continuation(BROWSE_URL, token).await
    }

    /// Every row of a listing's own track shelf, following that shelf's continuations
    /// to the end.
    ///
    /// Scoped to the shelf on purpose. A playlist page carries ~100 rows plus a token
    /// for the next slice, but it also carries unrelated carousels built from the same
    /// row renderer — searching the whole document for rows and for a token mixed
    /// "you might also like" entries into the track list and could follow the wrong
    /// token entirely. Without following the right one, a 1000-track Liked Music came
    /// back as its first 100 and any queue built from it simply stopped there.
    async fn shelf_rows_all<T, F>(&self, first: &Value, renderers: &[&str], map: F) -> Vec<T>
    where
        F: Fn(&Value) -> Option<T>,
    {
        const SHELVES: &[&str] = &["musicPlaylistShelfRenderer", "musicShelfRenderer"];
        const CONTINUATIONS: &[&str] = &[
            "musicPlaylistShelfContinuation",
            "musicShelfContinuation",
            "sectionListContinuation",
        ];

        let rows_of = |shelf: &Value| -> Vec<T> {
            shelf
                .get("contents")
                .and_then(|c| c.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|it| renderers.iter().find_map(|r| it.get(*r)))
                        .filter_map(&map)
                        .collect()
                })
                .unwrap_or_default()
        };
        let token_of = |shelf: &Value| -> Option<String> {
            if let Some(t) = shelf
                .get("continuations")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.iter().find_map(innertube::continuation_token))
            {
                return Some(t);
            }
            shelf
                .get("contents")
                .and_then(|c| c.as_array())
                .and_then(|items| {
                    items
                        .iter()
                        .filter_map(|it| it.get("continuationItemRenderer"))
                        .find_map(innertube::continuation_token)
                })
        };

        let Some((mut out, shelf)) = collect(first, SHELVES)
            .into_iter()
            .map(|(_, sh)| (rows_of(&sh), sh))
            .max_by_key(|(rows, _)| rows.len())
        else {
            return Vec::new();
        };
        let mut token = token_of(&shelf);
        let mut seen: HashSet<String> = HashSet::new();
        let mut guard = 0;
        while let Some(t) = token.take() {
            guard += 1;
            if guard > 60 || !seen.insert(t.clone()) {
                break;
            }
            let Ok(cont) = self.browse_continuation(&t).await else {
                break;
            };
            let node = cont
                .get("continuationContents")
                .and_then(|c| CONTINUATIONS.iter().find_map(|k| c.get(*k)))
                .cloned()
                .or_else(|| {
                    cont.get("onResponseReceivedActions")
                        .and_then(|a| a.as_array())
                        .and_then(|actions| {
                            actions.iter().find_map(|a| {
                                a.pointer("/appendContinuationItemsAction/continuationItems")
                                    .cloned()
                            })
                        })
                        .map(|items| json!({ "contents": items }))
                });
            let Some(node) = node else {
                break;
            };
            let rows = rows_of(&node);
            if rows.is_empty() {
                break;
            }
            out.extend(rows);
            token = token_of(&node);
        }
        out
    }

    pub async fn probe_session(&self) -> MhResult<bool> {
        let url = format!("{YTM_BASE}/account/account_menu?prettyPrint=false");
        let (status, v) = self
            .it
            .post_status(&url, json!({ "context": self.it.context() }))
            .await?;
        if matches!(status.as_u16(), 401 | 403) {
            return Ok(false);
        }
        if !status.is_success() {
            return Err(crate::services::common::http::api_error(
                "YT Music account probe",
                status,
                &v.to_string(),
            ));
        }
        Ok(!collect(
            &v,
            &[
                "activeAccountHeaderRenderer",
                "googleAccountHeaderRenderer",
                "accountName",
            ],
        )
        .is_empty())
    }
}

fn norm_dedup(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth: i32 = 0;
    for ch in s.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = (depth - 1).max(0),
            _ if depth == 0 && ch.is_alphanumeric() => out.extend(ch.to_lowercase()),
            _ => {}
        }
    }
    out
}
/// YT Music titles are split across runs ("Artist", " • ", "Album"), so every
/// run is joined; `first_run_text` is for the fields where only the head matters.
fn text_of(v: &Value) -> Option<String> {
    innertube::text_of(v, true)
}

fn first_run_text(v: &Value) -> Option<String> {
    innertube::text_of(v, false)
}

/// The artwork arrays YT Music publishes, in preference order.
///
/// `thumbnails` and `sources` are two names for the same thing — a list whose last
/// entry is the largest — so one loop reads both; splitting them by array name only
/// invited a new shape to be filed under the wrong half.
const THUMBNAIL_PATHS: &[&str] = &[
    "/musicThumbnailRenderer/thumbnail/thumbnails",
    "/thumbnail/thumbnails",
    "/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
    "/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails",
    "/contentImage/musicThumbnailRenderer/thumbnail/thumbnails",
    "/backgroundImage/musicThumbnailRenderer/thumbnail/thumbnails",
    "/contentImage/collageHeroImageViewModel/image/sources",
    "/contentImage/thumbnailViewModel/image/sources",
    "/image/sources",
    "/thumbnailViewModel/image/sources",
    "/collageHeroImageViewModel/image/sources",
    "/contentImage/collageThumbnailViewModel/primaryThumbnail/musicThumbnailRenderer/thumbnail/thumbnails",
];

fn last_thumbnail(v: &Value) -> Option<String> {
    for p in THUMBNAIL_PATHS {
        if let Some(u) = v
            .pointer(p)
            .and_then(|x| x.as_array())
            .and_then(|arr| arr.last())
            .and_then(|t| t.get("url"))
            .and_then(|u| u.as_str())
        {
            return Some(u.to_string());
        }
    }
    deepest_thumbnail(v, 0)
}

/// Last resort for `last_thumbnail`: the first `thumbnails`/`sources` array anywhere in
/// the node whose entries carry a `url`.
///
/// Personal mixes and auto-playlists wrap their artwork in renderers the fixed pointer
/// list above does not name — a generated 2x2 collage rather than a single cover — and
/// every unnamed one silently produced a card with no image. The depth cap keeps this
/// off the hot path for the big carousel payloads.
fn deepest_thumbnail(v: &Value, depth: u8) -> Option<String> {
    const MAX_DEPTH: u8 = 8;
    if depth > MAX_DEPTH {
        return None;
    }
    match v {
        Value::Object(map) => {
            for key in ["thumbnails", "sources"] {
                if let Some(url) = map
                    .get(key)
                    .and_then(|x| x.as_array())
                    .and_then(|arr| arr.last())
                    .and_then(|t| t.get("url"))
                    .and_then(|u| u.as_str())
                {
                    return Some(url.to_string());
                }
            }
            map.values()
                .find_map(|val| deepest_thumbnail(val, depth + 1))
        }
        Value::Array(arr) => arr
            .iter()
            .find_map(|item| deepest_thumbnail(item, depth + 1)),
        _ => None,
    }
}

fn two_row_page_type(v: &Value) -> &str {
    str_at(v, &["/navigationEndpoint/browseEndpoint/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType"]).unwrap_or("")
}

fn two_row_browse_id(v: &Value) -> Option<String> {
    str_at(v, &["/navigationEndpoint/browseEndpoint/browseId"]).map(str::to_string)
}

fn two_row_title(v: &Value) -> String {
    v.pointer("/title").and_then(text_of).unwrap_or_default()
}

fn two_row_subtitle(v: &Value) -> String {
    v.pointer("/subtitle").and_then(text_of).unwrap_or_default()
}

fn two_row_subtitle_browse_ids(v: &Value) -> (Option<String>, Option<String>) {
    let mut artist_id = None;
    let mut album_key = None;
    let mut consider = |b: &str| {
        if b.starts_with("UC") && artist_id.is_none() {
            artist_id = Some(b.to_string());
        } else if b.starts_with("MPRE") && album_key.is_none() {
            album_key = Some(b.to_string());
        }
    };
    if let Some(runs) = v.pointer("/subtitle/runs").and_then(|r| r.as_array()) {
        for run in runs {
            if let Some(b) = str_at(run, &["/navigationEndpoint/browseEndpoint/browseId"]) {
                consider(b);
            }
        }
    }
    if let Some(items) = v
        .pointer("/menu/menuRenderer/items")
        .and_then(|x| x.as_array())
    {
        for mi in items {
            if let Some(b) = str_at(
                mi,
                &["/menuNavigationItemRenderer/navigationEndpoint/browseEndpoint/browseId"],
            ) {
                consider(b);
            }
        }
    }
    (artist_id, album_key)
}

fn two_row_owner(v: &Value) -> Option<String> {
    two_row_subtitle(v)
        .split(" • ")
        .map(str::trim)
        .find(|p| {
            let l = p.to_lowercase();
            !p.is_empty()
                && l != "playlist"
                && l != "podcast"
                && !l.contains("song")
                && !l.contains("view")
                && !l.contains("episode")
        })
        .map(str::to_string)
}

fn map_two_row_to_album(v: &Value) -> Option<LibraryAlbumDto> {
    let browse_id = two_row_browse_id(v)?;
    let title = two_row_title(v);
    let subtitle = two_row_subtitle(v);
    let mut parts = subtitle.split(" • ").skip(1);
    let artist = parts.next().unwrap_or("").to_string();
    let year = parts.next().map(str::to_string);
    let cover = v.pointer("/thumbnailRenderer").and_then(last_thumbnail);
    let (artist_id, _) = two_row_subtitle_browse_ids(v);
    Some(LibraryAlbumDto {
        album_key: browse_id,
        title,
        artist,
        year,
        cover_id: cover.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
        track_count: 0,
        artist_id,
        ..Default::default()
    })
}

fn map_two_row_to_playlist(v: &Value) -> Option<LibraryPlaylistDto> {
    let browse_id = two_row_browse_id(v)?;
    let title = two_row_title(v);
    let cover = v.pointer("/thumbnailRenderer").and_then(last_thumbnail);
    Some(LibraryPlaylistDto {
        id: stable_hash_i64(&browse_id),
        name: title,
        created_at: 0,
        updated_at: 0,
        track_count: 0,
        cover_ids: cover
            .map(|u| vec![cover_id(ServicePlatform::YtMusic, &u)])
            .unwrap_or_default(),
        service_id: Some(browse_id),
        description: None,
        owner: two_row_owner(v),
    })
}

fn map_two_row_to_artist(v: &Value) -> Option<LibraryArtistDto> {
    let browse_id = two_row_browse_id(v)?;
    let name = two_row_title(v);
    let cover = v.pointer("/thumbnailRenderer").and_then(last_thumbnail);
    Some(LibraryArtistDto {
        key: browse_id,
        display: name,
        album_count: 0,
        track_count: 0,
        cover_id: cover.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
    })
}

fn responsive_album_browse_id(v: &Value, album_col: Option<&Value>) -> Option<String> {
    if let Some(items) = v
        .pointer("/menu/menuRenderer/items")
        .and_then(|x| x.as_array())
    {
        for mi in items {
            if let Some(bid) = str_at(
                mi,
                &["/menuNavigationItemRenderer/navigationEndpoint/browseEndpoint/browseId"],
            ) {
                if bid.starts_with("MPRE") {
                    return Some(bid.to_string());
                }
            }
        }
    }
    album_col
        .and_then(|c| c.pointer("/text/runs"))
        .and_then(|r| r.as_array())
        .and_then(|runs| {
            runs.iter().find_map(|run| {
                str_at(run, &["/navigationEndpoint/browseEndpoint/browseId"])
                    .filter(|b| b.starts_with("MPRE"))
                    .map(str::to_string)
            })
        })
}

fn map_responsive_to_track(v: &Value) -> Option<LibraryTrackDto> {
    let flex = v.get("flexColumns").and_then(|x| x.as_array())?;
    let title_col = flex
        .first()?
        .get("musicResponsiveListItemFlexColumnRenderer")?;
    let title = title_col.get("text").and_then(text_of).unwrap_or_default();
    let video_id = str_at(
        title_col,
        &["/text/runs/0/navigationEndpoint/watchEndpoint/videoId"],
    )
    .or_else(|| str_at(v, &["/playlistItemData/videoId"]))
    .map(str::to_string)?;
    let artist_col = flex
        .get(1)
        .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"));
    let artist = artist_col.and_then(|c| c.get("text").and_then(text_of));
    let album_col = flex
        .get(2)
        .and_then(|c| c.get("musicResponsiveListItemFlexColumnRenderer"));
    let album = album_col
        .and_then(|c| c.get("text").and_then(text_of))
        .filter(|s| !s.is_empty());
    let album_key = responsive_album_browse_id(v, album_col);
    let artist_id = artist_col
        .and_then(|c| c.pointer("/text/runs"))
        .and_then(|r| r.as_array())
        .and_then(|runs| {
            runs.iter().find_map(|run| {
                str_at(run, &["/navigationEndpoint/browseEndpoint/browseId"])
                    .filter(|b| b.starts_with("UC"))
                    .map(str::to_string)
            })
        });
    let dur = v
        .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text")
        .and_then(text_of)
        .as_deref()
        .and_then(parse_hms_seconds);
    let thumb = last_thumbnail(v);
    Some(LibraryTrackDto {
        path: format!("https://music.youtube.com/watch?v={video_id}"),
        title: Some(title),
        artist: artist.clone(),
        album,
        album_key,
        duration_secs: dur,
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: thumb.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
        primary_artist: artist,
        artist_id,
        ..Default::default()
    })
}

fn blank(slot: &Option<String>) -> bool {
    slot.as_deref().unwrap_or("").trim().is_empty()
}

fn fill_if_blank(slot: &mut Option<String>, value: Option<&str>) {
    if !blank(slot) {
        return;
    }
    if let Some(v) = value.map(str::trim).filter(|s| !s.is_empty()) {
        *slot = Some(v.to_string());
    }
}

/// Fill blank fields from what the page states once, in its header.
/// Never overwrites a value the row carried itself.
fn inherit_from_header(
    t: &mut LibraryTrackDto,
    artist: Option<&str>,
    album: Option<&str>,
    album_key: Option<&str>,
    cover_id: Option<&str>,
    artist_id: Option<&str>,
) {
    fill_if_blank(&mut t.artist, artist);
    fill_if_blank(&mut t.primary_artist, artist);
    fill_if_blank(&mut t.album, album);
    fill_if_blank(&mut t.album_key, album_key);
    fill_if_blank(&mut t.cover_id, cover_id);
    fill_if_blank(&mut t.artist_id, artist_id);
}

fn map_multi_row_to_episode(v: &Value) -> Option<LibraryTrackDto> {
    let video_id = str_at(v, &["/onTap/watchEndpoint/videoId"])?;
    let title = v.pointer("/title").and_then(text_of).unwrap_or_default();
    let subtitle = v.pointer("/subtitle").and_then(text_of);
    let thumb = last_thumbnail(v);
    Some(LibraryTrackDto {
        path: format!("https://music.youtube.com/watch?v={video_id}"),
        title: Some(title),
        artist: subtitle.clone(),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: thumb.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
        primary_artist: subtitle,
        ..Default::default()
    })
}

fn split_two_rows(items: Vec<(String, Value)>) -> SplitItems {
    let mut s = SplitItems::default();
    for (_, item) in items {
        let pt = two_row_page_type(&item);
        match pt {
            "MUSIC_PAGE_TYPE_ALBUM" => {
                if let Some(a) = map_two_row_to_album(&item) {
                    s.albums.push(a);
                }
            }
            "MUSIC_PAGE_TYPE_PLAYLIST"
            | "MUSIC_PAGE_TYPE_AUDIOBOOK"
            | "MUSIC_PAGE_TYPE_PODCAST" => {
                if let Some(p) = map_two_row_to_playlist(&item) {
                    s.playlists.push(p);
                }
            }
            "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => {
                if let Some(a) = map_two_row_to_artist(&item) {
                    s.artists.push(a);
                }
            }
            _ => {}
        }
    }
    s
}

#[derive(Default)]
struct SplitItems {
    albums: Vec<LibraryAlbumDto>,
    playlists: Vec<LibraryPlaylistDto>,
    artists: Vec<LibraryArtistDto>,
}

fn shelf_category(renderer: &str, items: &[RecommendationItem]) -> RecommendationCategory {
    if renderer == "musicShelfRenderer" {
        return RecommendationCategory::Charts;
    }
    if !items.is_empty()
        && items
            .iter()
            .all(|i| matches!(i, RecommendationItem::Track(_)))
    {
        return RecommendationCategory::Charts;
    }
    RecommendationCategory::Other
}

fn classify_two_row(v: &Value) -> Option<RecommendationItem> {
    match two_row_page_type(v) {
        "MUSIC_PAGE_TYPE_ALBUM" => map_two_row_to_album(v).map(RecommendationItem::Album),
        "MUSIC_PAGE_TYPE_PLAYLIST" | "MUSIC_PAGE_TYPE_AUDIOBOOK" | "MUSIC_PAGE_TYPE_PODCAST" => {
            map_two_row_to_playlist(v).map(RecommendationItem::Playlist)
        }
        "MUSIC_PAGE_TYPE_ARTIST" | "MUSIC_PAGE_TYPE_USER_CHANNEL" => {
            map_two_row_to_artist(v).map(RecommendationItem::Artist)
        }
        _ => {
            let video_id = str_at(v, &["/navigationEndpoint/watchEndpoint/videoId"])?;
            let title = two_row_title(v);
            let subtitle = two_row_subtitle(v);
            let cover = v.pointer("/thumbnailRenderer").and_then(last_thumbnail);
            let (artist_id, album_key) = two_row_subtitle_browse_ids(v);
            Some(RecommendationItem::Track(LibraryTrackDto {
                path: format!("https://music.youtube.com/watch?v={video_id}"),
                title: Some(title),
                artist: Some(subtitle.clone()),
                album_key,
                size: 0,
                mtime_ns: 0,
                is_video: false,
                cover_id: cover.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
                primary_artist: Some(subtitle),
                artist_id,
                ..Default::default()
            }))
        }
    }
}

fn next_home_token(root: &Value) -> Option<String> {
    const PATHS: &[&str] = &[
        "/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/continuations/0/nextContinuationData/continuation",
        "/continuationContents/sectionListContinuation/continuations/0/nextContinuationData/continuation",
    ];
    for p in PATHS {
        if let Some(t) = root.pointer(p).and_then(|v| v.as_str()) {
            return Some(t.to_string());
        }
    }
    innertube::continuation_token(root)
}

fn shelves_from_carousels(root: &Value) -> Vec<RecommendationShelf> {
    let shelves = collect(root, &["musicCarouselShelfRenderer", "musicShelfRenderer"]);
    let mut out = Vec::new();
    let mut seen_titles: HashSet<String> = HashSet::new();
    for (kind, shelf) in shelves {
        let title = shelf
            .pointer("/header/musicCarouselShelfBasicHeaderRenderer/title")
            .or_else(|| shelf.pointer("/title"))
            .and_then(text_of)
            .unwrap_or_default();
        if !title.is_empty() && !seen_titles.insert(title.clone()) {
            continue;
        }
        let contents = shelf
            .get("contents")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        let mut item_keys = HashSet::new();
        for c in contents {
            if let Some(tri) = c.get("musicTwoRowItemRenderer") {
                if let Some(item) = classify_two_row(tri) {
                    let key = format!("{item:?}");
                    if item_keys.insert(key) {
                        items.push(item);
                    }
                }
            } else if let Some(rli) = c.get("musicResponsiveListItemRenderer") {
                if let Some(t) = map_responsive_to_track(rli) {
                    items.push(RecommendationItem::Track(t));
                }
            }
        }
        if items.is_empty() {
            continue;
        }
        let category = shelf_category(&kind, &items);
        out.push(RecommendationShelf::new(
            title.clone(),
            title,
            category,
            items,
        ));
    }
    crate::services::common::library::trim_feed_tail(
        crate::services::common::library::merge_duplicate_shelves(out),
        crate::services::common::library::FeedTrim::default(),
    )
}

#[async_trait]
impl ServiceLibrary for YtMusicLibrary {
    fn as_mutations(
        self: std::sync::Arc<Self>,
    ) -> Option<std::sync::Arc<dyn ServiceLibraryMutations>> {
        Some(self)
    }

    fn platform(&self) -> ServicePlatform {
        ServicePlatform::YtMusic
    }
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::all().with_mutations(self.mutation_capabilities())
    }

    async fn tracks(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        let body = self.browse("VLLM").await?;
        let items: Vec<LibraryTrackDto> = self
            .shelf_rows_all(&body, &["musicResponsiveListItemRenderer"], |r| {
                map_responsive_to_track(r)
            })
            .await;
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn albums(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        let body = match self.browse("FEmusic_liked_albums").await {
            Ok(b) => b,
            Err(_) => self.browse("FEmusic_library_landing").await?,
        };
        let two_rows = collect(&body, &["musicTwoRowItemRenderer"]);
        let s = split_two_rows(two_rows);
        Ok(ServiceLibraryPage::counted(s.albums))
    }

    async fn playlists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        let body = self.browse("FEmusic_liked_playlists").await?;
        let two_rows = collect(&body, &["musicTwoRowItemRenderer"]);
        let mut items = split_two_rows(two_rows).playlists;

        if let Ok(pod_body) = self.browse("FEmusic_library_non_music_audio_list").await {
            let pod_rows = collect(&pod_body, &["musicTwoRowItemRenderer"]);
            for (_, r) in pod_rows {
                if let Some(p) = map_two_row_to_playlist(&r) {
                    if !items.iter().any(|e| e.service_id == p.service_id) {
                        items.push(p);
                    }
                }
            }
        }

        Ok(ServiceLibraryPage::counted(items))
    }

    async fn artists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        let body = self.browse("FEmusic_library_corpus_artists").await?;
        let two_rows = collect(&body, &["musicTwoRowItemRenderer"]);
        let s = split_two_rows(two_rows);
        let mut artists = s.artists;
        let rli = collect(&body, &["musicResponsiveListItemRenderer"]);
        for (_, r) in rli {
            if let Some(nav) = r.pointer("/navigationEndpoint/browseEndpoint") {
                let pt = str_at(nav, &["/browseEndpointContextSupportedConfigs/browseEndpointContextMusicConfig/pageType"]).unwrap_or("");
                if pt == "MUSIC_PAGE_TYPE_ARTIST" || pt == "MUSIC_PAGE_TYPE_USER_CHANNEL" {
                    let id = str_at(nav, &["browseId"]).unwrap_or("").to_string();
                    if id.is_empty() {
                        continue;
                    }
                    let name = r
                        .pointer("/flexColumns/0/musicResponsiveListItemFlexColumnRenderer/text")
                        .and_then(text_of)
                        .unwrap_or_default();
                    let cover = last_thumbnail(&r);
                    artists.push(LibraryArtistDto {
                        key: id,
                        display: name,
                        album_count: 0,
                        track_count: 0,
                        cover_id: cover.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
                    });
                }
            }
        }
        Ok(ServiceLibraryPage::counted(artists))
    }

    async fn videos(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        let body = self.browse("FEmusic_liked_videos").await?;
        let rows = collect(&body, &["musicResponsiveListItemRenderer"]);
        let items: Vec<LibraryTrackDto> = rows
            .iter()
            .filter_map(|(_, r)| {
                map_responsive_to_track(r).map(|mut t| {
                    t.is_video = true;
                    t
                })
            })
            .collect();
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        let body = self.browse("FEmusic_home").await?;
        let mut shelves = shelves_from_carousels(&body);
        let mut seen: HashSet<String> = shelves
            .iter()
            .map(|s| s.title.clone())
            .filter(|t| !t.is_empty())
            .collect();
        let mut token = next_home_token(&body);
        let mut used_tokens: HashSet<String> = HashSet::new();

        let mut guard = 0;
        while let Some(t) = token.take() {
            guard += 1;
            if guard > 8 || !used_tokens.insert(t.clone()) {
                break;
            }
            let Ok(cont) = self.browse_continuation(&t).await else {
                break;
            };
            for sh in shelves_from_carousels(&cont) {
                if sh.title.is_empty() || seen.insert(sh.title.clone()) {
                    shelves.push(sh);
                }
            }
            token = next_home_token(&cont);
        }

        Ok(RecommendationsPage { shelves })
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let body = self.browse(id).await?;
        let resp_header = body.pointer(
            "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer",
        );
        let title = body
            .pointer("/header/musicDetailHeaderRenderer/title")
            .and_then(text_of)
            .or_else(|| {
                resp_header
                    .and_then(|h| h.pointer("/title"))
                    .and_then(text_of)
            })
            .unwrap_or_default();
        let artist = body
            .pointer("/header/musicDetailHeaderRenderer/subtitle")
            .and_then(text_of)
            .or_else(|| {
                resp_header
                    .and_then(|h| h.pointer("/straplineTextOne"))
                    .and_then(text_of)
            })
            .or_else(|| {
                resp_header
                    .and_then(|h| h.pointer("/subtitle"))
                    .and_then(text_of)
            })
            .unwrap_or_default();
        let cover = body
            .pointer("/header/musicDetailHeaderRenderer")
            .and_then(last_thumbnail)
            .or_else(|| resp_header.and_then(last_thumbnail));
        let artist_id = resp_header
            .and_then(|h| h.pointer("/straplineTextOne/runs"))
            .or_else(|| resp_header.and_then(|h| h.pointer("/subtitle/runs")))
            .and_then(|r| r.as_array())
            .and_then(|runs| {
                runs.iter().find_map(|run| {
                    str_at(run, &["/navigationEndpoint/browseEndpoint/browseId"])
                        .filter(|b| b.starts_with("UC"))
                        .map(str::to_string)
                })
            });
        let album = LibraryAlbumDto {
            album_key: id.into(),
            title,
            artist,
            year: None,
            cover_id: cover.map(|u| cover_id(ServicePlatform::YtMusic, &u)),
            track_count: 0,
            artist_id,
            ..Default::default()
        };
        let mut tracks: Vec<LibraryTrackDto> = self
            .shelf_rows_all(&body, &["musicResponsiveListItemRenderer"], |r| {
                map_responsive_to_track(r)
            })
            .await;

        if let Some(advertised) = album_track_count(&body) {
            if tracks.len() < advertised {
                if let Some(pid) = find_audio_playlist_id(&body) {
                    let vl = format!("VL{pid}");
                    if let Ok(pl_body) = self.browse(&vl).await {
                        let full: Vec<LibraryTrackDto> = self
                            .shelf_rows_all(&pl_body, &["musicResponsiveListItemRenderer"], |r| {
                                map_responsive_to_track(r)
                            })
                            .await;
                        if full.len() > tracks.len() {
                            tracks = full;
                        }
                    }
                }
            }
        }
        for t in &mut tracks {
            inherit_from_header(
                t,
                Some(album.artist.as_str()),
                Some(album.title.as_str()),
                Some(id),
                album.cover_id.as_deref(),
                album.artist_id.as_deref(),
            );
        }

        Ok(LibraryAlbumDetail { album, tracks })
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let body = self.browse(id).await?;
        let display = body
            .pointer("/header/musicImmersiveHeaderRenderer/title")
            .and_then(text_of)
            .or_else(|| {
                body.pointer("/header/musicVisualHeaderRenderer/title")
                    .and_then(text_of)
            })
            .unwrap_or_default();
        let two_rows = collect(&body, &["musicTwoRowItemRenderer"]);
        let s = split_two_rows(two_rows);
        let rows = collect(&body, &["musicResponsiveListItemRenderer"]);
        let tracks: Vec<LibraryTrackDto> = rows
            .iter()
            .filter_map(|(_, r)| map_responsive_to_track(r))
            .take(20)
            .collect();
        Ok(LibraryArtistDetail {
            key: id.into(),
            display,
            albums: s.albums,
            tracks,
        })
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        let id = id
            .strip_prefix("podcast::")
            .or_else(|| id.strip_prefix("show::"))
            .or_else(|| id.strip_prefix("audiobook::"))
            .unwrap_or(id);
        let browse_id = if id.starts_with("VL") || id.starts_with("MPSP") || id.starts_with("MPRE")
        {
            id.to_string()
        } else {
            format!("VL{id}")
        };
        let body = self.browse(&browse_id).await?;
        let resp_header = body
            .pointer(
                "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicResponsiveHeaderRenderer",
            )
            .or_else(|| {
                body.pointer(
                    "/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/musicEditablePlaylistDetailHeaderRenderer/header/musicResponsiveHeaderRenderer",
                )
            });
        let title = body.pointer("/header/musicDetailHeaderRenderer/title").and_then(text_of)
            .or_else(|| body.pointer("/header/musicEditablePlaylistDetailHeaderRenderer/header/musicDetailHeaderRenderer/title").and_then(text_of))
            .or_else(|| resp_header.and_then(|h| h.pointer("/title")).and_then(text_of))
            .or_else(|| str_at(&body, &["/metadata/playlistMetadataRenderer/title"]).map(str::to_string))
            .unwrap_or_default();
        let cover_key = body
            .pointer("/header/musicDetailHeaderRenderer")
            .and_then(last_thumbnail)
            .or_else(|| resp_header.and_then(last_thumbnail))
            .map(|u| cover_id(ServicePlatform::YtMusic, &u));
        let description = body
            .pointer("/header/musicDetailHeaderRenderer/description")
            .and_then(text_of)
            .or_else(|| {
                resp_header
                    .and_then(|h| {
                        h.pointer("/description/musicDescriptionShelfRenderer/description")
                    })
                    .and_then(text_of)
            })
            .or_else(|| {
                resp_header
                    .and_then(|h| h.pointer("/description"))
                    .and_then(text_of)
            })
            .filter(|s| !s.is_empty());
        let owner = body
            .pointer("/header/musicDetailHeaderRenderer/subtitle")
            .and_then(text_of)
            .or_else(|| {
                resp_header
                    .and_then(|h| h.pointer("/straplineTextOne"))
                    .and_then(text_of)
            })
            .or_else(|| {
                resp_header
                    .and_then(|h| h.pointer("/subtitle"))
                    .and_then(text_of)
            })
            .filter(|s| !s.is_empty());
        let mut tracks: Vec<LibraryTrackDto> = self
            .shelf_rows_all(&body, &["musicResponsiveListItemRenderer"], |r| {
                map_responsive_to_track(r)
            })
            .await;
        if tracks.is_empty() {
            tracks = self
                .shelf_rows_all(&body, &["musicMultiRowListItemRenderer"], |r| {
                    map_multi_row_to_episode(r)
                })
                .await;
        }
        // Artwork only: a playlist's `owner` is not the track's artist.
        for t in &mut tracks {
            inherit_from_header(t, None, None, None, cover_key.as_deref(), None);
        }

        let playlist = LibraryPlaylistDto {
            id: stable_hash_i64(id),
            name: title,
            created_at: 0,
            updated_at: 0,
            track_count: tracks.len() as i64,
            cover_ids: cover_key.map(|c| vec![c]).unwrap_or_default(),
            service_id: Some(id.into()),
            description,
            owner,
        };
        Ok(LibraryPlaylistDetail { playlist, tracks })
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
        let video_id = service_track_id
            .rsplit("watch?v=")
            .next()
            .and_then(|s| s.split(['&', '#']).next())
            .unwrap_or(service_track_id);
        if video_id.is_empty() {
            return Ok(());
        }
        let _ = crate::services::ytmusic::telemetry::report_playback(
            self.it.client(),
            &self.it.authorization(),
            self.it.cookie_header(),
            self.it.context(),
            video_id,
            duration_secs,
        )
        .await;
        Ok(())
    }

    async fn artist_page(&self, id: &str) -> MhResult<Value> {
        let body = self.browse(id).await?;
        let shelves = shelves_from_carousels(&body);
        Ok(json!({
            "shelves": shelves,
            "raw": Value::Null,
        }))
    }

    async fn album_page(&self, id: &str) -> MhResult<Value> {
        let body = self.browse(id).await?;
        let shelves: Vec<RecommendationShelf> = shelves_from_carousels(&body)
            .into_iter()
            .filter(|sh| {
                sh.items
                    .iter()
                    .any(|it| matches!(it, RecommendationItem::Album(_)))
            })
            .collect();
        Ok(json!({
            "shelves": shelves,
            "raw": Value::Null,
        }))
    }

    async fn explore(&self) -> MhResult<RecommendationsPage> {
        let body = self.browse("FEmusic_explore").await?;
        let mut shelves = shelves_from_carousels(&body);

        let pills = mood_genre_pills(&body);
        if !pills.is_empty() {
            shelves.push(RecommendationShelf::new(
                "moods_and_genres",
                "Moods & genres",
                RecommendationCategory::Genre,
                pills,
            ));
        }

        Ok(RecommendationsPage { shelves })
    }

    async fn explore_page(&self, path: &str) -> MhResult<RecommendationsPage> {
        let params = path.strip_prefix("moods::").unwrap_or(path);
        let body = self
            .browse_with_params("FEmusic_moods_and_genres_category", params)
            .await?;
        Ok(RecommendationsPage {
            shelves: shelves_from_carousels(&body),
        })
    }
}

fn mood_genre_pills(body: &Value) -> Vec<RecommendationItem> {
    let buttons = collect(body, &["musicNavigationButtonRenderer"]);
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for (_, b) in buttons {
        let title = b
            .pointer("/buttonText")
            .and_then(text_of)
            .unwrap_or_default();
        let params = str_at(&b, &["/clickCommand/browseEndpoint/params"]).unwrap_or("");
        if title.is_empty() || params.is_empty() || !seen.insert(params.to_string()) {
            continue;
        }
        out.push(RecommendationItem::PageLink {
            api_path: format!("moods::{params}"),
            title,
            icon: None,
        });
    }
    out
}

/// One page of a `next` queue: the audio tracks, their music-video counterparts, and
/// the token for the page after this one.
struct QueuePage {
    tracks: Vec<crate::media::library::LibraryTrackDto>,
    video_urls: std::collections::HashMap<String, String>,
    continuation: Option<String>,
}

fn parse_queue_page(body: &Value, seed_id: &str) -> QueuePage {
    let mut tracks: Vec<crate::media::library::LibraryTrackDto> = Vec::new();
    let mut index_by_key: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut video_urls: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut pending_videos: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    const QUEUE_LIMIT: usize = 50;
    let found = collect(body, &["playlistPanelVideoRenderer"]);
    for (_, item) in found {
        if tracks.len() >= QUEUE_LIMIT {
            break;
        }
        let video_id = str_at(&item, &["videoId"]).unwrap_or("").to_string();
        if video_id.is_empty() {
            continue;
        }
        if video_id == seed_id {
            continue;
        }
        let title = item.get("title").and_then(text_of).unwrap_or_default();
        let artist = item
            .get("longBylineText")
            .and_then(first_run_text)
            .unwrap_or_default();
        let music_video_type = str_at(&item, &["/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType"])
            .unwrap_or("");
        let is_audio = music_video_type.is_empty() || music_video_type == "MUSIC_VIDEO_TYPE_ATV";
        let watch_url = format!("https://music.youtube.com/watch?v={video_id}");
        let key = format!("{}\u{1}{}", norm_dedup(&title), norm_dedup(&artist));

        if !is_audio {
            match index_by_key.get(&key) {
                Some(&idx) => {
                    video_urls
                        .entry(tracks[idx].path.clone())
                        .or_insert_with(|| watch_url.clone());
                }
                None => {
                    pending_videos.entry(key).or_insert(watch_url);
                }
            }
            continue;
        }

        let album = item
            .get("longBylineText")
            .and_then(|v| v.get("runs"))
            .and_then(|v| v.as_array())
            .and_then(|runs| runs.get(2))
            .and_then(|r| r.get("text"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty() && s != " • ");
        let duration_secs = item
            .pointer("/lengthText")
            .and_then(text_of)
            .as_deref()
            .and_then(parse_hms_seconds);
        tracks.push(crate::media::library::LibraryTrackDto {
            path: watch_url.clone(),
            title: Some(title),
            artist: Some(artist.clone()),
            album,
            duration_secs,
            size: 0,
            mtime_ns: 0,
            is_video: false,
            cover_id: last_thumbnail(&item).map(|u| cover_id(ServicePlatform::YtMusic, &u)),
            primary_artist: Some(artist),
            ..Default::default()
        });
        let idx = tracks.len() - 1;
        index_by_key.entry(key.clone()).or_insert(idx);
        if let Some(v) = pending_videos.remove(&key) {
            video_urls.entry(watch_url).or_insert(v);
        }
    }
    QueuePage {
        tracks,
        video_urls,
        continuation: next_radio_continuation(body),
    }
}

/// The token an endless station hands over for its next page. `playlistPanelRenderer`
/// publishes it as `nextRadioContinuationData`; a plain playlist panel uses
/// `nextContinuationData`, and both are accepted.
fn next_radio_continuation(body: &Value) -> Option<String> {
    for (_, node) in collect(body, &["nextRadioContinuationData", "nextContinuationData"]) {
        if let Some(token) = str_at(&node, &["continuation"]) {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

const YTM_BASE: &str = "https://music.youtube.com/youtubei/v1";

impl YtMusicLibrary {
    async fn action(&self, action: &str, payload: Value) -> MhResult<Value> {
        let mut body = json!({ "context": self.it.context() });
        if let (Value::Object(extra), Value::Object(b)) = (payload, &mut body) {
            b.extend(extra);
        }
        let url = format!("{YTM_BASE}/{action}?prettyPrint=false");
        self.it.post(&url, body, action).await
    }

    /// The station id for an artist. Unlike an album's, it cannot be built from the id
    /// you already hold: the artist page's `RDEM…` endpoint carries an opaque stem that
    /// is unrelated to the `UC…` channel id, so the page has to be asked for it.
    async fn artist_station_id(&self, artist_id: &str) -> MhResult<String> {
        let body = self.browse(artist_id).await?;
        find_station_playlist_id(&body).ok_or_else(|| {
            MhError::Other(format!(
                "YT Music: artist {artist_id} publishes no station on its page"
            ))
        })
    }

    async fn resolve_audio_playlist_id(&self, id: &str) -> MhResult<String> {
        if id.starts_with("OLAK5uy_") {
            return Ok(id.to_string());
        }
        let body = self.browse(id).await?;
        find_audio_playlist_id(&body).ok_or_else(|| {
            MhError::Other(format!(
                "YT Music: could not resolve audioPlaylistId for album {id}"
            ))
        })
    }
}

/// The first artist-station playlist id a page publishes. `RDEM…` is the audio
/// station; `RDAO…` is the same station's video variant and is the fallback.
fn find_station_playlist_id(v: &Value) -> Option<String> {
    let mut video_variant: Option<String> = None;
    fn walk(v: &Value, audio: &mut Option<String>, video: &mut Option<String>) {
        if audio.is_some() {
            return;
        }
        match v {
            Value::Object(m) => {
                if let Some(pid) = m
                    .get("watchPlaylistEndpoint")
                    .and_then(|e| e.get("playlistId"))
                    .and_then(|p| p.as_str())
                {
                    if pid.starts_with("RDEM") {
                        *audio = Some(pid.to_string());
                        return;
                    }
                    if pid.starts_with("RDAO") && video.is_none() {
                        *video = Some(pid.to_string());
                    }
                }
                for val in m.values() {
                    walk(val, audio, video);
                    if audio.is_some() {
                        return;
                    }
                }
            }
            Value::Array(arr) => {
                for item in arr {
                    walk(item, audio, video);
                    if audio.is_some() {
                        return;
                    }
                }
            }
            _ => {}
        }
    }
    let mut audio = None;
    walk(v, &mut audio, &mut video_variant);
    audio.or(video_variant)
}

/// How many songs a release page says it holds, from the `"204 songs • 10 hours"` line
/// under the title. The track shelf itself stops at 200 without saying so.
fn album_track_count(body: &Value) -> Option<usize> {
    let text = collect(body, &["musicResponsiveHeaderRenderer"])
        .iter()
        .find_map(|(_, h)| {
            h.pointer("/secondSubtitle")
                .and_then(text_of)
                .filter(|s| s.contains("song"))
        })
        .or_else(|| {
            body.pointer("/header/musicDetailHeaderRenderer/secondSubtitle")
                .and_then(text_of)
        })?;
    text.split_whitespace()
        .find_map(|w| w.replace([',', '.', '\u{a0}'], "").parse::<usize>().ok())
}

fn find_audio_playlist_id(v: &Value) -> Option<String> {
    match v {
        Value::Object(m) => {
            if let Some(pid) = m
                .get("playlistId")
                .and_then(|v| v.as_str())
                .filter(|s| s.starts_with("OLAK5uy_"))
            {
                return Some(pid.to_string());
            }
            m.values().find_map(find_audio_playlist_id)
        }
        Value::Array(a) => a.iter().find_map(find_audio_playlist_id),
        _ => None,
    }
}

#[async_trait]
impl ServiceLibraryMutations for YtMusicLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::YtMusic
    }

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities {
            reorder_playlists: false,
            ..MutationCapabilities::full_audio()
        }
    }

    async fn set_favorites(&self, kind: FavKind, op: FavOp, ids: &[String]) -> MhResult<()> {
        match kind {
            FavKind::Track | FavKind::Album => {
                let endpoint = match op {
                    FavOp::Add => "like/like",
                    FavOp::Remove => "like/removelike",
                };
                for id in ids {
                    let target = if kind == FavKind::Track {
                        json!({ "videoId": id })
                    } else {
                        json!({ "playlistId": self.resolve_audio_playlist_id(id).await? })
                    };
                    self.action(endpoint, json!({ "target": target })).await?;
                }
                Ok(())
            }
            FavKind::Artist => {
                let endpoint = match op {
                    FavOp::Add => "subscription/subscribe",
                    FavOp::Remove => "subscription/unsubscribe",
                };
                let channel_ids: Vec<Value> = ids.iter().map(|c| Value::String(c.into())).collect();
                self.action(endpoint, json!({ "channelIds": channel_ids }))
                    .await?;
                Ok(())
            }
            FavKind::Label => Err(MhError::Unsupported("YT Music has no labels".into())),
        }
    }

    async fn follow_playlist(&self, id: &str) -> MhResult<()> {
        self.action(
            "like/like",
            json!({
                "target": { "playlistId": id },
            }),
        )
        .await?;
        Ok(())
    }
    async fn unfollow_playlist(&self, id: &str) -> MhResult<()> {
        self.action(
            "like/removelike",
            json!({
                "target": { "playlistId": id },
            }),
        )
        .await?;
        Ok(())
    }

    async fn create_playlist(&self, input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        let video_ids: Vec<Value> = input
            .initial_track_ids
            .iter()
            .map(|v| Value::String(v.into()))
            .collect();
        let privacy = if input.is_public { "PUBLIC" } else { "PRIVATE" };
        let resp = self
            .action(
                "playlist/create",
                json!({
                    "title": input.name,
                    "description": input.description.unwrap_or_default(),
                    "privacyStatus": privacy,
                    "videoIds": video_ids,
                }),
            )
            .await?;
        let id = str_at(&resp, &["playlistId"]).ok_or_else(|| {
            MhError::Other(format!("YT Music playlist/create: no playlistId in {resp}"))
        })?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn rename_playlist(
        &self,
        id: &str,
        new_name: &str,
        new_description: Option<&str>,
        new_is_public: Option<bool>,
        _new_is_collaborative: Option<bool>,
    ) -> MhResult<()> {
        let mut actions = vec![json!({
            "action": "ACTION_SET_PLAYLIST_NAME",
            "playlistName": new_name,
        })];
        if let Some(d) = new_description {
            actions.push(json!({
                "action": "ACTION_SET_PLAYLIST_DESCRIPTION",
                "playlistDescription": d,
            }));
        }
        if let Some(p) = new_is_public {
            actions.push(json!({
                "action": "ACTION_SET_PLAYLIST_PRIVACY",
                "playlistPrivacy": if p { "PUBLIC" } else { "PRIVATE" },
            }));
        }
        self.action(
            "browse/edit_playlist",
            json!({
                "playlistId": id, "actions": actions,
            }),
        )
        .await?;
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> MhResult<()> {
        self.action("playlist/delete", json!({ "playlistId": id }))
            .await?;
        Ok(())
    }

    async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        let actions: Vec<Value> = track_ids
            .iter()
            .map(|vid| {
                json!({
                    "action": "ACTION_ADD_VIDEO",
                    "addedVideoId": vid,
                })
            })
            .collect();
        self.action(
            "browse/edit_playlist",
            json!({
                "playlistId": id, "actions": actions,
            }),
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
        let browse_id = if id.starts_with("VL") {
            id.to_string()
        } else {
            format!("VL{id}")
        };
        let body = self.browse(&browse_id).await?;
        let rows = collect(&body, &["musicResponsiveListItemRenderer"]);
        let mut set_ids: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        for (_, r) in &rows {
            let vid = str_at(r, &["/playlistItemData/videoId"]);
            let set_vid = str_at(r, &["/playlistItemData/playlistSetVideoId"])
                .or_else(|| {
                    str_at(r, &["/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/playlistSetVideoId"])
                });
            if let (Some(vid), Some(set_vid)) = (vid, set_vid) {
                set_ids.insert(vid.to_string(), set_vid.to_string());
            }
        }

        let actions: Vec<Value> = track_ids
            .iter()
            .filter_map(|vid| {
                set_ids.get(vid).map(|set_vid| {
                    json!({
                        "action": "ACTION_REMOVE_VIDEO",
                        "removedVideoId": vid,
                        "setVideoId": set_vid,
                    })
                })
            })
            .collect();
        if actions.is_empty() {
            return Err(MhError::Other(
                "YT Music: could not resolve setVideoId for the given track(s)".into(),
            ));
        }
        self.action(
            "browse/edit_playlist",
            json!({ "playlistId": id, "actions": actions }),
        )
        .await?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn radio_for(&self, seed_kind: RadioSeedKind, seed_id: &str) -> MhResult<RadioResult> {
        let radio_playlist_id = match seed_kind {
            RadioSeedKind::Track => format!("RDAMVM{seed_id}"),
            RadioSeedKind::Album => {
                format!("RDAMPL{}", self.resolve_audio_playlist_id(seed_id).await?)
            }
            RadioSeedKind::Playlist => {
                format!("RDAMPL{}", seed_id.strip_prefix("VL").unwrap_or(seed_id))
            }
            RadioSeedKind::Artist => self.artist_station_id(seed_id).await?,
        };
        let mut payload = json!({
            "playlistId": radio_playlist_id,
            "params": "wAEB",
            "isAudioOnly": true,
            "enablePersistentPlaylistPanel": true,
            "tunerSettingValue": "AUTOMIX_SETTING_NORMAL",
            "watchEndpointMusicSupportedConfigs": {
                "watchEndpointMusicConfig": {
                    "musicVideoType": "MUSIC_VIDEO_TYPE_ATV"
                }
            },
        });
        if seed_kind == RadioSeedKind::Track {
            payload["videoId"] = json!(seed_id);
        }
        let body = self.action("next", payload).await?;
        let page = parse_queue_page(&body, seed_id);
        Ok(RadioResult {
            seed_kind,
            seed_id: seed_id.to_string(),
            title: match seed_kind {
                RadioSeedKind::Track => format!("Track radio · {seed_id}"),
                RadioSeedKind::Album => format!("Album radio · {seed_id}"),
                RadioSeedKind::Artist => format!("Artist radio · {seed_id}"),
                RadioSeedKind::Playlist => format!("Playlist radio · {seed_id}"),
            },
            tracks: page.tracks,
            station_id: Some(radio_playlist_id),
            video_urls: page.video_urls,
            continuation: page.continuation,
        })
    }

    async fn radio_continue(&self, token: &str) -> MhResult<RadioResult> {
        let url = format!("{YTM_BASE}/next?prettyPrint=false");
        let body = self.it.continuation(&url, token).await?;
        let page = parse_queue_page(&body, "");
        Ok(RadioResult {
            seed_kind: RadioSeedKind::Track,
            seed_id: String::new(),
            title: String::new(),
            tracks: page.tracks,
            station_id: None,
            video_urls: page.video_urls,
            continuation: page.continuation,
        })
    }

    async fn fetch_saved_ids(&self, kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        let mut out = SavedIdSet::default();
        for kind in kinds {
            let (browse_id, renderer, ptr) = match kind {
                SaveKind::Track => (
                    "FEmusic_liked_videos",
                    "musicResponsiveListItemRenderer",
                    "/playlistItemData/videoId",
                ),
                SaveKind::Album => (
                    "FEmusic_liked_albums",
                    "musicTwoRowItemRenderer",
                    "/navigationEndpoint/browseEndpoint/browseId",
                ),
                SaveKind::Artist => (
                    "FEmusic_library_corpus_track_artists",
                    "musicResponsiveListItemRenderer",
                    "/navigationEndpoint/browseEndpoint/browseId",
                ),
                SaveKind::Playlist => (
                    "FEmusic_liked_playlists",
                    "musicTwoRowItemRenderer",
                    "/navigationEndpoint/browseEndpoint/browseId",
                ),
            };
            let body = self.browse(browse_id).await.unwrap_or(Value::Null);
            let found = collect(&body, &[renderer]);
            let ids: Vec<String> = found
                .iter()
                .filter_map(|(_, item)| {
                    item.pointer(ptr)
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .collect();
            out.set(*kind, ids);
        }
        Ok(out)
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        let body = self.browse("FEmusic_liked_playlists").await?;
        let found = collect(&body, &["musicTwoRowItemRenderer"]);
        let mut rows = Vec::new();
        for (_, item) in found {
            let id = str_at(&item, &["/navigationEndpoint/browseEndpoint/browseId"]).unwrap_or("");
            if id.is_empty() {
                continue;
            }
            let svc_id = id.strip_prefix("VL").unwrap_or(id).to_string();
            if svc_id == "LM" || !svc_id.starts_with("PL") {
                continue;
            }
            let name = item.get("title").and_then(text_of).unwrap_or_default();
            rows.push(OwnedPlaylistRow {
                platform: ServicePlatform::YtMusic.as_str().into(),
                service_id: svc_id,
                name,
                cover_id: last_thumbnail(&item).map(|u| cover_id(ServicePlatform::YtMusic, &u)),
                track_count: 0,
                updated_at: 0,
            });
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::{inherit_from_header, map_responsive_to_track, norm_dedup};
    use serde_json::json;

    /// An empty artist satisfies every `??` fallback downstream, so a row with
    /// no artist column has to report `None`.
    #[test]
    fn an_album_row_reports_a_missing_artist_as_missing() {
        let row = json!({
            "flexColumns": [{
                "musicResponsiveListItemFlexColumnRenderer": {
                    "text": { "runs": [{
                        "text": "Giorgio by Moroder",
                        "navigationEndpoint": { "watchEndpoint": { "videoId": "zhl-Cs1-sG4" } }
                    }] }
                }
            }]
        });
        let t = map_responsive_to_track(&row).expect("row maps");
        assert_eq!(t.title.as_deref(), Some("Giorgio by Moroder"));
        assert_eq!(t.artist, None, "no artist column means no artist");
        assert_eq!(t.primary_artist, None);
        assert_eq!(t.album, None);
        assert_eq!(t.cover_id, None);
    }

    #[test]
    fn a_row_that_names_its_own_artist_keeps_it() {
        let row = json!({
            "flexColumns": [
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{
                            "text": "Too Long",
                            "navigationEndpoint": { "watchEndpoint": { "videoId": "vid" } }
                        }] }
                    }
                },
                {
                    "musicResponsiveListItemFlexColumnRenderer": {
                        "text": { "runs": [{ "text": "Daft Punk" }] }
                    }
                }
            ]
        });
        let t = map_responsive_to_track(&row).expect("row maps");
        assert_eq!(t.artist.as_deref(), Some("Daft Punk"));
    }

    #[test]
    fn a_bare_row_inherits_what_the_header_states() {
        let row = json!({
            "flexColumns": [{
                "musicResponsiveListItemFlexColumnRenderer": {
                    "text": { "runs": [{
                        "text": "Aerodynamic",
                        "navigationEndpoint": { "watchEndpoint": { "videoId": "vid" } }
                    }] }
                }
            }]
        });
        let mut t = map_responsive_to_track(&row).expect("row maps");
        inherit_from_header(
            &mut t,
            Some("Daft Punk"),
            Some("Discovery"),
            Some("MPREb_x"),
            Some("cover-key"),
            Some("UC_artist"),
        );

        assert_eq!(t.artist.as_deref(), Some("Daft Punk"));
        assert_eq!(t.primary_artist.as_deref(), Some("Daft Punk"));
        assert_eq!(t.album.as_deref(), Some("Discovery"));
        assert_eq!(t.album_key.as_deref(), Some("MPREb_x"));
        assert_eq!(t.cover_id.as_deref(), Some("cover-key"));
        assert_eq!(t.artist_id.as_deref(), Some("UC_artist"));
    }

    #[test]
    fn the_header_never_overwrites_what_a_row_already_carries() {
        let mut t = super::LibraryTrackDto {
            path: "https://music.youtube.com/watch?v=vid".into(),
            artist: Some("Todd Edwards".into()),
            primary_artist: Some("Todd Edwards".into()),
            cover_id: Some("row-cover".into()),
            ..Default::default()
        };
        inherit_from_header(
            &mut t,
            Some("Various Artists"),
            Some("Discovery"),
            None,
            Some("album-cover"),
            None,
        );

        assert_eq!(t.artist.as_deref(), Some("Todd Edwards"));
        assert_eq!(t.primary_artist.as_deref(), Some("Todd Edwards"));
        assert_eq!(t.cover_id.as_deref(), Some("row-cover"));
        assert_eq!(t.album.as_deref(), Some("Discovery"), "still fills the gap");
    }

    #[test]
    fn norm_dedup_collapses_video_and_audio_variants() {
        assert_eq!(
            norm_dedup("Get Lucky (Official Video)"),
            norm_dedup("Get Lucky")
        );
        assert_eq!(
            norm_dedup("Get Lucky ft. Pharrell Williams (Official Video)"),
            norm_dedup("Get Lucky ft. Pharrell Williams")
        );
    }

    #[test]
    fn norm_dedup_ignores_punctuation_and_case() {
        assert_eq!(norm_dedup("Doin' It Right"), norm_dedup("doin it right"));
        assert_eq!(norm_dedup("One More Time?"), norm_dedup("one more time"));
    }

    #[test]
    fn norm_dedup_keeps_distinct_titles_apart() {
        assert_ne!(
            norm_dedup("Contact"),
            norm_dedup("Contact (Remix) feat DJ Falcon")
        );
    }
}

#[cfg(test)]
mod shelf_category_tests {
    use super::shelf_category;
    use crate::media::library::{LibraryAlbumDto, LibraryTrackDto};
    use crate::services::common::library::{RecommendationCategory as C, RecommendationItem};

    fn track() -> RecommendationItem {
        RecommendationItem::Track(LibraryTrackDto {
            path: "https://music.youtube.com/watch?v=x".into(),
            size: 0,
            mtime_ns: 0,
            is_video: false,
            ..Default::default()
        })
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

    #[test]
    fn a_list_renderer_is_a_list_regardless_of_language() {
        assert_eq!(shelf_category("musicShelfRenderer", &[album()]), C::Charts);
    }

    #[test]
    fn a_carousel_of_cards_stays_a_card_row() {
        assert_eq!(
            shelf_category("musicCarouselShelfRenderer", &[album(), album()]),
            C::Other,
        );
    }

    #[test]
    fn an_all_track_carousel_reads_as_a_list() {
        assert_eq!(
            shelf_category("musicCarouselShelfRenderer", &[track(), track()]),
            C::Charts,
        );
    }

    #[test]
    fn a_mixed_carousel_is_not_a_list() {
        assert_eq!(
            shelf_category("musicCarouselShelfRenderer", &[track(), album()]),
            C::Other,
        );
    }

    #[test]
    fn empty_never_claims_to_be_a_list() {
        assert_eq!(shelf_category("musicCarouselShelfRenderer", &[]), C::Other);
    }
}

#[cfg(test)]
mod thumbnail_tests {
    use super::last_thumbnail;
    use serde_json::json;

    /// The shapes the fixed pointer list already names must keep working.
    #[test]
    fn named_shapes_still_win() {
        let v = json!({
            "musicThumbnailRenderer": {
                "thumbnail": { "thumbnails": [
                    { "url": "https://lh3.googleusercontent.com/small", "width": 60 },
                    { "url": "https://lh3.googleusercontent.com/large", "width": 544 }
                ]}
            }
        });
        assert_eq!(
            last_thumbnail(&v).as_deref(),
            Some("https://lh3.googleusercontent.com/large")
        );
    }

    /// A renderer nobody named — the generated collage personal mixes and auto-playlists
    /// ship — used to yield no cover at all.
    #[test]
    fn an_unnamed_renderer_still_yields_a_cover() {
        let collage = json!({
            "musicCollageThumbnailRenderer": {
                "thumbnails": [
                    { "url": "https://lh3.googleusercontent.com/a", "width": 226 },
                    { "url": "https://lh3.googleusercontent.com/b", "width": 544 }
                ]
            }
        });
        assert_eq!(
            last_thumbnail(&collage).as_deref(),
            Some("https://lh3.googleusercontent.com/b")
        );

        let cropped = json!({
            "croppedSquareThumbnailRenderer": {
                "thumbnail": { "thumbnails": [{ "url": "https://yt3.googleusercontent.com/c" }] }
            }
        });
        assert_eq!(
            last_thumbnail(&cropped).as_deref(),
            Some("https://yt3.googleusercontent.com/c")
        );

        let viewmodel = json!({
            "someFutureRenderer": {
                "nested": { "image": { "sources": [{ "url": "https://lh3.googleusercontent.com/d" }] } }
            }
        });
        assert_eq!(
            last_thumbnail(&viewmodel).as_deref(),
            Some("https://lh3.googleusercontent.com/d")
        );
    }

    #[test]
    fn a_node_with_no_artwork_stays_none() {
        assert_eq!(last_thumbnail(&json!({ "title": { "runs": [] } })), None);
        assert_eq!(last_thumbnail(&json!({ "thumbnails": [] })), None);
        assert_eq!(
            last_thumbnail(&json!({ "thumbnails": [{ "width": 1 }] })),
            None
        );
    }
}

#[cfg(test)]
mod station_id_tests {
    use super::find_station_playlist_id;
    use serde_json::json;

    /// Shapes lifted from a live artist-page capture: the audio station is `RDEM…`,
    /// its video twin is `RDAO…`, and neither stem derives from the `UC…` channel id —
    /// which is why the page has to be asked rather than the id constructed.
    #[test]
    fn the_audio_station_wins_over_its_video_twin() {
        let page = json!({
            "contents": [
                { "musicTwoRowItemRenderer": {
                    "menu": { "menuRenderer": { "items": [
                        { "menuNavigationItemRenderer": { "navigationEndpoint": {
                            "watchPlaylistEndpoint": {
                                "playlistId": "RDAO5wiDa4yKRWA8UW9c3_hLMw",
                                "params": "wAEB8gECGAE%3D" } } } },
                        { "menuNavigationItemRenderer": { "navigationEndpoint": {
                            "watchPlaylistEndpoint": {
                                "playlistId": "RDEM5wiDa4yKRWA8UW9c3_hLMw",
                                "params": "wAEB" } } } }
                    ]}},
                    "navigationEndpoint": { "browseEndpoint": {
                        "browseId": "UClxuMOFeLBJXk8nt-SolPvw" } }
                }}
            ]
        });
        assert_eq!(
            find_station_playlist_id(&page).as_deref(),
            Some("RDEM5wiDa4yKRWA8UW9c3_hLMw")
        );
    }

    #[test]
    fn the_video_station_is_the_fallback() {
        let page = json!({ "a": { "watchPlaylistEndpoint": { "playlistId": "RDAOabc" } } });
        assert_eq!(find_station_playlist_id(&page).as_deref(), Some("RDAOabc"));
    }

    /// An album/playlist queue id is `RDAMPL` + the audio playlist id, which a page
    /// full of those must not be mistaken for an artist station.
    #[test]
    fn album_queue_ids_are_not_artist_stations() {
        let page = json!({
            "a": { "watchPlaylistEndpoint": { "playlistId": "RDAMPLOLAK5uy_kkWWtOjOCB3gS0gK4" } },
            "b": { "watchPlaylistEndpoint": { "playlistId": "OLAK5uy_kkWWtOjOCB3gS0gK4" } }
        });
        assert_eq!(find_station_playlist_id(&page), None);
    }
}

#[cfg(test)]
mod queue_page_tests {
    use super::{next_radio_continuation, parse_queue_page};
    use serde_json::{json, Value};

    const AUDIO_ENTRIES: usize = 47;
    const BARE_VIDEOS: usize = 3;
    const SEED: &str = "seed0000000";

    fn renderer(video_id: &str, n: usize, music_video_type: &str) -> Value {
        json!({
            "videoId": video_id,
            "title": { "runs": [{ "text": format!("Track {n}") }] },
            "longBylineText": { "runs": [
                { "text": "Station Artist" },
                { "text": " • " },
                { "text": "Station Album" },
            ]},
            "lengthText": { "runs": [{ "text": "3:24" }] },
            "thumbnail": { "thumbnails": [
                { "url": format!("https://example.invalid/{n}.jpg"), "width": 544, "height": 544 },
            ]},
            "navigationEndpoint": { "watchEndpoint": {
                "watchEndpointMusicSupportedConfigs": {
                    "watchEndpointMusicConfig": { "musicVideoType": music_video_type }
                }
            }},
        })
    }

    /// The shape an endless station returns: 50 panel items — 47 wrappers pairing an
    /// audio entry with its music-video counterpart, plus 3 bare video entries — which a
    /// recursive walk sees as 97 `playlistPanelVideoRenderer` nodes over 47 audio tracks.
    /// The seed arrives as a video, which is how a station started from a music video
    /// hands back its own first item.
    fn station_page() -> Value {
        let mut contents: Vec<Value> = (0..BARE_VIDEOS)
            .map(|i| {
                let id = if i == 0 {
                    SEED.to_string()
                } else {
                    format!("bare{i:07}")
                };
                json!({ "playlistPanelVideoRenderer":
                    renderer(&id, 900 + i, "MUSIC_VIDEO_TYPE_OMV") })
            })
            .collect();

        contents.extend((0..AUDIO_ENTRIES).map(|i| {
            json!({ "playlistPanelVideoWrapperRenderer": {
                "primaryRenderer": {
                    "playlistPanelVideoRenderer":
                        renderer(&format!("aud{i:08}"), i, "MUSIC_VIDEO_TYPE_ATV")
                },
                "counterpart": [{ "counterpartRenderer": {
                    "playlistPanelVideoRenderer":
                        renderer(&format!("vid{i:08}"), i, "MUSIC_VIDEO_TYPE_OMV")
                }}],
            }})
        }));

        json!({ "contents": { "singleColumnMusicWatchNextResultsRenderer": { "tabbedRenderer": {
            "watchNextTabbedResultsRenderer": { "tabs": [{ "tabRenderer": { "content": {
                "musicQueueRenderer": { "content": { "playlistPanelRenderer": {
                    "contents": contents,
                    "continuations": [{ "nextRadioContinuationData": {
                        "continuation": "CONTINUATION_TOKEN"
                    }}],
                }}}
            }}}]}
        }}}})
    }

    /// Capping the *raw nodes* at 50, which is what the parser used to do, cut the queue
    /// to the audio entries that happened to fall in the first 50 nodes. The cap counts
    /// audio tracks now.
    #[test]
    fn counterpart_renderers_do_not_eat_the_queue() {
        let body = station_page();
        let raw = super::collect(&body, &["playlistPanelVideoRenderer"]);
        assert_eq!(raw.len(), 97, "both primaries and counterparts are reachable");

        let audio_in_first_50 = raw[..50]
            .iter()
            .filter(|(_, n)| {
                n.pointer("/navigationEndpoint/watchEndpoint/watchEndpointMusicSupportedConfigs/watchEndpointMusicConfig/musicVideoType")
                    .and_then(Value::as_str)
                    == Some("MUSIC_VIDEO_TYPE_ATV")
            })
            .count();
        assert!(
            audio_in_first_50 < AUDIO_ENTRIES,
            "a raw-node cap has to lose tracks or this pins nothing"
        );

        let page = parse_queue_page(&body, SEED);
        assert_eq!(
            page.tracks.len(),
            AUDIO_ENTRIES,
            "every audio entry the station listed"
        );
        assert!(page.tracks.iter().all(|t| !t.is_video));
        assert!(
            page.tracks.iter().all(|t| t.path.contains("watch?v=")),
            "every entry should be a playable watch URL"
        );
    }

    /// The seed track is already playing, so it must not be appended again.
    #[test]
    fn the_seed_track_is_left_out() {
        let page = parse_queue_page(&station_page(), SEED);
        assert!(!page.tracks.iter().any(|t| t.path.ends_with(SEED)));
    }

    /// Counterparts are not queued as tracks; they are attached to the audio entry so a
    /// video toggle has somewhere to point.
    #[test]
    fn music_video_counterparts_become_video_urls() {
        let page = parse_queue_page(&station_page(), SEED);
        assert_eq!(page.video_urls.len(), AUDIO_ENTRIES);
        for url in page.video_urls.keys() {
            assert!(
                page.tracks.iter().any(|t| &t.path == url),
                "{url} has no audio track to hang off"
            );
        }
    }

    /// `isInfinite: true` stations publish a token for the next page.
    #[test]
    fn the_continuation_token_is_picked_up() {
        let body = station_page();
        let token = next_radio_continuation(&body).expect("station is endless");
        assert_eq!(token, "CONTINUATION_TOKEN");
        assert_eq!(
            parse_queue_page(&body, "").continuation.as_deref(),
            Some(token.as_str())
        );
    }

    #[test]
    fn a_finite_page_reports_no_continuation() {
        let body: Value = serde_json::json!({ "contents": [] });
        assert_eq!(next_radio_continuation(&body), None);
    }
}
