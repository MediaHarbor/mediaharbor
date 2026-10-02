use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};
use crate::services::tidal::client::TIDAL_CLIENT_ID;

use crate::services::common::library::{
    activity_feed_page, cover_id, f64_at, i64_at, id_at, nonempty_at, stable_hash_i64, str_at,
    string_at, u32_at, year4_at, ActivityFeedItem, FavKind, FavOp, MutationCapabilities,
    OwnedPlaylistRow, Page, PlaylistCreateInput, PlaylistMutateResult, RadioResult, RadioSeedKind,
    RecommendationCategory, RecommendationItem, RecommendationShelf, RecommendationsPage, SaveKind,
    SavedIdSet, ServiceCapabilities, ServiceLibrary, ServiceLibraryMutations, ServiceLibraryPage,
    ServicePlatform,
};

const API: &str = "https://api.tidal.com";
const AUTH_TOKEN_URL: &str = "https://auth.tidal.com/v1/oauth2/token";
const TIDAL_CLIENT_SECRET: &str = "Y8tIpqKJxs9BEIwYr0I9bSbMWDsogXJx9LaN3mCHwD4=";

async fn tidal_token_exchange(client: &Client, refresh_token: &str) -> MhResult<(String, i64)> {
    use base64::Engine;
    let auth = base64::engine::general_purpose::STANDARD
        .encode(format!("{TIDAL_CLIENT_ID}:{TIDAL_CLIENT_SECRET}"));
    let encoded: String = url::form_urlencoded::byte_serialize(refresh_token.as_bytes()).collect();
    let body = format!(
        "client_id={TIDAL_CLIENT_ID}&refresh_token={encoded}&grant_type=refresh_token&scope=r_usr%2Bw_usr%2Bw_sub"
    );
    let resp = client
        .post(AUTH_TOKEN_URL)
        .header("Authorization", format!("Basic {auth}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(MhError::Network)?;
    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let txt = resp.text().await.unwrap_or_default();
        return Err(MhError::Auth(format!(
            "Tidal refresh failed ({status}): {txt}"
        )));
    }
    let json: Value = resp.json().await.map_err(MhError::Network)?;
    let new_token = str_at(&json, &["access_token"])
        .ok_or_else(|| MhError::Auth("Tidal: no access_token in refresh response".into()))?
        .to_string();
    let expires_in = json
        .get("expires_in")
        .and_then(|v| v.as_i64())
        .unwrap_or(7 * 24 * 3600);
    Ok((new_token, expires_in))
}

pub async fn refresh_token_standalone(
    refresh_token: &str,
    settings_arc: &Arc<RwLock<Settings>>,
) -> MhResult<i64> {
    let client = crate::http_client::shared_client()?;
    let (new_token, expires_in) = tidal_token_exchange(&client, refresh_token).await?;
    let new_expiry = chrono::Utc::now().timestamp() + expires_in;
    let mut s = settings_arc.write().await;
    s.tidal_access_token = new_token;
    s.tidal_token_expiry = new_expiry.to_string();
    Ok(new_expiry)
}

pub struct TidalLibrary {
    client: Client,
    settings: Arc<RwLock<Settings>>,
    user_data: PathBuf,
    token: RwLock<String>,
    refresh_token: String,
    user_id: String,
    country: String,
    emitter: Arc<dyn crate::EventEmitter>,
}

impl TidalLibrary {
    fn log_line(&self, level: &str, message: String) {
        self.emitter
            .emit_log(&crate::ipc_contract::BackendLogEvent::new(
                level, "tidal", "Tidal", message,
            ));
    }
}

impl TidalLibrary {
    pub fn from_state(state: &crate::BackendState, s: &Settings) -> MhResult<Self> {
        if s.tidal_access_token.is_empty() {
            return Err(MhError::Auth(
                "Tidal not connected (no access token)".into(),
            ));
        }
        if s.tidal_user_id.is_empty() {
            return Err(MhError::Auth("Tidal user_id missing in settings".into()));
        }
        let country = if s.tidal_country_code.is_empty() {
            "US".to_string()
        } else {
            s.tidal_country_code.clone()
        };
        let client = crate::http_client::ua_client("MediaHarbor/2 (Tidal library)")?;
        Ok(Self {
            client,
            settings: state.settings.clone(),
            user_data: state.user_data.clone(),
            token: RwLock::new(s.tidal_access_token.clone()),
            refresh_token: s.tidal_refresh_token.clone(),
            user_id: s.tidal_user_id.clone(),
            country,
            emitter: state.emitter.clone(),
        })
    }

    async fn current_token(&self) -> String {
        self.token.read().await.clone()
    }

    async fn telemetry_device(&self) -> crate::services::tidal::telemetry::Device {
        let s = self.settings.read().await;
        crate::services::tidal::telemetry::Device {
            model: s.tidal_device_model.clone(),
            vendor: s.tidal_device_vendor.clone(),
            device_type: s.tidal_device_type.clone(),
            os_version: s.tidal_os_version.clone(),
            screen_width: s.tidal_screen_width,
            screen_height: s.tidal_screen_height,
        }
    }

    async fn refresh(&self) -> MhResult<String> {
        if self.refresh_token.is_empty() {
            return Err(MhError::Auth(
                "Tidal token expired and no refresh_token saved".into(),
            ));
        }
        let (new_token, _expires_in) =
            tidal_token_exchange(&self.client, &self.refresh_token).await?;
        {
            let mut t = self.token.write().await;
            *t = new_token.clone();
        }
        {
            let mut s = self.settings.write().await;
            s.tidal_access_token = new_token.clone();
            let snapshot = s.clone();
            let dir = self.user_data.clone();
            drop(s);
            tokio::spawn(async move {
                let _ = crate::settings::save_settings(&snapshot, &dir).await;
            });
        }
        Ok(new_token)
    }

    /// Runs `call` with the current token; on a 401 carrying Tidal's
    /// `subStatus: 11003` ("token expired") it refreshes once and retries.
    /// `label` names the operation in the error message.
    async fn with_token_refresh<F, Fut>(&self, label: &str, call: F) -> MhResult<Value>
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = MhResult<(StatusCode, Value)>>,
    {
        let token = self.current_token().await;
        let (status, body) = call(token).await?;
        if status.is_success() {
            return Ok(body);
        }
        let expired = status == StatusCode::UNAUTHORIZED
            && body.get("subStatus").and_then(|v| v.as_u64()) == Some(11003);
        if !expired {
            return Err(crate::services::common::http::api_error(
                label,
                status,
                &body.to_string(),
            ));
        }
        let fresh = self.refresh().await?;
        let (status, body) = call(fresh).await?;
        if !status.is_success() {
            return Err(crate::services::common::http::api_error(
                &format!("{label} (after token refresh)"),
                status,
                &body.to_string(),
            ));
        }
        Ok(body)
    }

    async fn get(&self, url: &str, extra: &[(&str, &str)]) -> MhResult<Value> {
        let try_once = |token: String| async move {
            let resp = self
                .client
                .get(url)
                .bearer_auth(&token)
                .header("accept", "application/json")
                .header("x-tidal-client-version", "2026.6.1")
                .query(&[
                    ("countryCode", self.country.as_str()),
                    ("deviceType", "BROWSER"),
                ])
                .query(extra)
                .send()
                .await
                .map_err(MhError::Network)?;
            let status = resp.status();
            let body: Value = resp.json().await.map_err(MhError::Network)?;
            Ok::<(StatusCode, Value), MhError>((status, body))
        };

        self.with_token_refresh(&format!("Tidal {url}"), try_once)
            .await
    }

    async fn home_feed_v2(&self) -> MhResult<RecommendationsPage> {
        let locale = system_locale(&self.country);
        let body = self
            .get(
                "https://tidal.com/v2/home/feed/static",
                &[("locale", &locale), ("platform", "WEB")],
            )
            .await?;
        let modules = body
            .get("items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut shelves = Vec::new();
        for module in modules {
            let title = str_at(&module, &["title"]).unwrap_or("").to_string();
            let module_id = str_at(&module, &["moduleId"]).unwrap_or("").to_string();
            let items: Vec<RecommendationItem> = module
                .get("items")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(classify_v2_item).collect())
                .unwrap_or_default();
            if items.is_empty() {
                continue;
            }
            shelves.push(RecommendationShelf {
                id: module_id.clone(),
                title,
                subtitle: str_at(&module, &["subtitle"]).map(str::to_string),
                category: v2_module_category(&module_id),
                items,
            });
        }
        Ok(RecommendationsPage { shelves })
    }

    fn page_qs(page: Page) -> Vec<(&'static str, String)> {
        vec![
            ("limit", page.limit.min(100).to_string()),
            ("offset", page.offset.to_string()),
        ]
    }

    async fn collect_all<T, F>(&self, url: &str, mapper: F) -> MhResult<Vec<T>>
    where
        F: Fn(&Value) -> Option<T> + Send + Sync,
    {
        self.collect_all_with(url, &[], mapper).await
    }

    async fn collect_all_with<T, F>(
        &self,
        url: &str,
        extra: &[(&str, &str)],
        mapper: F,
    ) -> MhResult<Vec<T>>
    where
        F: Fn(&Value) -> Option<T> + Send + Sync,
    {
        const PAGE: u32 = 100;
        let mut out = Vec::new();
        let mut offset: u32 = 0;
        loop {
            let limit = PAGE.to_string();
            let off = offset.to_string();
            let mut q: Vec<(&str, &str)> = Vec::with_capacity(extra.len() + 2);
            q.extend_from_slice(extra);
            q.push(("limit", &limit));
            q.push(("offset", &off));
            let body = self.get(url, &q).await?;
            let page = map_items(&body, &mapper);
            let returned = page.items.len() as u32;
            out.extend(page.items);
            let total = page.total.unwrap_or(0) as u32;
            offset += returned;
            if returned == 0 || returned < PAGE || (total > 0 && offset >= total) {
                break;
            }
            if offset > 10_000 {
                break;
            }
        }
        Ok(out)
    }
}

fn map_album(v: &Value) -> Option<LibraryAlbumDto> {
    Some(LibraryAlbumDto {
        album_key: id_at(v, &["id"])?,
        title: string_at(v, &["title"]),
        artist: string_at(v, &["/artist/name"]),
        year: year4_at(v, &["releaseDate"]),
        cover_id: str_at(v, &["cover"]).map(|s| cover_id(ServicePlatform::Tidal, s)),
        track_count: i64_at(v, &["numberOfTracks"]).unwrap_or(0),
        artist_id: id_at(v, &["/artist/id", "/artists/0/id"]),
        ..Default::default()
    })
}

fn map_activity(v: &Value) -> Option<ActivityFeedItem> {
    let act = v.get("followableActivity")?;
    let album = act.get("album")?;
    let id = id_at(album, &["id"])?;
    Some(ActivityFeedItem {
        id: format!(
            "{}::{id}",
            str_at(act, &["activityType"]).unwrap_or("activity")
        ),
        kind: "album".into(),
        title: string_at(album, &["title"]),
        subtitle: string_at(album, &["/artists/0/name", "/mainArtists/0/name"]),
        cover_id: str_at(album, &["cover"]).map(|c| cover_id(ServicePlatform::Tidal, c)),
        service_id: Some(id),
        occurred_at: str_at(act, &["occurredAt"]).map(str::to_string),
        seen: v.get("seen").and_then(|s| s.as_bool()).unwrap_or(false),
    })
}

fn map_track(v: &Value) -> Option<LibraryTrackDto> {
    let id = id_at(v, &["id"])?;
    let artist = string_at(v, &["/artist/name"]);
    Some(LibraryTrackDto {
        path: str_at(v, &["url"])
            .map(str::to_string)
            .unwrap_or_else(|| format!("http://www.tidal.com/track/{id}")),
        title: Some(string_at(v, &["title"])),
        artist: Some(artist.clone()),
        album: nonempty_at(v, &["/album/title"]),
        album_key: id_at(v, &["/album/id"]),
        year: year4_at(v, &["/album/releaseDate"]),
        duration_secs: f64_at(v, &["duration"]),
        track_no: u32_at(v, &["trackNumber"]),
        disc_no: u32_at(v, &["volumeNumber"]),
        size: 0,
        mtime_ns: 0,
        is_video: str_at(v, &["type"]) == Some("Music Video"),
        cover_id: str_at(v, &["/album/cover"]).map(|s| cover_id(ServicePlatform::Tidal, s)),
        primary_artist: Some(artist),
        artist_id: id_at(v, &["/artist/id", "/artists/0/id"]),
        ..Default::default()
    })
}

fn tidal_artist_image_url(picture_uuid: &str) -> String {
    format!(
        "https://resources.tidal.com/images/{}/750x750.jpg",
        picture_uuid.replace('-', "/")
    )
}

fn map_artist(v: &Value) -> Option<LibraryArtistDto> {
    Some(LibraryArtistDto {
        key: id_at(v, &["id"])?,
        display: string_at(v, &["name"]),
        album_count: 0,
        track_count: 0,
        cover_id: str_at(v, &["picture"])
            .map(|s| cover_id(ServicePlatform::Tidal, &tidal_artist_image_url(s))),
    })
}

fn map_playlist(v: &Value) -> Option<LibraryPlaylistDto> {
    let uuid = id_at(v, &["uuid", "id"])?;
    Some(LibraryPlaylistDto {
        id: stable_hash_i64(&uuid),
        name: string_at(v, &["title"]),
        created_at: 0,
        updated_at: 0,
        track_count: i64_at(v, &["numberOfTracks"]).unwrap_or(0),
        cover_ids: str_at(v, &["squareImage", "image"])
            .map(|s| vec![cover_id(ServicePlatform::Tidal, s)])
            .unwrap_or_default(),
        service_id: Some(uuid),
        description: nonempty_at(v, &["description"]),
        owner: nonempty_at(v, &["/creator/name", "/creator/profileName"]),
    })
}

fn classify_item(module_type: &str, it: &Value) -> Option<RecommendationItem> {
    if let (Some(inner), Some(t)) = (it.get("item"), str_at(it, &["type"])) {
        let inner_type = match t {
            "PLAYLIST" => "PLAYLIST_LIST",
            "ALBUM" => "ALBUM_LIST",
            "TRACK" | "TRACK_CREDITS" => "TRACK_LIST",
            "ARTIST" => "ARTIST_LIST",
            "VIDEO" => "VIDEO_LIST",
            _ => module_type,
        };
        return classify_item(inner_type, inner);
    }
    let by_type = match module_type {
        "ALBUM_LIST" | "ALBUM_HEADER" => map_album(it).map(RecommendationItem::Album),
        "TRACK_LIST" => map_track(it).map(RecommendationItem::Track),
        "PLAYLIST_LIST" | "FEATURED_PLAYLIST" => map_playlist(it).map(RecommendationItem::Playlist),
        "ARTIST_LIST" | "ARTIST_HEADER" => map_artist(it).map(RecommendationItem::Artist),
        "VIDEO_LIST" => map_track(it).map(|mut t| {
            t.is_video = true;
            RecommendationItem::Track(t)
        }),
        "MIX_LIST" => map_mix(it),
        _ => None,
    };
    if by_type.is_some() {
        return by_type;
    }
    if it.get("uuid").is_some() && it.get("numberOfTracks").is_some() {
        return map_playlist(it).map(RecommendationItem::Playlist);
    }
    if it.get("mixType").is_some() || it.pointer("/graphic/text").is_some() {
        return map_mix(it);
    }
    if it.get("trackNumber").is_some() {
        return map_track(it).map(RecommendationItem::Track);
    }
    if it.get("releaseDate").is_some() && it.get("artist").is_some() {
        return map_album(it).map(RecommendationItem::Album);
    }
    if it.get("picture").is_some() && it.get("name").is_some() && it.get("title").is_none() {
        return map_artist(it).map(RecommendationItem::Artist);
    }
    None
}

/// Parse a Tidal `pages/*` response (home, search_explore, album) into shelves.
/// Shared by the home feed and the Explore surface — both use the same
/// `rows[] -> modules[] -> pagedList.items[]` structure.
fn parse_pages_shelves(body: &Value) -> Vec<RecommendationShelf> {
    let mut shelves = Vec::new();
    let rows = body
        .get("rows")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for row in rows {
        let modules = row
            .get("modules")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        for module in modules {
            let title = str_at(&module, &["title"]).unwrap_or("").to_string();
            let id = str_at(&module, &["id"]).unwrap_or("").to_string();
            let mtype = str_at(&module, &["type"]).unwrap_or("");
            let items_v = module
                .pointer("/pagedList/items")
                .or_else(|| module.get("items"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let is_page_links = mtype.starts_with("PAGE_LINKS");
            let (items, category): (Vec<RecommendationItem>, RecommendationCategory) =
                if is_page_links {
                    let links = items_v
                        .iter()
                        .filter_map(|it| {
                            let api_path = str_at(it, &["apiPath"])?.to_string();
                            let t = str_at(it, &["title"]).unwrap_or("").to_string();
                            Some(RecommendationItem::PageLink {
                                api_path,
                                title: t,
                                icon: str_at(it, &["icon"]).map(str::to_string),
                            })
                        })
                        .collect();
                    (links, RecommendationCategory::Genre)
                } else {
                    let its = items_v
                        .iter()
                        .filter_map(|it| classify_item(mtype, it))
                        .collect();
                    (its, RecommendationCategory::Other)
                };
            if items.is_empty() {
                continue;
            }
            let title = if title.is_empty() && is_page_links {
                "Featured".to_string()
            } else {
                title
            };
            shelves.push(RecommendationShelf {
                id,
                title,
                subtitle: str_at(&module, &["description"]).map(str::to_string),
                category,
                items,
            });
        }
    }
    shelves
}

fn system_locale(country: &str) -> String {
    for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        let Ok(v) = std::env::var(var) else { continue };
        let base = v.split('.').next().unwrap_or("").replace('-', "_");
        if base.is_empty() || base == "C" || base == "POSIX" {
            continue;
        }
        if let Some((lang, region)) = base.split_once('_') {
            if lang.len() == 2 && region.len() == 2 {
                return format!("{lang}_{region}");
            }
        }
        if base.len() == 2 {
            return format!("{base}_{country}");
        }
    }
    format!("en_{country}")
}

fn v2_artist_name(data: &Value) -> String {
    if let Some(arr) = data.get("artists").and_then(|x| x.as_array()) {
        if let Some(main) = arr
            .iter()
            .find(|a| a.get("main").and_then(|m| m.as_bool()).unwrap_or(false))
            .and_then(|a| a.get("name"))
            .and_then(|x| x.as_str())
        {
            return main.to_string();
        }
        if let Some(first) = arr
            .first()
            .and_then(|a| a.get("name"))
            .and_then(|x| x.as_str())
        {
            return first.to_string();
        }
    }
    str_at(data, &["/artist/name"]).unwrap_or("").to_string()
}

fn tidal_mix_image(d: &Value) -> Option<String> {
    let rank = |sz: Option<&str>| match sz {
        Some("LARGE") => 3,
        Some("MEDIUM") => 2,
        Some("SMALL") => 1,
        _ => 0,
    };
    for arr_key in ["mixImages", "detailMixImages"] {
        let Some(arr) = d.get(arr_key).and_then(|x| x.as_array()) else {
            continue;
        };
        let best = arr
            .iter()
            .filter(|im| str_at(im, &["url"]).is_some())
            .max_by_key(|im| rank(str_at(im, &["size"])));
        if let Some(url) = best.and_then(|im| im.get("url")).and_then(|x| x.as_str()) {
            return Some(url.to_string());
        }
    }
    None
}

fn classify_v2_item(it: &Value) -> Option<RecommendationItem> {
    let kind = str_at(it, &["type"])?;
    let d = it.get("data")?;
    match kind {
        "ALBUM" => {
            let id = d.get("id")?.as_i64().map(|i| i.to_string())?;
            Some(RecommendationItem::Album(LibraryAlbumDto {
                album_key: id,
                title: str_at(d, &["title"]).unwrap_or("").to_string(),
                artist: v2_artist_name(d),
                year: str_at(d, &["releaseDate"]).map(|s| s.chars().take(4).collect()),
                cover_id: str_at(d, &["cover"]).map(|s| cover_id(ServicePlatform::Tidal, s)),
                track_count: d
                    .get("numberOfTracks")
                    .and_then(|x| x.as_i64())
                    .unwrap_or(0),
                artist_id: d
                    .pointer("/artists/0/id")
                    .or_else(|| d.pointer("/artist/id"))
                    .and_then(|x| x.as_i64())
                    .map(|i| i.to_string()),
                ..Default::default()
            }))
        }
        "TRACK" => {
            let id = d.get("id")?.as_i64().map(|i| i.to_string())?;
            let artist = v2_artist_name(d);
            Some(RecommendationItem::Track(LibraryTrackDto {
                path: format!("http://www.tidal.com/track/{id}"),
                title: str_at(d, &["title"]).map(str::to_string),
                artist: Some(artist.clone()),
                album: str_at(d, &["/album/title"]).map(str::to_string),
                album_key: d
                    .pointer("/album/id")
                    .and_then(|x| x.as_i64())
                    .map(|i| i.to_string()),
                duration_secs: d.get("duration").and_then(|x| x.as_f64()),
                track_no: d
                    .get("trackNumber")
                    .and_then(|x| x.as_u64())
                    .map(|n| n as u32),
                disc_no: d
                    .get("volumeNumber")
                    .and_then(|x| x.as_u64())
                    .map(|n| n as u32),
                size: 0,
                mtime_ns: 0,
                is_video: false,
                cover_id: str_at(d, &["/album/cover"]).map(|s| cover_id(ServicePlatform::Tidal, s)),
                primary_artist: Some(artist),
                ..Default::default()
            }))
        }
        "PLAYLIST" => map_playlist(d).map(RecommendationItem::Playlist),
        "ARTIST" => map_artist(d).map(RecommendationItem::Artist),
        "MIX" => {
            let id = str_at(d, &["id"])?.to_string();
            Some(RecommendationItem::Mix {
                id: format!("mix::{id}"),
                title: str_at(d, &["/titleTextInfo/text"])
                    .unwrap_or("")
                    .to_string(),
                subtitle: str_at(d, &["/subtitleTextInfo/text"]).map(str::to_string),
                cover_id: tidal_mix_cover(d).map(|u| cover_id(ServicePlatform::Tidal, &u)),
                url: None,
            })
        }
        _ => None,
    }
}

fn v2_module_category(module_id: &str) -> RecommendationCategory {
    match module_id {
        "DAILY_MIXES" => RecommendationCategory::DailyMix,
        "SUGGESTED_RADIOS_MIXES" => RecommendationCategory::Stations,
        "CONTINUE_LISTEN_TO" => RecommendationCategory::RecentlyPlayed,
        "NEW_TRACK_SUGGESTIONS" | "NEW_ALBUM_SUGGESTIONS" => RecommendationCategory::NewReleases,
        "ALBUM_RECOMMENDATIONS" | "RECOMMENDED_USERS_PLAYLISTS" => {
            RecommendationCategory::Editorial
        }
        _ => RecommendationCategory::Other,
    }
}

/// A mix's artwork, whichever of the two shapes Tidal used for it.
///
/// `mixImages` is the v2 shape and `graphic`/`imageId`/`images` the v1 one; only the
/// v2 path composed them, so a v1 mix carrying `mixImages` came back with no cover.
fn tidal_mix_cover(v: &Value) -> Option<String> {
    tidal_mix_image(v).or_else(|| tidal_image_key(v))
}

fn tidal_image_key(v: &Value) -> Option<String> {
    if let Some(id) = str_at(v, &["/graphic/images/0/id"]) {
        return Some(id.to_string());
    }
    if let Some(id) = str_at(v, &["imageId"]) {
        return Some(id.to_string());
    }
    for size in ["SQUARE", "LARGE", "MEDIUM", "SMALL"] {
        if let Some(url) = v
            .pointer(&format!("/images/{size}/url"))
            .and_then(|x| x.as_str())
        {
            return Some(url.to_string());
        }
    }
    if let Some(url) = str_at(v, &["/graphic/images/0/url"]) {
        return Some(url.to_string());
    }
    None
}

fn map_mix(v: &Value) -> Option<RecommendationItem> {
    let id = str_at(v, &["id"])
        .map(str::to_string)
        .or_else(|| str_at(v, &["/graphic/images/0/id"]).map(str::to_string))?;
    let title = str_at(v, &["/graphic/text"])
        .or_else(|| str_at(v, &["title"]))
        .unwrap_or("")
        .to_string();
    let cover_key = tidal_mix_cover(v).map(|k| cover_id(ServicePlatform::Tidal, &k));
    Some(RecommendationItem::Mix {
        id: format!("mix::{id}"),
        title,
        subtitle: str_at(v, &["subTitle"]).map(str::to_string),
        cover_id: cover_key,
        url: None,
    })
}

fn map_items<T>(body: &Value, mapper: impl Fn(&Value) -> Option<T>) -> ServiceLibraryPage<T> {
    let items_arr = body
        .get("items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let items: Vec<T> = items_arr
        .iter()
        .filter_map(|wrapper| {
            let inner = wrapper.get("item").unwrap_or(wrapper);
            mapper(inner)
        })
        .collect();
    let total = body
        .get("totalNumberOfItems")
        .or_else(|| body.get("total"))
        .and_then(|x| x.as_u64());
    ServiceLibraryPage::of(items, total)
}

#[async_trait]
impl ServiceLibrary for TidalLibrary {
    fn as_mutations(self: Arc<Self>) -> Option<Arc<dyn ServiceLibraryMutations>> {
        Some(self)
    }

    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Tidal
    }
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::all()
            .with_activity_feed()
            .with_mutations(self.mutation_capabilities())
    }

    async fn albums(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        self.favorites("albums", page, map_album).await
    }

    async fn tracks(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        self.favorites("tracks", page, map_track).await
    }

    async fn artists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        self.favorites("artists", page, map_artist).await
    }

    async fn playlists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        let qs = Self::page_qs(page);
        let qs: Vec<(&str, &str)> = qs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let mut merged: Vec<LibraryPlaylistDto> = Vec::new();
        for url in [
            format!("{API}/v1/users/{}/favorites/playlists", self.user_id),
            format!("{API}/v1/users/{}/playlists", self.user_id),
        ] {
            if let Ok(body) = self.get(&url, &qs).await {
                merged.extend(map_items(&body, map_playlist).items)
            }
        }
        Ok(ServiceLibraryPage::of(merged, None))
    }

    async fn videos(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        self.favorites("videos", page, |v| {
            let mut t = map_track(v)?;
            t.is_video = true;
            Some(t)
        })
        .await
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        match self.home_feed_v2().await {
            Ok(page) if !page.shelves.is_empty() => return Ok(page),
            Ok(_) => {}
            Err(e) => self.log_line(
                "warn",
                format!("Tidal v2 home feed failed, falling back to v1: {e}"),
            ),
        }

        let body = self.get(&format!("{API}/v1/pages/home"), &[]).await?;
        self.maybe_fire_telemetry(&body).await;
        Ok(RecommendationsPage {
            shelves: parse_pages_shelves(&body),
        })
    }

    async fn explore(&self) -> MhResult<RecommendationsPage> {
        let url =
            format!("{API}/v1/pages/search_explore?deviceType=PHONE&locale=en_US&platform=ANDROID");
        let body = self.get(&url, &[]).await?;
        self.maybe_fire_telemetry(&body).await;
        Ok(RecommendationsPage {
            shelves: parse_pages_shelves(&body),
        })
    }

    async fn explore_page(&self, path: &str) -> MhResult<RecommendationsPage> {
        let clean = path.trim_start_matches('/');
        let url = format!("{API}/v1/{clean}?deviceType=PHONE&locale=en_US&platform=ANDROID");
        let body = self.get(&url, &[]).await?;
        self.maybe_fire_telemetry(&body).await;
        Ok(RecommendationsPage {
            shelves: parse_pages_shelves(&body),
        })
    }

    async fn activity_feed(&self, limit: u32) -> MhResult<Value> {
        let url = format!("{API}/v2/feed/activities?limit={limit}");
        let body = self.get(&url, &[]).await?;
        let rows = body
            .get("activities")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(activity_feed_page(
            rows.iter().filter_map(map_activity).collect(),
        ))
    }

    async fn report_playback(
        &self,
        service_track_id: &str,
        duration_secs: u64,
        _context_uri: Option<&str>,
        _track_index: Option<u64>,
    ) -> MhResult<()> {
        let (enabled, quality) = {
            let s = self.settings.read().await;
            (
                s.sync_playback_history(ServicePlatform::Tidal),
                s.tidal_quality,
            )
        };
        if !enabled {
            return Ok(());
        }
        let product_id = service_track_id.to_string();
        let user_id = self.user_id.clone();
        let device = self.telemetry_device().await;
        let token = self.current_token().await;
        tokio::spawn(async move {
            crate::services::tidal::telemetry::report_playback(
                &token,
                &user_id,
                device,
                &product_id,
                duration_secs,
                quality,
            )
            .await;
        });
        Ok(())
    }

    async fn album_page(&self, id: &str) -> MhResult<Value> {
        let url = format!(
            "{API}/v1/pages/album?albumId={id}&deviceType=TABLET&locale=en_US&platform=ANDROID"
        );
        let body = self.get(&url, &[]).await?;
        Ok(serde_json::json!({
            "shelves": parse_pages_shelves(&body),
            "raw": body,
        }))
    }

    async fn artist_page(&self, id: &str) -> MhResult<Value> {
        let url = format!(
            "{API}/v1/pages/artist?artistId={id}&deviceType=PHONE&locale=en_US&platform=ANDROID"
        );
        let body = self.get(&url, &[]).await?;
        Ok(serde_json::json!({
            "shelves": parse_pages_shelves(&body),
            "raw": body,
        }))
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let album_body = self.get(&format!("{API}/v1/albums/{id}"), &[]).await?;
        let album =
            map_album(&album_body).ok_or_else(|| MhError::NotFound(format!("Tidal album {id}")))?;
        let tracks = self
            .collect_all(&format!("{API}/v1/albums/{id}/tracks"), map_track)
            .await?;
        Ok(LibraryAlbumDetail { album, tracks })
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let artist_body = self.get(&format!("{API}/v1/artists/{id}"), &[]).await?;
        let display = str_at(&artist_body, &["name"]).unwrap_or("").to_string();
        let url = format!("{API}/v1/artists/{id}/albums");
        let (lps, eps, comps) = tokio::join!(
            self.collect_all(&url, map_album),
            self.collect_all_with(&url, &[("filter", "EPSANDSINGLES")], map_album),
            self.collect_all_with(&url, &[("filter", "COMPILATIONS")], map_album),
        );
        let lps = lps.unwrap_or_default();
        let eps = eps.unwrap_or_default();
        let comps = comps.unwrap_or_default();
        let mut seen = std::collections::HashSet::new();
        let mut albums: Vec<LibraryAlbumDto> = Vec::new();
        for a in lps.into_iter().chain(eps).chain(comps) {
            if seen.insert(a.album_key.clone()) {
                albums.push(a);
            }
        }
        let top_body = self
            .get(
                &format!("{API}/v1/artists/{id}/toptracks"),
                &[("limit", "50")],
            )
            .await?;
        let tracks = map_items(&top_body, map_track).items;
        Ok(LibraryArtistDetail {
            key: id.to_string(),
            display,
            albums,
            tracks,
        })
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        if let Some(mix_id) = id.strip_prefix("mix::") {
            let tracks = self
                .collect_all(&format!("{API}/v1/mixes/{mix_id}/items"), |v| {
                    map_track(v.get("item").unwrap_or(v))
                })
                .await?;
            let playlist = LibraryPlaylistDto {
                id: crate::services::common::library::stable_hash_i64(id),
                name: "Mix".to_string(),
                created_at: 0,
                updated_at: 0,
                track_count: tracks.len() as i64,
                cover_ids: Vec::new(),
                service_id: Some(id.to_string()),
                description: None,
                owner: None,
            };
            return Ok(LibraryPlaylistDetail { playlist, tracks });
        }
        let pl_body = self.get(&format!("{API}/v1/playlists/{id}"), &[]).await?;
        let playlist = map_playlist(&pl_body)
            .ok_or_else(|| MhError::NotFound(format!("Tidal playlist {id}")))?;
        let tracks = self
            .collect_all(&format!("{API}/v1/playlists/{id}/items"), map_track)
            .await?;
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }
}

impl TidalLibrary {
    /// If the per-service telemetry opt-in is enabled, replicate the real client's
    /// `display_module` impression beacons for the modules in a freshly-loaded
    /// `pages/*` response. Best-effort, off by default, never blocks the response.
    async fn maybe_fire_telemetry(&self, body: &Value) {
        let enabled = self
            .settings
            .read()
            .await
            .telemetry_enabled(ServicePlatform::Tidal);
        if !enabled {
            return;
        }
        let page_id = str_at(body, &["id"]).unwrap_or("");
        let mut modules: Vec<(String, String)> = Vec::new();
        if let Some(rows) = body.get("rows").and_then(|v| v.as_array()) {
            for row in rows {
                if let Some(mods) = row.get("modules").and_then(|v| v.as_array()) {
                    for m in mods {
                        if let Some(mid) = str_at(m, &["id"]) {
                            modules.push((mid.to_string(), page_id.to_string()));
                        }
                    }
                }
            }
        }
        if modules.is_empty() {
            return;
        }
        let token = self.current_token().await;
        let user_id = self.user_id.clone();
        let device = self.telemetry_device().await;
        tokio::spawn(async move {
            crate::services::tidal::telemetry::display_modules(&token, &user_id, device, &modules)
                .await;
        });
    }

    async fn mutate(
        &self,
        method: reqwest::Method,
        url: &str,
        query: &[(&str, &str)],
        form: &[(&str, &str)],
    ) -> MhResult<Value> {
        async fn one_call(
            this: &TidalLibrary,
            method: reqwest::Method,
            url: &str,
            token: &str,
            query: &[(&str, &str)],
            form: &[(&str, &str)],
        ) -> MhResult<(StatusCode, Value)> {
            let mut req = this
                .client
                .request(method, url)
                .bearer_auth(token)
                .header("accept", "application/json")
                .query(&[
                    ("countryCode", this.country.as_str()),
                    ("deviceType", "BROWSER"),
                ])
                .query(query);
            if !form.is_empty() {
                req = req.form(form);
            }
            let resp = req.send().await.map_err(MhError::Network)?;
            let status = resp.status();
            let body: Value = resp.json().await.unwrap_or(Value::Null);
            Ok((status, body))
        }

        let label = format!("Tidal {method} {url}");
        self.with_token_refresh(&label, |token| {
            let method = method.clone();
            async move { one_call(self, method, url, &token, query, form).await }
        })
        .await
    }

    fn favorites_url(&self, kind: &str) -> String {
        format!("{API}/v1/users/{}/favorites/{kind}", self.user_id)
    }

    async fn favorites<T, F>(
        &self,
        kind: &str,
        page: Page,
        mapper: F,
    ) -> MhResult<ServiceLibraryPage<T>>
    where
        F: Fn(&Value) -> Option<T> + Send + Sync,
    {
        let qs = Self::page_qs(page);
        let qs: Vec<(&str, &str)> = qs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let body = self.get(&self.favorites_url(kind), &qs).await?;
        Ok(map_items(&body, mapper))
    }

    fn playlists_url(&self) -> String {
        format!("{API}/v1/users/{}/playlists", self.user_id)
    }
}

#[async_trait]
impl ServiceLibraryMutations for TidalLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Tidal
    }

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities::full_audio()
    }

    async fn set_favorites(&self, kind: FavKind, op: FavOp, ids: &[String]) -> MhResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let (collection, add_param) = match kind {
            FavKind::Track => ("tracks", "trackIds"),
            FavKind::Album => ("albums", "albumIds"),
            FavKind::Artist => ("artists", "artistIds"),
            FavKind::Label => return Err(MhError::Unsupported("Tidal has no labels".into())),
        };
        let base = self.favorites_url(collection);
        match op {
            FavOp::Add => {
                let joined = ids.join(",");
                self.mutate(
                    reqwest::Method::POST,
                    &base,
                    &[],
                    &[(add_param, joined.as_str()), ("onArtifactNotFound", "FAIL")],
                )
                .await?;
            }
            FavOp::Remove => {
                futures_util::future::try_join_all(ids.iter().map(|id| {
                    let url = format!("{base}/{id}");
                    async move { self.mutate(reqwest::Method::DELETE, &url, &[], &[]).await }
                }))
                .await?;
            }
        }
        Ok(())
    }

    async fn follow_playlist(&self, id: &str) -> MhResult<()> {
        self.mutate(
            reqwest::Method::POST,
            &self.favorites_url("playlists"),
            &[],
            &[("uuids", id)],
        )
        .await?;
        Ok(())
    }
    async fn unfollow_playlist(&self, id: &str) -> MhResult<()> {
        self.mutate(
            reqwest::Method::DELETE,
            &format!("{}/{id}", self.favorites_url("playlists")),
            &[],
            &[],
        )
        .await?;
        Ok(())
    }

    async fn create_playlist(&self, input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        let desc = input.description.clone().unwrap_or_default();
        let resp = self
            .mutate(
                reqwest::Method::POST,
                &self.playlists_url(),
                &[],
                &[
                    ("title", input.name.as_str()),
                    ("description", desc.as_str()),
                ],
            )
            .await?;
        let uuid = str_at(&resp, &["uuid"])
            .ok_or_else(|| MhError::Other(format!("Tidal createPlaylist: no uuid in {resp}")))?
            .to_string();
        if !input.initial_track_ids.is_empty() {
            let joined = input.initial_track_ids.join(",");
            self.mutate(
                reqwest::Method::POST,
                &format!("{API}/v1/playlists/{uuid}/items"),
                &[],
                &[
                    ("trackIds", joined.as_str()),
                    ("onArtifactNotFound", "FAIL"),
                    ("onDupes", "ADD"),
                ],
            )
            .await?;
        }
        Ok(PlaylistMutateResult::id(uuid))
    }

    async fn rename_playlist(
        &self,
        id: &str,
        new_name: &str,
        new_description: Option<&str>,
        _new_is_public: Option<bool>,
        _new_is_collaborative: Option<bool>,
    ) -> MhResult<()> {
        let desc = new_description.unwrap_or("");
        self.mutate(
            reqwest::Method::POST,
            &format!("{API}/v1/playlists/{id}"),
            &[],
            &[("title", new_name), ("description", desc)],
        )
        .await?;
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> MhResult<()> {
        self.mutate(
            reqwest::Method::DELETE,
            &format!("{API}/v1/playlists/{id}"),
            &[],
            &[],
        )
        .await?;
        Ok(())
    }

    async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        let joined = track_ids.join(",");
        self.mutate(
            reqwest::Method::POST,
            &format!("{API}/v1/playlists/{id}/items"),
            &[],
            &[
                ("trackIds", joined.as_str()),
                ("onArtifactNotFound", "FAIL"),
                ("onDupes", "ADD"),
            ],
        )
        .await?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn remove_playlist_tracks(
        &self,
        id: &str,
        _track_ids: &[String],
        positions: Option<&[u32]>,
    ) -> MhResult<PlaylistMutateResult> {
        let Some(positions) = positions else {
            return Err(MhError::Other(
                "Tidal remove_playlist_tracks requires positions[] (item indices)".into(),
            ));
        };
        let mut sorted: Vec<u32> = positions.to_vec();
        sorted.sort_by(|a, b| b.cmp(a));
        for idx in sorted {
            self.mutate(
                reqwest::Method::DELETE,
                &format!("{API}/v1/playlists/{id}/items/{idx}"),
                &[],
                &[],
            )
            .await?;
        }
        Ok(PlaylistMutateResult::id(id))
    }

    async fn reorder_playlist(
        &self,
        id: &str,
        from: u32,
        to: u32,
    ) -> MhResult<PlaylistMutateResult> {
        let to_str = to.to_string();
        self.mutate(
            reqwest::Method::POST,
            &format!("{API}/v1/playlists/{id}/items/{from}/move/{to}"),
            &[],
            &[("toIndex", to_str.as_str())],
        )
        .await?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn radio_for(&self, seed_kind: RadioSeedKind, seed_id: &str) -> MhResult<RadioResult> {
        let url = match seed_kind {
            RadioSeedKind::Track => format!("{API}/v1/tracks/{seed_id}/radio"),
            RadioSeedKind::Artist => format!("{API}/v1/artists/{seed_id}/radio"),
            _ => {
                return Err(MhError::Other(
                    "Tidal radio only supported for track / artist seeds".into(),
                ))
            }
        };
        let body = self.get(&url, &[("limit", "50")]).await?;
        let tracks = map_items(&body, map_track).items;
        let title = match seed_kind {
            RadioSeedKind::Track => format!("Track radio · {seed_id}"),
            RadioSeedKind::Artist => format!("Artist radio · {seed_id}"),
            _ => "Tidal radio".into(),
        };
        Ok(RadioResult {
            seed_kind,
            seed_id: seed_id.to_string(),
            title,
            tracks,
            station_id: None,
            video_urls: Default::default(),
            continuation: None,
        })
    }

    async fn fetch_saved_ids(&self, kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        let mut out = SavedIdSet::default();
        let url = format!("{API}/v1/users/{}/favorites/ids", self.user_id);
        let body = self.get(&url, &[]).await.unwrap_or(Value::Null);
        for kind in kinds {
            let key = match kind {
                SaveKind::Track => "TRACK",
                SaveKind::Album => "ALBUM",
                SaveKind::Artist => "ARTIST",
                SaveKind::Playlist => "PLAYLIST",
            };
            let ids = body
                .get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(crate::services::common::library::json_id)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            out.set(*kind, ids);
        }
        Ok(out)
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        let body = self
            .get(&self.playlists_url(), &[("limit", "100"), ("offset", "0")])
            .await?;
        let mut rows = Vec::new();
        if let Some(arr) = body.get("items").and_then(|v| v.as_array()) {
            for item in arr {
                let uuid = str_at(item, &["uuid"]).unwrap_or("");
                if uuid.is_empty() {
                    continue;
                }
                let creator_id = item
                    .pointer("/creator/id")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(-1);
                if creator_id.to_string() != self.user_id {
                    continue;
                }
                rows.push(OwnedPlaylistRow {
                    platform: ServicePlatform::Tidal.as_str().into(),
                    service_id: uuid.into(),
                    name: str_at(item, &["title"]).unwrap_or("").into(),
                    cover_id: str_at(item, &["image"])
                        .or_else(|| str_at(item, &["squareImage"]))
                        .map(|s| cover_id(ServicePlatform::Tidal, s)),
                    track_count: item
                        .get("numberOfTracks")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    updated_at: item
                        .get("lastUpdated")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                });
            }
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod mix_cover_tests {
    use super::{classify_v2_item, RecommendationItem};
    use serde_json::json;

    fn cover_of(item: RecommendationItem) -> Option<String> {
        match item {
            RecommendationItem::Mix { cover_id, .. } => cover_id,
            _ => None,
        }
    }

    #[test]
    fn a_mix_image_array_is_still_read() {
        let it = json!({
            "type": "MIX",
            "data": {
                "id": "0123456789abcdef",
                "titleTextInfo": { "text": "My Mix 1" },
                "mixImages": [
                    { "size": "SMALL", "url": "https://resources.tidal.com/s.jpg" },
                    { "size": "LARGE", "url": "https://resources.tidal.com/l.jpg" }
                ]
            }
        });
        assert_eq!(
            cover_of(classify_v2_item(&it).unwrap()).as_deref(),
            Some("tidal:https://resources.tidal.com/l.jpg")
        );
    }

    /// The shapes the v1 feed always handled but the v2 MIX arm never tried, which is
    /// why Daily Discovery / Welcome Mix / My Mix N came through with no artwork.
    #[test]
    fn the_v1_image_shapes_are_reachable_from_v2() {
        let by_id = json!({
            "type": "MIX",
            "data": { "id": "m1", "titleTextInfo": { "text": "My Daily Discovery" },
                      "imageId": "aaaa-bbbb-cccc" }
        });
        assert_eq!(
            cover_of(classify_v2_item(&by_id).unwrap()).as_deref(),
            Some("tidal:aaaa-bbbb-cccc")
        );

        let by_map = json!({
            "type": "MIX",
            "data": { "id": "m2", "titleTextInfo": { "text": "Welcome Mix" },
                      "images": { "MEDIUM": { "url": "https://resources.tidal.com/m.jpg" } } }
        });
        assert_eq!(
            cover_of(classify_v2_item(&by_map).unwrap()).as_deref(),
            Some("tidal:https://resources.tidal.com/m.jpg")
        );

        let by_graphic = json!({
            "type": "MIX",
            "data": { "id": "m3", "titleTextInfo": { "text": "My New Arrivals" },
                      "graphic": { "images": [{ "id": "dddd-eeee" }] } }
        });
        assert_eq!(
            cover_of(classify_v2_item(&by_graphic).unwrap()).as_deref(),
            Some("tidal:dddd-eeee")
        );

        let only_small = json!({
            "type": "MIX",
            "data": { "id": "m4", "titleTextInfo": { "text": "My Mix 8" },
                      "images": { "SMALL": { "url": "https://resources.tidal.com/s.jpg" } } }
        });
        assert_eq!(
            cover_of(classify_v2_item(&only_small).unwrap()).as_deref(),
            Some("tidal:https://resources.tidal.com/s.jpg")
        );
    }
}
