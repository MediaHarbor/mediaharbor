use crate::services::common::library::str_at;
use async_trait::async_trait;
use serde_json::Value;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};

use crate::services::common::innertube::{self, collect_renderers, Innertube};
use crate::services::common::library::{
    cover_id, parse_hms_seconds, stable_hash_i64, Page, RecommendationCategory, RecommendationItem,
    RecommendationShelf, RecommendationsPage, ServiceCapabilities, ServiceLibrary,
    ServiceLibraryPage, ServicePlatform,
};

const BROWSE_URL: &str = "https://www.youtube.com/youtubei/v1/browse?prettyPrint=false";

pub struct YoutubeLibrary {
    it: Innertube,
}

impl YoutubeLibrary {
    pub fn from_settings(s: &Settings) -> MhResult<Self> {
        let cookies_path = pick_cookies_path(s).ok_or_else(|| MhError::Auth(
            "YouTube not connected: no cookies_path / spotify_cookies_path with youtube.com entries".into()))?;
        Ok(Self {
            it: Innertube::from_cookies(
                &cookies_path,
                innertube::YOUTUBE,
                "YouTube cookies file has no SAPISID/__Secure-3PAPISID",
            )?,
        })
    }

    async fn browse(&self, browse_id: &str) -> MhResult<Value> {
        self.it.browse(BROWSE_URL, browse_id, None).await
    }

    /// Every video of a playlist, following the listing's continuations.
    ///
    /// A browse page carries about 100 entries and a token for the next slice; without
    /// following it a long playlist arrived cut off at its first page.
    async fn all_playlist_videos(&self, first: &Value) -> Vec<LibraryTrackDto> {
        let mut out = collect_videos(first);
        let mut token = innertube::continuation_token(first);
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut guard = 0;
        while let Some(t) = token.take() {
            guard += 1;
            if guard > 60 || !seen.insert(t.clone()) {
                break;
            }
            let Ok(page) = self.it.continuation(BROWSE_URL, &t).await else {
                break;
            };
            let rows = collect_videos(&page);
            if rows.is_empty() {
                break;
            }
            out.extend(rows);
            token = innertube::continuation_token(&page);
        }
        out
    }
}

/// The continuation token a listing publishes, in either of the two shapes YouTube
/// serves: a `nextContinuationData` block or a `continuationItemRenderer`.
fn pick_cookies_path(s: &Settings) -> Option<String> {
    if !s.youtube_cookies_path.trim().is_empty() {
        return Some(s.youtube_cookies_path.clone());
    }
    if !s.cookies.is_empty() {
        return Some(s.cookies.clone());
    }
    None
}

/// YouTube bylines put the channel in the first run; joining every run would
/// glue on the view count and upload date that follow it.
fn text_of(v: &Value) -> Option<String> {
    innertube::text_of(v, false)
}

fn pick_thumbnail(v: &Value) -> Option<String> {
    let arr = v.pointer("/thumbnails")?.as_array()?;
    arr.last()
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_string)
}

fn map_video_renderer(v: &Value) -> Option<LibraryTrackDto> {
    let video_id = str_at(v, &["videoId"])?.to_string();
    let title = v.get("title").and_then(text_of).unwrap_or_default();
    let channel = v
        .get("longBylineText")
        .and_then(text_of)
        .or_else(|| v.get("ownerText").and_then(text_of))
        .or_else(|| v.get("shortBylineText").and_then(text_of))
        .unwrap_or_default();
    let thumb = v.get("thumbnail").and_then(pick_thumbnail);
    let dur_text = v.get("lengthText").and_then(text_of);
    let dur = dur_text.as_deref().and_then(parse_hms_seconds);
    Some(LibraryTrackDto {
        path: format!("https://www.youtube.com/watch?v={video_id}"),
        title: Some(title),
        artist: Some(channel.clone()),
        duration_secs: dur,
        size: 0,
        mtime_ns: 0,
        is_video: true,
        cover_id: thumb.map(|u| cover_id(ServicePlatform::Youtube, &u)),
        primary_artist: Some(channel),
        ..Default::default()
    })
}

fn map_playlist_renderer(v: &Value) -> Option<LibraryPlaylistDto> {
    let id_str = str_at(v, &["playlistId"])?.to_string();
    let name = v.get("title").and_then(text_of).unwrap_or_default();
    let count = str_at(v, &["videoCount"])
        .and_then(|s| s.parse::<i64>().ok())
        .or_else(|| {
            v.pointer("/videoCountText")
                .and_then(text_of)
                .and_then(|s| {
                    s.split_whitespace()
                        .next()
                        .and_then(|n| n.parse::<i64>().ok())
                })
        })
        .unwrap_or(0);
    let thumb = v
        .pointer("/thumbnails/0")
        .and_then(pick_thumbnail)
        .or_else(|| v.get("thumbnail").and_then(pick_thumbnail))
        .or_else(|| v.pointer("/thumbnailRenderer").and_then(pick_thumbnail));
    let owner = v
        .get("longBylineText")
        .and_then(text_of)
        .or_else(|| v.get("shortBylineText").and_then(text_of))
        .filter(|s| !s.is_empty());
    let description = v
        .get("descriptionText")
        .and_then(text_of)
        .or_else(|| v.get("description").and_then(text_of))
        .filter(|s| !s.is_empty());
    Some(LibraryPlaylistDto {
        id: stable_hash_i64(&id_str),
        name,
        created_at: 0,
        updated_at: 0,
        track_count: count,
        cover_ids: thumb
            .map(|u| vec![cover_id(ServicePlatform::Youtube, &u)])
            .unwrap_or_default(),
        service_id: Some(id_str),
        description,
        owner,
    })
}

fn map_channel_renderer(v: &Value) -> Option<LibraryArtistDto> {
    let id = str_at(v, &["channelId"])?.to_string();
    let name = v.get("title").and_then(text_of).unwrap_or_default();
    let thumb = v.get("thumbnail").and_then(pick_thumbnail);
    Some(LibraryArtistDto {
        key: id,
        display: name,
        album_count: 0,
        track_count: 0,
        cover_id: thumb.map(|u| cover_id(ServicePlatform::Youtube, &u)),
    })
}

fn collect_videos(v: &Value) -> Vec<LibraryTrackDto> {
    let renderers = collect_renderers(
        v,
        &[
            "videoRenderer",
            "playlistVideoRenderer",
            "gridVideoRenderer",
            "compactVideoRenderer",
            "richItemRenderer",
            "lockupViewModel",
        ],
    );
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (kind, inner) in renderers {
        let mut maybe = match kind.as_str() {
            "videoRenderer" => map_video_renderer(&inner),
            "playlistVideoRenderer" => map_video_renderer(&inner),
            "gridVideoRenderer" => map_video_renderer(&inner),
            "compactVideoRenderer" => map_video_renderer(&inner),
            "richItemRenderer" => {
                if let Some(content) = inner.get("content") {
                    let inner2 = collect_renderers(
                        content,
                        &[
                            "videoRenderer",
                            "gridVideoRenderer",
                            "compactVideoRenderer",
                            "lockupViewModel",
                        ],
                    );
                    inner2.into_iter().find_map(|(k, x)| {
                        if k == "lockupViewModel" {
                            map_lockup_video(&x)
                        } else {
                            map_video_renderer(&x)
                        }
                    })
                } else {
                    None
                }
            }
            "lockupViewModel" => map_lockup_video(&inner),
            _ => None,
        };
        if let Some(ref t) = maybe {
            if !seen.insert(t.path.clone()) {
                maybe = None;
            }
        }
        if let Some(t) = maybe {
            out.push(t);
        }
    }
    out
}

fn home_shelves_from_rich_grid(body: &Value) -> Vec<RecommendationShelf> {
    let mut out = Vec::new();
    let grid_contents = body
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/richGridRenderer/contents")
        .and_then(|v| v.as_array()).cloned().unwrap_or_default();

    let mut loose: Vec<LibraryTrackDto> = Vec::new();
    let mut loose_seen = std::collections::HashSet::new();

    for entry in grid_contents {
        if let Some(section) = entry.get("richSectionRenderer") {
            let shelf = section.pointer("/content/richShelfRenderer");
            if let Some(shelf) = shelf {
                let title = shelf
                    .pointer("/title")
                    .and_then(text_of)
                    .unwrap_or_default();
                let items_v = shelf
                    .get("contents")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                let mut items = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for it in items_v {
                    let vids = collect_videos(&it);
                    for v in vids {
                        if seen.insert(v.path.clone()) {
                            items.push(RecommendationItem::Track(v));
                        }
                    }
                }
                if items.is_empty() {
                    continue;
                }
                out.push(RecommendationShelf {
                    id: title.clone(),
                    title: if title.is_empty() {
                        "Featured".into()
                    } else {
                        title
                    },
                    subtitle: None,
                    category: RecommendationCategory::Other,
                    items: items.into_iter().take(40).collect(),
                });
            }
        } else if entry.get("richItemRenderer").is_some() {
            let vids = collect_videos(&entry);
            for v in vids {
                if loose_seen.insert(v.path.clone()) {
                    loose.push(v);
                }
            }
        }
    }
    if !loose.is_empty() {
        out.insert(
            0,
            RecommendationShelf {
                id: "recommended".into(),
                title: "Recommended for You".into(),
                subtitle: None,
                category: RecommendationCategory::Discovery,
                items: loose
                    .into_iter()
                    .take(40)
                    .map(RecommendationItem::Track)
                    .collect(),
            },
        );
    }
    out
}

fn map_lockup_video(v: &Value) -> Option<LibraryTrackDto> {
    let content_type = str_at(v, &["contentType"]).unwrap_or("");
    if content_type != "LOCKUP_CONTENT_TYPE_VIDEO" && !content_type.is_empty() {
        return None;
    }
    let video_id = str_at(v, &["contentId"])?.to_string();
    let title = str_at(v, &["/metadata/lockupMetadataViewModel/title/content"])
        .unwrap_or("")
        .to_string();
    let channel = str_at(v, &["/metadata/lockupMetadataViewModel/metadata/contentMetadataViewModel/metadataRows/0/metadataParts/0/text/content"]).unwrap_or("").to_string();
    let thumb_url = v
        .pointer("/contentImage/thumbnailViewModel/image/sources")
        .and_then(|s| s.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_string);
    Some(LibraryTrackDto {
        path: format!("https://www.youtube.com/watch?v={video_id}"),
        title: Some(title),
        artist: Some(channel.clone()),
        size: 0,
        mtime_ns: 0,
        is_video: true,
        cover_id: thumb_url.map(|u| cover_id(ServicePlatform::Youtube, &u)),
        primary_artist: Some(channel),
        ..Default::default()
    })
}

/// A channel's playlists are the closest thing YouTube has to albums, and
/// `LibraryArtistDetail` only has an `albums` slot to put them in. `album_detail`
/// resolves the same key back through `playlist_detail`, so the round trip holds.
fn playlist_as_album(p: &LibraryPlaylistDto) -> LibraryAlbumDto {
    LibraryAlbumDto {
        album_key: p.service_id.clone().unwrap_or_default(),
        title: p.name.clone(),
        artist: p.owner.clone().unwrap_or_default(),
        year: None,
        cover_id: p.cover_ids.first().cloned(),
        track_count: p.track_count,
        artist_id: None,
        ..Default::default()
    }
}

fn collect_playlists(v: &Value) -> Vec<LibraryPlaylistDto> {
    let renderers = collect_renderers(
        v,
        &[
            "playlistRenderer",
            "gridPlaylistRenderer",
            "compactPlaylistRenderer",
        ],
    );
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, inner) in renderers {
        if let Some(p) = map_playlist_renderer(&inner) {
            if seen.insert(p.service_id.clone()) {
                out.push(p);
            }
        }
    }
    out
}

fn collect_channels(v: &Value) -> Vec<LibraryArtistDto> {
    let renderers = collect_renderers(
        v,
        &[
            "channelRenderer",
            "gridChannelRenderer",
            "subscriptionEntityRenderer",
        ],
    );
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (_, inner) in renderers {
        if let Some(a) = map_channel_renderer(&inner) {
            if seen.insert(a.key.clone()) {
                out.push(a);
            }
        }
    }
    out
}

#[async_trait]
impl ServiceLibrary for YoutubeLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Youtube
    }
    fn capabilities(&self) -> ServiceCapabilities {
        crate::services::common::library::ServiceCapabilities {
            albums: false,
            tracks: false,
            artists: true,
            playlists: true,
            videos: true,
            recommendations: true,
            followers: false,
            activity_feed: false,
            episode_bookmarks: false,
            mutations: crate::services::common::library::MutationCapabilities::none(),
        }
    }

    async fn videos(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        let body = self.browse("VLLL").await?;
        let items = collect_videos(&body);
        Ok(ServiceLibraryPage::of(items, None))
    }

    async fn playlists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        let body = self.browse("FElibrary").await?;
        let items = collect_playlists(&body);
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn artists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        let body = self.browse("FEsubscriptions").await?;
        let items = collect_channels(&body);
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        let mut shelves = Vec::new();

        let (watch, subs, watch_later, history) = tokio::join!(
            self.browse("FEwhat_to_watch"),
            self.browse("FEsubscriptions"),
            self.browse("VLWL"),
            self.browse("FEhistory"),
        );

        if let Ok(body) = watch {
            shelves.extend(home_shelves_from_rich_grid(&body));
        }

        if let Ok(body) = subs {
            let vids = collect_videos(&body);
            if !vids.is_empty() {
                shelves.push(RecommendationShelf {
                    id: "subscriptions".into(),
                    title: "Latest from your subscriptions".into(),
                    subtitle: None,
                    category: RecommendationCategory::Other,
                    items: vids
                        .into_iter()
                        .take(40)
                        .map(RecommendationItem::Track)
                        .collect(),
                });
            }
        }

        if let Ok(body) = watch_later {
            let vids = collect_videos(&body);
            if !vids.is_empty() {
                shelves.push(RecommendationShelf {
                    id: "watch_later".into(),
                    title: "Watch Later".into(),
                    subtitle: None,
                    category: RecommendationCategory::Other,
                    items: vids
                        .into_iter()
                        .take(40)
                        .map(RecommendationItem::Track)
                        .collect(),
                });
            }
        }

        if let Ok(body) = history {
            let vids = collect_videos(&body);
            if !vids.is_empty() {
                shelves.push(RecommendationShelf {
                    id: "history".into(),
                    title: "Recently Watched".into(),
                    subtitle: None,
                    category: RecommendationCategory::RecentlyPlayed,
                    items: vids
                        .into_iter()
                        .take(20)
                        .map(RecommendationItem::Track)
                        .collect(),
                });
            }
        }

        Ok(RecommendationsPage { shelves })
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let detail = self.playlist_detail(id).await?;
        Ok(LibraryAlbumDetail {
            album: playlist_as_album(&detail.playlist),
            tracks: detail.tracks,
        })
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let body = self.browse(id).await?;
        let display = str_at(&body, &["/header/c4TabbedHeaderRenderer/title"])
            .unwrap_or("")
            .to_string();
        let tracks: Vec<LibraryTrackDto> = collect_videos(&body).into_iter().take(40).collect();
        let albums = collect_playlists(&body)
            .iter()
            .map(playlist_as_album)
            .collect();
        Ok(LibraryArtistDetail {
            key: id.into(),
            display,
            albums,
            tracks,
        })
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        let body = self.browse(&format!("VL{id}")).await?;
        let title = body
            .pointer("/header/playlistHeaderRenderer/title")
            .and_then(text_of)
            .or_else(|| {
                str_at(&body, &["/metadata/playlistMetadataRenderer/title"]).map(str::to_string)
            })
            .unwrap_or_default();
        let description = body
            .pointer("/header/playlistHeaderRenderer/descriptionText")
            .and_then(text_of)
            .filter(|s| !s.is_empty());
        let owner = body
            .pointer("/header/playlistHeaderRenderer/ownerText")
            .and_then(text_of)
            .filter(|s| !s.is_empty());
        let tracks = self.all_playlist_videos(&body).await;
        let playlist = LibraryPlaylistDto {
            id: stable_hash_i64(id),
            name: title,
            created_at: 0,
            updated_at: 0,
            track_count: tracks.len() as i64,
            cover_ids: tracks
                .first()
                .and_then(|t| t.cover_id.clone())
                .map(|s| vec![s])
                .unwrap_or_default(),
            service_id: Some(id.into()),
            description,
            owner,
        };
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }
}
