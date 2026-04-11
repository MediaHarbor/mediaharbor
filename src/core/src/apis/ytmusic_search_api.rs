
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;

const YTM_HOME: &str = "https://music.youtube.com/";
const YTM_API: &str = "https://music.youtube.com/youtubei/v1";

const FALLBACK_CLIENT_NAME: &str = "WEB_REMIX";
const FALLBACK_CLIENT_VERSION: &str = "1.20240101.01.00";
const FALLBACK_API_VERSION: &str = "v1";

const FILTER_SONG: &str = "EgWKAQIIAWoKEAkQAxAEEAoQBQ==";
const FILTER_ALBUM: &str = "EgWKAQIYAWoKEAkQAxAEEAoQBQ==";
const FILTER_PLAYLIST: &str = "EgeKAQQoAEABagoQCRADEAQQChAF";
const FILTER_ARTIST: &str = "EgWKAQIgAWoKEAkQAxAEEAoQBQ==";
const FILTER_PODCAST: &str = "Eg+KAQwIABAAGAAgACgAMAFqChAEEAMQCRAFEAo=";
const FILTER_VIDEO: &str = "EgWKAQIQAWoKEAkQAxAEEAoQBQ==";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YtMusicFilter {
    Song,
    Video,
    Album,
    Playlist,
    Artist,
    Podcast,
}

impl YtMusicFilter {
    fn param(self) -> &'static str {
        match self {
            Self::Song => FILTER_SONG,
            Self::Video => FILTER_VIDEO,
            Self::Album => FILTER_ALBUM,
            Self::Playlist => FILTER_PLAYLIST,
            Self::Artist => FILTER_ARTIST,
            Self::Podcast => FILTER_PODCAST,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum YtMusicResultType {
    Song,
    Video,
    Album,
    Playlist,
    Artist,
    Podcast,
    Episode,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtMusicResult {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_secs: u32,
    pub thumbnail_url: Option<String>,
    pub result_type: YtMusicResultType,
    pub browse_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct YtMusicClient {
    pub client_name: String,
    pub client_version: String,
    pub api_key: String,
    pub api_version: String,
    http: Client,
}

impl YtMusicClient {
    pub async fn init() -> MhResult<Self> {
        let http = build_mozilla_client()?;

        let mut api_key = String::new();
        let mut client_name = FALLBACK_CLIENT_NAME.to_string();
        let mut client_version = FALLBACK_CLIENT_VERSION.to_string();
        let mut api_version = FALLBACK_API_VERSION.to_string();

        match scrape_innertube_config(&http).await {
            Ok(cfg) => {
                if !cfg.api_key.is_empty() {
                    api_key = cfg.api_key;
                }
                if !cfg.client_name.is_empty() {
                    client_name = cfg.client_name;
                }
                if !cfg.client_version.is_empty() {
                    client_version = cfg.client_version;
                }
                if !cfg.api_version.is_empty() {
                    api_version = cfg.api_version;
                }
            }
            Err(_) => {
            }
        }

        Ok(YtMusicClient {
            client_name,
            client_version,
            api_key,
            api_version,
            http,
        })
    }

    fn context(&self) -> Value {
        json!({
            "client": {
                "clientName":    self.client_name,
                "clientVersion": self.client_version,
                "hl":            "en",
                "gl":            "US",
            }
        })
    }

    fn endpoint(&self, name: &str) -> String {
        if self.api_key.is_empty() {
            format!("{}/{}", YTM_API, name)
        } else {
            format!("{}/{}?key={}", YTM_API, name, self.api_key)
        }
    }

    async fn post(&self, endpoint: &str, body: Value) -> MhResult<Value> {
        let resp = self
            .http
            .post(endpoint)
            .header("Origin", "https://music.youtube.com")
            .header("Referer", "https://music.youtube.com/")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(MhError::Network)?;

        if !resp.status().is_success() {
            return Err(MhError::Network(resp.error_for_status().unwrap_err()));
        }
        Ok(resp.json().await.map_err(MhError::Network)?)
    }

    pub async fn search(
        &self,
        query: &str,
        filter: YtMusicFilter,
    ) -> MhResult<Vec<YtMusicResult>> {
        let body = json!({
            "context": self.context(),
            "query":   query,
            "params":  filter.param(),
        });

        let data = self.post(&self.endpoint("search"), body).await?;

        match filter {
            YtMusicFilter::Podcast => Ok(parse_podcast_results(&data)),
            _ => Ok(parse_search_results(&data, filter)),
        }
    }

    pub async fn get_album_details(
        &self,
        browse_id: &str,
    ) -> MhResult<(Vec<YtMusicResult>, String, Option<String>)> {
        let body = json!({
            "context":  self.context(),
            "browseId": browse_id,
        });
        let data = self.post(&self.endpoint("browse"), body).await?;
        let tracks = parse_album_tracks(&data);
        let title = parse_header_title(&data).unwrap_or_default();
        let cover = parse_header_thumbnail(&data);
        Ok((tracks, title, cover))
    }

    pub async fn get_artist_albums(&self, browse_id: &str) -> MhResult<Vec<serde_json::Value>> {
        let body = json!({
            "context":  self.context(),
            "browseId": browse_id,
        });
        let data = self.post(&self.endpoint("browse"), body).await?;
        Ok(parse_artist_page_albums(&data))
    }

    pub async fn get_podcast_details(
        &self,
        browse_id: &str,
    ) -> MhResult<(Vec<YtMusicResult>, String, Option<String>)> {
        let body = json!({
            "context":  self.context(),
            "browseId": browse_id,
        });
        let data = self.post(&self.endpoint("browse"), body).await?;
        let episodes = parse_podcast_episodes(&data);
        let title = parse_header_title(&data).unwrap_or_default();
        let cover = parse_header_thumbnail(&data);
        Ok((episodes, title, cover))
    }

    pub async fn get_playlist_details(
        &self,
        browse_id: &str,
    ) -> MhResult<(Vec<YtMusicResult>, String, Option<String>)> {
        let bid = if browse_id.starts_with("VL") {
            browse_id.to_string()
        } else {
            format!("VL{}", browse_id)
        };

        let body = json!({
            "context":  self.context(),
            "browseId": bid,
        });

        let data = self.post(&self.endpoint("browse"), body).await?;
        let tracks = parse_playlist_tracks(&data);
        let title = parse_header_title(&data).unwrap_or_default();
        let cover = parse_header_thumbnail(&data);
        Ok((tracks, title, cover))
    }

    pub async fn fetch_lyrics(&self, video_id: &str) -> Option<String> {
        let next_body = json!({
            "context": self.context(),
            "videoId": video_id
        });
        let next_data = self.post(&self.endpoint("next"), next_body).await.ok()?;

        let browse_id = extract_lyrics_browse_id(&next_data)?;

        let browse_body = json!({
            "context": self.context(),
            "browseId": browse_id
        });
        let browse_data = self.post(&self.endpoint("browse"), browse_body).await.ok()?;

        extract_lyrics_text(&browse_data)
    }
}

struct InnertubeConfig {
    api_key: String,
    client_name: String,
    client_version: String,
    api_version: String,
}

async fn scrape_innertube_config(client: &Client) -> MhResult<InnertubeConfig> {
    let resp = client
        .get(YTM_HOME)
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .await
        .map_err(MhError::Network)?;

    let html = resp.text().await.map_err(MhError::Network)?;

    fn pick(html: &str, key: &str) -> String {
        let needle = format!("\"{}\":", key);
        if let Some(pos) = html.find(&needle) {
            let rest = &html[pos + needle.len()..].trim_start();
            if rest.starts_with('"') {
                let inner = &rest[1..];
                if let Some(end) = inner.find('"') {
                    return inner[..end].to_string();
                }
            }
        }
        String::new()
    }

    Ok(InnertubeConfig {
        api_key: pick(&html, "INNERTUBE_API_KEY"),
        client_name: pick(&html, "INNERTUBE_CLIENT_NAME"),
        client_version: pick(&html, "INNERTUBE_CLIENT_VERSION"),
        api_version: pick(&html, "INNERTUBE_API_VERSION"),
    })
}

fn find_all<'a>(obj: &'a Value, key: &str, results: &mut Vec<&'a Value>, depth: usize) {
    if depth > 20 {
        return;
    }
    match obj {
        Value::Object(map) => {
            if let Some(v) = map.get(key) {
                results.push(v);
            }
            for v in map.values() {
                find_all(v, key, results, depth + 1);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                find_all(v, key, results, depth + 1);
            }
        }
        _ => {}
    }
}

fn collect_ids(obj: &Value, browse_ids: &mut Vec<String>, video_ids: &mut Vec<String>, depth: usize) {
    if depth > 20 {
        return;
    }
    match obj {
        Value::Object(map) => {
            if let Some(Value::String(bid)) = map.get("browseId") {
                browse_ids.push(bid.clone());
            }
            if let Some(Value::String(vid)) = map.get("videoId") {
                video_ids.push(vid.clone());
            }
            for v in map.values() {
                collect_ids(v, browse_ids, video_ids, depth + 1);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                collect_ids(v, browse_ids, video_ids, depth + 1);
            }
        }
        _ => {}
    }
}

pub async fn search(
    client: &YtMusicClient,
    query: &str,
    filter: YtMusicFilter,
) -> MhResult<Vec<YtMusicResult>> {
    client.search(query, filter).await
}

fn extract_texts(obj: &Value, texts: &mut Vec<String>, depth: usize) {
    if depth > 20 {
        return;
    }
    match obj {
        Value::Object(map) => {
            if let Some(Value::Array(runs)) = map.get("runs") {
                for r in runs {
                    if let Some(t) = r["text"].as_str() {
                        texts.push(t.to_string());
                    }
                }
            }
            for v in map.values() {
                extract_texts(v, texts, depth + 1);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                extract_texts(v, texts, depth + 1);
            }
        }
        _ => {}
    }
}

fn find_thumbnails<'a>(obj: &'a Value, depth: usize) -> Option<&'a Vec<Value>> {
    if depth > 20 {
        return None;
    }
    match obj {
        Value::Object(map) => {
            if let Some(Value::Array(thumbs)) = map.get("thumbnails") {
                return Some(thumbs);
            }
            for v in map.values() {
                if let Some(found) = find_thumbnails(v, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(arr) => {
            for v in arr {
                if let Some(found) = find_thumbnails(v, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn best_thumbnail(obj: &Value) -> Option<String> {
    find_thumbnails(obj, 0)
        .and_then(|thumbs| thumbs.last())
        .and_then(|t| t["url"].as_str())
        .map(|s| s.to_string())
}

fn parse_title_from_item(item: &Value) -> String {
    if let Some(flex) = item["flexColumns"].as_array() {
        if let Some(runs) = flex
            .first()
            .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
        {
            let t: String = runs.iter().filter_map(|r| r["text"].as_str()).collect();
            if !t.is_empty() {
                return t;
            }
        }
    }
    if let Some(runs) = item["title"]["runs"].as_array() {
        let t: String = runs.iter().filter_map(|r| r["text"].as_str()).collect();
        if !t.is_empty() {
            return t;
        }
    }
    let mut texts = Vec::new();
    extract_texts(item, &mut texts, 0);
    texts.into_iter().next().unwrap_or_default()
}

fn parse_header_title(data: &Value) -> Option<String> {
    let header = &data["header"];
    for renderer in &[
        "musicDetailHeaderRenderer",
        "musicImmersiveHeaderRenderer",
        "musicEditablePlaylistDetailHeaderRenderer",
    ] {
        if let Some(runs) = header[renderer]["title"]["runs"].as_array() {
            let t: String = runs.iter().filter_map(|r| r["text"].as_str()).collect();
            if !t.is_empty() {
                return Some(t);
            }
        }
    }
    None
}

fn parse_header_thumbnail(data: &Value) -> Option<String> {
    let header = &data["header"];

    macro_rules! try_path {
        ($($key:expr),+) => {{
            let arr = &header$([$key])+;
            if let Some(thumbs) = arr.as_array() {
                if let Some(url) = thumbs.last().and_then(|t| t["url"].as_str()) {
                    return Some(url.to_string());
                }
            }
        }};
    }

    try_path!("musicDetailHeaderRenderer", "thumbnail", "croppedSquareThumbnailRenderer", "thumbnail", "thumbnails");
    try_path!("musicDetailHeaderRenderer", "thumbnail", "musicThumbnailRenderer", "thumbnail", "thumbnails");
    try_path!("musicImmersiveHeaderRenderer", "thumbnail", "musicThumbnailRenderer", "thumbnail", "thumbnails");
    try_path!("musicEditablePlaylistDetailHeaderRenderer", "header", "musicDetailHeaderRenderer", "thumbnail", "croppedSquareThumbnailRenderer", "thumbnail", "thumbnails");

    find_thumbnails(header, 0)
        .and_then(|t| t.last())
        .and_then(|t| t["url"].as_str())
        .map(String::from)
}

fn find_video_id(item: &Value) -> Option<String> {
    if let Some(id) = item["playlistItemData"]["videoId"].as_str() {
        return Some(id.to_string());
    }
    if let Some(id) = item["overlay"]["musicItemThumbnailOverlayRenderer"]
        ["content"]["musicPlayButtonRenderer"]
        ["playNavigationEndpoint"]["watchEndpoint"]["videoId"].as_str()
    {
        return Some(id.to_string());
    }
    if let Some(flex) = item["flexColumns"].as_array() {
        for col in flex {
            if let Some(runs) = col["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array() {
                for run in runs {
                    if let Some(id) = run["navigationEndpoint"]["watchEndpoint"]["videoId"].as_str() {
                        return Some(id.to_string());
                    }
                }
            }
        }
    }
    let mut browse_ids = Vec::new();
    let mut video_ids = Vec::new();
    collect_ids(item, &mut browse_ids, &mut video_ids, 0);
    video_ids.into_iter().next()
}

fn parse_duration_str(s: &str) -> u32 {
    let parts: Vec<u32> = s
        .split(':')
        .rev()
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    parts.iter().enumerate().map(|(i, &v)| v * 60u32.pow(i as u32)).sum()
}

fn parse_search_results(data: &Value, filter: YtMusicFilter) -> Vec<YtMusicResult> {
    let mut renderers = Vec::new();
    find_all(data, "musicResponsiveListItemRenderer", &mut renderers, 0);

    renderers
        .iter()
        .filter_map(|item| parse_responsive_item(item, filter))
        .collect()
}

fn parse_responsive_item(item: &Value, filter: YtMusicFilter) -> Option<YtMusicResult> {
    let flex = item["flexColumns"].as_array()?;

    let col0_runs: Option<&[Value]> = flex
        .first()
        .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
        .map(|v| v.as_slice());
    let col1_runs: Option<&[Value]> = flex
        .get(1)
        .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
        .map(|v| v.as_slice());
    let col2_runs: Option<&[Value]> = flex
        .get(2)
        .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
        .map(|v| v.as_slice());

    let title = runs_text(col0_runs);

    let fixed_runs: Option<&[Value]> = item["fixedColumns"]
        .as_array()
        .and_then(|fc| fc.first())
        .and_then(|c| c["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"].as_array())
        .map(|v| v.as_slice());
    let duration_str = runs_text(fixed_runs);
    let duration_secs = parse_duration_str(&duration_str);

    let thumbnail_url = best_thumbnail(item);

    let mut browse_ids = Vec::new();
    let mut video_ids = Vec::new();
    collect_ids(item, &mut browse_ids, &mut video_ids, 0);

    let result_type = match filter {
        YtMusicFilter::Song => YtMusicResultType::Song,
        YtMusicFilter::Video => YtMusicResultType::Video,
        YtMusicFilter::Album => YtMusicResultType::Album,
        YtMusicFilter::Playlist => YtMusicResultType::Playlist,
        YtMusicFilter::Artist => YtMusicResultType::Artist,
        YtMusicFilter::Podcast => YtMusicResultType::Podcast,
    };

    match filter {
        YtMusicFilter::Song | YtMusicFilter::Video => {
            let id = video_ids.into_iter().next()?;
            let artist = runs_text_option(col1_runs);
            let album = runs_text_option(col2_runs);
            Some(YtMusicResult {
                id,
                title,
                artist,
                album,
                duration_secs,
                thumbnail_url,
                result_type,
                browse_id: None,
            })
        }
        YtMusicFilter::Album => {
            let browse_id = browse_ids
                .iter()
                .find(|id| id.starts_with("MPRE"))
                .or_else(|| browse_ids.first())
                .cloned()?;
            let artist = runs_text_option(col1_runs);
            Some(YtMusicResult {
                id: browse_id.clone(),
                title,
                artist,
                album: None,
                duration_secs: 0,
                thumbnail_url,
                result_type,
                browse_id: Some(browse_id),
            })
        }
        YtMusicFilter::Playlist => {
            let playlist_browse_id = browse_ids
                .iter()
                .find(|id| id.starts_with("VL"))
                .or_else(|| browse_ids.first())
                .cloned()?;
            let list_id = if playlist_browse_id.starts_with("VL") {
                playlist_browse_id[2..].to_string()
            } else {
                playlist_browse_id.clone()
            };
            let owner = runs_text_option(col1_runs);
            Some(YtMusicResult {
                id: list_id,
                title,
                artist: owner,
                album: None,
                duration_secs: 0,
                thumbnail_url,
                result_type,
                browse_id: Some(playlist_browse_id),
            })
        }
        YtMusicFilter::Artist => {
            let browse_id = browse_ids.into_iter().next()?;
            Some(YtMusicResult {
                id: browse_id.clone(),
                title,
                artist: None,
                album: None,
                duration_secs: 0,
                thumbnail_url,
                result_type,
                browse_id: Some(browse_id),
            })
        }
        YtMusicFilter::Podcast => None, // handled by parse_podcast_results
    }
}

fn runs_text(runs: Option<&[Value]>) -> String {
    runs.map(|r| {
        r.iter()
            .filter_map(|run| run["text"].as_str())
            .collect::<String>()
    })
    .unwrap_or_default()
}

fn runs_text_option(runs: Option<&[Value]>) -> Option<String> {
    let s = runs_text(runs);
    if s.is_empty() { None } else { Some(s) }
}

fn parse_artist_page_albums(data: &Value) -> Vec<serde_json::Value> {
    let mut two_row_items = Vec::new();
    find_all(data, "musicTwoRowItemRenderer", &mut two_row_items, 0);

    two_row_items
        .iter()
        .filter_map(|item| {
            let mut browse_ids = Vec::new();
            let mut video_ids = Vec::new();
            collect_ids(item, &mut browse_ids, &mut video_ids, 0);

            let browse_id = browse_ids
                .into_iter()
                .find(|id| id.starts_with("MPRE"))?;

            let title = item["title"]["runs"]
                .as_array()
                .map(|runs| {
                    runs.iter()
                        .filter_map(|r| r["text"].as_str())
                        .collect::<String>()
                })
                .filter(|s| !s.is_empty())?;

            let thumbnail = best_thumbnail(item);

            let year = item["subtitle"]["runs"]
                .as_array()
                .and_then(|runs| runs.first())
                .and_then(|r| r["text"].as_str())
                .map(|s| s.to_string());

            Some(serde_json::json!({
                "id": browse_id,
                "title": title,
                "thumbnail": thumbnail,
                "releaseDate": year,
                "url": format!("https://music.youtube.com/browse/{}", browse_id),
            }))
        })
        .collect()
}

fn parse_podcast_results(data: &Value) -> Vec<YtMusicResult> {
    let mut renderers = Vec::new();
    find_all(data, "musicResponsiveListItemRenderer", &mut renderers, 0);
    find_all(data, "musicTwoRowItemRenderer", &mut renderers, 0);

    renderers
        .iter()
        .filter_map(|item| {
            let mut browse_ids = Vec::new();
            let mut video_ids = Vec::new();
            collect_ids(item, &mut browse_ids, &mut video_ids, 0);

            let browse_id = browse_ids
                .iter()
                .find(|id| id.starts_with("MPSP"))
                .cloned()?;

            let title = parse_title_from_item(item);
            if title.is_empty() {
                return None;
            }

            let thumbnail_url = best_thumbnail(item);

            Some(YtMusicResult {
                id: browse_id.clone(),
                title,
                artist: None,
                album: None,
                duration_secs: 0,
                thumbnail_url,
                result_type: YtMusicResultType::Podcast,
                browse_id: Some(browse_id),
            })
        })
        .collect()
}

fn parse_album_tracks(data: &Value) -> Vec<YtMusicResult> {
    let mut shelves = Vec::new();
    find_all(data, "musicShelfRenderer", &mut shelves, 0);

    let targeted: Vec<&Value> = shelves
        .first()
        .and_then(|shelf| shelf["contents"].as_array())
        .map(|arr| arr.iter().filter_map(|c| c.get("musicResponsiveListItemRenderer")).collect())
        .unwrap_or_default();

    let items: Vec<&Value> = if !targeted.is_empty() {
        targeted
    } else {
        let mut renderers = Vec::new();
        find_all(data, "musicResponsiveListItemRenderer", &mut renderers, 0);
        renderers
    };

    items
        .iter()
        .filter_map(|item| {
            let flex = item["flexColumns"].as_array()?;
            let col0_runs: Option<&[Value]> = flex
                .first()
                .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
                .map(|v| v.as_slice());
            let col1_runs: Option<&[Value]> = flex.get(1).and_then(|c| {
                c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array()
            }).map(|v| v.as_slice());

            let title = runs_text(col0_runs);
            let artist = runs_text_option(col1_runs);

            let id = find_video_id(item)?;
            let thumbnail_url = best_thumbnail(item);

            let fixed_runs: Option<&[Value]> = item["fixedColumns"]
                .as_array()
                .and_then(|fc| fc.first())
                .and_then(|c| c["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"].as_array())
                .map(|v| v.as_slice());
            let dur = runs_text(fixed_runs);
            let duration_secs = parse_duration_str(&dur);

            Some(YtMusicResult {
                id,
                title,
                artist,
                album: None,
                duration_secs,
                thumbnail_url,
                result_type: YtMusicResultType::Song,
                browse_id: None,
            })
        })
        .collect()
}

fn parse_podcast_episodes(data: &Value) -> Vec<YtMusicResult> {
    let mut responsive = Vec::new();
    let mut multirow = Vec::new();
    find_all(data, "musicResponsiveListItemRenderer", &mut responsive, 0);
    find_all(data, "musicMultiRowListItemRenderer", &mut multirow, 0);

    let all: Vec<&Value> = responsive.iter().chain(multirow.iter()).copied().collect();
    let mut results = Vec::new();

    for item in all {
        let mut browse_ids = Vec::new();
        let mut video_ids = Vec::new();
        collect_ids(item, &mut browse_ids, &mut video_ids, 0);

        let id = match video_ids.into_iter().next().or_else(|| browse_ids.into_iter().next()) {
            Some(id) => id,
            None => continue,
        };

        let title = if let Some(flex) = item["flexColumns"].as_array() {
            let col0_runs: Option<&[Value]> = flex
                .first()
                .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
                .map(|v| v.as_slice());
            let t = runs_text(col0_runs);
            if !t.is_empty() { t } else {
                let mut texts = Vec::new();
                extract_texts(item, &mut texts, 0);
                texts.into_iter().next().unwrap_or_default()
            }
        } else {
            let title_text = item["title"]["runs"][0]["text"].as_str()
                .or_else(|| item["subtitle"]["runs"][0]["text"].as_str())
                .unwrap_or("");
            if !title_text.is_empty() {
                title_text.to_string()
            } else {
                let mut texts = Vec::new();
                extract_texts(item, &mut texts, 0);
                texts.into_iter().next().unwrap_or_default()
            }
        };

        if title.is_empty() {
            continue;
        }

        let thumbnail_url = best_thumbnail(item);

        results.push(YtMusicResult {
            id,
            title,
            artist: None,
            album: None,
            duration_secs: 0,
            thumbnail_url,
            result_type: YtMusicResultType::Episode,
            browse_id: None,
        });
    }

    results
}

fn parse_playlist_tracks(data: &Value) -> Vec<YtMusicResult> {
    let mut playlist_shelves = Vec::new();
    find_all(data, "musicPlaylistShelfRenderer", &mut playlist_shelves, 0);

    let targeted: Vec<&Value> = playlist_shelves
        .first()
        .and_then(|shelf| shelf["contents"].as_array())
        .map(|arr| arr.iter().filter_map(|c| c.get("musicResponsiveListItemRenderer")).collect())
        .unwrap_or_default();

    let items: Vec<&Value> = if !targeted.is_empty() {
        targeted
    } else {
        let mut renderers = Vec::new();
        find_all(data, "musicResponsiveListItemRenderer", &mut renderers, 0);
        renderers
    };

    items
        .iter()
        .filter_map(|item| {
            let video_id = find_video_id(item)?;

            let flex = item["flexColumns"].as_array()?;
            let col0_runs: Option<&[Value]> = flex
                .first()
                .and_then(|c| c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array())
                .map(|v| v.as_slice());
            let col1_runs: Option<&[Value]> = flex.get(1).and_then(|c| {
                c["musicResponsiveListItemFlexColumnRenderer"]["text"]["runs"].as_array()
            }).map(|v| v.as_slice());

            let title = runs_text(col0_runs);
            let artist = runs_text_option(col1_runs);

            let fixed_runs: Option<&[Value]> = item["fixedColumns"]
                .as_array()
                .and_then(|fc| fc.first())
                .and_then(|c| c["musicResponsiveListItemFixedColumnRenderer"]["text"]["runs"].as_array())
                .map(|v| v.as_slice());
            let dur = runs_text(fixed_runs);
            let duration_secs = parse_duration_str(&dur);

            let thumbnail_url = best_thumbnail(item);

            Some(YtMusicResult {
                id: video_id,
                title,
                artist,
                album: None,
                duration_secs,
                thumbnail_url,
                result_type: YtMusicResultType::Song,
                browse_id: None,
            })
        })
        .collect()
}

fn extract_lyrics_browse_id(next_data: &Value) -> Option<String> {
    let tabs = next_data["contents"]["singleColumnMusicWatchNextResultsRenderer"]
        ["tabbedRenderer"]["watchNextTabbedResultsRenderer"]["tabs"]
        .as_array()?;

    for tab in tabs {
        let tab_title = tab["tabRenderer"]["title"].as_str().unwrap_or("");
        if tab_title.eq_ignore_ascii_case("lyrics") {
            let browse_id = tab["tabRenderer"]["endpoint"]["browseEndpoint"]["browseId"].as_str()?;
            return Some(browse_id.to_string());
        }
    }

    for tab in tabs {
        let endpoint = &tab["tabRenderer"]["endpoint"];
        let browse_id = endpoint["browseEndpoint"]["browseId"].as_str();
        if let Some(id) = browse_id {
            if id.starts_with("MPLA") || id.contains("lyrics") {
                return Some(id.to_string());
            }
        }
    }

    None
}

fn extract_lyrics_text(browse_data: &Value) -> Option<String> {
    let sections = browse_data["contents"]["sectionListRenderer"]["contents"].as_array()?;

    for section in sections {
        if let Some(desc) = section["musicDescriptionShelfRenderer"]["description"]["runs"]
            .as_array()
        {
            let text: String = desc
                .iter()
                .filter_map(|r| r["text"].as_str())
                .collect::<Vec<_>>()
                .join("");
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
    }

    None
}
