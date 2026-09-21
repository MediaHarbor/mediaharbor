use async_trait::async_trait;
use serde_json::Value;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract::{PerformSearchRequest, SearchType};
use crate::services::common::search::{page_of, SearchContext, SearchProvider};

pub struct YoutubeSearch;

fn yt_key(settings: &Settings, ctx: &SearchContext) -> String {
    crate::auth::credentials::preferred(&settings.youtube_api_key, &ctx.credentials.youtube_api_key)
}

#[async_trait]
impl SearchProvider for YoutubeSearch {
    async fn search(&self, req: &PerformSearchRequest, ctx: &SearchContext) -> MhResult<Value> {
        let settings = ctx.settings.read().await.clone();
        let (limit, _) = page_of(req, crate::services::common::limits::YOUTUBE_DATA_PAGE_MAX);
        let client = crate::services::youtube::api::YtSearchClient::new(yt_key(&settings, ctx))?;
        match req.search_type {
            SearchType::Playlist => client.search_playlists(&req.query, limit).await,
            SearchType::Channel | SearchType::Artist => {
                client.search_channels(&req.query, limit).await
            }
            _ => client.search_videos(&req.query, limit).await,
        }
    }
}
