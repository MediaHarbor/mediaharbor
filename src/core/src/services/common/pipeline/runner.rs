use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use crate::defaults::Settings;
use crate::downloads;
use crate::errors::{MhError, MhResult};
use crate::ipc_contract;
use crate::orpheus;
use crate::services::common::download::{AcceptedDownload, DownloadContext};
use crate::services::common::pipeline::orchestrator::TrackSourceClient;
use crate::{emit_terminal, make_log_buffer};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineRequest {
    pub url: String,
    pub output_dir: String,
    pub quality: Option<u8>,
    #[serde(default)]
    pub force_redownload: Option<bool>,
}

#[async_trait]
pub trait PipelineBackend: Send + Sync + 'static {
    fn platform(&self) -> &'static str;

    fn log_title(&self) -> &'static str;

    fn apply_quality(&self, settings: &mut Settings, quality: u8);

    async fn clients(
        &self,
        settings: &Settings,
        ctx: &DownloadContext,
    ) -> MhResult<Box<dyn TrackSourceClient>>;
}

/// What the progress bar is currently showing. Kept behind one lock so index,
/// total and label can never disagree.
#[derive(Clone, Default)]
struct ItemState {
    index: Option<u32>,
    total: Option<u32>,
    label: String,
}

fn accepted(download_id: u64) -> ipc_contract::StartDownloadResponse {
    ipc_contract::StartDownloadResponse::ok(download_id)
}

fn orpheus_handles(settings: &Settings, platform: &str) -> bool {
    settings.orpheus_dl
        && settings
            .orpheus_dl_enabled_modules
            .split(',')
            .any(|m| m.trim() == platform)
        && orpheus::is_orpheus_installed()
        && orpheus::is_module_installed(platform)
}

pub async fn run_pipeline_download(
    backend: Arc<dyn PipelineBackend>,
    req: Value,
    ctx: &DownloadContext,
    download_id: u64,
) -> ipc_contract::StartDownloadResponse {
    let req: PipelineRequest =
        match crate::services::common::download::parse_request(req, download_id) {
            Ok(r) => r,
            Err(resp) => return resp,
        };

    let AcceptedDownload {
        settings,
        cancel_flag,
    } = match crate::services::common::download::accept_download(ctx, download_id).await {
        Ok(a) => a,
        Err(resp) => return resp,
    };

    let platform = backend.platform();
    let platform_source: &'static str = platform;
    if orpheus_handles(&settings, platform) {
        let emitter = ctx.emitter.clone();
        let active = ctx.active_downloads.clone();
        let stdin_senders = ctx.stdin_senders.clone();
        let url = req.url.clone();
        let output_dir = req.output_dir.clone();
        let mut s = settings.clone();
        if let Some(q) = req.quality {
            backend.apply_quality(&mut s, q);
        }
        let (stdin_tx, stdin_rx) = tokio::sync::mpsc::channel::<String>(4);
        stdin_senders.insert(download_id, stdin_tx);
        tokio::spawn(async move {
            let _ = orpheus::run_orpheus_download(
                &url,
                &output_dir,
                platform,
                download_id,
                &s,
                cancel_flag,
                emitter,
                stdin_rx,
            )
            .await;
            active.remove(&download_id);
            stdin_senders.remove(&download_id);
        });
        return accepted(download_id);
    }

    let emitter = ctx.emitter.clone();
    let active = ctx.active_downloads.clone();
    let library_db = ctx.library.db().clone();
    let task_ctx = ctx.clone();
    tokio::spawn(async move {
        let mut settings = settings;
        settings.download_location = req.output_dir.clone();
        if let Some(q) = req.quality {
            backend.apply_quality(&mut settings, q);
        }

        let client = match backend.clients(&settings, &task_ctx).await {
            Ok(c) => c,
            Err(e) => {
                active.remove(&download_id);
                emit_terminal(&emitter, download_id, &Err(e));
                return;
            }
        };

        let cancel_clone = cancel_flag.clone();
        let dedup = downloads::dedup::DedupLedger::new(
            library_db,
            settings.native_skip_existing && !req.force_redownload.unwrap_or(false),
        );
        let (log_buf, buffer_log) = make_log_buffer();
        let stream_log = crate::services::common::download::log_sink(
            &emitter,
            platform_source,
            backend.log_title(),
            crate::services::common::download::always_info,
        );
        let on_log = move |line: String| {
            buffer_log(line.clone());
            stream_log(line);
        };

        let item: Arc<std::sync::Mutex<ItemState>> = Arc::new(std::sync::Mutex::new(ItemState {
            index: None,
            total: None,
            label: String::new(),
        }));
        let bytes = Arc::new(crate::downloads::ByteProgress::default());
        let quality_seen: Arc<std::sync::Mutex<Option<String>>> =
            Arc::new(std::sync::Mutex::new(None));
        let meter: Arc<std::sync::Mutex<crate::downloads::SpeedMeter>> =
            Arc::new(std::sync::Mutex::new(crate::downloads::SpeedMeter::new()));
        let started = std::time::Instant::now();
        let emit = crate::services::common::download::batch_progress_sink(&emitter, download_id);
        let result = tokio::select! {
            r = crate::services::common::pipeline::orchestrator::download_url(
                &req.url,
                &settings,
                &*client,
                {
                    let item = item.clone();
                    let bytes = bytes.clone();
                    let emit = emit.clone();
                    let quality_seen = quality_seen.clone();
                    let meter = meter.clone();
                    move |done: u64, total: u64| {
                        let pct = if total > 0 { (done as f32 / total as f32) * 100.0 } else { 0.0 };
                        let snapshot = item.lock().map(|g| g.clone()).unwrap_or_default();
                        let transferred = bytes.done.load(Ordering::Relaxed);
                        let speed = meter
                            .lock()
                            .ok()
                            .and_then(|mut m| m.sample(transferred))
                            .filter(|s| *s > 0.0);
                        let remaining = bytes
                            .total
                            .load(Ordering::Relaxed)
                            .saturating_sub(transferred);
                        let elapsed = started.elapsed().as_secs_f64();
                        let eta = match (speed, remaining) {
                            (Some(s), r) if r > 0 => Some(r as f64 / s),
                            _ if pct > 0.5 && elapsed > 1.0 => {
                                Some(elapsed * (100.0 - pct as f64) / pct as f64)
                            }
                            _ => None,
                        };
                        emit(crate::downloads::BatchProgress {
                            percent: pct,
                            current_track: snapshot.label,
                            completed: snapshot.index.unwrap_or(0),
                            total: snapshot.total.unwrap_or(0),
                            speed,
                            eta,
                            quality: quality_seen.lock().ok().and_then(|q| q.clone()),
                        });
                    }
                },
                on_log,
                {
                    let item = item.clone();
                    move |index: u32, total: u32, label: &str| {
                        if let Ok(mut g) = item.lock() {
                            *g = ItemState {
                                index: Some(index),
                                total: Some(total),
                                label: label.to_string(),
                            };
                        }
                    }
                },
                {
                    let quality_seen = quality_seen.clone();
                    move |served: String| {
                        if let Ok(mut g) = quality_seen.lock() {
                            *g = Some(served);
                        }
                    }
                },
                bytes.clone(),
                Some(&dedup),
            ) => r,
            _ = async {
                loop {
                    if cancel_clone.load(Ordering::Relaxed) { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                }
            } => Err(MhError::Cancelled),
        };

        active.remove(&download_id);
        let level = match &result {
            Err(_) => Some("error"),
            Ok(o) if !o.failures.is_empty() => Some("warning"),
            Ok(_) => None,
        };
        if let Some(level) = level {
            let log_lines = log_buf.lock().map(|v| v.join("\n")).unwrap_or_default();
            emitter.emit_log(&ipc_contract::BackendLogEvent::new(
                level,
                platform,
                backend.log_title(),
                log_lines,
            ));
        }
        crate::emit_batch_terminal(&emitter, download_id, backend.log_title(), &result);
    });

    accepted(download_id)
}
