//! Driving the managed Python downloaders (gamdl, votify) as subprocesses.
//!
//! Everything service-specific — which binary, which config file, which flags — is
//! supplied by a [`CliTool`] impl living in that service's own directory. What is left
//! here is the part that was identical for every tool: spawn it out of the venv, fold
//! its output with [`dialect::BatchState`], throttle progress, honour cancellation, and
//! turn the exit code into an error.

pub mod dialect;
pub mod driver;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::defaults::Settings;
use crate::downloads::BatchProgress;
use crate::errors::MhResult;
use crate::ipc_contract;
use crate::services::common::download::{
    batch_progress_sink, log_sink, python_log_level, DownloadContext,
};
use crate::subprocess::{self, LineSource};

use dialect::BatchState;
use driver::Running;

/// How long to wait on the output channel before re-checking cancellation.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

/// One of the Python downloaders MediaHarbor manages in its venv.
#[async_trait]
pub trait CliTool: Send + Sync {
    /// Executable name, resolved against the managed venv at spawn time.
    fn program(&self) -> &'static str;

    /// Human-readable name for logs and the terminal download event.
    fn log_title(&self) -> &'static str;

    fn config_path(&self, user_data: &Path) -> PathBuf;

    /// Project MediaHarbor's settings into the INI dialect this tool reads.
    async fn write_config(&self, settings: &Settings, config_path: &Path) -> MhResult<()>;

    fn build_args(
        &self,
        settings: &Settings,
        url: &str,
        config_path: &Path,
        quality: Option<&str>,
    ) -> Vec<String>;

    /// Checked on the download task before the process is spawned. `Some(reason)` aborts
    /// the download having run nothing. It gets the full context because a preflight may
    /// need to prompt — signing a companion service in, say — which can take minutes and
    /// so must not happen before the IPC call has been answered.
    async fn preflight(
        &self,
        _settings: &Settings,
        _ctx: &DownloadContext,
        _download_id: u64,
    ) -> Option<String> {
        None
    }
}

/// Flags every one of these tools takes: where its config lives, and where to put the
/// files when downloads are filed per platform.
pub fn base_args(settings: &Settings, config_path: &Path, subfolder: &str) -> Vec<String> {
    let mut args = vec![
        "--config-path".to_string(),
        config_path.to_string_lossy().into_owned(),
    ];
    if settings.create_platform_subfolders {
        args.push("-o".to_string());
        args.push(
            Path::new(&settings.download_location)
                .join(subfolder)
                .to_string_lossy()
                .into_owned(),
        );
    }
    args
}

/// Run one download to completion, reporting through the two callbacks.
pub async fn run_cli_download(
    tool: &dyn CliTool,
    settings: &Settings,
    url: &str,
    quality: Option<&str>,
    config_path: &Path,
    on_progress: impl Fn(BatchProgress) + Send + 'static,
    on_log: impl Fn(String) + Send + 'static,
    cancel: Arc<AtomicBool>,
) -> MhResult<()> {
    let args = tool.build_args(settings, url, config_path, quality);

    let (tx, mut rx) = mpsc::channel::<(LineSource, String)>(1024);
    let mut handle = subprocess::spawn_venv_cli(tool.program(), &args, tx).await?;

    driver::drive(
        &mut BatchState::new(),
        Running::new(&mut handle, &mut rx, POLL_INTERVAL),
        &cancel,
        on_progress,
        on_log,
    )
    .await
}

/// Provider-level entry point: writes the tool's config, runs the download on its own
/// task, and reports through the events the UI is already watching. The caller has
/// already validated the request and registered `cancel` in `active_downloads`.
pub fn spawn_cli_download(
    tool: Arc<dyn CliTool>,
    ctx: &DownloadContext,
    download_id: u64,
    settings: Settings,
    url: String,
    quality: Option<String>,
    cancel: Arc<AtomicBool>,
) -> ipc_contract::StartDownloadResponse {
    let emitter = ctx.emitter.clone();
    let active = ctx.active_downloads.clone();
    let config_path = tool.config_path(&ctx.user_data);
    let ctx = ctx.clone();

    tokio::spawn(async move {
        if let Some(problem) = tool.preflight(&settings, &ctx, download_id).await {
            active.remove(&download_id);
            emitter.emit_progress(&ipc_contract::DownloadProgressEvent::error(
                download_id,
                problem,
            ));
            return;
        }

        let on_log = log_sink(&emitter, tool.program(), tool.log_title(), python_log_level);

        // A config that could not be written leaves whatever is already on disk, which
        // may still be usable — so this is reported rather than fatal.
        if let Err(e) = tool.write_config(&settings, &config_path).await {
            on_log(format!(
                "[ERROR] could not write {}: {e}",
                config_path.display()
            ));
        }

        let result = run_cli_download(
            tool.as_ref(),
            &settings,
            &url,
            quality.as_deref(),
            &config_path,
            batch_progress_sink(&emitter, download_id),
            on_log,
            cancel,
        )
        .await;

        active.remove(&download_id);
        crate::emit_terminal(&emitter, download_id, &result);
    });

    ipc_contract::StartDownloadResponse::ok(download_id)
}
