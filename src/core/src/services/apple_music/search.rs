use async_trait::async_trait;
use serde_json::Value;

use crate::errors::MhResult;
use crate::ipc_contract::PerformSearchRequest;
use crate::services::common::search::{page_of, SearchContext, SearchProvider};

pub struct AppleMusicSearch;

#[async_trait]
impl SearchProvider for AppleMusicSearch {
    async fn search(&self, req: &PerformSearchRequest, _ctx: &SearchContext) -> MhResult<Value> {
        let (limit, _) = page_of(req, crate::services::common::limits::APPLE_MUSIC_PAGE_MAX);
        let client = crate::services::apple_music::api::AppleMusicApiClient::new(None)?;
        client
            .search(&req.query, map_apple_entity(&req.search_type), limit)
            .await
    }
}

fn map_apple_entity(
    t: &crate::ipc_contract::SearchType,
) -> crate::services::apple_music::api::AppleMusicMediaType {
    use crate::ipc_contract::SearchType::*;
    match t {
        Album => crate::services::apple_music::api::AppleMusicMediaType::Album,
        Artist => crate::services::apple_music::api::AppleMusicMediaType::Artist,
        Playlist => crate::services::apple_music::api::AppleMusicMediaType::Playlist,
        Video | MusicVideo => crate::services::apple_music::api::AppleMusicMediaType::MusicVideo,
        _ => crate::services::apple_music::api::AppleMusicMediaType::Song,
    }
}
