use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::services::common::download::DownloadContext;
use crate::services::common::pipeline::orchestrator::TrackSourceClient;
use crate::services::common::pipeline::runner::PipelineBackend;

#[derive(Clone)]
pub struct QobuzDownloader {
    client_cache: Arc<RwLock<Option<crate::services::qobuz::client::QobuzClient>>>,
}

impl QobuzDownloader {
    pub fn from_state(state: &crate::BackendState) -> Self {
        Self {
            client_cache: state.qobuz_client_cache.clone(),
        }
    }
}

#[async_trait]
impl PipelineBackend for QobuzDownloader {
    fn platform(&self) -> &'static str {
        "qobuz"
    }

    fn log_title(&self) -> &'static str {
        "Qobuz"
    }

    fn apply_quality(&self, settings: &mut Settings, quality: u8) {
        settings.qobuz_quality = quality;
    }

    async fn clients(
        &self,
        settings: &Settings,
        _ctx: &DownloadContext,
    ) -> MhResult<Box<dyn TrackSourceClient>> {
        let cached = self.client_cache.read().await.clone();
        let client = match cached {
            Some(c) => c,
            None => {
                let c = crate::services::qobuz::client::QobuzClient::authenticate(settings).await?;
                *self.client_cache.write().await = Some(c.clone());
                c
            }
        };
        Ok(Box::new(client))
    }
}

crate::pipeline_provider!(QobuzDownloader);
