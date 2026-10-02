use async_trait::async_trait;
use serde_json::Value;

use crate::errors::MhResult;
use crate::ipc_contract::PerformSearchRequest;
use crate::services::common::search::{SearchContext, SearchProvider};

pub struct YtMusicSearch;

#[async_trait]
impl SearchProvider for YtMusicSearch {
    async fn search(&self, req: &PerformSearchRequest, _ctx: &SearchContext) -> MhResult<Value> {
        let client = crate::services::ytmusic::api::YtMusicClient::shared().await?;
        let filter = map_ytmusic_filter(&req.search_type);
        let results = crate::services::ytmusic::api::search(client, &req.query, filter).await?;
        Ok(serde_json::to_value(results)?)
    }

    async fn suggestions(&self, query: &str, _ctx: &SearchContext) -> MhResult<Vec<String>> {
        let client = crate::services::ytmusic::api::YtMusicClient::shared().await?;
        client.get_search_suggestions(query).await
    }
}

fn map_ytmusic_filter(
    t: &crate::ipc_contract::SearchType,
) -> crate::services::ytmusic::api::YtMusicFilter {
    use crate::ipc_contract::SearchType::*;
    match t {
        Album => crate::services::ytmusic::api::YtMusicFilter::Album,
        Playlist => crate::services::ytmusic::api::YtMusicFilter::Playlist,
        Artist => crate::services::ytmusic::api::YtMusicFilter::Artist,
        Podcast => crate::services::ytmusic::api::YtMusicFilter::Podcast,
        Video => crate::services::ytmusic::api::YtMusicFilter::Video,
        _ => crate::services::ytmusic::api::YtMusicFilter::Song,
    }
}
