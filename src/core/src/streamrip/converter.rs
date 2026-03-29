use std::path::Path;
use std::process::Stdio;
use tokio::process::Command;

use crate::errors::{MhError, MhResult};

const VALID_SAMPLING_RATES: &[u32] = &[44100, 48000, 88200, 96000, 176400, 192000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Flac,
    Alac,
    Mp3,
    Aac,
    Opus,
    Vorbis,
}

impl AudioCodec {
    fn lib_name(&self) -> &'static str {
        match self {
            AudioCodec::Flac => "flac",
            AudioCodec::Alac => "alac",
            AudioCodec::Mp3 => "libmp3lame",
            AudioCodec::Aac => "aac",
            AudioCodec::Opus => "libopus",
            AudioCodec::Vorbis => "libvorbis",
        }
    }

    pub fn container(&self) -> &'static str {
        match self {
            AudioCodec::Flac => "flac",
            AudioCodec::Alac => "m4a",
            AudioCodec::Mp3 => "mp3",
            AudioCodec::Aac => "m4a",
            AudioCodec::Opus => "opus",
            AudioCodec::Vorbis => "ogg",
        }
    }

    fn is_lossless(&self) -> bool {
        matches!(self, AudioCodec::Flac | AudioCodec::Alac)
    }
}

#[derive(Debug, Clone)]
pub struct ConversionSettings {
    pub codec: AudioCodec,
    pub sampling_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub lossy_bitrate: Option<u32>,
}

pub fn build_aformat(max_rate: Option<u32>, max_depth: Option<u32>) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();

    if let Some(hz) = max_rate {
        let allowed: Vec<String> = VALID_SAMPLING_RATES
            .iter()
            .filter(|&&r| r <= hz)
            .map(|r| r.to_string())
            .collect();
        if !allowed.is_empty() {
            parts.push(format!("sample_rates={}", allowed.join("|")));
        }
    }

    if let Some(depth) = max_depth {
        let fmts: &[&str] = if depth >= 24 {
            &["s32p", "s32", "s16p", "s16"]
        } else {
            &["s16p", "s16"]
        };
        parts.push(format!("sample_fmts={}", fmts.join("|")));
    }

    if parts.is_empty() {
        None
    } else {
        Some(format!("aformat={}", parts.join(":")))
    }
}

pub fn get_lossy_bitrate_args(codec: AudioCodec, kbps: u32) -> Vec<String> {
    match codec {
        AudioCodec::Mp3 => {
            const VBR: &[(u32, u32)] =
                &[(245, 0), (225, 1), (190, 2), (175, 3), (165, 4), (130, 5), (115, 6), (100, 7), (85, 8), (65, 9)];
            if kbps >= 320 {
                return vec!["-b:a".to_string(), "320k".to_string()];
            }
            for &(threshold, q) in VBR {
                if kbps >= threshold {
                    return vec!["-q:a".to_string(), q.to_string()];
                }
            }
            vec!["-q:a".to_string(), "9".to_string()]
        }
        _ => vec!["-b:a".to_string(), format!("{}k", kbps)],
    }
}

pub async fn convert_audio(
    input: &Path,
    output: &Path,
    settings: &ConversionSettings,
    ffmpeg: &str,
) -> MhResult<()> {
    let aformat = build_aformat(settings.sampling_rate, settings.bit_depth);

    if settings.codec == AudioCodec::Flac
        && input.extension().and_then(|e| e.to_str()) == Some("flac")
        && aformat.is_none()
        && input == output
    {
        return Ok(());
    }

    let tmp_path = {
        let mut p = output.to_path_buf();
        let ext = output
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("tmp");
        p.set_extension(format!("conv.tmp.{}", ext));
        p
    };

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
    ];

    if settings.codec.is_lossless() {
        args.push("-c:a".into());
        args.push(settings.codec.lib_name().into());
        if let Some(ref af) = aformat {
            args.push("-af".into());
            args.push(af.clone());
        }
        args.push("-c:v".into());
        args.push("copy".into());
    } else {
        args.push("-c:a".into());
        args.push(settings.codec.lib_name().into());
        let bitrate_args =
            get_lossy_bitrate_args(settings.codec, settings.lossy_bitrate.unwrap_or(320));
        args.extend(bitrate_args);
        if let Some(ref af) = aformat {
            args.push("-af".into());
            args.push(af.clone());
        }
        args.push("-vn".into());
    }

    args.push(tmp_path.to_string_lossy().into_owned());

    let output_status = Command::new(ffmpeg)
        .args(&args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .await
        .map_err(|e| MhError::Subprocess(format!("failed to spawn ffmpeg: {}", e)))?;

    if !output_status.success() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(MhError::Subprocess(format!(
            "converter: ffmpeg exited with code {}",
            output_status.code().unwrap_or(-1)
        )));
    }

    if input != output {
        let _ = tokio::fs::remove_file(input).await;
    }
    tokio::fs::rename(&tmp_path, output).await?;

    Ok(())
}
