use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};
use crate::services::common::ids::{now_millis, now_secs};
use crate::services::spotify::session::LibrespotService;
use crate::services::spotify::web_player::{self, WebPlayerAssets};

use crate::services::common::library::{
    activity_feed_page, cover_id, f64_at, i64_at, nonempty_at, stable_hash_i64, str_at, string_at,
    u32_at, year4_at, ActivityFeedItem, FavKind, FavOp, MutationCapabilities, OwnedPlaylistRow,
    Page, PlaylistCreateInput, PlaylistMutateResult, RadioResult, RadioSeedKind,
    RecommendationCategory, RecommendationItem, RecommendationShelf, RecommendationsPage, SaveKind,
    SavedIdSet, ServiceCapabilities, ServiceLibrary, ServiceLibraryMutations, ServiceLibraryPage,
    ServicePlatform,
};

const PATHFINDER: &str = "https://api-partner.spotify.com/pathfinder/v2/query";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";
const HASHES_TTL_SECS: u64 = 7 * 24 * 3600;

pub struct SpotifyLibrary {
    client: Client,
    librespot: Arc<tokio::sync::RwLock<LibrespotService>>,
    hashes: RwLock<WebPlayerAssets>,
    cache_path: PathBuf,
    credentials: crate::auth::credentials::ApiCredentials,
    settings: Option<Arc<tokio::sync::RwLock<crate::defaults::Settings>>>,
    emitter: Option<Arc<dyn crate::EventEmitter>>,
}

impl SpotifyLibrary {
    pub fn from_state(state: &crate::BackendState) -> MhResult<Self> {
        let mut lib = Self::from_parts(state.librespot.clone(), &state.user_data)?;
        lib.settings = Some(state.settings.clone());
        lib.emitter = Some(state.emitter.clone());
        lib.credentials = state.credentials.clone();
        Ok(lib)
    }

    fn log_line(&self, level: &str, message: String) {
        if let Some(em) = &self.emitter {
            em.emit_log(&crate::ipc_contract::BackendLogEvent::new(
                level, "spotify", "Spotify", message,
            ));
        }
    }

    pub fn from_parts(
        librespot: Arc<tokio::sync::RwLock<LibrespotService>>,
        user_data: &std::path::Path,
    ) -> MhResult<Self> {
        let cache_path = user_data.join("spotify_pf_hashes.json");
        let hashes = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|s| serde_json::from_str::<WebPlayerAssets>(&s).ok())
            .unwrap_or_default();
        {
            let seed = hashes.clone();
            tokio::spawn(async move {
                if web_player::cached().await.is_none() {
                    web_player::seed(seed).await;
                }
            });
        }
        let client = crate::http_client::ua_client(UA)?;
        Ok(Self {
            client,
            librespot,
            hashes: RwLock::new(hashes),
            cache_path,
            credentials: crate::auth::credentials::bundled(),
            settings: None,
            emitter: None,
        })
    }

    /// A client_credentials Web API client, which — unlike the pathfinder path —
    /// needs no sp_dc cookie. Same precedence as every other call site: Settings
    /// first, then the resolved credentials.
    async fn web_api(&self) -> MhResult<crate::services::spotify::api::SpotifyApiClient> {
        let (id, secret) = match &self.settings {
            Some(s) => {
                let s = s.read().await;
                (s.spotify_client_id.clone(), s.spotify_client_secret.clone())
            }
            None => (String::new(), String::new()),
        };
        crate::services::spotify::api::SpotifyApiClient::new(
            crate::auth::credentials::preferred(&id, &self.credentials.spotify_client_id),
            crate::auth::credentials::preferred(&secret, &self.credentials.spotify_client_secret),
        )
    }

    async fn auth(&self) -> MhResult<(String, Option<String>)> {
        {
            let mut svc = self.librespot.write().await;
            if !svc.is_logged_in() {
                return Err(MhError::Auth(
                    "Spotify not connected. Set the cookies path in Settings → Spotify.".into(),
                ));
            }
            svc.ensure_tokens().await?;
        }
        let svc = self.librespot.read().await;
        let token = svc
            .cached_access_token()
            .ok_or_else(|| MhError::Auth("Spotify access token unavailable".into()))?;
        let client_token = svc.cached_client_token();
        Ok((token, client_token))
    }

    async fn known_hash(&self, op: &str) -> Option<String> {
        self.hashes.read().await.hashes.get(op).cloned()
    }

    async fn hashes_age_secs(&self) -> u64 {
        let g = self.hashes.read().await;
        now_secs().saturating_sub(g.scraped_at)
    }

    /// A fresh sweep of the web player bundles. The walk itself lives in
    /// `web_player` because the TOTP secret comes out of the same download.
    async fn rescrape_hashes(&self) -> MhResult<WebPlayerAssets> {
        let assets = web_player::scrape(&self.client).await?;
        if assets.hashes.is_empty() {
            return Err(MhError::Other(
                "Spotify pathfinder hash scrape returned 0 entries — bundle format may have changed"
                    .into(),
            ));
        }
        let snapshot = assets.clone();
        let path = self.cache_path.clone();
        tokio::spawn(async move {
            if let Ok(json) = serde_json::to_string_pretty(&snapshot) {
                let _ = tokio::fs::write(&path, json).await;
            }
        });
        *self.hashes.write().await = assets.clone();
        Ok(assets)
    }

    async fn hash_for(&self, op: &str) -> MhResult<String> {
        if let Some(h) = self.known_hash(op).await {
            if self.hashes_age_secs().await < HASHES_TTL_SECS {
                return Ok(h);
            }
        }
        let fresh = self.rescrape_hashes().await?;
        fresh.hashes.get(op).cloned().ok_or_else(|| {
            MhError::Other(format!(
                "Spotify pathfinder hash for {op} not found in bundle"
            ))
        })
    }

    async fn pathfinder(&self, op: &str, variables: Value) -> MhResult<Value> {
        let (token, client_token) = self.auth().await?;
        let hash = self.hash_for(op).await?;
        let attempt =
            |hash: String, token: String, client_token: Option<String>, variables: Value| {
                let this = self;
                let op_str = op.to_string();
                async move {
                    this.pathfinder_with_hash(
                        &op_str,
                        &variables,
                        &hash,
                        &token,
                        client_token.as_deref(),
                    )
                    .await
                }
            };
        match attempt(
            hash.clone(),
            token.clone(),
            client_token.clone(),
            variables.clone(),
        )
        .await
        {
            Ok(v) => Ok(v),
            Err(e) => {
                let is_network = matches!(&e, MhError::Network(_));
                let forbidden = matches!(&e, MhError::Other(msg) if msg.contains("returned 403") || msg.contains("returned 401"));
                let rotated = matches!(&e, MhError::Other(msg) if msg.contains("PersistedQueryNotFound") || msg.contains("BAD_REQUEST"));
                if is_network {
                    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                    return attempt(hash, token, client_token, variables).await;
                }
                if forbidden {
                    let fresh_ct = {
                        let mut svc = self.librespot.write().await;
                        svc.force_refresh_client_token().await.ok().flatten()
                    };
                    let (t2, _) = self.auth().await?;
                    return attempt(hash, t2, fresh_ct, variables).await;
                }
                if !rotated {
                    return Err(e);
                }
                let fresh = self.rescrape_hashes().await?;
                let h2 = fresh.hashes.get(op).cloned().ok_or_else(|| {
                    MhError::Other(format!("Spotify hash for {op} still missing after refresh"))
                })?;
                let (t2, ct2) = self.auth().await?;
                self.pathfinder_with_hash(op, &variables, &h2, &t2, ct2.as_deref())
                    .await
            }
        }
    }

    async fn pathfinder_with_hash(
        &self,
        op: &str,
        variables: &Value,
        sha256: &str,
        token: &str,
        client_token: Option<&str>,
    ) -> MhResult<Value> {
        let body_json = json!({
            "operationName": op,
            "variables": variables,
            "extensions": { "persistedQuery": { "version": 1, "sha256Hash": sha256 } },
        });
        let mut h = HeaderMap::new();
        if let Ok(v) = HeaderValue::from_str(&format!("Bearer {token}")) {
            h.insert("authorization", v);
        }
        h.insert("accept", HeaderValue::from_static("application/json"));
        h.insert("content-type", HeaderValue::from_static("application/json"));
        h.insert("app-platform", HeaderValue::from_static("WebPlayer"));
        h.insert(
            "spotify-app-version",
            HeaderValue::from_static("1.2.70.61.g856ccd63"),
        );
        h.insert(
            "origin",
            HeaderValue::from_static("https://open.spotify.com"),
        );
        h.insert(
            "referer",
            HeaderValue::from_static("https://open.spotify.com/"),
        );
        if let Some(ct) = client_token {
            h.insert("client-token", HeaderValue::from_str(ct)?);
        }
        let resp = self
            .client
            .post(PATHFINDER)
            .headers(h)
            .json(&body_json)
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let body: Value = resp.json().await.map_err(MhError::Network)?;
        if status != StatusCode::OK {
            return Err(MhError::Other(format!(
                "Spotify pathfinder {op} returned {}: {body}",
                status.as_u16()
            )));
        }
        if body.get("errors").is_some() {
            return Err(MhError::Other(format!(
                "Spotify pathfinder {op} errors: {body}"
            )));
        }
        Ok(body)
    }

    pub async fn search(&self, query: &str, search_type: &str) -> MhResult<Value> {
        let (op, container) = match search_type {
            "album" => ("searchAlbums", "albumsV2"),
            "artist" => ("searchArtists", "artists"),
            "playlist" => ("searchPlaylists", "playlists"),
            "podcast" | "show" => ("searchPodcasts", "podcasts"),
            "episode" => ("searchEpisodes", "episodes"),
            "audiobook" => ("searchAudiobooks", "audiobooks"),
            _ => ("searchTracks", "tracksV2"),
        };
        let vars = json!({
            "searchTerm": query,
            "limit": crate::services::common::limits::SPOTIFY_PAGE_MAX,
            "offset": 0,
            "numberOfTopResults": crate::services::common::limits::SPOTIFY_PAGE_MAX,
            "includeArtistHasConcertsField": false,
            "includeAudiobooks": true,
            "includeAuthors": false,
            "includePreReleases": true,
            "includeAlbumPreReleases": false,
            "includeEpisodeContentRatingsV2": true,
        });
        let body = self.pathfinder(op, vars).await?;
        Ok(transform_search_v2(&body, search_type, container))
    }

    pub async fn search_suggestions(&self, query: &str) -> MhResult<Vec<String>> {
        let vars = json!({
            "query": query,
            "limit": 30,
            "numberOfTopResults": 30,
            "offset": 0,
            "includeAuthors": false,
            "includeAlbumPreReleases": false,
            "includeEpisodeContentRatingsV2": true,
        });
        let body = self.pathfinder("searchSuggestions", vars).await?;
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        collect_suggestion_strings(&body["data"], &mut out, &mut seen);
        out.truncate(crate::services::common::limits::SUGGESTIONS_LIMIT);
        Ok(out)
    }
}

fn sv_uri_id(uri: &str) -> String {
    uri_to_id(uri).unwrap_or_default()
}

fn uri_to_url(uri: &str) -> Option<String> {
    let mut parts = uri.split(':');
    if parts.next()? != "spotify" {
        return None;
    }
    let kind = parts.next()?;
    let id = parts.next()?;
    Some(format!("https://open.spotify.com/{kind}/{id}"))
}

fn first_cover_url(cover: &Value) -> Option<String> {
    cover["sources"]
        .as_array()
        .and_then(|a| a.last())
        .and_then(|s| s["url"].as_str())
        .map(str::to_string)
}

fn artist_obj(name: &Value, uri: &str) -> Value {
    json!({ "name": name, "id": sv_uri_id(uri), "uri": uri })
}

fn track_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let artists: Vec<Value> = data["artists"]["items"]
        .as_array()
        .or_else(|| data["firstArtist"]["items"].as_array())
        .map(|arr| {
            arr.iter()
                .map(|a| artist_obj(&a["profile"]["name"], a["uri"].as_str().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();
    let images: Vec<Value> = first_cover_url(&data["albumOfTrack"]["coverArt"])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    json!({
        "name": data["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "artists": artists,
        "album": {
            "name": data["albumOfTrack"]["name"],
            "uri": data["albumOfTrack"]["uri"],
            "images": images,
        },
        "duration_ms": data["duration"]["totalMilliseconds"],
        "explicit": data["contentRating"]["label"].as_str() == Some("EXPLICIT"),
    })
}

fn album_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let images: Vec<Value> = first_cover_url(&data["coverArt"])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    let artists: Vec<Value> = data["artists"]["items"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|a| artist_obj(&a["profile"]["name"], a["uri"].as_str().unwrap_or("")))
                .collect()
        })
        .unwrap_or_default();
    json!({
        "name": data["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "artists": artists,
        "images": images,
    })
}

fn artist_result_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let images: Vec<Value> = first_cover_url(&data["visuals"]["avatarImage"])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    json!({
        "name": data["profile"]["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "images": images,
    })
}

fn playlist_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let images: Vec<Value> = first_cover_url(&data["images"]["items"][0])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    json!({
        "name": data["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "owner": { "display_name": data["ownerV2"]["data"]["name"] },
        "images": images,
    })
}

fn show_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let images: Vec<Value> = first_cover_url(&data["coverArt"])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    json!({
        "name": data["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "publisher": data["publisher"]["name"],
        "media_type": data["mediaType"],
        "images": images,
    })
}

fn episode_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let images: Vec<Value> = first_cover_url(&data["coverArt"])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    json!({
        "name": data["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "duration_ms": data["duration"]["totalMilliseconds"],
        "images": images,
    })
}

fn audiobook_obj(data: &Value) -> Value {
    let uri = data["uri"].as_str().unwrap_or("");
    let images: Vec<Value> = first_cover_url(&data["coverArt"])
        .map(|u| vec![json!({ "url": u })])
        .unwrap_or_default();
    let authors: Vec<Value> = data["authors"]
        .as_array()
        .map(|arr| arr.iter().map(|a| json!({ "name": a["name"] })).collect())
        .unwrap_or_default();
    json!({
        "name": data["name"],
        "id": sv_uri_id(uri),
        "uri": uri,
        "external_urls": { "spotify": uri_to_url(uri) },
        "authors": authors,
        "images": images,
    })
}

fn transform_search_v2(body: &Value, search_type: &str, container: &str) -> Value {
    let items = body["data"]["searchV2"][container]["items"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut results = Vec::new();
    for it in &items {
        let node = if it.get("item").is_some() {
            &it["item"]
        } else {
            it
        };
        let data = &node["data"];
        let obj = match node["__typename"].as_str() {
            Some("TrackResponseWrapper") => track_obj(data),
            Some("AlbumResponseWrapper") => album_obj(data),
            Some("ArtistResponseWrapper") => artist_result_obj(data),
            Some("PlaylistResponseWrapper") => playlist_obj(data),
            Some("PodcastResponseWrapper") => show_obj(data),
            Some("EpisodeResponseWrapper") => episode_obj(data),
            Some("AudiobookResponseWrapper") => audiobook_obj(data),
            _ => continue,
        };
        results.push(obj);
    }
    let key = match search_type {
        "album" => "albums",
        "artist" => "artists",
        "playlist" => "playlists",
        "podcast" | "show" => "shows",
        "episode" => "episodes",
        "audiobook" => "audiobooks",
        _ => "tracks",
    };
    let mut out = serde_json::Map::new();
    out.insert(key.into(), json!({ "items": results }));
    Value::Object(out)
}

fn collect_suggestion_strings(
    v: &Value,
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match v {
        Value::Object(m) => {
            let is_autocomplete =
                m.get("__typename").and_then(|v| v.as_str()) == Some("SearchAutoCompleteEntity");
            if is_autocomplete {
                if let Some(s) = m
                    .get("data")
                    .and_then(|d| d.get("text"))
                    .and_then(|x| x.as_str())
                {
                    let t = s.trim();
                    if !t.is_empty() && seen.insert(t.to_lowercase()) {
                        out.push(t.to_string());
                    }
                }
            }
            for child in m.values() {
                collect_suggestion_strings(child, out, seen);
            }
        }
        Value::Array(a) => {
            for child in a {
                collect_suggestion_strings(child, out, seen);
            }
        }
        _ => {}
    }
}

fn classify_shelf(id: &str, title: &str) -> RecommendationCategory {
    let lt = title.trim().to_ascii_lowercase();
    let li = id.to_ascii_lowercase();
    if lt.is_empty() || li.contains("shortcut") {
        return RecommendationCategory::Hero;
    }
    if lt.contains("recently played") || lt.contains("jump back in") {
        return RecommendationCategory::RecentlyPlayed;
    }
    if lt.contains("daily mix")
        || lt.contains("made for you")
        || lt.contains("discover weekly")
        || lt.contains("release radar")
        || lt.ends_with(" mix")
        || li.contains("daily-mix")
        || li.contains("made-for-x")
    {
        return RecommendationCategory::DailyMix;
    }
    if lt.contains("station") || lt.contains("radio") {
        return RecommendationCategory::Stations;
    }
    if lt.contains("new release") || lt.contains("new music") || lt.contains("just dropped") {
        return RecommendationCategory::NewReleases;
    }
    if lt.contains("top ") || lt.contains("charts") {
        return RecommendationCategory::Charts;
    }
    if lt.contains("more like")
        || lt.contains("for fans of")
        || lt.contains("recommended")
        || lt.contains("based on your")
        || lt.contains("for you")
        || lt.contains("discover")
    {
        return RecommendationCategory::Discovery;
    }
    if lt.contains("editor") || lt.contains("popular") {
        return RecommendationCategory::Editorial;
    }
    RecommendationCategory::Other
}

fn home_shelves_from_body(body: &Value) -> Vec<RecommendationShelf> {
    let sections_paths = [
        "/data/home/sections/items",
        "/data/home/sectionContainer/sections/items",
        "/data/browse/sections/items",
        "/data/browseStart/sections/items",
        "/data/homePage/sections/items",
    ];
    let sections = sections_paths
        .iter()
        .find_map(|p| body.pointer(p).and_then(|v| v.as_array()).cloned())
        .unwrap_or_default();

    let mut shelves = Vec::new();
    for sec in sections {
        let title = sec
            .pointer("/data/title/transformedLabel")
            .or_else(|| sec.pointer("/data/title/text"))
            .or_else(|| sec.pointer("/title/transformedLabel"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let items_v = sec
            .pointer("/sectionItems/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut items = Vec::new();
        for it in items_v {
            let content = it
                .pointer("/content/data")
                .cloned()
                .or_else(|| it.get("content").cloned())
                .unwrap_or(Value::Null);
            let typename = str_at(&content, &["__typename"]).unwrap_or("");
            let uri = str_at(&content, &["uri"]).unwrap_or("");
            let tn_lower = typename.to_ascii_lowercase();
            let mapped = if tn_lower.contains("episode") || uri.starts_with("spotify:episode:") {
                map_episode_node(&content)
            } else if tn_lower.contains("podcast")
                || tn_lower.contains("show")
                || uri.starts_with("spotify:show:")
            {
                map_show_node(&content)
            } else {
                match typename {
                    "Album" | "AlbumResponseWrapper" => {
                        map_album_node(&content).map(RecommendationItem::Album)
                    }
                    "Playlist"
                    | "PlaylistResponseWrapper"
                    | "PseudoPlaylist"
                    | "PinnedPlaylist" => {
                        map_playlist_node(&content).map(RecommendationItem::Playlist)
                    }
                    "Artist" | "ArtistResponseWrapper" => {
                        map_artist_node(&content).map(RecommendationItem::Artist)
                    }
                    "Track" | "TrackResponseWrapper" => {
                        map_track_node(&content).map(RecommendationItem::Track)
                    }
                    _ => {
                        if !uri.is_empty() {
                            map_playlist_node(&content).map(RecommendationItem::Playlist)
                        } else {
                            None
                        }
                    }
                }
            };
            if let Some(m) = mapped {
                items.push(m);
            }
        }
        if items.is_empty() {
            continue;
        }
        let id = str_at(&sec, &["uri"]).unwrap_or("").to_string();
        let category = classify_shelf(&id, &title);
        shelves.push(RecommendationShelf::new(id, title, category, items));
    }
    crate::services::common::library::trim_feed_tail(
        crate::services::common::library::merge_duplicate_shelves(shelves),
        crate::services::common::library::FeedTrim::default(),
    )
}

fn uri_to_id(uri: &str) -> Option<String> {
    uri.rsplit(':').next().map(str::to_string)
}

fn first_image(v: &Value, paths: &[&str]) -> Option<String> {
    for p in paths {
        if let Some(arr) = v.pointer(p).and_then(|x| x.as_array()) {
            if let Some(url) = arr
                .first()
                .and_then(|i| i.get("url"))
                .and_then(|u| u.as_str())
            {
                return Some(url.to_string());
            }
        }
        if let Some(url) = v.pointer(&format!("{p}/url")).and_then(|x| x.as_str()) {
            return Some(url.to_string());
        }
    }
    None
}

fn find_first_image_recursive(v: &Value, max_depth: u32) -> Option<String> {
    if max_depth == 0 {
        return None;
    }
    match v {
        Value::Object(map) => {
            if let Some(arr) = map.get("sources").and_then(|x| x.as_array()) {
                if let Some(u) = arr
                    .first()
                    .and_then(|i| i.get("url"))
                    .and_then(|u| u.as_str())
                {
                    return Some(u.to_string());
                }
            }
            if let Some(arr) = map.get("thumbnails").and_then(|x| x.as_array()) {
                if let Some(u) = arr
                    .first()
                    .and_then(|i| i.get("url"))
                    .and_then(|u| u.as_str())
                {
                    return Some(u.to_string());
                }
            }
            for (k, child) in map.iter() {
                if matches!(
                    k.as_str(),
                    "owner"
                        | "tracks"
                        | "tracksV2"
                        | "content"
                        | "artists"
                        | "albumOfTrack"
                        | "discography"
                ) {
                    continue;
                }
                if let Some(u) = find_first_image_recursive(child, max_depth - 1) {
                    return Some(u);
                }
            }
            None
        }
        Value::Array(arr) => {
            for child in arr.iter().take(8) {
                if let Some(u) = find_first_image_recursive(child, max_depth - 1) {
                    return Some(u);
                }
            }
            None
        }
        _ => None,
    }
}

const SPOTIFY_IMAGE_PATHS: &[&str] = &[
    "/coverArt/sources",
    "/images/items/0/sources",
    "/image/sources",
    "/data/image/sources",
    "/playlistV2/images/items/0/sources",
    "/visuals/avatarImage/sources",
    "/visuals/headerImage/sources",
    "/albumOfTrack/coverArt/sources",
    "/attributes/formatListAttributes/pictureUrl",
    "/backgroundImage/sources",
    "/avatarImage/sources",
];

fn any_image(v: &Value) -> Option<String> {
    first_image(v, SPOTIFY_IMAGE_PATHS).or_else(|| find_first_image_recursive(v, 4))
}

/// Spotify identifies everything by URI (`spotify:album:…`); the app keys on the
/// bare id, so every id read is a URI read followed by a split.
fn uri_id_at(v: &Value, paths: &[&str]) -> Option<String> {
    str_at(v, paths).and_then(uri_to_id)
}

fn spotify_cover(v: &Value) -> Option<String> {
    any_image(v).map(|u| cover_id(ServicePlatform::Spotify, &u))
}

fn map_feed_item(v: &Value) -> Option<ActivityFeedItem> {
    let data = v.pointer("/content/data")?;
    let (kind, subtitle) = match str_at(data, &["__typename"]) {
        Some("Episode") => ("episode", string_at(data, &["/podcastV2/data/name"])),
        _ => ("album", string_at(data, &["/artists/items/0/profile/name"])),
    };
    Some(ActivityFeedItem {
        id: str_at(v, &["id"])
            .map(str::to_string)
            .or_else(|| str_at(data, &["uri"]).map(str::to_string))?,
        kind: kind.into(),
        title: string_at(data, &["name"]),
        subtitle,
        cover_id: spotify_cover(data),
        service_id: uri_id_at(data, &["uri"]),
        occurred_at: str_at(v, &["/timestamp/isoString"])
            .or_else(|| str_at(data, &["/date/isoString", "/releaseDate/isoString"]))
            .map(str::to_string),
        seen: str_at(v, &["/state/state"]) != Some("NEW"),
    })
}

fn map_album_node(v: &Value) -> Option<LibraryAlbumDto> {
    Some(LibraryAlbumDto {
        album_key: uri_id_at(v, &["uri"])?,
        title: string_at(v, &["name"]),
        artist: string_at(
            v,
            &["/artists/items/0/profile/name", "/artists/items/0/name"],
        ),
        year: i64_at(v, &["/date/year"])
            .map(|y| y.to_string())
            .or_else(|| year4_at(v, &["/date/isoString"])),
        cover_id: spotify_cover(v),
        track_count: i64_at(v, &["/tracksV2/totalCount"]).unwrap_or(0),
        artist_id: uri_id_at(v, &["/artists/items/0/uri"]),
        ..Default::default()
    })
}

fn map_track_node(v: &Value) -> Option<LibraryTrackDto> {
    let id = uri_id_at(v, &["uri"])?;
    let artist = string_at(v, &["/artists/items/0/profile/name"]);
    Some(LibraryTrackDto {
        path: format!("https://open.spotify.com/track/{id}"),
        title: Some(string_at(v, &["name"])),
        artist: Some(artist.clone()),
        album: nonempty_at(v, &["/albumOfTrack/name"]),
        album_key: uri_id_at(
            v,
            &["/albumOfTrack/uri", "/album/uri", "/albumOfTrack/data/uri"],
        ),
        duration_secs: f64_at(v, &["/duration/totalMilliseconds"]).map(|m| m / 1000.0),
        track_no: u32_at(v, &["/trackNumber"]),
        disc_no: u32_at(v, &["/discNumber"]),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: spotify_cover(v),
        primary_artist: Some(artist),
        artist_id: uri_id_at(
            v,
            &[
                "/artists/items/0/uri",
                "/artists/items/0/data/uri",
                "/firstArtist/items/0/uri",
                "/artist/uri",
                "/artists/0/uri",
            ],
        ),
        ..Default::default()
    })
}

fn map_artist_node(v: &Value) -> Option<LibraryArtistDto> {
    Some(LibraryArtistDto {
        key: uri_id_at(v, &["uri"])?,
        display: string_at(v, &["/profile/name", "name"]),
        album_count: 0,
        track_count: 0,
        cover_id: spotify_cover(v),
    })
}

fn map_playlist_node(v: &Value) -> Option<LibraryPlaylistDto> {
    let id = uri_id_at(v, &["uri"])?;
    Some(LibraryPlaylistDto {
        id: stable_hash_i64(&id),
        name: string_at(v, &["name"]),
        created_at: 0,
        updated_at: 0,
        track_count: i64_at(v, &["/content/totalCount"]).unwrap_or(0),
        cover_ids: spotify_cover(v).map(|c| vec![c]).unwrap_or_default(),
        service_id: Some(id),
        description: nonempty_at(v, &["description"]),
        owner: nonempty_at(
            v,
            &["/ownerV2/data/name", "/owner/name", "/owner/display_name"],
        ),
    })
}

fn find_show_name_recursive(v: &Value, depth: u8) -> Option<String> {
    if depth == 0 {
        return None;
    }
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                let kl = k.to_ascii_lowercase();
                if kl == "podcast" || kl == "podcastv2" || kl == "show" {
                    if let Some(name) = val
                        .pointer("/data/name")
                        .or_else(|| val.get("name"))
                        .and_then(|x| x.as_str())
                    {
                        if !name.is_empty() {
                            return Some(name.to_string());
                        }
                    }
                }
            }
            for (_, val) in map.iter().take(20) {
                if let Some(n) = find_show_name_recursive(val, depth - 1) {
                    return Some(n);
                }
            }
            None
        }
        Value::Array(arr) => {
            for item in arr.iter().take(8) {
                if let Some(n) = find_show_name_recursive(item, depth - 1) {
                    return Some(n);
                }
            }
            None
        }
        _ => None,
    }
}

fn map_episode_node(v: &Value) -> Option<RecommendationItem> {
    Some(RecommendationItem::Episode {
        id: str_at(v, &["uri"])?.to_string(),
        title: string_at(v, &["name"]),
        show_name: nonempty_at(
            v,
            &[
                "/podcastV2/data/name",
                "/podcast/data/name",
                "/show/data/name",
                "/podcast/name",
                "/show/name",
                "/podcastV2/name",
                "/publisher/name",
                "/publisher",
            ],
        )
        .or_else(|| find_show_name_recursive(v, 4)),
        cover_id: spotify_cover(v),
        duration_ms: i64_at(v, &["/duration/totalMilliseconds"]).map(|m| m as u64),
    })
}

fn map_show_node(v: &Value) -> Option<RecommendationItem> {
    Some(RecommendationItem::Episode {
        id: str_at(v, &["uri"])?.to_string(),
        title: string_at(v, &["name"]),
        show_name: nonempty_at(v, &["/publisher/name", "publisher"]),
        cover_id: spotify_cover(v),
        duration_ms: None,
    })
}

fn library_variables(filter: Option<&str>, page: Page) -> Value {
    let mut filters: Vec<String> = Vec::new();
    if let Some(f) = filter {
        filters.push(f.to_string());
    }
    json!({
        "filters": filters,
        "order": null,
        "textFilter": "",
        "features": ["LIKED_SONGS","YOUR_EPISODES"],
        "limit": page.limit as i64,
        "offset": page.offset as i64,
        "flatten": false,
        "expandedFolders": [],
        "folderUri": null,
        "includeFoldersWhenFlattening": true,
    })
}

fn library_items(body: &Value) -> &[Value] {
    body.pointer("/data/me/libraryV3/items")
        .and_then(|v| v.as_array())
        .map(|v| v.as_slice())
        .unwrap_or(&[])
}

fn library_total(body: &Value) -> Option<u64> {
    body.pointer("/data/me/libraryV3/totalCount")
        .and_then(|v| v.as_u64())
}

fn wrapped_data<'a>(item: &'a Value, expected_typename: &str) -> Option<&'a Value> {
    let inner = item.get("item")?;
    let data = inner.get("data")?;
    let tn = str_at(data, &["__typename"]).unwrap_or("");
    if tn == expected_typename {
        Some(data)
    } else {
        None
    }
}

#[async_trait]
impl ServiceLibrary for SpotifyLibrary {
    fn as_mutations(self: Arc<Self>) -> Option<Arc<dyn ServiceLibraryMutations>> {
        Some(self)
    }

    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Spotify
    }

    async fn report_playback(
        &self,
        service_track_id: &str,
        duration_secs: u64,
        context_uri: Option<&str>,
        track_index: Option<u64>,
    ) -> MhResult<()> {
        let (history, telemetry) = match &self.settings {
            Some(s) => {
                let g = s.read().await;
                (
                    g.sync_playback_history(ServicePlatform::Spotify),
                    g.telemetry_enabled(ServicePlatform::Spotify),
                )
            }
            None => (false, false),
        };
        if !history && !telemetry {
            return Ok(());
        }
        let (access_token, client_token) = match self.auth().await {
            Ok(v) => v,
            Err(e) => {
                self.log_line(
                    "error",
                    format!("Spotify playback report skipped, auth failed: {e}"),
                );
                return Ok(());
            }
        };
        let client = self.client.clone();
        let track_id = service_track_id.to_string();
        let context = crate::services::spotify::telemetry::SpotifyPlayContext {
            context_uri: context_uri.map(str::to_string),
            track_index,
        };
        let emitter = self.emitter.clone();
        tokio::spawn(async move {
            let result = crate::services::spotify::telemetry::report_playback(
                client,
                access_token,
                client_token,
                track_id.clone(),
                duration_secs,
                context,
                history,
                telemetry,
            )
            .await;
            if let Some(em) = emitter {
                let (level, message) = match result {
                    Ok(summary) => (
                        "info",
                        format!("Spotify playback reported for track {track_id} [{summary}]"),
                    ),
                    Err(e) => (
                        "error",
                        format!("Spotify playback report failed for track {track_id}: {e}"),
                    ),
                };
                em.emit_log(&crate::ipc_contract::BackendLogEvent::new(
                    level, "spotify", "Spotify", message,
                ));
            }
        });
        Ok(())
    }
    async fn canvas_url(&self, service_track_id: &str) -> MhResult<Option<String>> {
        let (access_token, _) = self.auth().await?;
        crate::services::spotify::canvas::canvas_url(&self.client, &access_token, service_track_id)
            .await
    }
    async fn set_playlist_cover(&self, playlist_id: &str, jpeg_base64: &str) -> MhResult<()> {
        let (token, client_token) = self.auth().await?;
        let url = format!(
            "https://spclient.wg.spotify.com/playlist/v2/playlist/{playlist_id}/register-image"
        );
        let mut req = self
            .client
            .post(&url)
            .bearer_auth(&token)
            .header("accept", "application/json")
            .header("content-type", "image/jpeg")
            .header("app-platform", "WebPlayer")
            .header("spotify-app-version", "1.2.92.18.gfdac4f00")
            .header("origin", "https://open.spotify.com")
            .header("referer", "https://open.spotify.com/");
        if let Some(ct) = &client_token {
            req = req.header("client-token", ct.clone());
        }
        let resp = req
            .body(jpeg_base64.to_string())
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let file_id = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(MhError::Other(format!(
                "Spotify register-image returned {}: {file_id}",
                status.as_u16()
            )));
        }
        let file_id = file_id.trim().trim_matches('"').to_string();
        let mut values = serde_json::Map::new();
        values.insert("pictureFileId".into(), Value::String(file_id));
        let body = json!({
            "deltas": [{
                "ops": [{
                    "kind": "UPDATE_LIST_ATTRIBUTES",
                    "updateListAttributes": {
                        "newAttributes": { "values": Value::Object(values) }
                    }
                }],
                "info": { "source": { "client": "WEBPLAYER" } }
            }]
        });
        let changes =
            format!("https://spclient.wg.spotify.com/playlist/v2/playlist/{playlist_id}/changes");
        self.spclient_post(&changes, &body).await?;
        Ok(())
    }
    async fn podcast_transcript(&self, episode_id: &str) -> MhResult<Value> {
        let (token, client_token) = self.auth().await?;
        let url = format!(
            "https://spclient.wg.spotify.com/transcript-read-along/v2/episode/{episode_id}"
        );
        let mut req = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .header("accept", "application/json")
            .header("app-platform", "WebPlayer")
            .header("origin", "https://open.spotify.com")
            .header("referer", "https://open.spotify.com/");
        if let Some(ct) = &client_token {
            req = req.header("client-token", ct.clone());
        }
        let resp = req.send().await.map_err(MhError::Network)?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(Value::Null);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(MhError::Other(format!(
                "Spotify transcript {status}: {body}"
            )));
        }
        let body: Value = resp.json().await.map_err(MhError::Network)?;
        let mut lines = Vec::new();
        let mut speaker: Option<String> = None;
        if let Some(sections) = body.get("section").and_then(|v| v.as_array()) {
            for sec in sections {
                let start_ms = sec.get("startMs").and_then(|v| v.as_u64()).unwrap_or(0);
                if let Some(title) = str_at(sec, &["/title/title"]) {
                    speaker = Some(title.to_string());
                    continue;
                }
                let text = sec
                    .pointer("/text/sentence/text")
                    .or_else(|| sec.pointer("/fallback/sentence/text"))
                    .or_else(|| sec.pointer("/musicClosedCaption/text"))
                    .and_then(|v| v.as_str());
                if let Some(text) = text {
                    if text.trim().is_empty() {
                        continue;
                    }
                    lines.push(json!({
                        "startMs": start_ms,
                        "speaker": speaker.clone(),
                        "text": text,
                    }));
                }
            }
        }
        if lines.is_empty() {
            return Ok(Value::Null);
        }
        Ok(json!({
            "episodeName": body.get("episodeName"),
            "showName": body.get("showName"),
            "language": body.get("language"),
            "lines": lines,
        }))
    }
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::audio_only()
            .with_activity_feed()
            .with_mutations(self.mutation_capabilities())
    }

    async fn activity_feed(&self, limit: u32) -> MhResult<Value> {
        let body = self
            .pathfinder(
                "queryWhatsNewFeed",
                json!({ "limit": limit, "offset": 0, "onlyUnPlayedItems": false }),
            )
            .await?;
        let rows = body
            .pointer("/data/whatsNewFeedItems/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(activity_feed_page(
            rows.iter().filter_map(map_feed_item).collect(),
        ))
    }

    async fn albums(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        let body = self
            .pathfinder("libraryV3", library_variables(Some("Albums"), page))
            .await?;
        let total = library_total(&body);
        let items: Vec<LibraryAlbumDto> = library_items(&body)
            .iter()
            .filter_map(|w| wrapped_data(w, "Album").and_then(map_album_node))
            .collect();
        Ok(ServiceLibraryPage::of(items, total))
    }

    async fn tracks(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        let body = self
            .pathfinder(
                "fetchLibraryTracks",
                json!({
                    "offset": page.offset as i64,
                    "limit": page.limit.max(1) as i64,
                }),
            )
            .await?;
        let items_arr = body
            .pointer("/data/me/library/tracks/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let items: Vec<LibraryTrackDto> = items_arr
            .iter()
            .filter_map(|w| {
                let uri = w
                    .pointer("/track/_uri")
                    .or_else(|| w.pointer("/track/data/uri"))
                    .or_else(|| w.pointer("/track/uri"))
                    .and_then(|v| v.as_str())?
                    .to_string();
                let data = w.pointer("/track/data").unwrap_or(w);
                let mut merged = data.clone();
                if let Some(obj) = merged.as_object_mut() {
                    obj.insert("uri".into(), Value::String(uri));
                }
                map_track_node(&merged)
            })
            .collect();
        let total = body
            .pointer("/data/me/library/tracks/totalCount")
            .and_then(|v| v.as_u64());
        Ok(ServiceLibraryPage::of(items, total))
    }

    async fn artists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        let body = self
            .pathfinder("libraryV3", library_variables(Some("Artists"), page))
            .await?;
        let total = library_total(&body);
        let items: Vec<LibraryArtistDto> = library_items(&body)
            .iter()
            .filter_map(|w| wrapped_data(w, "Artist").and_then(map_artist_node))
            .collect();
        Ok(ServiceLibraryPage::of(items, total))
    }

    async fn playlists(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        let body = self
            .pathfinder("libraryV3", library_variables(Some("Playlists"), page))
            .await?;
        let total = library_total(&body);
        let items: Vec<LibraryPlaylistDto> = library_items(&body)
            .iter()
            .filter_map(|w| wrapped_data(w, "Playlist").and_then(map_playlist_node))
            .collect();
        Ok(ServiceLibraryPage::of(items, total))
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        let home_eui = json!("INTEGRATION_WEB_PLAYER");
        let candidates: Vec<(&str, Value)> = vec![
            (
                "home",
                json!({
                    "timeZone": "UTC",
                    "sp_t": "",
                    "country": null,
                    "facet": "",
                    "sectionItemsLimit": 10,
                    "sectionItemsOffset": 0,
                    "homeEndUserIntegration": home_eui.clone(),
                    "includeEpisodeContentRatingsV2": true,
                }),
            ),
            (
                "homeSections",
                json!({ "homeEndUserIntegration": home_eui.clone() }),
            ),
            (
                "browseAll",
                json!({
                    "pagePagination": { "offset": 0, "limit": 30 },
                    "sectionPagination": { "offset": 0, "limit": 30 },
                    "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER",
                }),
            ),
        ];
        let mut last_err: Option<MhError> = None;
        let mut body_opt: Option<Value> = None;
        for (op, vars) in candidates {
            if self.known_hash(op).await.is_none() && self.hash_for(op).await.is_err() {
                continue;
            }
            match self.pathfinder(op, vars).await {
                Ok(v) => {
                    body_opt = Some(v);
                    break;
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
        let body = match body_opt {
            Some(b) => b,
            None => {
                return Err(last_err.unwrap_or_else(|| {
                    MhError::Other("Spotify home: no known pathfinder operation succeeded".into())
                }))
            }
        };

        let shelves = home_shelves_from_body(&body);

        let mut merged: Vec<RecommendationShelf> = Vec::with_capacity(shelves.len());
        let mut episode_bucket: Vec<RecommendationItem> = Vec::new();
        let flush = |bucket: &mut Vec<RecommendationItem>, out: &mut Vec<RecommendationShelf>| {
            if !bucket.is_empty() {
                out.push(RecommendationShelf::new(
                    "spotify:home:recommended-episodes".to_string(),
                    "Recommended Episodes".to_string(),
                    RecommendationCategory::Other,
                    std::mem::take(bucket),
                ));
            }
        };
        for sh in shelves {
            let is_singleton_episode = sh.title.trim().is_empty()
                && sh.items.len() == 1
                && matches!(sh.items[0], RecommendationItem::Episode { .. });
            if is_singleton_episode {
                episode_bucket.extend(sh.items);
            } else {
                flush(&mut episode_bucket, &mut merged);
                merged.push(sh);
            }
        }
        flush(&mut episode_bucket, &mut merged);

        Ok(RecommendationsPage { shelves: merged })
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let body = self
            .pathfinder(
                "getAlbum",
                json!({
                    "uri": format!("spotify:album:{id}"),
                    "locale": "",
                    "offset": 0,
                    "limit": 200,
                }),
            )
            .await?;
        let node = body
            .pointer("/data/albumUnion")
            .or_else(|| body.pointer("/data/album"))
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Spotify album {id}")))?;
        let album = map_album_node(&node)
            .ok_or_else(|| MhError::NotFound(format!("Spotify album {id}")))?;
        let map_items = |arr: &[Value]| -> Vec<LibraryTrackDto> {
            arr.iter()
                .filter_map(|t| {
                    let track = t.pointer("/track").unwrap_or(t);
                    map_track_node(track)
                })
                .collect()
        };
        let items_at = |n: &Value| -> Vec<Value> {
            n.pointer("/tracksV2/items")
                .or_else(|| n.pointer("/tracks/items"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default()
        };
        let first = items_at(&node);
        let total = node
            .pointer("/tracksV2/totalCount")
            .or_else(|| node.pointer("/tracks/totalCount"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let mut tracks = map_items(&first);
        let mut offset = first.len() as i64;
        let mut guard = 0;
        while offset < total && !first.is_empty() {
            guard += 1;
            if guard > 100 {
                break;
            }
            let page = self
                .pathfinder(
                    "getAlbum",
                    json!({
                        "uri": format!("spotify:album:{id}"),
                        "locale": "",
                        "offset": offset,
                        "limit": 200,
                    }),
                )
                .await?;
            let page_node = page
                .pointer("/data/albumUnion")
                .or_else(|| page.pointer("/data/album"))
                .cloned()
                .unwrap_or(Value::Null);
            let items = items_at(&page_node);
            if items.is_empty() {
                break;
            }
            offset += items.len() as i64;
            tracks.extend(map_items(&items));
        }
        Ok(LibraryAlbumDetail { album, tracks })
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let body = self
            .pathfinder(
                "queryArtistOverview",
                json!({
                    "uri": format!("spotify:artist:{id}"),
                    "locale": "",
                    "includePrerelease": false,
                }),
            )
            .await?;
        let node = body
            .pointer("/data/artistUnion")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Spotify artist {id}")))?;
        let display = str_at(&node, &["/profile/name"]).unwrap_or("").to_string();
        let albums_arr = node
            .pointer("/discography/popularReleasesAlbums/items")
            .or_else(|| node.pointer("/discography/albums/items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let albums: Vec<LibraryAlbumDto> = albums_arr.iter().filter_map(map_album_node).collect();
        let tracks_arr = node
            .pointer("/discography/topTracks/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = tracks_arr
            .iter()
            .filter_map(|t| {
                let track = t.pointer("/track").unwrap_or(t);
                map_track_node(track)
            })
            .collect();
        Ok(LibraryArtistDetail {
            key: id.into(),
            display,
            albums,
            tracks,
        })
    }

    async fn artist_page(&self, id: &str) -> MhResult<Value> {
        let body = self
            .pathfinder(
                "queryArtistOverview",
                json!({
                    "uri": format!("spotify:artist:{id}"),
                    "locale": "",
                    "includePrerelease": false,
                }),
            )
            .await?;
        let node = body
            .pointer("/data/artistUnion")
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Spotify artist {id}")))?;

        let mut shelves: Vec<RecommendationShelf> = Vec::new();
        let mut push =
            |title: &str, cat: RecommendationCategory, items: Vec<RecommendationItem>| {
                if !items.is_empty() {
                    shelves.push(RecommendationShelf::new(
                        format!("artist-{id}-{}", title.replace(' ', "-").to_lowercase()),
                        title,
                        cat,
                        items,
                    ));
                }
            };

        let top_tracks: Vec<RecommendationItem> = node
            .pointer("/discography/topTracks/items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| map_track_node(t.pointer("/track").unwrap_or(t)))
                    .map(RecommendationItem::Track)
                    .collect()
            })
            .unwrap_or_default();
        push("Popular", RecommendationCategory::Charts, top_tracks);

        let release_items = |ptr: &str| -> Vec<RecommendationItem> {
            node.pointer(ptr)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|it| {
                            it.pointer("/releases/items/0")
                                .and_then(map_album_node)
                                .map(RecommendationItem::Album)
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        push(
            "Albums",
            RecommendationCategory::NewReleases,
            release_items("/discography/albums/items"),
        );
        push(
            "Singles & EPs",
            RecommendationCategory::NewReleases,
            release_items("/discography/singles/items"),
        );
        push(
            "Compilations",
            RecommendationCategory::NewReleases,
            release_items("/discography/compilations/items"),
        );

        let appears_on: Vec<RecommendationItem> = node
            .pointer("/relatedContent/appearsOn/items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| it.pointer("/releases/items/0").and_then(map_album_node))
                    .map(RecommendationItem::Album)
                    .collect()
            })
            .unwrap_or_default();
        push("Appears On", RecommendationCategory::Editorial, appears_on);

        let related: Vec<RecommendationItem> = node
            .pointer("/relatedContent/relatedArtists/items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(map_artist_node)
                    .map(RecommendationItem::Artist)
                    .collect()
            })
            .unwrap_or_default();
        push("Fans Also Like", RecommendationCategory::Discovery, related);

        let discovered: Vec<RecommendationItem> = node
            .pointer("/relatedContent/discoveredOnV2/items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|it| map_playlist_node(it.pointer("/data").unwrap_or(it)))
                    .map(RecommendationItem::Playlist)
                    .collect()
            })
            .unwrap_or_default();
        push(
            "Discovered On",
            RecommendationCategory::Discovery,
            discovered,
        );

        Ok(json!({ "shelves": shelves, "raw": Value::Null }))
    }

    async fn album_page(&self, id: &str) -> MhResult<Value> {
        let body = self
            .pathfinder(
                "getAlbum",
                json!({
                    "uri": format!("spotify:album:{id}"),
                    "locale": "",
                    "offset": 0,
                    "limit": 200,
                }),
            )
            .await?;
        let node = body
            .pointer("/data/albumUnion")
            .or_else(|| body.pointer("/data/album"))
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Spotify album {id}")))?;

        let mut shelves: Vec<RecommendationShelf> = Vec::new();

        let mut more: Vec<RecommendationItem> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        seen.insert(id.to_string());
        if let Some(groups) = node
            .pointer("/moreAlbumsByArtist/items")
            .and_then(|v| v.as_array())
        {
            for g in groups {
                if let Some(rel) = g
                    .pointer("/discography/popularReleasesAlbums/items")
                    .and_then(|v| v.as_array())
                {
                    for it in rel {
                        let album = map_album_node(it)
                            .or_else(|| it.pointer("/releases/items/0").and_then(map_album_node));
                        if let Some(album) = album {
                            if seen.insert(album.album_key.clone()) {
                                more.push(RecommendationItem::Album(album));
                            }
                        }
                    }
                }
            }
        }
        if !more.is_empty() {
            shelves.push(RecommendationShelf::new(
                format!("album-{id}-more-by-artist"),
                "More by this artist",
                RecommendationCategory::NewReleases,
                more,
            ));
        }

        Ok(json!({ "shelves": shelves, "raw": Value::Null }))
    }

    async fn explore(&self) -> MhResult<RecommendationsPage> {
        let body = self
            .pathfinder(
                "browseAll",
                json!({
                    "pagePagination": { "offset": 0, "limit": 30 },
                    "sectionPagination": { "offset": 0, "limit": 30 },
                    "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER",
                }),
            )
            .await?;
        let sections = body
            .pointer("/data/browseStart/sections/items")
            .or_else(|| body.pointer("/data/browse/sections/items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut shelves = Vec::new();
        for sec in &sections {
            let shelf_title = sec
                .pointer("/data/title/transformedLabel")
                .or_else(|| sec.pointer("/data/title/text"))
                .and_then(|x| x.as_str())
                .unwrap_or("Browse")
                .to_string();
            let mut pills = Vec::new();
            let cards = sec
                .pointer("/sectionItems/items")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for card in &cards {
                let uri = str_at(card, &["uri"]).unwrap_or("");
                let Some(sid) = uri.strip_prefix("spotify:page:") else {
                    continue;
                };
                let rep = card.pointer("/content/data/data/cardRepresentation");
                let title = rep
                    .and_then(|r| r.pointer("/title/transformedLabel"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                if title.is_empty() {
                    continue;
                }
                let icon = rep
                    .and_then(|r| r.pointer("/artwork/sources/0/url"))
                    .and_then(|x| x.as_str())
                    .map(|u| cover_id(ServicePlatform::Spotify, u));
                pills.push(RecommendationItem::PageLink {
                    api_path: format!("page::{sid}::{title}"),
                    title,
                    icon,
                });
            }
            if !pills.is_empty() {
                let header = if shelf_title.eq_ignore_ascii_case("browse all") {
                    String::new()
                } else {
                    shelf_title
                };
                shelves.push(RecommendationShelf::new(
                    format!("explore-{}", header.replace(' ', "-").to_lowercase()),
                    header,
                    RecommendationCategory::Genre,
                    pills,
                ));
            }
        }
        Ok(RecommendationsPage { shelves })
    }

    async fn explore_page(&self, path: &str) -> MhResult<RecommendationsPage> {
        let rest = path.strip_prefix("page::").unwrap_or(path);
        let mut parts = rest.splitn(2, "::");
        let page_id = parts.next().unwrap_or("").trim();
        let title = parts.next().unwrap_or("").trim();

        if !page_id.is_empty() {
            if let Ok(body) = self
                .pathfinder(
                    "browsePage",
                    json!({
                        "uri": format!("spotify:page:{page_id}"),
                        "pagePagination": { "offset": 0, "limit": 30 },
                        "sectionPagination": { "offset": 0, "limit": 30 },
                        "browseEndUserIntegration": "INTEGRATION_WEB_PLAYER",
                    }),
                )
                .await
            {
                let shelves = home_shelves_from_body(&body);
                if !shelves.is_empty() {
                    return Ok(RecommendationsPage { shelves });
                }
            }
        }

        if title.is_empty() {
            return Ok(RecommendationsPage::default());
        }
        let mut shelves = Vec::new();
        for (op, container, cat, suffix, is_playlist) in [
            (
                "searchPlaylists",
                "playlists",
                RecommendationCategory::Editorial,
                "Playlists",
                true,
            ),
            (
                "searchAlbums",
                "albumsV2",
                RecommendationCategory::NewReleases,
                "Albums",
                false,
            ),
        ] {
            let vars = json!({
                "searchTerm": title,
                "offset": 0,
                "limit": 20,
                "numberOfTopResults": 20,
                "includeAudiobooks": false,
                "includeEpisodeContentRatingsV2": true,
            });
            let Ok(body) = self.pathfinder(op, vars).await else {
                continue;
            };
            let items: Vec<RecommendationItem> = body
                .pointer(&format!("/data/searchV2/{container}/items"))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|it| {
                            let d = it
                                .pointer("/item/data")
                                .or_else(|| it.get("data"))
                                .unwrap_or(it);
                            if is_playlist {
                                map_playlist_node(d).map(RecommendationItem::Playlist)
                            } else {
                                map_album_node(d).map(RecommendationItem::Album)
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            if !items.is_empty() {
                shelves.push(RecommendationShelf::new(
                    format!("explore-{title}-{suffix}").to_lowercase(),
                    format!("{title} {suffix}"),
                    cat,
                    items,
                ));
            }
        }
        Ok(RecommendationsPage { shelves })
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        if let Some(show_id) = id
            .strip_prefix("show::")
            .or_else(|| id.strip_prefix("podcast::"))
        {
            return self.show_detail(show_id).await;
        }
        let body = self
            .pathfinder(
                "fetchPlaylist",
                json!({
                    "uri": format!("spotify:playlist:{id}"),
                    "offset": 0,
                    "limit": 200,
                    "enableWatchFeedEntrypoint": false,
                }),
            )
            .await?;
        let node = body
            .pointer("/data/playlistV2")
            .or_else(|| body.pointer("/data/playlist"))
            .cloned()
            .ok_or_else(|| MhError::NotFound(format!("Spotify playlist {id}")))?;
        let playlist = map_playlist_node(&node)
            .ok_or_else(|| MhError::NotFound(format!("Spotify playlist {id}")))?;
        let map_items = |arr: &[Value]| -> Vec<LibraryTrackDto> {
            arr.iter()
                .filter_map(|it| {
                    let track = it
                        .pointer("/itemV2/data")
                        .or_else(|| it.pointer("/item"))
                        .unwrap_or(it);
                    map_track_node(track)
                })
                .collect()
        };
        let first: Vec<Value> = node
            .pointer("/content/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let total = node
            .pointer("/content/totalCount")
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let mut tracks = map_items(&first);
        let mut offset = first.len() as i64;
        let mut guard = 0;
        while offset < total && !first.is_empty() {
            guard += 1;
            if guard > 200 {
                break;
            }
            let page = self
                .playlist_page(&format!("spotify:playlist:{id}"), offset, 200)
                .await?;
            let items = playlist_items(&page);
            if items.is_empty() {
                break;
            }
            offset += items.len() as i64;
            tracks.extend(map_items(&items));
        }
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }
}

impl SpotifyLibrary {
    /// One page of `fetchPlaylistContents`. The request shape was written out at three
    /// call sites, each of which then repeated the same response pointer.
    async fn playlist_page(&self, uri: &str, offset: i64, limit: i64) -> MhResult<Value> {
        self.pathfinder(
            "fetchPlaylistContents",
            json!({
                "uri": uri,
                "offset": offset,
                "limit": limit,
                "includeEpisodeContentRatingsV2": false,
            }),
        )
        .await
    }

    /// Every track id in a playlist, paged so playlists longer than one page are not
    /// silently truncated. Returns the playlist name alongside the ids so the
    /// downloader can name the folder.
    pub async fn playlist_track_ids(&self, id: &str) -> MhResult<(String, Vec<String>)> {
        const PAGE: i64 = 100;
        const MAX_PAGES: u32 = 200;

        let playlist_uri = format!("spotify:playlist:{id}");
        let mut name = String::new();
        let mut ids: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut offset: i64 = 0;

        for _ in 0..MAX_PAGES {
            let body = self.playlist_page(&playlist_uri, offset, PAGE).await?;
            if name.is_empty() {
                name = str_at(&body, &["/data/playlistV2/name"])
                    .unwrap_or_default()
                    .to_string();
            }
            let items = playlist_items(&body);
            if items.is_empty() {
                break;
            }
            let page_len = items.len() as i64;
            for it in &items {
                let uri = item_uri(it).unwrap_or("");
                if let Some(tid) = uri.strip_prefix("spotify:track:") {
                    if seen.insert(tid.to_string()) {
                        ids.push(tid.to_string());
                    }
                }
            }
            if page_len < PAGE {
                break;
            }
            offset += page_len;
        }

        if ids.is_empty() {
            return Err(MhError::NotFound(format!(
                "Spotify playlist {id} has no downloadable tracks (episodes and local files are skipped)"
            )));
        }
        Ok((name, ids))
    }

    /// Podcasts are the one browsable thing Spotify serves without a login, so a
    /// keys-only user gets the Web API rather than an auth error.
    async fn show_detail(&self, show_id: &str) -> MhResult<LibraryPlaylistDetail> {
        let pf_err = match self.show_detail_pathfinder(show_id).await {
            Ok(detail) => return Ok(detail),
            Err(e) => e,
        };

        match self.show_detail_web_api(show_id).await {
            Ok(detail) => {
                self.log_line(
                    "info",
                    format!("Podcast {show_id} served from the Web API: {pf_err}"),
                );
                Ok(detail)
            }
            Err(web_err) => Err(MhError::Other(format!(
                "Spotify podcast {show_id} failed on both paths — pathfinder: {pf_err}; web API: {web_err}"
            ))),
        }
    }

    async fn show_detail_web_api(&self, show_id: &str) -> MhResult<LibraryPlaylistDetail> {
        let data = self.web_api().await?.get_show_episodes(show_id).await?;
        let name = string_at(&data, &["/show_name"]);
        let tracks: Vec<LibraryTrackDto> = data["episodes"]
            .as_array()
            .map(|a| a.iter().filter_map(|e| map_web_episode(e, &name)).collect())
            .unwrap_or_default();
        let cover = data["cover_url"]
            .as_str()
            .map(|u| cover_id(ServicePlatform::Spotify, u));

        let playlist = LibraryPlaylistDto {
            id: stable_hash_i64(show_id),
            name,
            created_at: 0,
            updated_at: 0,
            track_count: tracks.len() as i64,
            cover_ids: cover.map(|c| vec![c]).unwrap_or_default(),
            service_id: Some(format!("show::{show_id}")),
            description: data["publisher"].as_str().map(str::to_string),
            owner: data["publisher"].as_str().map(str::to_string),
        };
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }

    async fn show_detail_pathfinder(&self, show_id: &str) -> MhResult<LibraryPlaylistDetail> {
        let meta = self
            .pathfinder(
                "queryShowMetadataV2",
                json!({ "uri": format!("spotify:show:{show_id}") }),
            )
            .await?;
        let sh = meta
            .pointer("/data/podcastUnionV2")
            .or_else(|| meta.pointer("/data/showUnion"))
            .or_else(|| meta.pointer("/data/podcastUnion"))
            .cloned()
            .unwrap_or(Value::Null);
        let name = str_at(&sh, &["/name"]).unwrap_or("").to_string();
        let base_desc = str_at(&sh, &["/description"])
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let topics: Vec<String> = sh
            .pointer("/topics/items")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| str_at(t, &["title"]).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let rating_line = sh
            .pointer("/rating/averageRating")
            .filter(|r| {
                r.get("showAverage")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false)
            })
            .and_then(|r| {
                let avg = r.get("average").and_then(|x| x.as_f64())?;
                let total = r.get("totalRatings").and_then(|x| x.as_u64()).unwrap_or(0);
                Some(format!("★ {avg:.1} ({total} ratings)"))
            });
        let mut header_bits: Vec<String> = Vec::new();
        if !topics.is_empty() {
            header_bits.push(topics.join(" · "));
        }
        if let Some(r) = rating_line {
            header_bits.push(r);
        }
        let description = match (header_bits.is_empty(), base_desc) {
            (true, d) => d,
            (false, Some(d)) => Some(format!("{}\n\n{d}", header_bits.join("  ·  "))),
            (false, None) => Some(header_bits.join("  ·  ")),
        };
        let owner = sh
            .pointer("/publisher/name")
            .or_else(|| sh.pointer("/publisherName"))
            .and_then(|x| x.as_str())
            .map(str::to_string);
        let cover = first_cover_url(&sh["coverArt"])
            .or_else(|| first_cover_url(sh.pointer("/coverArtV2").unwrap_or(&Value::Null)))
            .map(|u| cover_id(ServicePlatform::Spotify, &u));

        let episodes = self
            .pathfinder(
                "queryPodcastEpisodes",
                json!({
                    "uri": format!("spotify:show:{show_id}"),
                    "offset": 0,
                    "limit": 200,
                }),
            )
            .await?;
        let items = episodes
            .pointer("/data/podcastUnionV2/episodesV2/items")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = items
            .iter()
            .filter_map(|it| {
                let ep = it.pointer("/entity/data").unwrap_or(it);
                map_show_episode(ep, &name)
            })
            .collect();

        let playlist = LibraryPlaylistDto {
            id: stable_hash_i64(show_id),
            name,
            created_at: 0,
            updated_at: 0,
            track_count: tracks.len() as i64,
            cover_ids: cover.map(|c| vec![c]).unwrap_or_default(),
            service_id: Some(format!("show::{show_id}")),
            description,
            owner,
        };
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }
}

/// Web API episode objects, which spell duration and covers differently from the
/// pathfinder shape [`map_show_episode`] reads.
fn map_web_episode(v: &Value, show_name: &str) -> Option<LibraryTrackDto> {
    let id = str_at(v, &["id"])
        .map(str::to_string)
        .or_else(|| str_at(v, &["uri"]).and_then(uri_to_id))?;
    Some(LibraryTrackDto {
        path: format!("https://open.spotify.com/episode/{id}"),
        title: str_at(v, &["name"]).map(str::to_string),
        artist: Some(show_name.to_string()),
        album: Some(show_name.to_string()),
        year: str_at(v, &["release_date"]).map(|d| d.chars().take(4).collect()),
        duration_secs: v
            .get("duration_ms")
            .and_then(|x| x.as_f64())
            .map(|m| m / 1000.0),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: first_image(v, &["/images"]).map(|u| cover_id(ServicePlatform::Spotify, &u)),
        primary_artist: Some(show_name.to_string()),
        ..Default::default()
    })
}

fn map_show_episode(v: &Value, show_name: &str) -> Option<LibraryTrackDto> {
    let uri = str_at(v, &["uri"])?;
    let id = uri_to_id(uri)?;
    let title = str_at(v, &["name"]).unwrap_or("").to_string();
    let dur = v
        .pointer("/duration/totalMilliseconds")
        .and_then(|x| x.as_f64())
        .map(|m| m / 1000.0);
    let cover = first_cover_url(&v["coverArt"]).map(|u| cover_id(ServicePlatform::Spotify, &u));
    Some(LibraryTrackDto {
        path: format!("https://open.spotify.com/episode/{id}"),
        title: Some(title),
        artist: Some(show_name.to_string()),
        album: Some(show_name.to_string()),
        duration_secs: dur,
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: cover,
        primary_artist: Some(show_name.to_string()),
        ..Default::default()
    })
}

impl SpotifyLibrary {
    async fn user_id(&self) -> MhResult<String> {
        if let Some(profile) = self.librespot.read().await.cached_profile() {
            if let Some(id) = str_at(&profile, &["id"]) {
                if !id.is_empty() {
                    return Ok(id.to_string());
                }
            }
        }
        let (token, _) = self.auth().await?;
        if let Ok(resp) = self
            .client
            .get("https://api.spotify.com/v1/me")
            .header("authorization", format!("Bearer {token}"))
            .header("accept", "application/json")
            .send()
            .await
        {
            if resp.status().is_success() {
                if let Ok(body) = resp.json::<Value>().await {
                    if let Some(id) = str_at(&body, &["id"]) {
                        if !id.is_empty() {
                            return Ok(id.to_string());
                        }
                    }
                }
            }
        }
        let body = self.pathfinder("profileAttributes", json!({})).await?;
        for path in [
            "/data/me/profile/username",
            "/data/profileAttributes/username",
            "/data/me/profile/id",
            "/data/me/id",
        ] {
            if let Some(id) = body.pointer(path).and_then(|v| v.as_str()) {
                if !id.is_empty() {
                    return Ok(id.to_string());
                }
            }
        }
        Err(MhError::Auth(format!(
            "Spotify user id not resolvable via cache/me/pathfinder. profileAttributes body: {body}"
        )))
    }

    async fn spclient_post(&self, url: &str, body: &Value) -> MhResult<Value> {
        let (token, client_token) = self.auth().await?;
        let mut req = self
            .client
            .post(url)
            .bearer_auth(&token)
            .header("accept", "application/json")
            .header("content-type", "application/json;charset=UTF-8")
            .header("app-platform", "WebPlayer")
            .header("spotify-app-version", "1.2.92.18.gfdac4f00")
            .header("origin", "https://open.spotify.com")
            .header("referer", "https://open.spotify.com/");
        if let Some(ct) = &client_token {
            req = req.header("client-token", ct.clone());
        }
        let resp = req.json(body).send().await.map_err(MhError::Network)?;
        let status = resp.status();
        let txt = resp.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(MhError::Other(format!(
                "Spotify spclient {url} returned {}: {txt}",
                status.as_u16()
            )));
        }
        Ok(serde_json::from_str(&txt).unwrap_or(Value::Null))
    }

    async fn rootlist_add(&self, uri: &str) -> MhResult<()> {
        let uid = self.user_id().await?;
        let ts = now_millis();
        let body = json!({
            "deltas": [{
                "ops": [{
                    "kind": "ADD",
                    "add": {
                        "items": [{ "uri": uri, "attributes": { "timestamp": ts.to_string() } }],
                        "addFirst": true
                    }
                }],
                "info": { "source": { "client": "WEBPLAYER" } }
            }]
        });
        let url =
            format!("https://spclient.wg.spotify.com/playlist/v2/user/{uid}/rootlist/changes");
        self.spclient_post(&url, &body).await?;
        Ok(())
    }

    async fn rootlist_remove(&self, uri: &str) -> MhResult<()> {
        let uid = self.user_id().await?;
        let body = json!({
            "deltas": [{
                "ops": [{
                    "kind": "REM",
                    "rem": { "items": [{ "uri": uri }], "itemsAsKey": true }
                }],
                "info": { "source": { "client": "WEBPLAYER" } }
            }]
        });
        let url =
            format!("https://spclient.wg.spotify.com/playlist/v2/user/{uid}/rootlist/changes");
        self.spclient_post(&url, &body).await?;
        Ok(())
    }

    fn uris_for(kind: &str, ids: &[String]) -> Vec<String> {
        ids.iter()
            .map(|id| format!("spotify:{kind}:{id}"))
            .collect()
    }

    async fn library_mutate(&self, op: &str, kind: &str, ids: &[String]) -> MhResult<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let uris = Self::uris_for(kind, ids);
        self.pathfinder(op, json!({ "libraryItemUris": uris }))
            .await?;
        Ok(())
    }

    async fn walk_pages<T, F, Fut, G>(&self, pull: F, extract: G) -> MhResult<Vec<String>>
    where
        F: FnMut(Page) -> Fut,
        Fut: std::future::Future<Output = MhResult<ServiceLibraryPage<T>>>,
        G: Fn(&T) -> Option<String>,
    {
        let items = crate::services::common::library::paginate_all(100, pull).await?;
        Ok(items.iter().filter_map(extract).collect())
    }
}

#[async_trait]
impl ServiceLibraryMutations for SpotifyLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Spotify
    }

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities {
            save_tracks: true,
            save_albums: true,
            follow_artists: true,
            follow_playlists: true,
            create_playlists: true,
            edit_playlists: true,
            reorder_playlists: false,
            radio: true,
        }
    }

    async fn set_favorites(&self, kind: FavKind, op: FavOp, ids: &[String]) -> MhResult<()> {
        let mutation = match op {
            FavOp::Add => "addToLibrary",
            FavOp::Remove => "removeFromLibrary",
        };
        if kind == FavKind::Label {
            return Err(MhError::Unsupported("Spotify has no labels".into()));
        }
        self.library_mutate(mutation, kind.as_str(), ids).await
    }

    async fn follow_playlist(&self, id: &str) -> MhResult<()> {
        let uri = format!("spotify:playlist:{id}");
        self.rootlist_add(&uri).await
    }
    async fn unfollow_playlist(&self, id: &str) -> MhResult<()> {
        let uri = format!("spotify:playlist:{id}");
        self.rootlist_remove(&uri).await
    }

    async fn create_playlist(&self, input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        let (token, client_token) = self.auth().await?;
        let mut values = serde_json::Map::new();
        values.insert("name".into(), Value::String(input.name.clone()));
        if let Some(d) = &input.description {
            if !d.is_empty() {
                values.insert("description".into(), Value::String(d.clone()));
            }
        }
        let body = json!({
            "ops": [{
                "kind": "UPDATE_LIST_ATTRIBUTES",
                "updateListAttributes": {
                    "newAttributes": { "values": Value::Object(values) }
                }
            }]
        });
        let _ = (token, client_token);
        let resp_body = self
            .spclient_post(
                "https://spclient.wg.spotify.com/playlist/v2/playlist",
                &body,
            )
            .await?;
        let uri = str_at(&resp_body, &["uri"])
            .ok_or_else(|| {
                MhError::Other(format!(
                    "Spotify createPlaylist: no uri in response: {resp_body}"
                ))
            })?
            .to_string();
        let playlist_id = uri_to_id(&uri).unwrap_or_else(|| uri.clone());

        if let Err(e) = self.rootlist_add(&uri).await {
            self.log_line(
                "warn",
                format!("create_playlist: rootlist add for {uri} failed: {e}"),
            );
        }

        if !input.initial_track_ids.is_empty() {
            let uris = Self::uris_for("track", &input.initial_track_ids);
            self.pathfinder(
                "addToPlaylist",
                json!({
                    "playlistUri": uri,
                    "playlistItemUris": uris,
                    "newPosition": { "moveType": "BOTTOM_OF_PLAYLIST", "fromUid": null },
                }),
            )
            .await?;
        }

        Ok(PlaylistMutateResult::id(playlist_id))
    }

    async fn rename_playlist(
        &self,
        id: &str,
        new_name: &str,
        new_description: Option<&str>,
        _new_is_public: Option<bool>,
        new_is_collaborative: Option<bool>,
    ) -> MhResult<()> {
        let mut values = serde_json::Map::new();
        values.insert("name".into(), Value::String(new_name.into()));
        if let Some(d) = new_description {
            values.insert("description".into(), Value::String(d.into()));
        }
        if let Some(c) = new_is_collaborative {
            values.insert("collaborative".into(), Value::Bool(c));
        }
        let body = json!({
            "deltas": [{
                "ops": [{
                    "kind": "UPDATE_LIST_ATTRIBUTES",
                    "updateListAttributes": {
                        "newAttributes": { "values": Value::Object(values) }
                    }
                }],
                "info": { "source": { "client": "WEBPLAYER" } }
            }]
        });
        let url = format!("https://spclient.wg.spotify.com/playlist/v2/playlist/{id}/changes");
        self.spclient_post(&url, &body).await?;
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> MhResult<()> {
        self.unfollow_playlist(id).await
    }

    async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        let playlist_uri = format!("spotify:playlist:{id}");
        let uris = Self::uris_for("track", track_ids);
        let resp = self
            .pathfinder(
                "addToPlaylist",
                json!({
                    "playlistUri": playlist_uri,
                    "playlistItemUris": uris,
                    "newPosition": { "moveType": "BOTTOM_OF_PLAYLIST", "fromUid": null },
                }),
            )
            .await?;
        let snapshot_id = str_at(&resp, &["/data/addItemsToPlaylist/snapshotId"]).map(String::from);
        Ok(PlaylistMutateResult {
            playlist_id: id.to_string(),
            library_id: None,
            snapshot_id,
        })
    }

    async fn remove_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
        _positions: Option<&[u32]>,
    ) -> MhResult<PlaylistMutateResult> {
        if track_ids.is_empty() {
            return Ok(PlaylistMutateResult::id(id));
        }
        let playlist_uri = format!("spotify:playlist:{id}");
        let target_uris: std::collections::HashSet<String> = track_ids
            .iter()
            .map(|tid| format!("spotify:track:{tid}"))
            .collect();
        let mut uids: Vec<String> = Vec::new();
        let mut consumed: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut offset: u32 = 0;
        for _ in 0..200 {
            let body = self
                .playlist_page(&playlist_uri, offset as i64, 100)
                .await?;
            let items = playlist_items(&body);
            if items.is_empty() {
                break;
            }
            for it in &items {
                let uid = str_at(it, &["uid"]).unwrap_or("");
                let uri = item_uri(it).unwrap_or("");
                if uid.is_empty() || uri.is_empty() {
                    continue;
                }
                if target_uris.contains(uri) && !consumed.contains(uri) {
                    uids.push(uid.to_string());
                    consumed.insert(uri.to_string());
                    if consumed.len() == target_uris.len() {
                        break;
                    }
                }
            }
            if consumed.len() == target_uris.len() {
                break;
            }
            offset += items.len() as u32;
            let total = body
                .pointer("/data/playlistV2/content/totalCount")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if (offset as u64) >= total {
                break;
            }
        }
        if uids.is_empty() {
            return Err(MhError::Other(format!(
                "Spotify remove-from-playlist: no matching tracks found in playlist {id}"
            )));
        }
        let resp = self
            .pathfinder(
                "removeFromPlaylist",
                json!({
                    "playlistUri": playlist_uri,
                    "uids": uids,
                }),
            )
            .await?;
        let snapshot_id = str_at(&resp, &["/data/removeFromPlaylist/snapshotId"]).map(String::from);
        Ok(PlaylistMutateResult {
            playlist_id: id.to_string(),
            library_id: None,
            snapshot_id,
        })
    }

    async fn fetch_saved_ids(&self, kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        let mut out = SavedIdSet::default();
        for kind in kinds {
            match kind {
                SaveKind::Track => {
                    out.track_ids = self
                        .walk_pages(
                            |p| self.tracks(p),
                            |t| t.path.rsplit('/').next().map(str::to_string),
                        )
                        .await?;
                }
                SaveKind::Album => {
                    out.album_ids = self
                        .walk_pages(|p| self.albums(p), |a| Some(a.album_key.clone()))
                        .await?;
                }
                SaveKind::Artist => {
                    out.artist_ids = self
                        .walk_pages(|p| self.artists(p), |a| Some(a.key.clone()))
                        .await?;
                }
                SaveKind::Playlist => {
                    out.playlist_ids = self
                        .walk_pages(|p| self.playlists(p), |p| p.service_id.clone())
                        .await?;
                }
            }
        }
        Ok(out)
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        let playlists =
            crate::services::common::library::paginate_all(100, |p| self.playlists(p)).await?;
        let rows = playlists
            .iter()
            .filter_map(|p| {
                let svc_id = p.service_id.clone()?;
                Some(OwnedPlaylistRow {
                    platform: ServicePlatform::Spotify.as_str().to_string(),
                    service_id: svc_id,
                    name: p.name.clone(),
                    cover_id: p.cover_ids.first().cloned(),
                    track_count: p.track_count,
                    updated_at: p.updated_at,
                })
            })
            .collect();
        Ok(rows)
    }

    async fn radio_for(&self, seed_kind: RadioSeedKind, seed_id: &str) -> MhResult<RadioResult> {
        if matches!(seed_kind, RadioSeedKind::Playlist) {
            return self.extend_playlist(seed_id).await;
        }
        let kind_seg = match seed_kind {
            RadioSeedKind::Track => "track",
            RadioSeedKind::Album => "album",
            RadioSeedKind::Artist => "artist",
            RadioSeedKind::Playlist => "playlist",
        };
        let seed_uri = format!("spotify:{kind_seg}:{seed_id}");
        let url = format!("https://spclient.wg.spotify.com/radio-apollo/v3/stations/{seed_uri}");
        let (token, client_token) = self.auth().await?;
        let mut req = self
            .client
            .get(&url)
            .bearer_auth(&token)
            .header("accept", "application/json")
            .header("app-platform", "WebPlayer")
            .header("origin", "https://open.spotify.com");
        if let Some(ct) = &client_token {
            req = req.header("client-token", ct.clone());
        }
        let resp = req.send().await.map_err(MhError::Network)?;
        let status = resp.status();
        if !status.is_success() {
            let txt = resp.text().await.unwrap_or_default();
            return Err(MhError::Other(format!(
                "Spotify radio-apollo {seed_uri} returned {}: {txt}",
                status.as_u16()
            )));
        }
        let body: Value = resp.json().await.map_err(MhError::Network)?;
        let title = str_at(&body, &["title"])
            .or_else(|| str_at(&body, &["subtitle"]))
            .map(String::from)
            .unwrap_or_else(|| format!("Radio · {seed_uri}"));
        let station_id = str_at(&body, &["uri"])
            .or_else(|| str_at(&body, &["station_uri"]))
            .map(String::from);
        let arr = body
            .get("tracks")
            .and_then(|v| v.as_array())
            .cloned()
            .or_else(|| {
                body.get("recommendedTracks")
                    .and_then(|v| v.as_array())
                    .cloned()
            })
            .unwrap_or_default();
        let tracks = Self::apollo_tracks(&arr);
        Ok(RadioResult {
            seed_kind,
            seed_id: seed_id.to_string(),
            title,
            tracks,
            station_id,
            video_urls: Default::default(),
            continuation: None,
        })
    }
}

impl SpotifyLibrary {
    fn apollo_tracks(arr: &[Value]) -> Vec<LibraryTrackDto> {
        let mut tracks = Vec::with_capacity(arr.len());
        for raw in arr.iter().take(50) {
            let uri = raw
                .get("uri")
                .or_else(|| raw.get("originalId"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let id = uri_to_id(uri).filter(|s| !s.is_empty()).or_else(|| {
                str_at(raw, &["id"])
                    .filter(|s| !s.is_empty())
                    .map(String::from)
            });
            let id = match id {
                Some(i) => i,
                None => continue,
            };
            let title = raw
                .pointer("/metadata/title")
                .or_else(|| raw.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let artist = raw
                .pointer("/metadata/artist_name")
                .or_else(|| raw.pointer("/artists/0/name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let album = raw
                .pointer("/metadata/album_title")
                .or_else(|| raw.pointer("/album/name"))
                .or_else(|| raw.pointer("/albumOfTrack/name"))
                .and_then(|v| v.as_str())
                .map(String::from);
            let duration_secs = raw
                .pointer("/metadata/duration")
                .or_else(|| raw.get("duration_ms"))
                .or_else(|| raw.get("duration"))
                .and_then(|v| v.as_f64())
                .map(|m| if m > 1_000.0 { m / 1000.0 } else { m });
            let cover_url = raw
                .pointer("/metadata/image_url")
                .or_else(|| raw.pointer("/album/largeImageUrl"))
                .or_else(|| raw.pointer("/album/imageUrl"))
                .or_else(|| raw.pointer("/album/images/0/url"))
                .or_else(|| raw.pointer("/albumOfTrack/coverArt/sources/0/url"))
                .or_else(|| raw.pointer("/coverArt/sources/0/url"))
                .and_then(|v| v.as_str())
                .map(|u| {
                    if let Some(rest) = u.strip_prefix("spotify:image:") {
                        format!("https://i.scdn.co/image/{rest}")
                    } else {
                        u.to_string()
                    }
                });
            tracks.push(LibraryTrackDto {
                path: format!("https://open.spotify.com/track/{id}"),
                title: Some(title),
                artist: Some(artist.clone()),
                album,
                duration_secs,
                size: 0,
                mtime_ns: 0,
                is_video: false,
                cover_id: cover_url.map(|u| cover_id(ServicePlatform::Spotify, &u)),
                primary_artist: Some(artist),
                artist_id: str_at(raw, &["/artists/0/uri"])
                    .and_then(uri_to_id)
                    .or_else(|| {
                        str_at(raw, &["/artists/0/id"])
                            .filter(|s| !s.is_empty())
                            .map(String::from)
                    }),
                ..Default::default()
            });
        }
        tracks
    }

    async fn extend_playlist(&self, playlist_id: &str) -> MhResult<RadioResult> {
        let detail = self.playlist_detail(playlist_id).await?;
        let seed_ids: Vec<String> = detail
            .tracks
            .iter()
            .filter_map(|t| t.path.rsplit('/').next().map(String::from))
            .filter(|s| !s.is_empty())
            .take(50)
            .collect();
        let url = "https://spclient.wg.spotify.com/playlistextender/v2/extendp";
        let body = json!({ "trackIDs": seed_ids, "numResults": 30 });
        let resp = self.spclient_post(url, &body).await?;
        let arr = resp
            .get("recommendedTracks")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(RadioResult {
            seed_kind: RadioSeedKind::Playlist,
            seed_id: playlist_id.to_string(),
            title: format!("Recommended · {}", detail.playlist.name),
            tracks: Self::apollo_tracks(&arr),
            station_id: None,
            video_urls: Default::default(),
            continuation: None,
        })
    }
}

#[cfg(test)]
mod classify_shelf_tests {
    use super::classify_shelf;
    use crate::services::common::library::RecommendationCategory as C;

    #[test]
    fn classifies_a_real_home_feed() {
        let cases = [
            ("", C::Hero),
            ("Albums featuring songs you like", C::Other),
            ("Made For You", C::DailyMix),
            ("Made for you", C::DailyMix),
            ("Recently played", C::RecentlyPlayed),
            ("Your playlists", C::Other),
            ("Recommended for today", C::Discovery),
            ("The Golden Age of Disco", C::Other),
            ("Albums by artists you follow", C::Other),
            ("Episodes you might like", C::Other),
            ("More like Daft Punk", C::Discovery),
            ("Daft Punk collection", C::Other),
            ("For fans of Daft Punk", C::Discovery),
            ("Based on your recent listening", C::Discovery),
            ("Videos you might like", C::Other),
            ("Daft Punk Mix", C::DailyMix),
            ("Daily Mix 1", C::DailyMix),
            ("Discover Weekly", C::DailyMix),
            ("Release Radar", C::DailyMix),
        ];
        for (title, want) in cases {
            assert_eq!(
                classify_shelf("spotify:section:0000", title),
                want,
                "title: {title:?}",
            );
        }
    }

    #[test]
    fn made_for_you_is_not_swallowed_by_the_generic_for_you_rule() {
        assert_eq!(classify_shelf("", "Made For You"), C::DailyMix);
        assert_eq!(classify_shelf("", "Picked for you"), C::Discovery);
    }

    #[test]
    fn untitled_section_is_the_shortcuts_grid() {
        assert_eq!(classify_shelf("spotify:section:abc", "   "), C::Hero);
    }
}

#[cfg(test)]
mod map_track_node_tests {
    use super::map_track_node;
    use serde_json::json;

    #[test]
    fn reads_the_artist_uri_from_any_of_the_shapes_spotify_returns() {
        let shapes = [
            json!({"uri":"spotify:track:t1","name":"T",
                   "artists":{"items":[{"uri":"spotify:artist:a1","profile":{"name":"A"}}]}}),
            json!({"uri":"spotify:track:t1","name":"T",
                   "artists":{"items":[{"data":{"uri":"spotify:artist:a1"},"profile":{"name":"A"}}]}}),
            json!({"uri":"spotify:track:t1","name":"T",
                   "firstArtist":{"items":[{"uri":"spotify:artist:a1"}]},
                   "artists":{"items":[{"profile":{"name":"A"}}]}}),
            json!({"uri":"spotify:track:t1","name":"T","artist":{"uri":"spotify:artist:a1"}}),
        ];
        for (i, v) in shapes.iter().enumerate() {
            let t = map_track_node(v).expect("mapped");
            assert_eq!(t.artist_id.as_deref(), Some("a1"), "shape {i}");
        }
    }

    #[test]
    fn album_key_falls_back_when_album_of_track_is_absent() {
        let v = json!({"uri":"spotify:track:t1","name":"T","album":{"uri":"spotify:album:al1"}});
        assert_eq!(
            map_track_node(&v).unwrap().album_key.as_deref(),
            Some("al1")
        );
    }

    #[test]
    fn missing_artist_uri_yields_none_not_a_panic() {
        let v = json!({"uri":"spotify:track:t1","name":"T",
                       "artists":{"items":[{"profile":{"name":"A"}}]}});
        assert!(map_track_node(&v).unwrap().artist_id.is_none());
    }
}

/// The items array of a `fetchPlaylistContents` response.
fn playlist_items(body: &Value) -> Vec<Value> {
    body.pointer("/data/playlistV2/content/items")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

/// A playlist item's track URI. Spotify has renamed this field twice — `tracksV2`
/// arrived alongside `itemV2` — and every reader has to try all three spellings, so
/// the chain lives here rather than at each call site.
fn item_uri(it: &Value) -> Option<&str> {
    it.pointer("/itemV2/data/uri")
        .or_else(|| it.pointer("/item/data/uri"))
        .or_else(|| it.pointer("/track/uri"))
        .and_then(|v| v.as_str())
}
