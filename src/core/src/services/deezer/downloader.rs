use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::{MhError, MhResult};
use crate::services::common::download::DownloadContext;
use crate::services::common::pipeline::orchestrator::TrackSourceClient;
use crate::services::common::pipeline::runner::PipelineBackend;

#[derive(Clone)]
pub struct DeezerDownloader;

#[async_trait]
impl PipelineBackend for DeezerDownloader {
    fn platform(&self) -> &'static str {
        "deezer"
    }

    fn log_title(&self) -> &'static str {
        "Deezer"
    }

    fn apply_quality(&self, settings: &mut Settings, quality: u8) {
        settings.deezer_quality = match quality {
            2 => "FLAC".into(),
            1 => "MP3_320".into(),
            _ => "MP3_128".into(),
        };
    }

    async fn clients(
        &self,
        settings: &Settings,
        _ctx: &DownloadContext,
    ) -> MhResult<Box<dyn TrackSourceClient>> {
        if settings.deezer_arl.trim().is_empty() {
            return Err(MhError::Other(
                "Deezer ARL not set. Go to Settings → Deezer and paste your ARL token.".into(),
            ));
        }
        let client = crate::services::deezer::client::DeezerClient::new(&settings.deezer_arl)?;
        client.authenticate().await?;
        Ok(Box::new(client))
    }
}

crate::pipeline_provider!(DeezerDownloader);
