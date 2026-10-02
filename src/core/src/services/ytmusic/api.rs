use crate::services::common::library::str_at;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::errors::{MhError, MhResult};
use crate::http_client::build_mozilla_client;
use crate::services::common::http::read_json;

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
#[serde(rename_all = "camelCase")]
pub struct YtMusicResult {
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_secs: u32,
    pub thumbnail_url: Option<String>,
    pub result_type: YtMusicResultType,
    pub browse_id: Option<String>,
    #[serde(default)]
    pub artist_id: Option<String>,
    #[serde(default)]
    pub album_id: Option<String>,
}

fn extract_artist_album_ids(cols: &[Option<&[Value]>]) -> (Option<String>, Option<String>) {
    let mut artist_id = None;
    let mut album_id = None;
    for runs in cols.iter().flatten() {
        for run in runs.iter() {
            if let Some(bid) = run["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str() {
                if bid.starts_with("UC") && artist_id.is_none() {
                    artist_id = Some(bid.to_string());
                } else if bid.starts_with("MPRE") && album_id.is_none() {
                    album_id = Some(bid.to_string());
                }
            }
        }
    }
    (artist_id, album_id)
}

fn extract_ids_from_menu(item: &Value) -> (Option<String>, Option<String>) {
    let mut artist_id = None;
    let mut album_id = None;
    let Some(items) = item["menu"]["menuRenderer"]["items"].as_array() else {
        return (artist_id, album_id);
    };
    for mi in items {
        if let Some(bid) = mi["menuNavigationItemRenderer"]["navigationEndpoint"]["browseEndpoint"]
            ["browseId"]
            .as_str()
        {
            if bid.starts_with("UC") && artist_id.is_none() {
                artist_id = Some(bid.to_string());
            } else if bid.starts_with("MPRE") && album_id.is_none() {
                album_id = Some(bid.to_string());
            }
        }
    }
    (artist_id, album_id)
}

fn artist_album_ids(item: &Value, cols: &[Option<&[Value]>]) -> (Option<String>, Option<String>) {
    let (m_artist, m_album) = extract_ids_from_menu(item);
    let (f_artist, f_album) = extract_artist_album_ids(cols);
    (m_artist.or(f_artist), m_album.or(f_album))
}

fn run_nav_browse_id(run: &Value) -> Option<&str> {
    run["navigationEndpoint"]["browseEndpoint"]["browseId"].as_str()
}

fn is_separator_run(run: &Value) -> bool {
    let t = run["text"].as_str().unwrap_or("");
    let trimmed = t.trim();
    trimmed.is_empty()
        || trimmed == "•"
        || trimmed
            .chars()
            .all(|c| c.is_ascii_digit() || c == ':' || c == '.')
}

fn clean_artist_text(cols: &[Option<&[Value]>]) -> Option<String> {
    for runs in cols.iter().flatten() {
        for run in runs.iter() {
            if run_nav_browse_id(run)
                .map(|b| b.starts_with("UC"))
                .unwrap_or(false)
            {
                if let Some(t) = run["text"].as_str() {
                    if !t.trim().is_empty() {
                        return Some(t.to_string());
                    }
                }
            }
        }
    }
    for runs in cols.iter().flatten() {
        for run in runs.iter() {
            if !is_separator_run(run) {
                if let Some(t) = run["text"].as_str() {
                    return Some(t.to_string());
                }
            }
        }
    }
    None
}

fn clean_album_text(cols: &[Option<&[Value]>]) -> Option<String> {
    for runs in cols.iter().flatten() {
        for run in runs.iter() {
            if run_nav_browse_id(run)
                .map(|b| b.starts_with("MPRE"))
                .unwrap_or(false)
            {
                if let Some(t) = run["text"].as_str() {
                    if !t.trim().is_empty() {
                        return Some(t.to_string());
                    }
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
pub struct YtMusicClient {
    pub client_name: String,
    pub client_version: String,
    pub api_key: String,
    pub api_version: String,
    http: Client,
}

static SHARED_CLIENT: tokio::sync::OnceCell<YtMusicClient> = tokio::sync::OnceCell::const_new();

impl YtMusicClient {
    /// Cached across calls: `init` scrapes the whole YT Music homepage, which the
    /// search-as-you-type path would otherwise repeat on every keystroke batch.
    pub async fn shared() -> MhResult<&'static YtMusicClient> {
        SHARED_CLIENT.get_or_try_init(Self::init).await
    }

    pub async fn init() -> MhResult<Self> {
        let http = build_mozilla_client()?;

        let mut api_key = String::new();
        let mut client_name = FALLBACK_CLIENT_NAME.to_string();
        let mut client_version = FALLBACK_CLIENT_VERSION.to_string();
        let mut api_version = FALLBACK_API_VERSION.to_string();

        if let Ok(cfg) = scrape_innertube_config(&http).await {
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

    fn android_context() -> Value {
        json!({
            "client": {
                "clientName":    "ANDROID_MUSIC",
                "clientVersion": "7.21.50",
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

        read_json("YT Music", resp).await
    }

    pub async fn search(&self, query: &str, filter: YtMusicFilter) -> MhResult<Vec<YtMusicResult>> {
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

    pub async fn get_search_suggestions(&self, query: &str) -> MhResult<Vec<String>> {
        let body = json!({
            "context": self.context(),
            "input":   query,
        });
        let data = self
            .post(&self.endpoint("music/get_search_suggestions"), body)
            .await?;
        let mut out = Vec::new();
        let mut seen = std::collections::HashSet::new();
        collect_suggestion_texts(&data, &mut out, &mut seen);
        out.truncate(crate::services::common::limits::SUGGESTIONS_LIMIT);
        Ok(out)
    }

    pub async fn fetch_lyrics(&self, video_id: &str) -> Option<(Option<String>, Option<String>)> {
        let next_body = json!({
            "context": self.context(),
            "videoId": video_id
        });
        let next_data = self.post(&self.endpoint("next"), next_body).await.ok()?;

        let browse_id = extract_lyrics_browse_id(&next_data)?;

        let android_body = json!({
            "context": Self::android_context(),
            "browseId": browse_id,
        });
        if let Ok(android_data) = self.post(&self.endpoint("browse"), android_body).await {
            let synced = extract_timed_lyrics(&android_data);
            let plain_android = extract_lyrics_text(&android_data);
            if synced.is_some() || plain_android.is_some() {
                return Some((synced, plain_android));
            }
        }

        let browse_body = json!({
            "context": self.context(),
            "browseId": browse_id,
        });
        let browse_data = self
            .post(&self.endpoint("browse"), browse_body)
            .await
            .ok()?;
        let plain = extract_lyrics_text(&browse_data);
        if plain.is_some() {
            Some((None, plain))
        } else {
            None
        }
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
            if let Some(inner) = rest.strip_prefix('"') {
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

fn collect_ids(
    obj: &Value,
    browse_ids: &mut Vec<String>,
    video_ids: &mut Vec<String>,
    depth: usize,
) {
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

fn find_thumbnails(obj: &Value, depth: usize) -> Option<&Vec<Value>> {
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

fn parse_duration_str(s: &str) -> u32 {
    s.split(':')
        .rev()
        .take(3)
        .filter_map(|p| p.trim().parse::<u32>().ok())
        .enumerate()
        .map(|(i, v)| v.saturating_mul(60u32.saturating_pow(i as u32)))
        .fold(0u32, |acc, v| acc.saturating_add(v))
}

fn is_clock_string(s: &str) -> bool {
    let t = s.trim();
    !t.is_empty()
        && t.contains(':')
        && t.split(':')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn duration_from_runs(cols: &[Option<&[Value]>]) -> u32 {
    for runs in cols.iter().flatten() {
        for run in runs.iter() {
            if let Some(t) = run["text"].as_str() {
                if is_clock_string(t) {
                    return parse_duration_str(t);
                }
            }
        }
    }
    0
}

fn collect_suggestion_texts(
    v: &Value,
    out: &mut Vec<String>,
    seen: &mut std::collections::HashSet<String>,
) {
    match v {
        Value::Object(m) => {
            if let Some(sug) = m.get("searchSuggestionRenderer") {
                if let Some(runs) = sug.pointer("/suggestion/runs").and_then(|r| r.as_array()) {
                    let text: String = runs.iter().filter_map(|r| str_at(r, &["text"])).collect();
                    let trimmed = text.trim();
                    if !trimmed.is_empty() && seen.insert(trimmed.to_lowercase()) {
                        out.push(trimmed.to_string());
                    }
                }
            }
            for child in m.values() {
                collect_suggestion_texts(child, out, seen);
            }
        }
        Value::Array(a) => {
            for child in a {
                collect_suggestion_texts(child, out, seen);
            }
        }
        _ => {}
    }
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
    let mut duration_secs = parse_duration_str(&duration_str);
    if duration_secs == 0 {
        duration_secs = duration_from_runs(&[col1_runs, col2_runs]);
    }

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
            let artist =
                clean_artist_text(&[col1_runs, col2_runs]).or_else(|| runs_text_option(col1_runs));
            let album = clean_album_text(&[col1_runs, col2_runs]);
            let (artist_id, album_id) = artist_album_ids(item, &[col1_runs, col2_runs]);
            Some(YtMusicResult {
                id,
                title,
                artist,
                album,
                duration_secs,
                thumbnail_url,
                result_type,
                browse_id: None,
                artist_id,
                album_id,
            })
        }
        YtMusicFilter::Album => {
            let browse_id = browse_ids
                .iter()
                .find(|id| id.starts_with("MPRE"))
                .or_else(|| browse_ids.first())
                .cloned()?;
            let artist =
                clean_artist_text(&[col1_runs, col2_runs]).or_else(|| runs_text_option(col1_runs));
            let (artist_id, _) = extract_artist_album_ids(&[col1_runs]);
            Some(YtMusicResult {
                id: browse_id.clone(),
                title,
                artist,
                album: None,
                duration_secs: 0,
                thumbnail_url,
                result_type,
                browse_id: Some(browse_id.clone()),
                artist_id,
                album_id: Some(browse_id),
            })
        }
        YtMusicFilter::Playlist => {
            let playlist_browse_id = browse_ids
                .iter()
                .find(|id| id.starts_with("VL"))
                .or_else(|| browse_ids.first())
                .cloned()?;
            let list_id = playlist_browse_id
                .strip_prefix("VL")
                .map(str::to_string)
                .unwrap_or_else(|| playlist_browse_id.clone());
            let owner = clean_artist_text(&[col1_runs]).or_else(|| runs_text_option(col1_runs));
            Some(YtMusicResult {
                id: list_id,
                title,
                artist: owner,
                album: None,
                duration_secs: 0,
                thumbnail_url,
                result_type,
                browse_id: Some(playlist_browse_id),
                artist_id: None,
                album_id: None,
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
                browse_id: Some(browse_id.clone()),
                artist_id: Some(browse_id),
                album_id: None,
            })
        }
        YtMusicFilter::Podcast => None,
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
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
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
                artist_id: None,
                album_id: None,
            })
        })
        .collect()
}

fn extract_lyrics_browse_id(next_data: &Value) -> Option<String> {
    let tabs = next_data["contents"]["singleColumnMusicWatchNextResultsRenderer"]["tabbedRenderer"]
        ["watchNextTabbedResultsRenderer"]["tabs"]
        .as_array()?;

    for tab in tabs {
        let tab_title = tab["tabRenderer"]["title"].as_str().unwrap_or("");
        if tab_title.eq_ignore_ascii_case("lyrics") {
            let browse_id =
                tab["tabRenderer"]["endpoint"]["browseEndpoint"]["browseId"].as_str()?;
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

fn extract_timed_lyrics(browse_data: &Value) -> Option<String> {
    fn find_timed_lyrics_data(v: &Value, depth: usize) -> Option<&Vec<Value>> {
        if depth > 32 {
            return None;
        }
        match v {
            Value::Object(map) => {
                if let Some(arr) = map.get("timedLyricsData").and_then(|x| x.as_array()) {
                    return Some(arr);
                }
                for child in map.values() {
                    if let Some(found) = find_timed_lyrics_data(child, depth + 1) {
                        return Some(found);
                    }
                }
                None
            }
            Value::Array(arr) => {
                for child in arr {
                    if let Some(found) = find_timed_lyrics_data(child, depth + 1) {
                        return Some(found);
                    }
                }
                None
            }
            _ => None,
        }
    }

    let entries = find_timed_lyrics_data(browse_data, 0)?;
    if entries.is_empty() {
        return None;
    }

    let raw: Vec<(u64, u64, String)> = entries
        .iter()
        .map(|entry| {
            let text = entry["lyricLine"]
                .as_str()
                .or_else(|| entry["line"].as_str())
                .unwrap_or("")
                .to_string();
            let start = parse_cue_ms(&entry["cueRange"]["startTimeMilliseconds"])
                .or_else(|| parse_cue_ms(&entry["startTimeMs"]))
                .unwrap_or(0);
            let end = parse_cue_ms(&entry["cueRange"]["endTimeMilliseconds"])
                .or_else(|| parse_cue_ms(&entry["endTimeMs"]))
                .unwrap_or(start);
            (start, end, text)
        })
        .collect();

    const GAP_THRESHOLD_MS: u64 = 1_500;
    let mut lines: Vec<String> = Vec::with_capacity(raw.len() * 2);
    for (i, (start, end, text)) in raw.iter().enumerate() {
        lines.push(format!("{}{}", ms_to_lrc_ts(*start), text));
        let next_start = raw.get(i + 1).map(|n| n.0).unwrap_or(*end);
        if next_start > *end && next_start - *end >= GAP_THRESHOLD_MS {
            lines.push(format!("{}♪", ms_to_lrc_ts(*end)));
        }
    }
    Some(lines.join("\n"))
}

fn parse_cue_ms(v: &Value) -> Option<u64> {
    v.as_str()
        .and_then(|s| s.parse::<u64>().ok())
        .or_else(|| v.as_u64())
        .or_else(|| v.as_f64().map(|f| f as u64))
}

fn ms_to_lrc_ts(ms: u64) -> String {
    let total_cs = ms / 10;
    let cs = total_cs % 100;
    let total_s = total_cs / 100;
    let s = total_s % 60;
    let m = total_s / 60;
    format!("[{:02}:{:02}.{:02}]", m, s, cs)
}

fn extract_lyrics_text(browse_data: &Value) -> Option<String> {
    let sections = browse_data["contents"]["sectionListRenderer"]["contents"].as_array()?;

    for section in sections {
        if let Some(desc) =
            section["musicDescriptionShelfRenderer"]["description"]["runs"].as_array()
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
