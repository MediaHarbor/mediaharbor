use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::process::Command;

use crate::downloads::native_common::MIN_PLAUSIBLE_MEDIA_BYTES;
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

    pub fn label(&self) -> &'static str {
        match self {
            AudioCodec::Flac => "FLAC",
            AudioCodec::Alac => "ALAC",
            AudioCodec::Mp3 => "MP3",
            AudioCodec::Aac => "AAC",
            AudioCodec::Opus => "Opus",
            AudioCodec::Vorbis => "Ogg Vorbis",
        }
    }

    pub fn is_lossless(&self) -> bool {
        matches!(self, AudioCodec::Flac | AudioCodec::Alac)
    }

    /// The highest rate the encoder accepts. ffmpeg resamples on its own for
    /// encoders that advertise `supported_samplerates` (mp3, aac, opus), but
    /// libvorbis advertises none and simply fails setup on anything above
    /// 48 kHz — so the rate has to be pinned here instead.
    fn max_sample_rate(&self) -> Option<u32> {
        match self {
            AudioCodec::Flac | AudioCodec::Alac => None,
            AudioCodec::Aac => Some(96_000),
            AudioCodec::Mp3 | AudioCodec::Opus | AudioCodec::Vorbis => Some(48_000),
        }
    }
}

/// Parses the `conversion_codec` setting. Returns `None` for anything
/// unrecognised so the caller can report it instead of silently picking FLAC.
pub fn parse_codec(s: &str) -> Option<AudioCodec> {
    match s.trim().to_uppercase().as_str() {
        "FLAC" => Some(AudioCodec::Flac),
        "ALAC" => Some(AudioCodec::Alac),
        "MP3" => Some(AudioCodec::Mp3),
        "AAC" => Some(AudioCodec::Aac),
        "OPUS" => Some(AudioCodec::Opus),
        "VORBIS" | "OGG" => Some(AudioCodec::Vorbis),
        _ => None,
    }
}

/// `m4a` deliberately yields `None`: the container carries AAC and ALAC alike,
/// so only the downloader that produced it knows which one is inside.
pub fn codec_from_ext(ext: &str) -> Option<AudioCodec> {
    match ext.trim().trim_start_matches('.').to_lowercase().as_str() {
        "flac" => Some(AudioCodec::Flac),
        "mp3" => Some(AudioCodec::Mp3),
        "opus" => Some(AudioCodec::Opus),
        "ogg" | "oga" => Some(AudioCodec::Vorbis),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct ConversionSettings {
    pub codec: AudioCodec,
    pub sampling_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub lossy_bitrate: Option<u32>,
}

/// What the downloader knows about the file it just wrote, for the cases the
/// probe cannot answer: `m4a` codec ambiguity, and Apple's spatial renditions
/// whose E-AC-3 payload lofty does not parse at all.
#[derive(Debug, Clone, Default)]
pub struct ConversionSource {
    pub codec: Option<AudioCodec>,
    pub surround: bool,
}

impl ConversionSource {
    pub fn from_ext(ext: &str) -> Self {
        Self {
            codec: codec_from_ext(ext),
            surround: false,
        }
    }

    pub fn with_codec(codec: AudioCodec) -> Self {
        Self {
            codec: Some(codec),
            surround: false,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbedAudio {
    pub codec: Option<AudioCodec>,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u32>,
    pub bitrate_kbps: Option<u32>,
    pub channels: Option<u32>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConversionPlan {
    Skip(String),
    Encode { note: Option<String> },
}

/// Reads codec and stream properties with lofty, which is already the tagging
/// backend, so no ffprobe subprocess is needed. An unreadable file yields an
/// empty probe and the caller falls back to its own hint.
pub fn probe_audio(path: &Path) -> ProbedAudio {
    let Ok(tagged) = lofty::read_from_path(path) else {
        return ProbedAudio::default();
    };
    use lofty::file::{AudioFile, FileType, TaggedFileExt};

    let codec = match tagged.file_type() {
        FileType::Flac => Some(AudioCodec::Flac),
        FileType::Mpeg => Some(AudioCodec::Mp3),
        FileType::Aac => Some(AudioCodec::Aac),
        FileType::Opus => Some(AudioCodec::Opus),
        FileType::Vorbis => Some(AudioCodec::Vorbis),
        _ => None,
    };

    let props = tagged.properties();
    ProbedAudio {
        codec,
        sample_rate: props.sample_rate(),
        bit_depth: props.bit_depth().map(u32::from),
        bitrate_kbps: props.audio_bitrate(),
        channels: props.channels().map(u32::from),
        duration_ms: Some(props.duration().as_millis() as u64).filter(|ms| *ms > 0),
    }
}

/// Decides whether the re-encode is worth doing. Guards against the three ways
/// "convert everything" quietly destroys audio: flattening spatial mixes,
/// re-encoding a codec into itself, and inflating lossy audio into a lossless
/// container under the impression that quality is recovered.
pub fn plan_conversion(
    source: &ConversionSource,
    probed: &ProbedAudio,
    settings: &ConversionSettings,
) -> ConversionPlan {
    let source_codec = source.codec.or(probed.codec);
    let surround = source.surround || probed.channels.map(|c| c > 2).unwrap_or(false);

    if surround {
        return ConversionPlan::Skip(
            "the source is a surround / Dolby Atmos mix, which no re-encode can preserve; \
             the original file was kept"
                .to_string(),
        );
    }

    let rate_binds = match (settings.sampling_rate, probed.sample_rate) {
        (Some(max), Some(have)) => have > max,
        (Some(_), None) => true,
        _ => false,
    };
    let depth_binds = match (settings.bit_depth, probed.bit_depth) {
        (Some(max), Some(have)) => have > max,
        (Some(_), None) => true,
        _ => false,
    };
    let caps_bind = rate_binds || depth_binds;

    if source_codec == Some(settings.codec) && !caps_bind {
        let target = settings.codec.label();
        if settings.codec.is_lossless() {
            return ConversionPlan::Skip(format!(
                "the file is already {target} within the requested limits"
            ));
        }
        let target_kbps = settings.lossy_bitrate.unwrap_or(320);
        if let Some(have) = probed.bitrate_kbps {
            if target_kbps >= have {
                return ConversionPlan::Skip(format!(
                    "the file is already {target} at {have} kbps; re-encoding it at \
                     {target_kbps} kbps would only lose quality"
                ));
            }
        }
    }

    if settings.codec.is_lossless() && source_codec.map(|c| !c.is_lossless()).unwrap_or(false) {
        let from = source_codec.map(|c| c.label()).unwrap_or("lossy");
        let rate = probed
            .bitrate_kbps
            .map(|k| format!(" {k} kbps"))
            .unwrap_or_default();
        return ConversionPlan::Encode {
            note: Some(format!(
                "converting {from}{rate} to {}: a lossless container cannot recover what the \
                 lossy encode discarded, and the file will grow",
                settings.codec.label()
            )),
        };
    }

    ConversionPlan::Encode { note: None }
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

/// LAME's VBR ladder: the average kbps each `-V` level targets. VBR at a given average
/// beats CBR at the same average, so a request inside this range is encoded as VBR.
const MP3_VBR: &[(u32, u32)] = &[
    (245, 0),
    (225, 1),
    (190, 2),
    (175, 3),
    (165, 4),
    (130, 5),
    (115, 6),
    (100, 7),
    (85, 8),
    (65, 9),
];

/// How an MP3 request is actually encoded, and what it will really weigh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mp3Plan {
    /// `-b:a`, honoured exactly.
    Cbr(u32),
    /// `-q:a`, which lands near `nominal` rather than at the number that was asked for.
    Vbr { q: u32, nominal: u32 },
}

impl Mp3Plan {
    /// What the output will weigh, which is the only number worth putting in a filename.
    pub fn effective_kbps(self) -> u32 {
        match self {
            Mp3Plan::Cbr(k) => k,
            Mp3Plan::Vbr { nominal, .. } => nominal,
        }
    }
}

/// The encoder settings an MP3 request maps to.
///
/// `-q:a 9` is the bottom of LAME's quality scale and still averages ~65 kbps, so a
/// request under that floor cannot be met by VBR at all — asking for 32 kbps used to
/// silently produce a ~74 kbps V9 file. Below the floor the request becomes an explicit
/// bitrate instead, which LAME honours down to 32 kbps at 44.1 kHz stereo.
pub fn mp3_plan(kbps: u32) -> Mp3Plan {
    if kbps >= 320 {
        return Mp3Plan::Cbr(320);
    }
    for &(nominal, q) in MP3_VBR {
        if kbps >= nominal {
            return Mp3Plan::Vbr { q, nominal };
        }
    }
    Mp3Plan::Cbr(kbps)
}

pub fn get_lossy_bitrate_args(codec: AudioCodec, kbps: u32) -> Vec<String> {
    match codec {
        AudioCodec::Mp3 => match mp3_plan(kbps) {
            Mp3Plan::Cbr(k) => vec!["-b:a".to_string(), format!("{k}k")],
            Mp3Plan::Vbr { q, .. } => vec!["-q:a".to_string(), q.to_string()],
        },
        _ => vec!["-b:a".to_string(), format!("{}k", kbps)],
    }
}

/// The bitrate a lossy request will really produce, for naming purposes.
pub fn effective_lossy_kbps(codec: AudioCodec, kbps: u32) -> u32 {
    match codec {
        AudioCodec::Mp3 => mp3_plan(kbps).effective_kbps(),
        _ => kbps,
    }
}

/// Names the in-flight output so the library watcher ignores it: a leading dot
/// and a `.tmp` suffix are both filtered by `media::library::watcher`.
fn temp_output_for(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("output");
    let mut p = output.to_path_buf();
    p.set_file_name(format!(".{name}.conv.tmp"));
    p
}

pub async fn convert_audio(
    input: &Path,
    output: &Path,
    settings: &ConversionSettings,
    ffmpeg: &str,
) -> MhResult<()> {
    let aformat = build_aformat(
        effective_rate_cap(settings),
        settings.bit_depth.filter(|_| settings.codec.is_lossless()),
    );
    let tmp_path = temp_output_for(output);

    let mut args: Vec<String> = vec![
        "-y".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        input.to_string_lossy().into_owned(),
        "-map".into(),
        "0:a:0".into(),
        "-c:a".into(),
        settings.codec.lib_name().into(),
    ];

    if !settings.codec.is_lossless() {
        args.extend(get_lossy_bitrate_args(
            settings.codec,
            settings.lossy_bitrate.unwrap_or(320),
        ));
    }

    if let Some(ref af) = aformat {
        args.push("-af".into());
        args.push(af.clone());
    }

    args.push("-f".into());
    args.push(muxer_for(settings.codec).into());
    args.push(tmp_path.to_string_lossy().into_owned());

    let mut ffmpeg_cmd = Command::new(ffmpeg);
    ffmpeg_cmd
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::subprocess::apply_no_window(&mut ffmpeg_cmd);

    let out = ffmpeg_cmd
        .output()
        .await
        .map_err(|e| MhError::Subprocess(format!("failed to spawn ffmpeg: {e}")))?;

    if !out.status.success() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(MhError::Subprocess(format!(
            "ffmpeg conversion to {} failed (exit {}): {}",
            settings.codec.label(),
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    let produced = tokio::fs::metadata(&tmp_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if produced < MIN_PLAUSIBLE_MEDIA_BYTES {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(MhError::Subprocess(format!(
            "ffmpeg conversion to {} produced only {produced} bytes from a {} byte source, so \
             the output holds no audio. ffmpeg said: {}",
            settings.codec.label(),
            tokio::fs::metadata(input)
                .await
                .map(|m| m.len())
                .unwrap_or(0),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }

    if input != output {
        tokio::fs::remove_file(input).await?;
    }
    tokio::fs::rename(&tmp_path, output).await?;

    Ok(())
}

/// The user's cap narrowed by whatever the target encoder can actually take.
fn effective_rate_cap(settings: &ConversionSettings) -> Option<u32> {
    match (settings.sampling_rate, settings.codec.max_sample_rate()) {
        (Some(user), Some(codec)) => Some(user.min(codec)),
        (Some(user), None) => Some(user),
        (None, codec) => codec,
    }
}

fn muxer_for(codec: AudioCodec) -> &'static str {
    match codec {
        AudioCodec::Flac => "flac",
        AudioCodec::Alac | AudioCodec::Aac => "mp4",
        AudioCodec::Mp3 => "mp3",
        AudioCodec::Opus => "opus",
        AudioCodec::Vorbis => "ogg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(codec: AudioCodec) -> ConversionSettings {
        ConversionSettings {
            codec,
            sampling_rate: None,
            bit_depth: None,
            lossy_bitrate: Some(320),
        }
    }

    #[test]
    fn aformat_is_absent_without_limits() {
        assert_eq!(build_aformat(None, None), None);
    }

    #[test]
    fn aformat_lists_only_rates_at_or_below_the_cap() {
        assert_eq!(
            build_aformat(Some(48000), None).unwrap(),
            "aformat=sample_rates=44100|48000"
        );
        assert_eq!(
            build_aformat(Some(192000), None).unwrap(),
            "aformat=sample_rates=44100|48000|88200|96000|176400|192000"
        );
    }

    #[test]
    fn aformat_bit_depth_splits_at_24() {
        assert_eq!(
            build_aformat(None, Some(16)).unwrap(),
            "aformat=sample_fmts=s16p|s16"
        );
        assert_eq!(
            build_aformat(None, Some(24)).unwrap(),
            "aformat=sample_fmts=s32p|s32|s16p|s16"
        );
        assert_eq!(
            build_aformat(Some(44100), Some(16)).unwrap(),
            "aformat=sample_rates=44100:sample_fmts=s16p|s16"
        );
    }

    #[test]
    fn mp3_bitrate_maps_onto_lame_vbr_tiers() {
        assert_eq!(
            get_lossy_bitrate_args(AudioCodec::Mp3, 320),
            vec!["-b:a", "320k"]
        );
        assert_eq!(
            get_lossy_bitrate_args(AudioCodec::Mp3, 245),
            vec!["-q:a", "0"]
        );
        assert_eq!(
            get_lossy_bitrate_args(AudioCodec::Mp3, 244),
            vec!["-q:a", "1"]
        );
        assert_eq!(
            get_lossy_bitrate_args(AudioCodec::Mp3, 65),
            vec!["-q:a", "9"]
        );
    }

    /// `-q:a 9` is the bottom of LAME's quality scale and still averages ~65 kbps, so a
    /// request under that floor cannot be met by VBR: asking for 32 kbps used to encode
    /// V9 and silently produce a ~74 kbps file. Under the floor it becomes a real
    /// bitrate, which LAME honours down to 32 kbps at 44.1 kHz stereo.
    #[test]
    fn an_mp3_request_below_the_vbr_floor_becomes_a_real_bitrate() {
        assert_eq!(mp3_plan(32), Mp3Plan::Cbr(32));
        assert_eq!(mp3_plan(64), Mp3Plan::Cbr(64));
        assert_eq!(
            get_lossy_bitrate_args(AudioCodec::Mp3, 32),
            vec!["-b:a", "32k"]
        );
        assert_eq!(effective_lossy_kbps(AudioCodec::Mp3, 32), 32);
    }

    /// Inside the ladder VBR is the better encode, but the number asked for is not the
    /// number that comes out — a filename has to carry the latter.
    #[test]
    fn an_mp3_request_inside_the_ladder_reports_the_average_it_lands_on() {
        assert_eq!(mp3_plan(200), Mp3Plan::Vbr { q: 2, nominal: 190 });
        assert_eq!(effective_lossy_kbps(AudioCodec::Mp3, 200), 190);
        assert_eq!(mp3_plan(65), Mp3Plan::Vbr { q: 9, nominal: 65 });
        assert_eq!(mp3_plan(320), Mp3Plan::Cbr(320));
        assert_eq!(mp3_plan(512), Mp3Plan::Cbr(320));
    }

    /// Every other encoder takes the bitrate literally, so nothing is reinterpreted.
    #[test]
    fn other_codecs_report_the_bitrate_they_were_given() {
        for codec in [AudioCodec::Aac, AudioCodec::Opus, AudioCodec::Vorbis] {
            assert_eq!(effective_lossy_kbps(codec, 96), 96);
        }
    }

    #[test]
    fn other_codecs_take_a_constant_bitrate() {
        assert_eq!(
            get_lossy_bitrate_args(AudioCodec::Opus, 128),
            vec!["-b:a", "128k"]
        );
    }

    #[test]
    fn codec_parsing_rejects_the_unknown_instead_of_defaulting() {
        assert_eq!(parse_codec("flac"), Some(AudioCodec::Flac));
        assert_eq!(parse_codec(" Vorbis "), Some(AudioCodec::Vorbis));
        assert_eq!(parse_codec("wav"), None);
        assert_eq!(parse_codec(""), None);
    }

    #[test]
    fn m4a_is_ambiguous_so_the_extension_yields_nothing() {
        assert_eq!(codec_from_ext("flac"), Some(AudioCodec::Flac));
        assert_eq!(codec_from_ext(".mp3"), Some(AudioCodec::Mp3));
        assert_eq!(codec_from_ext("ogg"), Some(AudioCodec::Vorbis));
        assert_eq!(codec_from_ext("m4a"), None);
    }

    #[test]
    fn surround_sources_are_never_re_encoded() {
        let probed = ProbedAudio::default();
        let src = ConversionSource {
            codec: None,
            surround: true,
        };
        assert!(matches!(
            plan_conversion(&src, &probed, &settings(AudioCodec::Flac)),
            ConversionPlan::Skip(_)
        ));

        let multichannel = ProbedAudio {
            channels: Some(6),
            ..Default::default()
        };
        assert!(matches!(
            plan_conversion(
                &ConversionSource::default(),
                &multichannel,
                &settings(AudioCodec::Mp3)
            ),
            ConversionPlan::Skip(_)
        ));
    }

    #[test]
    fn re_encoding_a_codec_into_itself_at_no_gain_is_skipped() {
        let probed = ProbedAudio {
            codec: None,
            sample_rate: Some(44100),
            bit_depth: None,
            bitrate_kbps: Some(256),
            channels: Some(2),
            duration_ms: None,
        };
        let src = ConversionSource::with_codec(AudioCodec::Aac);
        assert!(matches!(
            plan_conversion(&src, &probed, &settings(AudioCodec::Aac)),
            ConversionPlan::Skip(_)
        ));
    }

    #[test]
    fn a_genuine_bitrate_reduction_still_runs() {
        let probed = ProbedAudio {
            bitrate_kbps: Some(256),
            channels: Some(2),
            ..Default::default()
        };
        let src = ConversionSource::with_codec(AudioCodec::Aac);
        let mut s = settings(AudioCodec::Aac);
        s.lossy_bitrate = Some(128);
        assert!(matches!(
            plan_conversion(&src, &probed, &s),
            ConversionPlan::Encode { .. }
        ));
    }

    #[test]
    fn a_binding_sampling_cap_overrides_the_same_codec_shortcut() {
        let probed = ProbedAudio {
            codec: Some(AudioCodec::Flac),
            sample_rate: Some(96000),
            bit_depth: Some(24),
            channels: Some(2),
            ..Default::default()
        };
        let mut s = settings(AudioCodec::Flac);
        s.sampling_rate = Some(44100);
        assert!(matches!(
            plan_conversion(&ConversionSource::default(), &probed, &s),
            ConversionPlan::Encode { .. }
        ));

        s.sampling_rate = Some(192000);
        assert!(matches!(
            plan_conversion(&ConversionSource::default(), &probed, &s),
            ConversionPlan::Skip(_)
        ));
    }

    #[test]
    fn lossy_into_lossless_runs_but_says_why_it_is_pointless() {
        let probed = ProbedAudio {
            bitrate_kbps: Some(256),
            channels: Some(2),
            ..Default::default()
        };
        let src = ConversionSource::with_codec(AudioCodec::Aac);
        match plan_conversion(&src, &probed, &settings(AudioCodec::Flac)) {
            ConversionPlan::Encode { note: Some(n) } => {
                assert!(n.contains("AAC"), "{n}");
                assert!(n.contains("FLAC"), "{n}");
            }
            other => panic!("expected an advisory encode, got {other:?}"),
        }
    }

    #[test]
    fn vorbis_is_pinned_to_a_rate_its_encoder_accepts() {
        let mut s = settings(AudioCodec::Vorbis);
        s.sampling_rate = None;
        assert_eq!(effective_rate_cap(&s), Some(48_000));
        s.sampling_rate = Some(192_000);
        assert_eq!(effective_rate_cap(&s), Some(48_000));
        s.sampling_rate = Some(44_100);
        assert_eq!(effective_rate_cap(&s), Some(44_100));
    }

    #[test]
    fn lossless_targets_keep_the_users_cap_verbatim() {
        let mut s = settings(AudioCodec::Flac);
        assert_eq!(effective_rate_cap(&s), None);
        s.sampling_rate = Some(192_000);
        assert_eq!(effective_rate_cap(&s), Some(192_000));
    }

    fn which_ffmpeg() -> Option<String> {
        let candidate = crate::venv_manager::resolve_ffmpeg();
        std::process::Command::new(&candidate)
            .arg("-version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .ok()
            .filter(|s| s.success())
            .map(|_| candidate)
    }

    fn hires_flac_fixture(ffmpeg: &str, path: &Path) {
        let status = std::process::Command::new(ffmpeg)
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1:sample_rate=96000",
                "-sample_fmt",
                "s32",
                "-c:a",
                "flac",
            ])
            .arg(path)
            .status()
            .expect("spawn ffmpeg");
        assert!(status.success(), "ffmpeg could not build the fixture");
    }

    /// Every output format the settings offer, driven off a 96 kHz / 24-bit
    /// source — the case that used to make libvorbis fail encoder setup.
    #[tokio::test]
    async fn every_target_format_encodes_from_a_hi_res_source() {
        let Some(ffmpeg) = which_ffmpeg() else {
            return;
        };
        let dir = tempfile::tempdir().expect("tempdir");

        for codec in [
            AudioCodec::Flac,
            AudioCodec::Alac,
            AudioCodec::Mp3,
            AudioCodec::Aac,
            AudioCodec::Opus,
            AudioCodec::Vorbis,
        ] {
            let input = dir.path().join(format!("{}-src.flac", codec.label()));
            hires_flac_fixture(&ffmpeg, &input);
            let output = input.with_extension(codec.container());

            let mut s = settings(codec);
            s.sampling_rate = Some(44100);
            s.bit_depth = Some(16);
            s.lossy_bitrate = Some(192);

            convert_audio(&input, &output, &s, &ffmpeg)
                .await
                .unwrap_or_else(|e| panic!("{} conversion failed: {e}", codec.label()));

            let landed = std::fs::metadata(&output).expect("output exists").len();
            assert!(
                landed > MIN_PLAUSIBLE_MEDIA_BYTES,
                "{} produced only {landed} bytes",
                codec.label()
            );
            if input != output {
                assert!(!input.exists(), "{} left the source behind", codec.label());
            }
            assert!(
                std::fs::read_dir(dir.path())
                    .unwrap()
                    .filter_map(|e| e.ok())
                    .all(|e| !e.file_name().to_string_lossy().contains(".conv.tmp")),
                "{} left a temp file behind",
                codec.label()
            );
        }
    }

    #[tokio::test]
    async fn a_failed_encode_reports_what_ffmpeg_said_and_keeps_the_source() {
        let Some(ffmpeg) = which_ffmpeg() else {
            return;
        };
        let dir = tempfile::tempdir().expect("tempdir");
        let input = dir.path().join("not-audio.flac");
        std::fs::write(&input, b"this is not a media file at all").unwrap();
        let output = dir.path().join("not-audio.mp3");

        let err = convert_audio(&input, &output, &settings(AudioCodec::Mp3), &ffmpeg)
            .await
            .expect_err("garbage input must not convert");
        let msg = err.to_string();
        assert!(msg.contains("MP3"), "{msg}");
        assert!(msg.len() > 60, "the ffmpeg output was swallowed: {msg}");
        assert!(input.exists(), "the source was destroyed on failure");
        assert!(!output.exists());
    }

    #[test]
    fn the_temp_output_is_invisible_to_the_watcher() {
        let tmp = temp_output_for(Path::new("/music/01. Artist - Title.mp3"));
        let name = tmp.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with('.'), "{name}");
        assert!(name.ends_with(".tmp"), "{name}");
        assert_eq!(
            tmp.parent(),
            Path::new("/music/01. Artist - Title.mp3").parent()
        );
    }
}
