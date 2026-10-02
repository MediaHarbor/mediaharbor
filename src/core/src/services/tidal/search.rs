use async_trait::async_trait;
use serde_json::Value;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract::{self, PerformSearchRequest};
use crate::services::common::search::{SearchContext, SearchProvider};

pub struct TidalSearch;

fn api_client(
    settings: &Settings,
    ctx: &SearchContext,
) -> MhResult<crate::services::tidal::api::TidalApiClient> {
    let client_id = crate::auth::credentials::preferred(
        &settings.tidal_client_id,
        &ctx.credentials.tidal_client_id,
    );
    let client_secret = crate::auth::credentials::preferred(
        &settings.tidal_client_secret,
        &ctx.credentials.tidal_client_secret,
    );
    crate::services::tidal::api::TidalApiClient::new(client_id, client_secret)
}

async fn authenticate(
    settings: &Settings,
    ctx: &SearchContext,
) -> MhResult<crate::services::tidal::client::TidalClient> {
    crate::services::tidal::client::TidalClient::authenticate_and_persist(
        settings,
        &ctx.settings,
        &ctx.user_data,
    )
    .await
}

#[async_trait]
impl SearchProvider for TidalSearch {
    async fn search(&self, req: &PerformSearchRequest, ctx: &SearchContext) -> MhResult<Value> {
        let settings = ctx.settings.read().await.clone();
        let client = api_client(&settings, ctx)?;
        if !settings.tidal_access_token.is_empty() {
            match client
                .search_v1(
                    &req.query,
                    map_tidal_type(&req.search_type),
                    &settings.tidal_country_code,
                    &settings.tidal_access_token,
                )
                .await
            {
                Ok(r) => Ok(r),
                Err(e)
                    if e.to_string().contains("401") || e.to_string().contains("Unauthorized") =>
                {
                    if !settings.tidal_refresh_token.is_empty() {
                        if let Ok(tidal_client) = authenticate(&settings, ctx).await {
                            if let Ok(r) = client
                                .search_v1(
                                    &req.query,
                                    map_tidal_type(&req.search_type),
                                    &settings.tidal_country_code,
                                    &tidal_client.access_token,
                                )
                                .await
                            {
                                return Ok(r);
                            }
                        }
                    }
                    ctx.emitter.emit_app_error(&ipc_contract::AppErrorEvent {
                        message: "Tidal session expired — please sign in again.".into(),
                        context: Some("tidal_search".into()),
                        needs_auth: Some("tidal".into()),
                    });
                    client
                        .search_v2(
                            &req.query,
                            map_tidal_type(&req.search_type),
                            &settings.tidal_country_code,
                        )
                        .await
                }
                Err(e) => Err(e),
            }
        } else {
            client
                .search_v2(
                    &req.query,
                    map_tidal_type(&req.search_type),
                    &settings.tidal_country_code,
                )
                .await
        }
    }

    async fn suggestions(&self, query: &str, ctx: &SearchContext) -> MhResult<Vec<String>> {
        let settings = ctx.settings.read().await.clone();
        let client = api_client(&settings, ctx)?;
        client
            .suggestions(query, &settings.tidal_country_code)
            .await
    }
}

fn map_tidal_type(
    t: &crate::ipc_contract::SearchType,
) -> crate::services::tidal::api::TidalSearchType {
    use crate::ipc_contract::SearchType::*;
    match t {
        Album => crate::services::tidal::api::TidalSearchType::Albums,
        Artist => crate::services::tidal::api::TidalSearchType::Artists,
        Playlist => crate::services::tidal::api::TidalSearchType::Playlists,
        Video => crate::services::tidal::api::TidalSearchType::Videos,
        _ => crate::services::tidal::api::TidalSearchType::Tracks,
    }
}
