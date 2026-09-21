use crate::services::common::library::str_at;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::Client;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::media::library::{
    LibraryAlbumDetail, LibraryAlbumDto, LibraryArtistDetail, LibraryArtistDto,
    LibraryPlaylistDetail, LibraryPlaylistDto, LibraryTrackDto,
};

use crate::services::common::library::{
    cover_id, f64_at, i64_at, id_at, nonempty_at, stable_hash_i64, string_at, u32_at, year4_at,
    FavKind, FavOp, MutationCapabilities, OwnedPlaylistRow, Page, PlaylistCreateInput,
    PlaylistMutateResult, RadioResult, RadioSeedKind, RecommendationCategory, RecommendationItem,
    RecommendationShelf, RecommendationsPage, SaveKind, SavedIdSet, ServiceCapabilities,
    ServiceLibrary, ServiceLibraryMutations, ServiceLibraryPage, ServicePlatform,
};

const GW: &str = "https://www.deezer.com/ajax/gw-light.php";
const UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0 Safari/537.36";

const PIPE_GQL: &str = "https://pipe.deezer.com/api";
const AUTH_ARL: &str = "https://auth.deezer.com/login/arl";
const DEV_VERSION: &str = "10020260716135120";

pub struct DeezerLibrary {
    client: Client,
    arl: String,
    session: RwLock<Option<Session>>,
    jwt: RwLock<Option<Jwt>>,
    settings: Option<Arc<RwLock<Settings>>>,
}

#[derive(Clone, Default)]
struct Session {
    sid: Option<String>,
    api_token: String,
    user_id: String,
}

#[derive(Clone)]
struct Jwt {
    token: String,
    expires_at: i64,
}

impl DeezerLibrary {
    pub fn from_settings(s: &Settings) -> MhResult<Self> {
        Self::build(s, None)
    }

    pub fn from_state(state: &crate::BackendState, s: &Settings) -> MhResult<Self> {
        Self::build(s, Some(state.settings.clone()))
    }

    fn build(s: &Settings, settings: Option<Arc<RwLock<Settings>>>) -> MhResult<Self> {
        if s.deezer_arl.is_empty() {
            return Err(MhError::Auth("Deezer not connected (no ARL)".into()));
        }
        let client = crate::http_client::ua_client(UA)?;
        Ok(Self {
            client,
            arl: s.deezer_arl.trim().to_string(),
            session: RwLock::new(None),
            jwt: RwLock::new(None),
            settings,
        })
    }

    fn cookie_header(&self, sid: Option<&str>) -> String {
        let mut c = format!("arl={}", self.arl);
        if let Some(s) = sid {
            if !s.is_empty() {
                c.push_str("; sid=");
                c.push_str(s);
            }
        }
        c
    }

    fn parse_id(key: &str, id: &str) -> MhResult<u64> {
        id.parse()
            .map_err(|_| MhError::Other(format!("Deezer: invalid {key} {id}")))
    }

    async fn id_mutation(&self, method: &str, key: &'static str, id: &str) -> MhResult<()> {
        let n = Self::parse_id(key, id)?;
        self.gw(method, &json!({ key: n })).await?;
        Ok(())
    }

    async fn id_mutations(&self, method: &str, key: &'static str, ids: &[String]) -> MhResult<()> {
        futures_util::future::try_join_all(ids.iter().map(|id| self.id_mutation(method, key, id)))
            .await?;
        Ok(())
    }

    async fn raw_gw(
        &self,
        method: &str,
        body: &Value,
        api_token: &str,
        sid: Option<&str>,
    ) -> MhResult<(Value, Option<String>)> {
        let url = format!("{GW}?method={method}&input=3&api_version=1.0&api_token={api_token}");
        let mut headers = HeaderMap::new();
        headers.insert("cookie", HeaderValue::from_str(&self.cookie_header(sid))?);
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        headers.insert("accept", HeaderValue::from_static("application/json"));
        let resp = self
            .client
            .post(&url)
            .headers(headers)
            .json(body)
            .send()
            .await
            .map_err(MhError::Network)?;
        let new_sid = resp.headers().get_all("set-cookie").iter().find_map(|h| {
            h.to_str().ok().and_then(|s| {
                if s.starts_with("sid=") {
                    s.split(';')
                        .next()
                        .and_then(|kv| kv.split('=').nth(1))
                        .map(str::to_string)
                } else {
                    None
                }
            })
        });
        let v: Value =
            crate::services::common::http::read_json(&format!("Deezer {method}"), resp).await?;
        if let Some(err) = v.get("error") {
            if !err.is_null() && !err.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                return Err(MhError::Other(format!("Deezer {method} error: {err}")));
            }
        }
        Ok((v, new_sid))
    }

    async fn ensure_session(&self) -> MhResult<Session> {
        {
            let g = self.session.read().await;
            if let Some(s) = g.as_ref() {
                if !s.user_id.is_empty() && !s.api_token.is_empty() {
                    return Ok(s.clone());
                }
            }
        }
        let (v, sid) = self
            .raw_gw("deezer.getUserData", &json!({}), "null", None)
            .await?;
        let api_token = str_at(&v, &["/results/checkForm"])
            .unwrap_or("")
            .to_string();
        let user_id = v
            .pointer("/results/USER/USER_ID")
            .and_then(|x| {
                x.as_u64()
                    .map(|u| u.to_string())
                    .or_else(|| x.as_str().map(str::to_string))
            })
            .unwrap_or_default();
        if user_id.is_empty() || user_id == "0" {
            return Err(MhError::Auth(
                "Deezer ARL is invalid or expired. Update it in Settings → Deezer → ARL Token."
                    .into(),
            ));
        }
        let s = Session {
            sid,
            api_token,
            user_id,
        };
        *self.session.write().await = Some(s.clone());
        Ok(s)
    }

    async fn gw(&self, method: &str, body: &Value) -> MhResult<Value> {
        let s = self.ensure_session().await?;
        let (v, _) = self
            .raw_gw(method, body, &s.api_token, s.sid.as_deref())
            .await?;
        Ok(v)
    }

    async fn ensure_jwt(&self) -> MhResult<String> {
        let now = chrono::Utc::now().timestamp();
        {
            let g = self.jwt.read().await;
            if let Some(j) = g.as_ref() {
                if j.expires_at - 60 > now {
                    return Ok(j.token.clone());
                }
            }
        }
        let resp = self
            .client
            .post(format!("{AUTH_ARL}?jo=p&rto=c&i=c"))
            .header("cookie", format!("arl={}", self.arl))
            .header("accept", "application/json")
            .header("origin", "https://www.deezer.com")
            .header("content-length", "0")
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let text = resp.text().await.map_err(MhError::Network)?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| MhError::Other(format!("Deezer auth/arl decode ({e}): {text}")))?;
        if !status.is_success() {
            return Err(crate::services::common::http::api_error(
                "Deezer auth/arl",
                status,
                &v.to_string(),
            ));
        }
        let token = str_at(&v, &["jwt"])
            .ok_or_else(|| MhError::Auth(format!("Deezer JWT missing in auth response: {v}")))?
            .to_string();
        let expires_at = jwt_exp(&token).unwrap_or(now + 300);
        *self.jwt.write().await = Some(Jwt {
            token: token.clone(),
            expires_at,
        });
        Ok(token)
    }

    async fn pipe_graphql(
        &self,
        operation: &str,
        query: &str,
        variables: Value,
    ) -> MhResult<Value> {
        let jwt = self.ensure_jwt().await?;
        let body = json!({
            "operationName": operation,
            "variables": variables,
            "query": query,
        });
        let resp = self
            .client
            .post(PIPE_GQL)
            .header("authorization", format!("Bearer {jwt}"))
            .header("content-type", "application/json")
            .header("accept", "*/*")
            .header("origin", "https://www.deezer.com")
            .json(&body)
            .send()
            .await
            .map_err(MhError::Network)?;
        let status = resp.status();
        let text = resp.text().await.map_err(MhError::Network)?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|e| MhError::Other(format!("Deezer pipe {operation} decode ({e}): {text}")))?;
        if !status.is_success() {
            return Err(crate::services::common::http::api_error(
                &format!("Deezer pipe {operation}"),
                status,
                &v.to_string(),
            ));
        }
        if let Some(errs) = v.get("errors") {
            if errs.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                return Err(MhError::Other(format!(
                    "Deezer pipe {operation} error: {errs}"
                )));
            }
        }
        Ok(v)
    }

    async fn profile_tab(&self, tab: &str, want_type: &str) -> Vec<Value> {
        self.try_profile_tab(tab, tab, want_type)
            .await
            .unwrap_or_default()
    }

    async fn profile_tab_keyed(&self, tab: &str, result_key: &str, want_type: &str) -> Vec<Value> {
        self.try_profile_tab(tab, result_key, want_type)
            .await
            .unwrap_or_default()
    }

    async fn try_profile_tab(
        &self,
        tab: &str,
        result_key: &str,
        want_type: &str,
    ) -> MhResult<Vec<Value>> {
        let s = self.ensure_session().await?;
        let uid: u64 = s.user_id.parse().unwrap_or(0);
        let body = self
            .gw(
                "deezer.pageProfile",
                &json!({
                    "USER_ID": uid, "tab": tab, "nb": 200,
                }),
            )
            .await?;
        Ok(body
            .pointer(&format!("/results/TAB/{result_key}/data"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| str_at(n, &["__TYPE__"]) == Some(want_type))
            .collect())
    }

    async fn show_detail(&self, show_id: &str) -> MhResult<LibraryPlaylistDetail> {
        let body = self
            .gw(
                "deezer.pageShow",
                &json!({
                    "SHOW_ID": show_id, "lang": "en", "nb": 1000, "start": 0,
                }),
            )
            .await?;
        let meta = body
            .pointer("/results/DATA")
            .cloned()
            .unwrap_or(Value::Null);
        let playlist =
            map_show(&meta).ok_or_else(|| MhError::NotFound(format!("Deezer show {show_id}")))?;
        let arr = body
            .pointer("/results/EPISODES/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = arr.iter().filter_map(map_episode).collect();
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }

    async fn maybe_fire_telemetry(&self) {
        let enabled = match &self.settings {
            Some(s) => s.read().await.telemetry_enabled(ServicePlatform::Deezer),
            None => return,
        };
        if !enabled {
            return;
        }
        let uid = match self.ensure_session().await {
            Ok(s) => s.user_id,
            Err(_) => return,
        };
        if uid.is_empty() {
            return;
        }
        tokio::spawn(async move {
            crate::services::deezer::telemetry::on_activity(&uid).await;
        });
    }

    async fn smart_tracklist_detail(&self, stl_id: &str) -> MhResult<LibraryPlaylistDetail> {
        let body = self
            .gw(
                "smartTracklist.getSongs",
                &json!({ "SMARTTRACKLIST_ID": stl_id, "nb": 100 }),
            )
            .await?;
        let arr = body
            .pointer("/results/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = arr.iter().filter_map(map_track).collect();
        let playlist = LibraryPlaylistDto {
            id: stable_hash_i64(stl_id),
            name: "Mix".into(),
            created_at: 0,
            updated_at: 0,
            track_count: tracks.len() as i64,
            cover_ids: Vec::new(),
            service_id: Some(format!("stl::{stl_id}")),
            description: None,
            owner: None,
        };
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }

    async fn page_get(&self, page_path: &str) -> MhResult<Value> {
        self.gw("page.get", &json!({
            "PAGE": page_path,
            "VERSION": "2.5",
            "SUPPORT": {
                "grid": ["album","artist","channel","flow","playlist","radio","show","smarttracklist","track","user"],
                "horizontal-grid": ["album","artist","channel","flow","playlist","radio","show","smarttracklist","track","user"],
                "long-card-horizontal-grid": ["album","artist","channel","flow","playlist","radio","show","smarttracklist","track","user"]
            },
            "LANG": "en",
        })).await
    }
}

fn cover_for(kind: &str, hash: &str) -> Option<String> {
    if hash.is_empty() {
        return None;
    }
    Some(cover_id(ServicePlatform::Deezer, &format!("{kind}:{hash}")))
}

fn deezer_any_cover(v: &Value) -> Option<String> {
    let picture_type = str_at(v, &["PICTURE_TYPE"]).unwrap_or("misc");
    for field in [
        "PLAYLIST_PICTURE",
        "ALB_PICTURE",
        "ART_PICTURE",
        "MD5_IMAGE",
        "PICTURE",
        "PICTURE_MD5",
        "TALK_PICTURE",
    ] {
        if let Some(h) = v
            .get(field)
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
        {
            if let Some(c) = cover_for(picture_type, h) {
                return Some(c);
            }
        }
    }
    for field in ["LOGO_IMAGE_URL", "IMAGE_URL", "URL_PICTURE"] {
        if let Some(u) = v
            .get(field)
            .and_then(|x| x.as_str())
            .filter(|s| s.starts_with("http"))
        {
            return Some(cover_id(ServicePlatform::Deezer, u));
        }
    }
    None
}

fn jwt_exp(token: &str) -> Option<i64> {
    use base64::Engine;
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v.get("exp").and_then(|x| x.as_i64())
}

/// Deezer sends cover art as a bare md5 hash under several key names, and an
/// empty string where it means "none".
fn cover_hash_at(v: &Value, kind: &str, paths: &[&str]) -> Option<String> {
    nonempty_at(v, paths).and_then(|h| cover_for(kind, &h))
}

fn map_album_gql(node: &Value) -> Option<LibraryAlbumDto> {
    Some(LibraryAlbumDto {
        album_key: id_at(node, &["id"])?,
        title: string_at(node, &["displayTitle"]),
        artist: string_at(node, &["/contributors/edges/0/node/name"]),
        year: year4_at(node, &["releaseDate"]),
        cover_id: cover_hash_at(node, "cover", &["/cover/md5"]),
        track_count: 0,
        artist_id: id_at(node, &["/contributors/edges/0/node/id"]),
        ..Default::default()
    })
}

fn map_album(v: &Value) -> Option<LibraryAlbumDto> {
    Some(LibraryAlbumDto {
        album_key: id_at(v, &["ALB_ID"])?,
        title: string_at(v, &["ALB_TITLE"]),
        artist: string_at(v, &["ART_NAME"]),
        year: year4_at(v, &["PHYSICAL_RELEASE_DATE", "DIGITAL_RELEASE_DATE"]),
        cover_id: cover_hash_at(v, "cover", &["ALB_PICTURE"]).or_else(|| deezer_any_cover(v)),
        track_count: i64_at(v, &["NUMBER_TRACK"]).unwrap_or(0),
        artist_id: id_at(v, &["ART_ID"]),
        ..Default::default()
    })
}

fn map_track(v: &Value) -> Option<LibraryTrackDto> {
    let id = id_at(v, &["SNG_ID"])?;
    let artist = string_at(v, &["ART_NAME"]);
    Some(LibraryTrackDto {
        path: format!("https://www.deezer.com/track/{id}"),
        title: Some(string_at(v, &["SNG_TITLE"])),
        artist: Some(artist.clone()),
        album: nonempty_at(v, &["ALB_TITLE"]),
        album_key: id_at(v, &["ALB_ID"]),
        duration_secs: f64_at(v, &["DURATION"]),
        track_no: u32_at(v, &["TRACK_NUMBER"]),
        disc_no: u32_at(v, &["DISK_NUMBER"]),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: cover_hash_at(v, "cover", &["ALB_PICTURE"]).or_else(|| deezer_any_cover(v)),
        primary_artist: Some(artist),
        artist_id: id_at(v, &["ART_ID"]),
        ..Default::default()
    })
}

fn map_episode(v: &Value) -> Option<LibraryTrackDto> {
    let id = id_at(v, &["EPISODE_ID"])?;
    let show = string_at(v, &["SHOW_NAME"]);
    Some(LibraryTrackDto {
        path: format!("https://www.deezer.com/episode/{id}"),
        title: Some(string_at(v, &["EPISODE_TITLE"])),
        artist: Some(show.clone()),
        album: Some(show.clone()),
        duration_secs: f64_at(v, &["DURATION"]),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: cover_hash_at(v, "talk", &["SHOW_ART_MD5", "EPISODE_IMAGE_MD5"])
            .or_else(|| deezer_any_cover(v)),
        primary_artist: Some(show),
        ..Default::default()
    })
}

fn map_show(v: &Value) -> Option<LibraryPlaylistDto> {
    let id_str = id_at(v, &["SHOW_ID"])?;
    let id = id_str
        .parse::<i64>()
        .unwrap_or_else(|_| stable_hash_i64(&id_str));
    Some(LibraryPlaylistDto {
        id,
        name: string_at(v, &["SHOW_NAME", "TITLE"]),
        created_at: 0,
        updated_at: 0,
        track_count: i64_at(v, &["NB_EPISODE", "EPISODE_COUNT"]).unwrap_or(0),
        cover_ids: cover_hash_at(v, "talk", &["SHOW_ART_MD5"])
            .or_else(|| deezer_any_cover(v))
            .map(|s| vec![s])
            .unwrap_or_default(),
        service_id: Some(format!("podcast::{id_str}")),
        description: nonempty_at(v, &["DESCRIPTION"]),
        owner: None,
    })
}

fn map_track_public(v: &Value) -> Option<LibraryTrackDto> {
    let id = id_at(v, &["id"])?;
    let artist = string_at(v, &["/artist/name"]);
    Some(LibraryTrackDto {
        path: format!("https://www.deezer.com/track/{id}"),
        title: Some(string_at(v, &["title"])),
        artist: Some(artist.clone()),
        album: nonempty_at(v, &["/album/title"]),
        album_key: id_at(v, &["/album/id"]),
        duration_secs: f64_at(v, &["duration"]),
        size: 0,
        mtime_ns: 0,
        is_video: false,
        cover_id: cover_hash_at(v, "cover", &["/album/md5_image", "/md5_image"]),
        primary_artist: Some(artist),
        artist_id: id_at(v, &["/artist/id"]),
        ..Default::default()
    })
}

fn map_artist(v: &Value) -> Option<LibraryArtistDto> {
    Some(LibraryArtistDto {
        key: id_at(v, &["ART_ID"])?,
        display: string_at(v, &["ART_NAME"]),
        album_count: i64_at(v, &["NB_ALBUM"]).unwrap_or(0),
        track_count: 0,
        cover_id: cover_hash_at(v, "artist", &["ART_PICTURE"]).or_else(|| deezer_any_cover(v)),
    })
}

fn map_user(v: &Value) -> Option<LibraryArtistDto> {
    let id = id_at(v, &["USER_ID"])?;
    if id.is_empty() || id == "0" {
        return None;
    }
    let name = nonempty_at(v, &["BLOG_NAME"])
        .or_else(|| {
            let joined = format!(
                "{} {}",
                string_at(v, &["FIRSTNAME"]),
                string_at(v, &["LASTNAME"])
            );
            let joined = joined.trim().to_string();
            (!joined.is_empty()).then_some(joined)
        })
        .unwrap_or_default();
    Some(LibraryArtistDto {
        key: format!("user::{id}"),
        display: name,
        album_count: 0,
        track_count: 0,
        cover_id: cover_hash_at(v, "user", &["USER_PICTURE"]),
    })
}

fn map_playlist(v: &Value) -> Option<LibraryPlaylistDto> {
    let id_str = id_at(v, &["PLAYLIST_ID"])?;
    let id = id_str
        .parse::<i64>()
        .unwrap_or_else(|_| stable_hash_i64(&id_str));
    Some(LibraryPlaylistDto {
        id,
        name: string_at(v, &["TITLE"]),
        created_at: 0,
        updated_at: 0,
        track_count: i64_at(v, &["NB_SONG"]).unwrap_or(0),
        cover_ids: deezer_any_cover(v).map(|s| vec![s]).unwrap_or_default(),
        service_id: Some(id_str),
        description: nonempty_at(v, &["DESCRIPTION", "description"]),
        owner: nonempty_at(v, &["PARENT_USERNAME", "/creator/name", "/user/name"]),
    })
}

fn flatten_typed(v: &Value, out: &mut Vec<Value>) {
    match v {
        Value::Object(m) => {
            if m.get("__TYPE__").and_then(|v| v.as_str()).is_some() {
                out.push(Value::Object(m.clone()));
            }
            for child in m.values() {
                flatten_typed(child, out);
            }
        }
        Value::Array(a) => {
            for v in a {
                flatten_typed(v, out);
            }
        }
        _ => {}
    }
}

/// Every Deezer page endpoint answers with `/results/sections`, and each section carries
/// a title, an id under one of a few keys, and a list of items. Only the id key and the
/// item extractor differ between the shelves, mixes and channel tiles built from them.
fn sections_shelves(
    body: &Value,
    id_keys: &[&str],
    default_id: &str,
    category: RecommendationCategory,
    items_of: impl Fn(&Value) -> Vec<RecommendationItem>,
) -> Vec<RecommendationShelf> {
    let sections = body
        .pointer("/results/sections")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    sections
        .iter()
        .filter_map(|sec| {
            let title = str_at(sec, &["title"]).unwrap_or("").to_string();
            let id = id_keys
                .iter()
                .find_map(|k| str_at(sec, &[k]).filter(|v| !v.is_empty()))
                .unwrap_or(default_id);
            let items = items_of(sec);
            (!items.is_empty()).then(|| RecommendationShelf::new(id, title, category, items))
        })
        .collect()
}

fn shelves_from_page(body: &Value) -> Vec<RecommendationShelf> {
    sections_shelves(
        body,
        &["group_id"],
        "",
        RecommendationCategory::Other,
        |sec| {
            let mut nodes = Vec::new();
            flatten_typed(sec, &mut nodes);
            nodes.iter().filter_map(classify_item).take(50).collect()
        },
    )
}

fn map_smart_mix(v: &Value) -> Option<RecommendationItem> {
    let stl = str_at(v, &["SMARTTRACKLIST_ID"]).filter(|s| !s.is_empty())?;
    let title = str_at(v, &["TITLE"])
        .filter(|s| !s.is_empty())
        .or_else(|| str_at(v, &["AUTO_GENERATED_TITLE"]))
        .unwrap_or("Mix")
        .to_string();
    let subtitle = str_at(v, &["SUBTITLE"])
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let cover_id = str_at(v, &["/PICTURES/0/md5"])
        .filter(|s| !s.is_empty())
        .and_then(|h| cover_for("misc", h))
        .or_else(|| {
            str_at(v, &["COVER"])
                .filter(|s| !s.is_empty())
                .and_then(|h| cover_for("misc", h))
        });
    Some(RecommendationItem::Mix {
        id: format!("stl::{stl}"),
        title,
        subtitle,
        cover_id,
        url: None,
    })
}

fn smart_mix_shelves(body: &Value) -> Vec<RecommendationShelf> {
    sections_shelves(
        body,
        &[],
        "smart_mixes",
        RecommendationCategory::Stations,
        |sec| {
            sec.get("items")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .map(|it| it.get("data").unwrap_or(it))
                        .filter_map(map_smart_mix)
                        .collect()
                })
                .unwrap_or_default()
        },
    )
}

fn classify_item(it: &Value) -> Option<RecommendationItem> {
    let ty = str_at(it, &["__TYPE__"]).unwrap_or("");
    match ty {
        "album" => map_album(it).map(RecommendationItem::Album),
        "song" => map_track(it).map(RecommendationItem::Track),
        "artist" => map_artist(it).map(RecommendationItem::Artist),
        "playlist" => map_playlist(it).map(RecommendationItem::Playlist),
        _ => None,
    }
}

fn channel_cover(item: &Value) -> Option<String> {
    str_at(item, &["/data/pictures/0/md5"])
        .filter(|s| !s.is_empty())
        .and_then(|h| cover_for("misc", h))
        .or_else(|| {
            str_at(item, &["/image_linked_item/md5"])
                .filter(|s| !s.is_empty())
                .and_then(|h| {
                    let ty = str_at(item, &["/image_linked_item/type"]).unwrap_or("cover");
                    cover_for(ty, h)
                })
        })
}

fn map_channel_tile(item: &Value) -> Option<RecommendationItem> {
    let target = str_at(item, &["target"])?;
    let api_path = target.trim_start_matches('/');
    if !api_path.starts_with("channels/") {
        return None;
    }
    let title = str_at(item, &["title"])
        .or_else(|| str_at(item, &["/data/title"]))
        .or_else(|| str_at(item, &["/data/name"]))
        .unwrap_or("")
        .to_string();
    if title.is_empty() {
        return None;
    }
    Some(RecommendationItem::PageLink {
        api_path: api_path.to_string(),
        title,
        icon: channel_cover(item),
    })
}

fn channel_tiles_from_page(body: &Value) -> Vec<RecommendationShelf> {
    sections_shelves(
        body,
        &["module_id", "group_id"],
        "",
        RecommendationCategory::Genre,
        |sec| {
            sec.get("items")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .filter_map(map_channel_tile)
                .collect()
        },
    )
}

#[async_trait]
impl ServiceLibrary for DeezerLibrary {
    fn as_mutations(self: Arc<Self>) -> Option<Arc<dyn ServiceLibraryMutations>> {
        Some(self)
    }

    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Deezer
    }
    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::audio_only()
            .with_followers()
            .with_episode_bookmarks()
            .with_mutations(self.mutation_capabilities())
    }

    async fn albums(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryAlbumDto>> {
        let nodes = self.profile_tab("albums", "album").await;
        let items: Vec<LibraryAlbumDto> = nodes.iter().filter_map(map_album).collect();
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn tracks(&self, page: Page) -> MhResult<ServiceLibraryPage<LibraryTrackDto>> {
        let s = self.ensure_session().await?;
        let uid: u64 = s.user_id.parse().unwrap_or(0);
        let body = self
            .gw(
                "favorite_song.getList",
                &json!({
                    "USER_ID": uid,
                    "NB": page.limit,
                    "START": page.offset,
                    "TAB": "loved",
                }),
            )
            .await?;
        let arr = body
            .pointer("/results/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let items: Vec<LibraryTrackDto> = arr.iter().filter_map(map_track).collect();
        let total = body.pointer("/results/total").and_then(|v| v.as_u64());
        Ok(ServiceLibraryPage::of(items, total))
    }

    async fn artists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryArtistDto>> {
        let nodes = self.profile_tab("artists", "artist").await;
        let items: Vec<LibraryArtistDto> = nodes.iter().filter_map(map_artist).collect();
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn playlists(&self, _page: Page) -> MhResult<ServiceLibraryPage<LibraryPlaylistDto>> {
        let mut items: Vec<LibraryPlaylistDto> = Vec::new();
        let mut seen = std::collections::HashSet::new();

        if let Ok(body) = self.gw("deezer.userMenu", &json!({})).await {
            let arr = body
                .pointer("/results/PLAYLISTS/data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            for n in arr {
                if let Some(p) = map_playlist(&n) {
                    if seen.insert(p.service_id.clone().unwrap_or_default()) {
                        items.push(p);
                    }
                }
            }
        }

        for n in self.profile_tab("playlists", "playlist").await {
            if let Some(p) = map_playlist(&n) {
                if seen.insert(p.service_id.clone().unwrap_or_default()) {
                    items.push(p);
                }
            }
        }

        for n in self.profile_tab_keyed("podcasts", "shows", "show").await {
            if let Some(p) = map_show(&n) {
                if seen.insert(p.service_id.clone().unwrap_or_default()) {
                    items.push(p);
                }
            }
        }
        Ok(ServiceLibraryPage::counted(items))
    }

    async fn recommendations(&self) -> MhResult<RecommendationsPage> {
        let mut shelves = Vec::new();
        let uid: u64 = self
            .ensure_session()
            .await
            .ok()
            .and_then(|s| s.user_id.parse().ok())
            .unwrap_or(0);

        let flow_args = json!({});
        let history_args = json!({ "USER_ID": uid, "nb": 50 });
        let (flow, history, foryou, home, explore, discovery) = tokio::join!(
            self.gw("radio.getUserRadio", &flow_args),
            self.gw("user.getSongsHistory", &history_args),
            self.page_get("channels/foryou"),
            self.page_get("home"),
            self.page_get("channels/explore"),
            self.page_get("channels/explore/discovery"),
        );

        if let Ok(body) = flow {
            let arr = body
                .pointer("/results/data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            let items: Vec<RecommendationItem> = arr
                .iter()
                .filter_map(map_track)
                .map(RecommendationItem::Track)
                .collect();
            if !items.is_empty() {
                shelves.push(RecommendationShelf {
                    id: "flow".into(),
                    title: "Flow".into(),
                    subtitle: Some("Your personalised mix".into()),
                    category: RecommendationCategory::Discovery,
                    items,
                });
            }
        }

        if let Ok(body) = history {
            let items: Vec<RecommendationItem> = body
                .pointer("/results/data")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(map_track)
                        .map(RecommendationItem::Track)
                        .collect()
                })
                .unwrap_or_default();
            if !items.is_empty() {
                shelves.push(RecommendationShelf {
                    id: "history".into(),
                    title: "Recently played".into(),
                    subtitle: Some("Pick up where you left off".into()),
                    category: RecommendationCategory::RecentlyPlayed,
                    items,
                });
            }
        }

        if let Ok(body) = foryou {
            shelves.extend(shelves_from_page(&body));
            shelves.extend(smart_mix_shelves(&body));
        }

        if let Ok(body) = home {
            shelves.extend(shelves_from_page(&body));
        }

        if let Ok(body) = explore {
            shelves.extend(shelves_from_page(&body));
        }
        if let Ok(body) = discovery {
            shelves.extend(shelves_from_page(&body));
        }

        let mut seen = std::collections::HashSet::new();
        shelves.retain(|s| !s.items.is_empty() && seen.insert(s.title.clone()));

        self.maybe_fire_telemetry().await;
        Ok(RecommendationsPage { shelves })
    }

    async fn explore(&self) -> MhResult<RecommendationsPage> {
        let body = self.page_get("channels/explore").await?;
        self.maybe_fire_telemetry().await;
        Ok(RecommendationsPage {
            shelves: channel_tiles_from_page(&body),
        })
    }

    async fn explore_page(&self, path: &str) -> MhResult<RecommendationsPage> {
        let clean = path.trim_start_matches('/');
        let body = self.page_get(clean).await?;
        let mut shelves = channel_tiles_from_page(&body);
        shelves.extend(shelves_from_page(&body));
        Ok(RecommendationsPage { shelves })
    }

    async fn album_page(&self, id: &str) -> MhResult<Value> {
        let id = id.strip_prefix("audiobook::").unwrap_or(id);
        const QUERY: &str = "query AlternativeAlbumVersions($albumId: String!) { album(albumId: $albumId) { id alternativeVersions { edges { node { id displayTitle releaseDate isExplicit cover { md5 } contributors(first: 1) { edges { roles node { ... on Artist { id name } } } } } } } } }";
        let body = self
            .pipe_graphql("AlternativeAlbumVersions", QUERY, json!({ "albumId": id }))
            .await?;
        let versions: Vec<RecommendationItem> = body
            .pointer("/data/album/alternativeVersions/edges")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|e| e.get("node"))
                    .filter(|n| str_at(n, &["id"]) != Some(id))
                    .filter_map(map_album_gql)
                    .map(RecommendationItem::Album)
                    .collect()
            })
            .unwrap_or_default();
        let shelves: Vec<RecommendationShelf> = if versions.is_empty() {
            Vec::new()
        } else {
            vec![RecommendationShelf::new(
                "alt_versions",
                "Other versions",
                RecommendationCategory::Other,
                versions,
            )]
        };
        Ok(json!({ "shelves": shelves, "raw": body }))
    }

    async fn artist_page(&self, id: &str) -> MhResult<Value> {
        let body = self
            .gw(
                "deezer.pageArtist",
                &json!({ "ART_ID": id, "lang": "en", "tab": 0 }),
            )
            .await?;
        let mut shelves = Vec::new();

        let section = |ptr: &str,
                       title: &str,
                       cat: RecommendationCategory,
                       make: &dyn Fn(&Value) -> Option<RecommendationItem>|
         -> Option<RecommendationShelf> {
            let items: Vec<RecommendationItem> = body
                .pointer(ptr)
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(make).collect())
                .unwrap_or_default();
            if items.is_empty() {
                return None;
            }
            Some(RecommendationShelf::new(
                format!("artist-{id}-{title}"),
                title,
                cat,
                items,
            ))
        };

        if let Some(s) = section(
            "/results/TOP/data",
            "Top Tracks",
            RecommendationCategory::Charts,
            &|v| map_track(v).map(RecommendationItem::Track),
        ) {
            shelves.push(s);
        }
        if let Some(s) = section(
            "/results/ALBUMS/data",
            "Discography",
            RecommendationCategory::NewReleases,
            &|v| map_album(v).map(RecommendationItem::Album),
        ) {
            shelves.push(s);
        }
        if let Some(s) = section(
            "/results/RELATED_ARTISTS/data",
            "Fans Also Like",
            RecommendationCategory::Discovery,
            &|v| map_artist(v).map(RecommendationItem::Artist),
        ) {
            shelves.push(s);
        }
        let has_items = |ptr: &str| {
            body.pointer(ptr)
                .and_then(|v| v.as_array())
                .is_some_and(|a| !a.is_empty())
        };
        let playlist_ptr = if has_items("/results/SELECTED_PLAYLIST/data") {
            "/results/SELECTED_PLAYLIST/data"
        } else {
            "/results/RELATED_PLAYLIST/data"
        };
        if let Some(s) = section(
            playlist_ptr,
            "Playlists",
            RecommendationCategory::Editorial,
            &|v| map_playlist(v).map(RecommendationItem::Playlist),
        ) {
            shelves.push(s);
        }

        Ok(json!({ "shelves": shelves, "raw": body }))
    }

    async fn report_playback(
        &self,
        service_track_id: &str,
        _duration_secs: u64,
        _context_uri: Option<&str>,
        _track_index: Option<u64>,
    ) -> MhResult<()> {
        let enabled = match &self.settings {
            Some(s) => s
                .read()
                .await
                .sync_playback_history(ServicePlatform::Deezer),
            None => false,
        };
        if !enabled {
            return Ok(());
        }
        let sng_id = service_track_id;
        let now = chrono::Utc::now().timestamp();
        let lt = _duration_secs.max(1) as i64;
        let stream_id = uuid::Uuid::new_v4().to_string();
        let body = json!({
            "params": {
                "media": { "id": sng_id, "type": "song", "format": "MP3_128" },
                "type": 0,
                "stat": { "seek": 0, "pause": 0, "sync": 0 },
                "lt": lt,
                "ctxt": { "t": "track_page", "id": sng_id },
                "payload": {},
                "dev": { "v": DEV_VERSION, "t": 0 },
                "ls": [],
                "ts_listen": now,
                "timestamp": now - lt,
                "is_shuffle": false,
                "stream_id": stream_id,
            },
            "next_media": { "media": { "id": sng_id, "type": "song" } },
        });
        self.gw("log.listen", &body).await?;
        Ok(())
    }

    async fn episode_bookmarks(&self) -> MhResult<Vec<LibraryTrackDto>> {
        let body = self.gw("deezer.userMenu", &json!({})).await?;
        let arr = body
            .pointer("/results/EPISODE_BOOKMARKS/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(arr.iter().filter_map(map_episode).collect())
    }

    async fn followers(&self) -> MhResult<Vec<LibraryArtistDto>> {
        let rows = self
            .try_profile_tab("followers", "followers", "user")
            .await?;
        Ok(rows.iter().filter_map(map_user).collect())
    }

    async fn following(&self) -> MhResult<Vec<LibraryArtistDto>> {
        let rows = self
            .try_profile_tab("following", "following", "user")
            .await?;
        Ok(rows.iter().filter_map(map_user).collect())
    }

    async fn album_detail(&self, id: &str) -> MhResult<LibraryAlbumDetail> {
        let id = id.strip_prefix("audiobook::").unwrap_or(id);
        let body = self
            .gw(
                "deezer.pageAlbum",
                &json!({
                    "ALB_ID": id, "lang": "en", "tab": 0,
                }),
            )
            .await?;
        let meta = body
            .pointer("/results/DATA")
            .cloned()
            .unwrap_or(Value::Null);
        let album =
            map_album(&meta).ok_or_else(|| MhError::NotFound(format!("Deezer album {id}")))?;
        let arr = body
            .pointer("/results/SONGS/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = arr.iter().filter_map(map_track).collect();
        Ok(LibraryAlbumDetail { album, tracks })
    }

    async fn artist_detail(&self, id: &str) -> MhResult<LibraryArtistDetail> {
        let body = self
            .gw(
                "deezer.pageArtist",
                &json!({
                    "ART_ID": id, "lang": "en", "tab": 0,
                }),
            )
            .await?;
        let meta = body
            .pointer("/results/DATA")
            .cloned()
            .unwrap_or(Value::Null);
        let display = str_at(&meta, &["ART_NAME"]).unwrap_or("").to_string();
        let albums_arr = body
            .pointer("/results/ALBUMS/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let albums: Vec<LibraryAlbumDto> = albums_arr.iter().filter_map(map_album).collect();
        let top_arr = body
            .pointer("/results/TOP/data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<LibraryTrackDto> = top_arr.iter().filter_map(map_track).collect();
        Ok(LibraryArtistDetail {
            key: id.into(),
            display,
            albums,
            tracks,
        })
    }

    async fn playlist_detail(&self, id: &str) -> MhResult<LibraryPlaylistDetail> {
        if let Some(show_id) = id
            .strip_prefix("podcast::")
            .or_else(|| id.strip_prefix("show::"))
        {
            return self.show_detail(show_id).await;
        }
        if let Some(stl_id) = id.strip_prefix("stl::") {
            return self.smart_tracklist_detail(stl_id).await;
        }
        const PAGE: u64 = 500;
        let body = self.gw("deezer.pagePlaylist", &json!({
            "PLAYLIST_ID": id, "lang": "en", "nb": PAGE, "start": 0, "tab": 0, "tags": true, "header": true,
        })).await?;
        let meta = body
            .pointer("/results/DATA")
            .cloned()
            .unwrap_or(Value::Null);
        let playlist = map_playlist(&meta)
            .ok_or_else(|| MhError::NotFound(format!("Deezer playlist {id}")))?;
        let rows_of = |b: &Value| -> Vec<LibraryTrackDto> {
            b.pointer("/results/SONGS/data")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(map_track).collect())
                .unwrap_or_default()
        };
        let total = body
            .pointer("/results/SONGS/total")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let mut tracks = rows_of(&body);
        let mut start = tracks.len() as u64;
        let mut guard = 0;
        while start < total {
            guard += 1;
            if guard > 40 {
                break;
            }
            let page = self.gw("deezer.pagePlaylist", &json!({
                "PLAYLIST_ID": id, "lang": "en", "nb": PAGE, "start": start, "tab": 0, "tags": true, "header": false,
            })).await?;
            let rows = rows_of(&page);
            if rows.is_empty() {
                break;
            }
            start += rows.len() as u64;
            tracks.extend(rows);
        }
        Ok(LibraryPlaylistDetail { playlist, tracks })
    }
}

#[async_trait]
impl ServiceLibraryMutations for DeezerLibrary {
    fn platform(&self) -> ServicePlatform {
        ServicePlatform::Deezer
    }

    fn mutation_capabilities(&self) -> MutationCapabilities {
        MutationCapabilities {
            reorder_playlists: false,
            ..MutationCapabilities::full_audio()
        }
    }

    async fn set_favorites(&self, kind: FavKind, op: FavOp, ids: &[String]) -> MhResult<()> {
        let (method, key) = match (kind, op) {
            (FavKind::Track, FavOp::Add) => ("favorite_song.add", "SNG_ID"),
            (FavKind::Track, FavOp::Remove) => ("favorite_song.remove", "SNG_ID"),
            (FavKind::Album, FavOp::Add) => ("album.addFavorite", "ALB_ID"),
            (FavKind::Album, FavOp::Remove) => ("album.deleteFavorite", "ALB_ID"),
            (FavKind::Artist, FavOp::Add) => ("artist.addFavorite", "ART_ID"),
            (FavKind::Artist, FavOp::Remove) => ("artist.deleteFavorite", "ART_ID"),
            (FavKind::Label, _) => return Err(MhError::Unsupported("Deezer has no labels".into())),
        };
        if (kind, op) == (FavKind::Track, FavOp::Add) {
            futures_util::future::try_join_all(ids.iter().map(|id| async move {
                let n = Self::parse_id(key, id)?;
                match self.gw(method, &json!({ key: n })).await {
                    Ok(_) => Ok(()),
                    Err(e) if e.to_string().contains("ERROR_DATA_EXISTS") => Ok(()),
                    Err(e) => Err(e),
                }
            }))
            .await?;
            return Ok(());
        }
        self.id_mutations(method, key, ids).await
    }

    async fn follow_playlist(&self, id: &str) -> MhResult<()> {
        self.id_mutation("playlist.subscribe", "PLAYLIST_ID", id)
            .await
    }
    async fn unfollow_playlist(&self, id: &str) -> MhResult<()> {
        self.id_mutation("playlist.unsubscribe", "PLAYLIST_ID", id)
            .await
    }
    async fn follow_user(&self, id: &str) -> MhResult<()> {
        let id = id.strip_prefix("user::").unwrap_or(id);
        self.id_mutation("friend.follow", "FRIEND_ID", id).await
    }
    async fn unfollow_user(&self, id: &str) -> MhResult<()> {
        let id = id.strip_prefix("user::").unwrap_or(id);
        self.id_mutation("friend.unfollow", "FRIEND_ID", id).await
    }

    async fn create_playlist(&self, input: PlaylistCreateInput) -> MhResult<PlaylistMutateResult> {
        let songs: Vec<Value> = input
            .initial_track_ids
            .iter()
            .filter_map(|id| id.parse::<u64>().ok())
            .map(|n| json!([n, 0]))
            .collect();
        let resp = self
            .gw(
                "playlist.create",
                &json!({
                    "title": input.name,
                    "description": input.description.unwrap_or_default(),
                    "status": if input.is_public { 1 } else { 0 },
                    "songs": songs,
                }),
            )
            .await?;
        let id = crate::services::common::library::json_id_at(&resp, "results")
            .ok_or_else(|| MhError::Other(format!("Deezer playlist.create: no id in {resp}")))?;
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
        let n: u64 = id
            .parse()
            .map_err(|_| MhError::Other(format!("Deezer: invalid PLAYLIST_ID {id}")))?;
        let mut body = json!({
            "PLAYLIST_ID": n,
            "title": new_name,
        });
        if let Some(d) = new_description {
            body["description"] = Value::String(d.to_string());
        }
        if new_is_collaborative == Some(true) {
            body["status"] = json!(2);
        } else if let Some(p) = new_is_public {
            body["status"] = json!(if p { 1 } else { 0 });
        }
        self.gw("playlist.update", &body).await?;
        Ok(())
    }

    async fn delete_playlist(&self, id: &str) -> MhResult<()> {
        let n: u64 = id
            .parse()
            .map_err(|_| MhError::Other(format!("Deezer: invalid PLAYLIST_ID {id}")))?;
        self.gw("playlist.delete", &json!({ "PLAYLIST_ID": n }))
            .await?;
        Ok(())
    }

    async fn add_playlist_tracks(
        &self,
        id: &str,
        track_ids: &[String],
    ) -> MhResult<PlaylistMutateResult> {
        let pl: u64 = id
            .parse()
            .map_err(|_| MhError::Other(format!("Deezer: invalid PLAYLIST_ID {id}")))?;
        let songs: Vec<Value> = track_ids
            .iter()
            .filter_map(|t| t.parse::<u64>().ok())
            .map(|n| json!([n, 0]))
            .collect();
        self.gw(
            "playlist.addSongs",
            &json!({
                "PLAYLIST_ID": pl, "SONGS": songs,
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
        let pl: u64 = id
            .parse()
            .map_err(|_| MhError::Other(format!("Deezer: invalid PLAYLIST_ID {id}")))?;
        let songs: Vec<Value> = track_ids
            .iter()
            .filter_map(|t| t.parse::<u64>().ok())
            .map(|n| json!([n, 0]))
            .collect();
        self.gw(
            "playlist.deleteSongs",
            &json!({
                "PLAYLIST_ID": pl, "SONGS": songs,
            }),
        )
        .await?;
        Ok(PlaylistMutateResult::id(id))
    }

    async fn radio_for(&self, seed_kind: RadioSeedKind, seed_id: &str) -> MhResult<RadioResult> {
        let http = self.client.clone();

        async fn get_json(c: &reqwest::Client, url: &str) -> MhResult<Value> {
            let resp = c
                .get(url)
                .header("accept", "application/json")
                .send()
                .await
                .map_err(MhError::Network)?;
            let st = resp.status();
            let txt = resp.text().await.map_err(MhError::Network)?;
            if !st.is_success() {
                return Err(crate::services::common::http::api_error("Deezer", st, &txt));
            }
            let body: Value = serde_json::from_str(&txt)
                .map_err(|e| MhError::Other(format!("Deezer {url}: non-JSON: {e}")))?;
            if let Some(err) = body.get("error") {
                if !err.is_null() {
                    return Err(MhError::Other(format!("Deezer {url} error: {err}")));
                }
            }
            Ok(body)
        }

        let artist_id: String = match seed_kind {
            RadioSeedKind::Track => {
                let t = get_json(&http, &format!("https://api.deezer.com/track/{seed_id}")).await?;
                crate::services::common::library::json_id_ptr(&t, "/artist/id").ok_or_else(
                    || MhError::Other(format!("Deezer radio: no artist.id on track {seed_id}")),
                )?
            }
            RadioSeedKind::Artist => seed_id.to_string(),
            _ => {
                return Err(MhError::Other(
                    "Deezer radio supports track / artist seeds only".into(),
                ))
            }
        };

        let body = get_json(
            &http,
            &format!("https://api.deezer.com/artist/{artist_id}/radio?limit=40"),
        )
        .await?;
        let arr = body
            .get("data")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let tracks: Vec<_> = arr.iter().filter_map(map_track_public).collect();
        if tracks.is_empty() {
            return Err(MhError::Other(format!(
                "Deezer radio: 0 tracks for artist {artist_id} (seed {seed_id})"
            )));
        }
        Ok(RadioResult {
            seed_kind,
            seed_id: seed_id.to_string(),
            title: format!("Deezer radio · {seed_id}"),
            tracks,
            station_id: None,
            video_urls: Default::default(),
            continuation: None,
        })
    }

    async fn fetch_saved_ids(&self, kinds: &[SaveKind]) -> MhResult<SavedIdSet> {
        let mut out = SavedIdSet::default();
        for kind in kinds {
            let (tab, ty, field) = match kind {
                SaveKind::Track => ("loved", "song", "SNG_ID"),
                SaveKind::Album => ("albums", "album", "ALB_ID"),
                SaveKind::Artist => ("artists", "artist", "ART_ID"),
                SaveKind::Playlist => ("playlists", "playlist", "PLAYLIST_ID"),
            };
            let ids: Vec<String> = self
                .profile_tab(tab, ty)
                .await
                .iter()
                .filter_map(|t| crate::services::common::library::json_id_at(t, field))
                .collect();
            out.set(*kind, ids);
        }
        Ok(out)
    }

    async fn fetch_owned_playlists(&self) -> MhResult<Vec<OwnedPlaylistRow>> {
        let s = self.ensure_session().await?;
        let uid_self = s.user_id.clone();
        let items = self.profile_tab("playlists", "playlist").await;
        let mut rows = Vec::new();
        for item in items {
            let id = crate::services::common::library::json_id_at(&item, "PLAYLIST_ID")
                .unwrap_or_default();
            if id.is_empty() {
                continue;
            }
            let owner = crate::services::common::library::json_id_ptr(&item, "/PARENT_USER_ID")
                .unwrap_or_default();
            if !owner.is_empty() && owner != uid_self {
                continue;
            }
            rows.push(OwnedPlaylistRow {
                platform: ServicePlatform::Deezer.as_str().into(),
                service_id: id,
                name: str_at(&item, &["TITLE"]).unwrap_or("").into(),
                cover_id: deezer_any_cover(&item),
                track_count: item
                    .get("NB_SONG")
                    .and_then(|v| {
                        v.as_i64()
                            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
                    })
                    .unwrap_or(0),
                updated_at: item.get("DATE_MOD").and_then(|v| v.as_i64()).unwrap_or(0),
            });
        }
        Ok(rows)
    }
}
