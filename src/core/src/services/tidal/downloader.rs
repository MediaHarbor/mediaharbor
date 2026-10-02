use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::services::common::download::DownloadContext;
use crate::services::common::pipeline::orchestrator::TrackSourceClient;
use crate::services::common::pipeline::runner::PipelineBackend;

#[derive(Clone)]
pub struct TidalDownloader;

#[async_trait]
impl PipelineBackend for TidalDownloader {
    fn platform(&self) -> &'static str {
        "tidal"
    }

    fn log_title(&self) -> &'static str {
        "Tidal"
    }

    fn apply_quality(&self, settings: &mut Settings, quality: u8) {
        settings.tidal_quality = quality;
    }

    async fn clients(
        &self,
        settings: &Settings,
        ctx: &DownloadContext,
    ) -> MhResult<Box<dyn TrackSourceClient>> {
        let client = crate::services::tidal::client::TidalClient::authenticate_and_persist(
            settings,
            &ctx.settings,
            &ctx.user_data,
        )
        .await?;
        Ok(Box::new(client))
    }
}

crate::pipeline_provider!(TidalDownloader);
