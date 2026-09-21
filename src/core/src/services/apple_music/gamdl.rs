//! gamdl, the Apple Music CLI downloader, as a [`CliTool`].

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::defaults::Settings;
use crate::errors::MhResult;
use crate::ipc_contract;
use crate::services::common::cli::{base_args, CliTool};
use crate::services::common::download::DownloadContext;
use crate::settings;

/// Codecs gamdl accepts for `--song-codec-priority`.
const VALID_CODECS: &[&str] = &[
    "aac-web",
    "aac-he-web",
    "aac",
    "aac-he",
    "aac-binaural",
    "aac-downmix",
    "aac-he-binaural",
    "aac-he-downmix",
    "atmos",
    "ac3",
    "alac",
    "ask",
];

/// Translate MediaHarbor's quality ids into gamdl's codec names. The native engine and
/// gamdl name the same codec differently, and gamdl aborts on an unknown value, so an
/// unrecognised id falls back to plain AAC rather than failing the download.
fn normalize_song_codec(quality: Option<&str>) -> Option<String> {
    let raw = quality?.trim();
    if raw.is_empty() {
        return None;
    }
    let q = raw.to_ascii_lowercase();

    let mapped = match q.as_str() {
        "aac-256" | "aac-legacy" | "aac-high" => "aac-web",
        "aac-he-64" | "aac-he-legacy" | "aac-medium" => "aac-he-web",
        "aac-256-binaural" => "aac-binaural",
        "aac-256-downmix" => "aac-downmix",
        "aac-he-64-binaural" => "aac-he-binaural",
        "aac-he-64-downmix" => "aac-he-downmix",
        "lossless" | "hi-res" | "hires" | "flac" => "alac",
        other if VALID_CODECS.contains(&other) => other,
        _ => "aac-web",
    };

    Some(mapped.to_string())
}

/// How long to leave the two-factor dialog open before giving up. Apple's codes
/// expire on their own, so this only bounds a download that was abandoned.
const TWOFA_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);
const TWOFA_ATTEMPTS: usize = 3;

/// Makes sure a configured wrapper is running and signed in, prompting for a
/// two-factor code through the same dialog the interactive CLIs use.
/// Returns `Some(reason)` when the download cannot proceed.
async fn ensure_wrapper_ready(
    settings: &Settings,
    ctx: &DownloadContext,
    download_id: u64,
) -> Option<String> {
    use crate::services::apple_music::wrapper::{LoginOutcome, WrapperClient, WrapperConfig};

    let emitter = &ctx.emitter;
    let stdin_senders = &ctx.stdin_senders;

    let cfg = WrapperConfig::from_settings(settings)?;
    let client = match WrapperClient::new(cfg) {
        Ok(c) => c,
        Err(e) => return Some(e.to_string()),
    };

    match client.status().await {
        Ok(s) if s.authenticated => return None,
        Ok(_) => {}
        Err(e) => return Some(e.to_string()),
    }

    let email = settings.apple_wrapper_email.trim();
    if email.is_empty() || settings.apple_wrapper_password.is_empty() {
        return Some(
            "wrapper-v2 is running but not signed in, and no Apple ID is configured. \
             Add your Apple ID and password in Settings → Apple Music → Wrapper."
                .to_string(),
        );
    }

    let mut outcome = match client.login(email, &settings.apple_wrapper_password).await {
        Ok(o) => o,
        Err(e) => return Some(e.to_string()),
    };

    for attempt in 1..=TWOFA_ATTEMPTS {
        match outcome {
            LoginOutcome::Authenticated => return None,
            LoginOutcome::Failed(reason) => return Some(reason),
            LoginOutcome::Needs2fa => {}
        }

        let mut lines = vec![
            "Apple sent a verification code to your trusted devices.".to_string(),
            format!("Enter it to finish signing {email} in to the wrapper:"),
        ];
        if attempt > 1 {
            lines.insert(
                0,
                format!("That code was not accepted — attempt {attempt} of {TWOFA_ATTEMPTS}."),
            );
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(4);
        stdin_senders.insert(download_id, tx);
        emitter.emit_stdin_prompt(&ipc_contract::ProcessStdinPromptEvent {
            download_id,
            prompt_lines: lines,
        });
        let received = tokio::time::timeout(TWOFA_TIMEOUT, rx.recv()).await;
        stdin_senders.remove(&download_id);

        let code = match received {
            Ok(Some(code)) => code,
            Ok(None) => return Some("The two-factor prompt was dismissed.".to_string()),
            Err(_) => {
                return Some("Timed out waiting for the Apple verification code.".to_string())
            }
        };
        if code.trim().is_empty() {
            return Some("No verification code was entered.".to_string());
        }

        outcome = match client.submit_2fa(&code).await {
            Ok(o) => o,
            Err(e) => return Some(e.to_string()),
        };
    }

    Some(format!(
        "wrapper-v2 rejected the verification code {TWOFA_ATTEMPTS} times."
    ))
}

pub struct Gamdl;

#[async_trait]
impl CliTool for Gamdl {
    fn program(&self) -> &'static str {
        "gamdl"
    }

    fn log_title(&self) -> &'static str {
        "gamdl"
    }

    fn config_path(&self, user_data: &Path) -> PathBuf {
        settings::apple_config_path(user_data)
    }

    async fn write_config(&self, settings: &Settings, config_path: &Path) -> MhResult<()> {
        settings::save_service_config(config_path, settings, "apple").await
    }

    fn build_args(
        &self,
        settings: &Settings,
        url: &str,
        config_path: &Path,
        quality: Option<&str>,
    ) -> Vec<String> {
        let mut args = base_args(settings, config_path, "Apple Music");

        if let Some(q) = normalize_song_codec(quality) {
            args.push("--song-codec-priority".into());
            args.push(q);
        }

        if !url.is_empty() {
            args.push(url.to_string());
        }
        args
    }

    async fn preflight(
        &self,
        settings: &Settings,
        ctx: &DownloadContext,
        download_id: u64,
    ) -> Option<String> {
        ensure_wrapper_ready(settings, ctx, download_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args_for(quality: Option<&str>) -> Vec<String> {
        let settings = Settings::default();
        let config = std::env::temp_dir().join("gamdl_config.ini");
        Gamdl.build_args(
            &settings,
            "https://music.apple.com/album/123",
            &config,
            quality,
        )
    }

    #[test]
    fn args_have_config_and_url() {
        let config = std::env::temp_dir().join("gamdl_config.ini");
        let args = args_for(None);
        assert!(args.contains(&"--config-path".to_string()));
        assert!(args.contains(&config.to_string_lossy().to_string()));
        assert_eq!(args.last().unwrap(), "https://music.apple.com/album/123");
    }

    #[test]
    fn args_normalize_codec_priority() {
        let args = args_for(Some("aac-256"));
        let idx = args
            .iter()
            .position(|a| a == "--song-codec-priority")
            .expect("codec flag present");
        assert_eq!(args[idx + 1], "aac-web");
    }

    #[test]
    fn normalize_codec_maps_native_and_legacy_labels() {
        assert_eq!(
            normalize_song_codec(Some("aac-256")).as_deref(),
            Some("aac-web")
        );
        assert_eq!(
            normalize_song_codec(Some("aac-legacy")).as_deref(),
            Some("aac-web")
        );
        assert_eq!(
            normalize_song_codec(Some("aac-he-64")).as_deref(),
            Some("aac-he-web")
        );
        assert_eq!(
            normalize_song_codec(Some("aac-he-legacy")).as_deref(),
            Some("aac-he-web")
        );
        assert_eq!(
            normalize_song_codec(Some("lossless")).as_deref(),
            Some("alac")
        );
    }

    #[test]
    fn normalize_codec_passes_through_valid_and_defaults_unknown() {
        assert_eq!(normalize_song_codec(Some("alac")).as_deref(), Some("alac"));
        assert_eq!(
            normalize_song_codec(Some("atmos")).as_deref(),
            Some("atmos")
        );
        assert_eq!(
            normalize_song_codec(Some("AAC-HE")).as_deref(),
            Some("aac-he")
        );
        assert_eq!(
            normalize_song_codec(Some("garbage")).as_deref(),
            Some("aac-web")
        );
        assert_eq!(normalize_song_codec(None), None);
        assert_eq!(normalize_song_codec(Some("  ")), None);
    }

    #[test]
    fn native_spatial_ids_keep_their_channel_layout() {
        for (native, gamdl) in [
            ("aac-256-binaural", "aac-binaural"),
            ("aac-256-downmix", "aac-downmix"),
            ("aac-he-64-binaural", "aac-he-binaural"),
            ("aac-he-64-downmix", "aac-he-downmix"),
        ] {
            assert_eq!(
                normalize_song_codec(Some(native)).as_deref(),
                Some(gamdl),
                "{native} must not collapse to plain stereo"
            );
        }
    }
}
