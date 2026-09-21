use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::auth::credentials::ApiCredentials;
use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract::PerformSearchRequest;
use crate::EventEmitter;

#[derive(Clone)]
pub struct SearchContext {
    pub settings: Arc<RwLock<Settings>>,
    pub credentials: ApiCredentials,
    pub emitter: Arc<dyn EventEmitter>,
    pub user_data: PathBuf,
    pub librespot: Arc<RwLock<crate::services::spotify::session::LibrespotService>>,
}

impl SearchContext {
    pub fn from_state(state: &crate::BackendState) -> Self {
        Self {
            settings: state.settings.clone(),
            credentials: state.credentials.clone(),
            emitter: state.emitter.clone(),
            user_data: state.user_data.clone(),
            librespot: state.librespot.clone(),
        }
    }
}

#[async_trait]
pub trait SearchProvider: Send + Sync {
    async fn search(&self, req: &PerformSearchRequest, ctx: &SearchContext) -> MhResult<Value>;

    async fn suggestions(&self, _query: &str, _ctx: &SearchContext) -> MhResult<Vec<String>> {
        Ok(Vec::new())
    }
}

/// The `(limit, offset)` window a search request is asking for, clamped to what
/// the service's API actually accepts.
///
/// `page_max` comes from [`crate::services::common::limits`]; asking past it
/// gets silently clamped upstream anyway, so clamping here keeps the requested
/// and served page sizes honest.
pub fn page_of(req: &PerformSearchRequest, page_max: u32) -> (u32, u32) {
    let limit = req
        .limit
        .unwrap_or(crate::services::common::limits::DEFAULT_SEARCH_LIMIT)
        .min(page_max);
    (limit, req.offset.unwrap_or(0))
}

/// Build the autocomplete list from search hits.
///
/// `Title — Artist` (or just the title when the artist is unknown), deduped
/// case-insensitively, capped at [`crate::services::common::limits::SUGGESTIONS_LIMIT`].
/// `pick` is the only per-service part: where the title and artist live in one
/// result object.
pub fn suggestion_lines(items: &[Value], pick: impl Fn(&Value) -> (&str, &str)) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in items {
        let (title, artist) = pick(item);
        let text = if artist.is_empty() {
            title.to_string()
        } else {
            format!("{title} — {artist}")
        };
        let text = text.trim().to_string();
        if !text.is_empty() && seen.insert(text.to_lowercase()) {
            out.push(text);
        }
        if out.len() >= crate::services::common::limits::SUGGESTIONS_LIMIT {
            break;
        }
    }
    out
}

pub fn search_type_to_str(t: &crate::ipc_contract::SearchType) -> &'static str {
    use crate::ipc_contract::SearchType::*;
    match t {
        Track | Song => "track",
        Album => "album",
        Artist => "artist",
        Playlist => "playlist",
        Episode => "episode",
        Podcast | Show => "podcast",
        Audiobook => "audiobook",
        Video | MusicVideo => "video",
        Channel => "channel",
    }
}
