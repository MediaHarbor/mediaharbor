use async_trait::async_trait;
use serde_json::Value;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract::PerformSearchRequest;
use crate::services::common::search::search_type_to_str;
use crate::services::common::search::{page_of, SearchContext, SearchProvider};
use crate::services::spotify::library::SpotifyLibrary;

pub struct SpotifySearch;

impl SpotifySearch {
    fn oauth_client(
        &self,
        settings: &Settings,
        ctx: &SearchContext,
    ) -> MhResult<crate::services::spotify::api::SpotifyApiClient> {
        let client_id = crate::auth::credentials::preferred(
            &settings.spotify_client_id,
            &ctx.credentials.spotify_client_id,
        );
        let client_secret = crate::auth::credentials::preferred(
            &settings.spotify_client_secret,
            &ctx.credentials.spotify_client_secret,
        );
        crate::services::spotify::api::SpotifyApiClient::new(client_id, client_secret)
    }
}

#[async_trait]
impl SearchProvider for SpotifySearch {
    async fn search(&self, req: &PerformSearchRequest, ctx: &SearchContext) -> MhResult<Value> {
        let logged_in = ctx.librespot.read().await.is_logged_in();
        if logged_in {
            let lib = SpotifyLibrary::from_parts(ctx.librespot.clone(), &ctx.user_data)?;
            return lib
                .search(&req.query, search_type_to_str(&req.search_type))
                .await;
        }
        let settings = ctx.settings.read().await.clone();
        let (limit, offset) = page_of(req, crate::services::common::limits::SPOTIFY_PAGE_MAX);
        let client = self.oauth_client(&settings, ctx)?;
        client
            .search(
                &req.query,
                map_spotify_type(&req.search_type),
                limit,
                offset,
            )
            .await
    }

    async fn suggestions(&self, query: &str, ctx: &SearchContext) -> MhResult<Vec<String>> {
        let logged_in = ctx.librespot.read().await.is_logged_in();
        if !logged_in {
            return Ok(Vec::new());
        }
        let lib = SpotifyLibrary::from_parts(ctx.librespot.clone(), &ctx.user_data)?;
        lib.search_suggestions(query).await
    }
}

fn map_spotify_type(
    t: &crate::ipc_contract::SearchType,
) -> crate::services::spotify::api::SpotifySearchType {
    use crate::ipc_contract::SearchType::*;
    match t {
        Album => crate::services::spotify::api::SpotifySearchType::Album,
        Artist => crate::services::spotify::api::SpotifySearchType::Artist,
        Playlist => crate::services::spotify::api::SpotifySearchType::Playlist,
        Episode => crate::services::spotify::api::SpotifySearchType::Episode,
        Podcast | Show => crate::services::spotify::api::SpotifySearchType::Show,
        Audiobook => crate::services::spotify::api::SpotifySearchType::Audiobook,
        _ => crate::services::spotify::api::SpotifySearchType::Track,
    }
}
