use async_trait::async_trait;
use serde_json::Value;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract::PerformSearchRequest;
use crate::services::common::search::search_type_to_str;
use crate::services::common::search::{page_of, SearchContext, SearchProvider};

pub struct QobuzSearch;

fn qobuz_client(settings: &Settings) -> MhResult<crate::services::qobuz::api::QobuzApiClient> {
    let pair = crate::services::qobuz::app_credentials::configured_pair(settings);
    match pair {
        Some(pair) if !settings.qobuz_password_or_token.trim().is_empty() => {
            crate::services::qobuz::api::QobuzApiClient::new(
                pair.app_id,
                settings.qobuz_password_or_token.clone(),
                pair.secret,
            )
        }
        _ => crate::services::qobuz::api::QobuzApiClient::with_bundled_credentials(),
    }
}

#[async_trait]
impl SearchProvider for QobuzSearch {
    async fn search(&self, req: &PerformSearchRequest, ctx: &SearchContext) -> MhResult<Value> {
        let settings = ctx.settings.read().await.clone();
        let (limit, offset) = page_of(req, crate::services::common::limits::QOBUZ_PAGE_MAX);
        let client = qobuz_client(&settings)?;
        client
            .search(
                &req.query,
                search_type_to_str(&req.search_type),
                limit,
                offset,
            )
            .await
    }

    async fn suggestions(&self, query: &str, ctx: &SearchContext) -> MhResult<Vec<String>> {
        let settings = ctx.settings.read().await.clone();
        let client = qobuz_client(&settings)?;
        client.suggestions(query).await
    }
}
