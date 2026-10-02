use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use tokio::sync::{mpsc, RwLock};

use crate::defaults::Settings;
use crate::ipc_contract;
use crate::media::library::Library;
use crate::EventEmitter;

/// Deserialize a download request, turning a bad payload straight into the
/// failure response every provider would otherwise hand-write.
pub fn parse_request<T: serde::de::DeserializeOwned>(
    req: serde_json::Value,
    download_id: u64,
) -> Result<T, ipc_contract::StartDownloadResponse> {
    serde_json::from_value(req).map_err(|e| {
        ipc_contract::StartDownloadResponse::failed(download_id, format!("bad request: {e}"))
    })
}

#[derive(Clone)]
pub struct DownloadContext {
    pub settings: Arc<RwLock<Settings>>,
    pub active_downloads: Arc<DashMap<u64, Arc<AtomicBool>>>,
    pub stdin_senders: Arc<DashMap<u64, mpsc::Sender<String>>>,
    pub emitter: Arc<dyn EventEmitter>,
    pub library: Arc<Library>,
    pub user_data: PathBuf,
}

impl DownloadContext {
    pub fn from_state(state: &crate::BackendState) -> Self {
        Self {
            settings: state.settings.clone(),
            active_downloads: state.active_downloads.clone(),
            stdin_senders: state.stdin_senders.clone(),
            emitter: state.emitter.clone(),
            library: state.library.clone(),
            user_data: state.user_data.clone(),
        }
    }

    pub fn emit_no_download_location(
        &self,
        download_id: u64,
    ) -> ipc_contract::StartDownloadResponse {
        const MSG: &str =
            "No download location configured. Please select a download folder in Settings.";
        self.emitter.emit_app_error(&ipc_contract::AppErrorEvent {
            message: MSG.to_string(),
            context: Some("download".to_string()),
            needs_auth: None,
        });
        ipc_contract::StartDownloadResponse::failed(download_id, MSG.to_string())
    }
}

/// A download the backend has agreed to run: the settings it will run under, and the
/// flag the UI's cancel button will set.
pub struct AcceptedDownload {
    pub settings: Settings,
    pub cancel_flag: Arc<AtomicBool>,
}

/// The checks and registration every provider does before it can spawn anything.
/// Registering the cancel flag here is what stops a provider from accidentally
/// starting a download the user has no way to stop. `Err` is the response to return
/// to the caller unchanged.
pub async fn accept_download(
    ctx: &DownloadContext,
    download_id: u64,
) -> Result<AcceptedDownload, ipc_contract::StartDownloadResponse> {
    let settings = ctx.settings.read().await.clone();
    if settings.download_location.is_empty() {
        return Err(ctx.emit_no_download_location(download_id));
    }
    let cancel_flag = Arc::new(AtomicBool::new(false));
    ctx.active_downloads
        .insert(download_id, cancel_flag.clone());
    Ok(AcceptedDownload {
        settings,
        cancel_flag,
    })
}

/// Per-item outcome of a multi-track download. Collecting failures rather than
/// keeping only the first one is what stops "2 of 6 downloaded" from being reported
/// as a plain success.
#[derive(Debug, Clone, Default)]
pub struct BatchOutcome {
    pub total: u32,
    pub succeeded: u32,
    pub skipped: u32,
    pub failures: Vec<ipc_contract::DownloadFailure>,
    pub dest_dir: Option<String>,
}

impl BatchOutcome {
    pub fn new(total: usize) -> Self {
        Self {
            total: total as u32,
            ..Default::default()
        }
    }

    pub fn record_success(&mut self) {
        self.succeeded += 1;
    }

    pub fn record_skip(&mut self) {
        self.skipped += 1;
    }

    /// Routes a finished track to the success or the skip counter.
    pub fn record(&mut self, track: &crate::services::common::pipeline::TrackOutcome) {
        if track.was_skipped() {
            self.record_skip();
        } else {
            self.record_success();
        }
    }

    pub fn with_dest(mut self, dir: &std::path::Path) -> Self {
        self.dest_dir = Some(dir.to_string_lossy().into_owned());
        self
    }

    pub fn record_failure(&mut self, id: &str, label: &str, reason: impl std::fmt::Display) {
        self.failures.push(ipc_contract::DownloadFailure {
            id: id.to_string(),
            label: label.to_string(),
            reason: reason.to_string(),
        });
    }

    /// Items that produced a file or were deliberately skipped.
    pub fn settled(&self) -> u32 {
        self.succeeded + self.skipped
    }

    /// Folds a nested run (e.g. one album of an artist download) into this one.
    pub fn absorb(&mut self, other: BatchOutcome) {
        self.total += other.total;
        self.succeeded += other.succeeded;
        self.skipped += other.skipped;
        self.failures.extend(other.failures);
        if self.dest_dir.is_none() {
            self.dest_dir = other.dest_dir;
        }
    }

    pub fn headline(&self, service: &str) -> String {
        let mut line = format!(
            "{}: {} of {} track(s) downloaded",
            service, self.succeeded, self.total
        );
        if self.skipped > 0 {
            line.push_str(&format!(", {} skipped", self.skipped));
        }
        if !self.failures.is_empty() {
            line.push_str(&format!(", {} failed", self.failures.len()));
        }
        line
    }

    pub fn failure_report(&self, service: &str) -> String {
        let mut report = self.headline(service);
        for f in &self.failures {
            report.push_str(&format!("\n  ✗ {}: {}", f.label, f.reason));
        }
        report
    }

    /// `Err` only when nothing at all landed; a partial run stays `Ok` and is
    /// reported through the summary so the finished files aren't thrown away.
    pub fn require_any(self, service: &str) -> crate::errors::MhResult<Self> {
        if self.settled() == 0 {
            let reason = self
                .failures
                .first()
                .map(|f| f.reason.clone())
                .unwrap_or_else(|| "see log for details".into());
            return Err(crate::errors::MhError::Other(format!(
                "No {service} tracks downloaded successfully. Reason: {reason}"
            )));
        }
        Ok(self)
    }

    pub fn to_event(&self, download_id: u64) -> ipc_contract::DownloadSummaryEvent {
        ipc_contract::DownloadSummaryEvent {
            download_id,
            succeeded: self.succeeded,
            skipped: self.skipped,
            failed: self.failures.len() as u32,
            total: self.total,
            failures: self.failures.clone(),
            dest_dir: self.dest_dir.clone(),
        }
    }
}

/// Report a failure that happened before the download task got going, through
/// the progress channel the UI is already watching.
///
/// Returns `ok`: the request itself was accepted, and the row shows the outcome
/// from the progress event rather than from the response.
pub fn emit_start_error(
    emitter: &Arc<dyn EventEmitter>,
    download_id: u64,
    msg: impl std::fmt::Display,
) -> ipc_contract::StartDownloadResponse {
    emitter.emit_progress(&ipc_contract::DownloadProgressEvent::error(
        download_id,
        msg,
    ));
    ipc_contract::StartDownloadResponse::ok(download_id)
}

pub fn always_info(_line: &str) -> &'static str {
    "info"
}

pub fn python_log_level(line: &str) -> &'static str {
    if line.contains("[CRITICAL") || line.contains("[ERROR") {
        "error"
    } else {
        "info"
    }
}

pub fn batch_progress_sink(
    emitter: &Arc<dyn EventEmitter>,
    download_id: u64,
) -> impl Fn(crate::downloads::BatchProgress) + Clone + Send + Sync + 'static {
    let emitter = emitter.clone();
    move |p: crate::downloads::BatchProgress| {
        emitter.emit_progress(&ipc_contract::DownloadProgressEvent {
            download_id,
            percent: p.percent,
            speed: p.speed.map(crate::downloads::format_speed),
            eta: p.eta.map(crate::downloads::format_eta),
            status: if p.current_track.is_empty() {
                "downloading".into()
            } else {
                p.current_track.clone()
            },
            item_index: Some(p.completed),
            item_total: Some(p.total),
            quality: p.quality.clone(),
        });
    }
}

pub fn log_sink(
    emitter: &Arc<dyn EventEmitter>,
    source: &'static str,
    title: &'static str,
    level_for: fn(&str) -> &'static str,
) -> impl Fn(String) + Clone + Send + Sync + 'static {
    let emitter = emitter.clone();
    move |line: String| {
        emitter.emit_log(&ipc_contract::BackendLogEvent::new(
            level_for(&line),
            source,
            title,
            line,
        ));
    }
}

#[async_trait]
pub trait DownloadProvider: Send + Sync {
    async fn start(
        &self,
        req: Value,
        ctx: &DownloadContext,
        download_id: u64,
    ) -> ipc_contract::StartDownloadResponse;
}
