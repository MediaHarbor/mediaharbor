use async_trait::async_trait;
use serde_json::Value;

use crate::errors::MhResult;
use crate::ipc_contract::PerformSearchRequest;
use crate::services::common::search::{page_of, SearchContext, SearchProvider};

pub struct DeezerSearch;

#[async_trait]
impl SearchProvider for DeezerSearch {
    async fn search(&self, req: &PerformSearchRequest, _ctx: &SearchContext) -> MhResult<Value> {
        let (limit, offset) = page_of(req, crate::services::common::limits::DEEZER_PAGE_MAX);
        let client = crate::services::deezer::api::DeezerApiClient::new()?;
        client
            .search(&req.query, map_deezer_type(&req.search_type), limit, offset)
            .await
    }

    async fn suggestions(&self, query: &str, _ctx: &SearchContext) -> MhResult<Vec<String>> {
        let client = crate::services::deezer::api::DeezerApiClient::new()?;
        client.suggestions(query).await
    }
}

fn map_deezer_type(
    t: &crate::ipc_contract::SearchType,
) -> crate::services::deezer::api::DeezerSearchType {
    use crate::ipc_contract::SearchType::*;
    match t {
        Album => crate::services::deezer::api::DeezerSearchType::Album,
        Artist => crate::services::deezer::api::DeezerSearchType::Artist,
        Playlist => crate::services::deezer::api::DeezerSearchType::Playlist,
        Podcast => crate::services::deezer::api::DeezerSearchType::Podcast,
        Episode => crate::services::deezer::api::DeezerSearchType::Episode,
        _ => crate::services::deezer::api::DeezerSearchType::Track,
    }
}
