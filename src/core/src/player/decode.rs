use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::audio::well_known::{
    CODEC_ID_AAC, CODEC_ID_AC3, CODEC_ID_AC4, CODEC_ID_ADPCM_IMA_QT, CODEC_ID_ADPCM_IMA_WAV,
    CODEC_ID_ADPCM_MS, CODEC_ID_ALAC, CODEC_ID_ATRAC3, CODEC_ID_ATRAC3PLUS, CODEC_ID_ATRAC9,
    CODEC_ID_COOK, CODEC_ID_DCA, CODEC_ID_EAC3, CODEC_ID_FLAC, CODEC_ID_MONKEYS_AUDIO,
    CODEC_ID_MP1, CODEC_ID_MP2, CODEC_ID_MP3, CODEC_ID_MUSEPACK, CODEC_ID_OPUS, CODEC_ID_SPEEX,
    CODEC_ID_TRUEHD, CODEC_ID_TTA, CODEC_ID_VORBIS, CODEC_ID_WAVPACK, CODEC_ID_WMA,
};
use symphonia::core::codecs::audio::{AudioCodecId, AudioDecoder, AudioDecoderOptions};
use symphonia::core::codecs::CodecId;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::{Time, TimeBase, Timestamp};

use crate::errors::{MhError, MhResult};
use crate::ipc_contract::UndecodableStream;

/// Probe options tuned for scrubbing.
///
/// Symphonia's default fills the seek index every 1000 ms and its own docs say
/// that "should be decreased" for highly-interactive applications — a seek bar
/// over a local MP3 with no container index is exactly that case. The struct is
/// `#[non_exhaustive]`, so it has to be built by mutation rather than literal.
fn format_opts() -> FormatOptions {
    let mut opts = FormatOptions::default();
    opts.seek_index_fill_period_ms = 250;
    opts
}

/// Human-readable codec name for error messages, so a rejected stream says
/// which codec it was rather than an opaque numeric id.
fn codec_name(id: AudioCodecId) -> Option<&'static str> {
    Some(match id {
        CODEC_ID_FLAC => "FLAC",
        CODEC_ID_MP1 => "MP1",
        CODEC_ID_MP2 => "MP2",
        CODEC_ID_MP3 => "MP3",
        CODEC_ID_AAC => "AAC",
        CODEC_ID_ALAC => "ALAC",
        CODEC_ID_VORBIS => "Vorbis",
        CODEC_ID_OPUS => "Opus",
        CODEC_ID_SPEEX => "Speex",
        CODEC_ID_MUSEPACK => "Musepack",
        CODEC_ID_WAVPACK => "WavPack",
        CODEC_ID_MONKEYS_AUDIO => "Monkey's Audio",
        CODEC_ID_TTA => "TTA",
        CODEC_ID_AC3 => "AC-3",
        CODEC_ID_EAC3 => "E-AC-3",
        CODEC_ID_AC4 => "AC-4",
        CODEC_ID_DCA => "DTS",
        CODEC_ID_TRUEHD => "TrueHD",
        CODEC_ID_WMA => "WMA",
        CODEC_ID_ATRAC3 => "ATRAC3",
        CODEC_ID_ATRAC3PLUS => "ATRAC3plus",
        CODEC_ID_ATRAC9 => "ATRAC9",
        CODEC_ID_COOK => "RealAudio Cook",
        CODEC_ID_ADPCM_MS | CODEC_ID_ADPCM_IMA_WAV | CODEC_ID_ADPCM_IMA_QT => "ADPCM",
        _ => return None,
    })
}

/// [`codec_name`], falling back to symphonia's id so an unnamed codec is still
/// identifiable from a report.
fn codec_label(id: AudioCodecId) -> String {
    codec_name(id)
        .map(str::to_string)
        .unwrap_or_else(|| format!("codec id {id}"))
}

/// Reject a stream the bundled decoders cannot handle, naming the codec.
///
/// The check asks symphonia's registry rather than a hand-maintained list, so
/// it stays accurate if the enabled codec features ever change. Playback that
/// silently produces no audio is the failure mode this exists to prevent.
pub fn ensure_supported(id: CodecId) -> MhResult<()> {
    let CodecId::Audio(audio) = id else {
        return Err(MhError::Unsupported(
            "stream carries no audio codec".to_string(),
        ));
    };

    if audio == CODEC_ID_OPUS
        || symphonia::default::get_codecs()
            .get_audio_decoder(audio)
            .is_some()
    {
        return Ok(());
    }

    Err(MhError::Unsupported(format!(
        "no decoder for {}",
        codec_label(audio)
    )))
}

/// Name the container from its first bytes.
///
/// A failed probe names nothing, and a stream from a service has no file
/// extension — so without this such a report says nothing about the format at
/// all. The non-audio answers matter as much: a playlist, web page or JSON body
/// where audio was expected means the service sent the wrong thing, not that a
/// decoder is missing.
fn sniff_container(head: &[u8]) -> Option<String> {
    let at = |off: usize, magic: &[u8]| head.get(off..off + magic.len()) == Some(magic);
    let sync = |mask: u8, want: u8| head.len() >= 2 && head[0] == 0xFF && head[1] & mask == want;

    if at(4, b"ftyp") {
        let brand = head
            .get(8..12)
            .map(|b| String::from_utf8_lossy(b).trim().to_string())
            .filter(|b| !b.is_empty());
        return Some(match brand {
            Some(brand) => format!("MP4 ({brand})"),
            None => "MP4".to_string(),
        });
    }

    let name = if at(0, b"fLaC") {
        "FLAC"
    } else if at(0, b"OggS") {
        "Ogg"
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        "WAV"
    } else if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
        "AIFF"
    } else if at(0, b"caff") {
        "CAF"
    } else if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        "Matroska/WebM"
    } else if at(0, &oxideav_ape::MAGIC) {
        "APE"
    } else if at(0, b"TTA1") {
        "TTA"
    } else if at(0, b"MPCK") || at(0, b"MP+") {
        "Musepack"
    } else if at(0, b"wvpk") {
        "WavPack"
    } else if at(0, b"ID3") {
        "ID3-tagged audio"
    } else if sync(0xF6, 0xF0) {
        "AAC (ADTS)"
    } else if sync(0xE0, 0xE0) {
        "MPEG audio"
    } else if at(0, b"#EXTM3U") {
        "HLS playlist, not audio"
    } else {
        let text = head.trim_ascii_start();
        if text.starts_with(b"<") {
            "HTML/XML, not audio"
        } else if text.starts_with(b"{") || text.starts_with(b"[") {
            "JSON, not audio"
        } else {
            return None;
        }
    };
    Some(name.to_string())
}

/// The message inside an error, without the variant's label — the report's own
/// `Error:` line already says what kind of line it is.
fn bare_message(err: MhError) -> String {
    match err {
        MhError::Unsupported(m) | MhError::Parse(m) | MhError::Other(m) => m,
        other => other.to_string(),
    }
}

/// What is known about a stream so far, gathered as [`StreamDecoder::new`]
/// gets further into it, so whichever step gives up reports all of it.
///
/// The library scanner deliberately keeps indexing formats there is no decoder
/// for, so these reports are the only signal of which codec is worth
/// implementing next — and for a service stream, the only sign that the stream
/// itself is wrong.
#[derive(Default)]
struct Findings<'a> {
    url: Option<&'a str>,
    container: Option<String>,
    codec: Option<AudioCodecId>,
    sample_rate: Option<u32>,
    channels: Option<usize>,
}

impl Findings<'_> {
    fn undecodable(&self, detail: impl Into<String>) -> MhError {
        MhError::Undecodable(Box::new(UndecodableStream {
            url: self.url.unwrap_or_default().to_string(),
            detail: detail.into(),
            container: self.container.clone(),
            codec: self.codec.map(codec_label),
            sample_rate: self.sample_rate,
            channels: self.channels.map(|c| c as u32),
            app: format!(
                "MediaHarbor {} · {} {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        }))
    }
}

/// Build a RIFF/WAVE wrapper around already-decoded interleaved PCM.
fn wav_container(pcm: &[u8], rate: u32, channels: u16, bits: u16) -> Vec<u8> {
    let block_align = channels * bits / 8;
    let byte_rate = rate * u32::from(block_align);
    let mut out = Vec::with_capacity(pcm.len() + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36u32 + pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    out.extend_from_slice(pcm);
    out
}

/// Decode a format symphonia has no demuxer for, returning it as WAV bytes.
///
/// APE, TTA and friends are codec and container in one, and the oxideav
/// decoders for them are whole-file adapters rather than packet streams — so
/// they cannot join the normal `FormatReader` loop. Decoding once here and
/// handing symphonia in-memory WAV keeps `next_chunk`, `seek` and `recycle`
/// working unchanged, at the cost of holding the decoded track in memory.
///
/// Returns `None` when the bytes are not a format this handles, so the caller
/// falls through to the normal probe.
fn is_standalone(head: &[u8]) -> bool {
    if head.starts_with(b"TTA1")
        || head.starts_with(b"MPCK")
        || head.starts_with(&oxideav_ape::MAGIC)
    {
        return true;
    }
    // Ogg carries several codecs and symphonia demuxes the ones it has mappers
    // for (Vorbis, Opus, FLAC) perfectly well. Only Speex, which it cannot map,
    // is worth pulling out of the normal path — so the identification string has
    // to be found before committing an Ogg file to whole-file buffering.
    head.starts_with(b"OggS") && head.windows(8).any(|w| w == b"Speex   ")
}

fn standalone_to_wav(bytes: &[u8]) -> Option<MhResult<Vec<u8>>> {
    if bytes.starts_with(b"OggS") {
        return Some(spx_to_wav(bytes));
    }
    if bytes.starts_with(&oxideav_ape::MAGIC) {
        return Some(ape_to_wav(bytes));
    }
    if bytes.starts_with(b"TTA1") {
        return Some(tta_to_wav(bytes));
    }
    if bytes.starts_with(b"MPCK") {
        return Some(mpc_to_wav(bytes));
    }
    None
}

/// Speex in Ogg. Symphonia's Ogg demuxer has mappers for Vorbis, Opus and FLAC
/// only, so a Speex stream never gets identified there.
fn spx_to_wav(bytes: &[u8]) -> MhResult<Vec<u8>> {
    let packets = oxideav_ogg::framing::pages_to_packets(bytes)
        .map_err(|e| MhError::Parse(format!("Ogg framing: {e}")))?;

    // Speex uses two header packets: the identification header, then comments.
    let ident = packets
        .first()
        .ok_or_else(|| MhError::Parse("Ogg stream carries no packets".to_string()))?;
    let header = oxideav_speex::SpeexHeader::parse(ident)
        .map_err(|e| MhError::Parse(format!("Speex header: {e}")))?;
    // The decoder emits at the rate its *mode* implies, which is not always the
    // `rate` field the header advertises — the WAV wrapper has to agree with the
    // samples actually produced, so it takes the same source the decoder does.
    let rate = header
        .mode_sampling_rate_hz()
        .ok_or_else(|| MhError::Unsupported(format!("Speex mode {} is not known", header.mode)))?;
    let channels = header.nb_channels.clamp(1, 8) as u16;

    let mut decoder = oxideav_speex::SpeexStreamDecoder::for_header(&header)
        .map_err(|e| MhError::Unsupported(format!("Speex decoder init failed: {e}")))?;
    let mut pcm: Vec<u8> = Vec::new();
    for packet in packets.iter().skip(2) {
        match decoder.decode_packet_pcm_i16(packet) {
            Ok(samples) => {
                pcm.extend(samples.iter().flat_map(|v| v.to_le_bytes()));
            }
            // A damaged packet is a dropout, not the end of the track.
            Err(_) => continue,
        }
    }
    if pcm.is_empty() {
        return Err(MhError::Parse(
            "Speex stream decoded to nothing".to_string(),
        ));
    }
    Ok(wav_container(&pcm, rate, channels, 16))
}

/// Musepack SV8. SV7 (`MP+`) is deliberately not handled — `oxideav-musepack`
/// documents its outer framing as unimplemented for that version.
fn mpc_to_wav(bytes: &[u8]) -> MhResult<Vec<u8>> {
    use oxideav_musepack::framing;

    let first = framing::parse_sv8_magic(bytes)
        .map_err(|e| MhError::Parse(format!("Musepack magic: {e}")))?;
    let header = framing::parse_packet_header(&bytes[first..])
        .map_err(|e| MhError::Parse(format!("Musepack packet header: {e}")))?;
    if header.key != framing::PacketKey::StreamHeader {
        return Err(MhError::Parse(
            "Musepack stream does not open with an SH packet".to_string(),
        ));
    }

    // The crate documents both readings of the size varint and notes it has not
    // confirmed which the spec intends, so parse under one and check the result
    // rather than trusting either. A wrong split yields a nonsense rate or
    // channel count, which is exactly what is tested for here.
    let start = first + header.header_len;
    let sane = |sh: &oxideav_musepack::sh_header::StreamHeaderFields| {
        matches!(sh.sample_rate_hz(), Some(r) if r >= 8_000) && (1..=8).contains(&sh.channels)
    };
    let candidates = [
        header.payload_len_inclusive().unwrap_or(0) as usize,
        header.payload_len_exclusive() as usize,
    ];
    let fields = candidates
        .iter()
        .filter(|len| **len > 0 && start + **len <= bytes.len())
        .find_map(|len| {
            oxideav_musepack::sh_header::StreamHeaderFields::parse(&bytes[start..start + len])
                .ok()
                .filter(sane)
        })
        .ok_or_else(|| MhError::Parse("Musepack stream header is unreadable".to_string()))?;

    let rate = fields.sample_rate_hz().ok_or_else(|| {
        MhError::Parse("Musepack sample rate is not one of the four defined".to_string())
    })?;
    let channels = u16::from(fields.channels);

    let mut params = oxideav_core::CodecParameters::audio(oxideav_core::CodecId::new("musepack"));
    params.sample_rate = Some(rate);
    params.channels = Some(channels);
    let mut decoder = oxideav_musepack::registry::make_decoder(&params)
        .map_err(|e| MhError::Unsupported(format!("Musepack decoder init failed: {e}")))?;

    let packet = oxideav_core::Packet::new(
        0,
        oxideav_core::TimeBase::new(1, i64::from(rate.max(1))),
        bytes.to_vec(),
    );
    decoder
        .send_packet(&packet)
        .map_err(|e| MhError::Parse(format!("Musepack decode failed: {e}")))?;

    let mut pcm = Vec::new();
    while let Ok(oxideav_core::Frame::Audio(frame)) = decoder.receive_frame() {
        for plane in &frame.data {
            pcm.extend_from_slice(plane);
        }
    }
    if pcm.is_empty() {
        return Err(MhError::Parse(
            "Musepack stream decoded to nothing".to_string(),
        ));
    }
    // The decoder emits 16-bit packed PCM.
    Ok(wav_container(&pcm, rate, channels, 16))
}

fn ape_to_wav(bytes: &[u8]) -> MhResult<Vec<u8>> {
    let info = oxideav_ape::FileInfo::parse(bytes)
        .map_err(|e| MhError::Parse(format!("APE header: {e}")))?;

    let mut params = oxideav_core::CodecParameters::audio(oxideav_core::CodecId::new("ape"));
    params.sample_rate = Some(info.sample_rate);
    params.channels = Some(info.channels);
    let mut decoder = oxideav_ape::registry::make_decoder(&params)
        .map_err(|e| MhError::Unsupported(format!("APE decoder init failed: {e}")))?;

    let packet = oxideav_core::Packet::new(
        0,
        oxideav_core::TimeBase::new(1, i64::from(info.sample_rate.max(1))),
        bytes.to_vec(),
    );
    decoder
        .send_packet(&packet)
        .map_err(|e| MhError::Parse(format!("APE decode failed: {e}")))?;

    // Frames already carry packed little-endian PCM at the stream's bit depth,
    // which is exactly what the WAV data chunk wants.
    let mut pcm = Vec::new();
    while let Ok(oxideav_core::Frame::Audio(frame)) = decoder.receive_frame() {
        for plane in &frame.data {
            pcm.extend_from_slice(plane);
        }
    }
    if pcm.is_empty() {
        return Err(MhError::Parse("APE stream decoded to nothing".to_string()));
    }
    Ok(wav_container(
        &pcm,
        info.sample_rate,
        info.channels,
        info.bits_per_sample,
    ))
}

fn tta_to_wav(bytes: &[u8]) -> MhResult<Vec<u8>> {
    let (info, samples) = oxideav_tta::decode(bytes)
        .map_err(|e| MhError::Parse(format!("TTA decode failed: {e}")))?;
    let pcm = oxideav_tta::pack_pcm(&samples, info.bits_per_sample);
    Ok(wav_container(
        &pcm,
        info.sample_rate,
        info.channels,
        info.bits_per_sample,
    ))
}

/// A block of interleaved f32 samples ready for the output device.
pub struct DecodedChunk {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub rate: u32,
    /// Presentation time of the first frame, in seconds.
    pub timestamp: f64,
}

/// Format and codec details resolved once the stream has been probed.
#[derive(Debug, Clone, Copy)]
pub struct StreamInfo {
    pub channels: usize,
    pub rate: u32,
    /// Total duration in seconds, when the container reports one.
    pub duration: Option<f64>,
    pub seekable: bool,
}

/// Symphonia covers most codecs, but two gaps need dedicated decoders:
/// Opus (which it does not implement, and which YouTube/YT Music serve and
/// local libraries hold as `.opus`/`.webm`) and HE-AAC (which it rejects
/// outright as "too complex" because of SBR).
enum Codec {
    Symphonia(Box<dyn AudioDecoder>),
    Opus {
        decoder: Box<rusty_opus::OpusDecoder>,
        channels: usize,
        rate: u32,
    },
    /// HE-AAC. Symphonia rejects any AAC carrying SBR, so those streams go to
    /// a decoder that reconstructs the replicated high band.
    HeAac {
        decoder: Box<dyn oxideav_core::Decoder>,
        /// The stream's AudioSpecificConfig, needed to re-frame each packet.
        asc: Vec<u8>,
        channels: usize,
        /// Presentation rate, corrected from the first decoded frame.
        rate: u32,
        /// Rate the container advertised, kept so the doubling is applied once.
        core_rate: u32,
    },
}

/// Decoder over one audio stream, yielding interleaved f32 chunks.
pub struct StreamDecoder {
    reader: Box<dyn FormatReader>,
    codec: Codec,
    track_id: u32,
    /// Fixed for the decoder's life; resolved once so the packet loop does not
    /// re-scan the track list twice per packet.
    time_base: Option<TimeBase>,
    info: StreamInfo,
    /// Reused between packets so steady-state decoding does not allocate.
    /// `next_chunk` hands it out by value, so the caller must return it with
    /// [`StreamDecoder::recycle`] for that reuse to actually happen.
    scratch: Vec<f32>,
    /// Reusable ADTS framing buffer for the HE-AAC path.
    framing: Vec<u8>,
}

impl StreamDecoder {
    /// Probe `source`, select its default audio track, and build a decoder.
    ///
    /// `mime` seeds the format hint; probing still sniffs the actual bytes, so
    /// a wrong or missing hint costs speed rather than correctness.
    pub fn new(
        source: Box<dyn MediaSource>,
        mime: Option<&str>,
        origin: Option<&str>,
    ) -> MhResult<Self> {
        let seekable = source.is_seekable();

        // APE and TTA are codec and container in one, and no symphonia demuxer
        // claims them — they used to fail at probe. Sniff for them first and,
        // when matched, decode the whole file and continue as in-memory WAV so
        // everything below is unchanged. Only possible on a seekable source,
        // since it needs a rewind after the sniff and the whole stream after.
        let mut source = source;
        let mut head = [0u8; 4096];
        let sniffed = if seekable {
            let n = std::io::Read::read(&mut source, &mut head).unwrap_or(0);
            std::io::Seek::seek(&mut source, std::io::SeekFrom::Start(0))
                .map_err(|e| MhError::Other(format!("could not rewind after sniffing: {e}")))?;
            n
        } else {
            0
        };
        let standalone = is_standalone(&head[..sniffed]);
        let mut found = Findings {
            url: origin,
            container: sniff_container(&head[..sniffed]),
            ..Findings::default()
        };

        let mss = if standalone {
            let mut all = Vec::new();
            std::io::Read::read_to_end(&mut source, &mut all)
                .map_err(|e| MhError::Other(format!("could not read stream: {e}")))?;
            let wav = match standalone_to_wav(&all) {
                Some(result) => result.map_err(|e| found.undecodable(bare_message(e)))?,
                None => {
                    return Err(found
                        .undecodable("sniffed as a standalone format but no decoder claimed it"))
                }
            };
            MediaSourceStream::new(
                Box::new(std::io::Cursor::new(wav)),
                MediaSourceStreamOptions::default(),
            )
        } else {
            MediaSourceStream::new(source, MediaSourceStreamOptions::default())
        };

        let mut hint = Hint::new();
        if let Some(mime) = mime {
            hint.mime_type(mime.split(';').next().unwrap_or(mime).trim());
        }

        let reader = symphonia::default::get_probe()
            .probe(&hint, mss, format_opts(), MetadataOptions::default())
            .map_err(|e| {
                if is_transport_failure(&e) {
                    MhError::Other(e.to_string())
                } else {
                    found.undecodable(e.to_string())
                }
            })?;

        let track = reader
            .default_track(TrackType::Audio)
            .ok_or_else(|| found.undecodable("the container holds no audio track"))?;
        let track_id = track.id;
        let time_base = track.time_base;

        let params = track
            .codec_params
            .as_ref()
            .and_then(|p| p.audio())
            .ok_or_else(|| found.undecodable("the audio track carries no codec parameters"))?
            .clone();
        found.codec = Some(params.codec);
        found.sample_rate = params.sample_rate;
        found.channels = params.channels.as_ref().map(|c| c.count());
        ensure_supported(CodecId::Audio(params.codec))
            .map_err(|e| found.undecodable(bare_message(e)))?;

        let duration = track
            .time_base
            .zip(track.num_frames)
            .and_then(|(tb, frames)| {
                tb.calc_time(Timestamp::from(frames as i64))
                    .map(|t| t.as_secs_f64())
            })
            .or_else(|| {
                let info = reader.media_info();
                let tb = info.time_base?;
                tb.calc_time(Timestamp::from(info.duration?.get() as i64))
                    .map(|t| t.as_secs_f64())
            });

        let channels = params.channels.as_ref().map(|c| c.count()).unwrap_or(2);
        let rate = if params.codec == CODEC_ID_OPUS {
            48_000
        } else {
            params.sample_rate.unwrap_or(44_100)
        };

        let codec = if params.codec == CODEC_ID_OPUS {
            Codec::Opus {
                decoder: Box::new(
                    rusty_opus::OpusDecoder::new(rate as i32, channels)
                        .map_err(|e| found.undecodable(format!("Opus decoder init failed: {e}")))?,
                ),
                channels,
                rate,
            }
        } else {
            match symphonia::default::get_codecs()
                .make_audio_decoder(&params, &AudioDecoderOptions::default())
            {
                Ok(decoder) => Codec::Symphonia(decoder),
                Err(e) if params.codec == CODEC_ID_AAC => {
                    let asc = params
                        .extra_data
                        .as_ref()
                        .map(|d| d.to_vec())
                        .ok_or_else(|| {
                            found.undecodable(format!(
                                "AAC stream carries no AudioSpecificConfig: {e}"
                            ))
                        })?;
                    build_he_aac(&asc, channels, params.sample_rate.unwrap_or(44_100))
                        .map_err(|e| found.undecodable(bare_message(e)))?
                }
                Err(e) => return Err(found.undecodable(e.to_string())),
            }
        };

        let rate = match &codec {
            Codec::HeAac { rate, .. } => *rate,
            _ => rate,
        };

        let info = StreamInfo {
            channels,
            rate,
            duration,
            seekable,
        };

        Ok(Self {
            reader,
            codec,
            track_id,
            time_base,
            info,
            scratch: Vec::new(),
            framing: Vec::new(),
        })
    }

    /// Take back a chunk's buffer so the next packet reuses the allocation.
    ///
    /// `next_chunk` moves `scratch` out into the returned chunk; without this
    /// the field is left empty and every packet reallocates and re-zeroes.
    pub fn recycle(&mut self, mut buf: Vec<f32>) {
        if buf.capacity() > self.scratch.capacity() {
            buf.clear();
            self.scratch = buf;
        }
    }

    pub fn info(&self) -> StreamInfo {
        self.info
    }

    /// Decode the next chunk, or `None` at end of stream.
    ///
    /// Packets from other tracks and decoder hiccups on individual packets are
    /// skipped rather than ending playback; only a reader-level failure stops.
    pub fn next_chunk(&mut self) -> MhResult<Option<DecodedChunk>> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(e) if is_end_of_stream(&e) => return Ok(None),
                Err(e) => return Err(MhError::Parse(format!("stream read failed: {e}"))),
            };

            if packet.track_id != self.track_id {
                continue;
            }

            let time_base = self.time_base;

            let timestamp = time_base
                .and_then(|tb| tb.calc_time(packet.pts))
                .map(|t| t.as_secs_f64())
                .unwrap_or(0.0);

            match &mut self.codec {
                Codec::Symphonia(decoder) => match decoder.decode(&packet) {
                    Ok(buf) => {
                        if buf.frames() == 0 {
                            continue;
                        }
                        return Ok(Some(interleave(buf, &mut self.scratch, timestamp)));
                    }
                    Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
                    Err(e) => return Err(MhError::Parse(format!("audio decode failed: {e}"))),
                },
                Codec::HeAac {
                    decoder,
                    asc,
                    channels,
                    rate,
                    ..
                } => {
                    if !adts_wrap(asc, &packet.data, &mut self.framing) {
                        continue;
                    }
                    let packet_ticks = packet.dur.get();
                    let framed = oxideav_core::Packet::new(
                        0,
                        oxideav_core::TimeBase::new(1, *rate as i64),
                        std::mem::take(&mut self.framing),
                    );
                    if decoder.send_packet(&framed).is_err() {
                        continue;
                    }
                    self.framing = framed.data;

                    self.scratch.clear();
                    while let Ok(oxideav_core::Frame::Audio(frame)) = decoder.receive_frame() {
                        if let (Some(tb), true) = (time_base, packet_ticks > 0) {
                            let seconds = packet_ticks as f64 * f64::from(tb.numer.get())
                                / f64::from(tb.denom.get());
                            if seconds > 0.0 {
                                let measured = (frame.samples as f64 / seconds).round() as u32;
                                if let Some(std_rate) = nearest_standard_rate(measured) {
                                    if std_rate != *rate {
                                        *rate = std_rate;
                                        self.info.rate = std_rate;
                                    }
                                }
                            }
                        }
                        for plane in &frame.data {
                            self.scratch.extend(
                                plane
                                    .as_chunks::<2>()
                                    .0
                                    .iter()
                                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0),
                            );
                        }
                    }
                    if self.scratch.is_empty() {
                        continue;
                    }
                    return Ok(Some(DecodedChunk {
                        samples: std::mem::take(&mut self.scratch),
                        channels: *channels,
                        rate: *rate,
                        timestamp,
                    }));
                }
                Codec::Opus {
                    decoder,
                    channels,
                    rate,
                } => {
                    let Some(frames) = opus_frames_per_channel(&packet.data, *rate) else {
                        continue;
                    };
                    self.scratch.clear();
                    self.scratch.resize(frames * *channels, 0.0);

                    match decoder.decode(&packet.data, frames, &mut self.scratch[..]) {
                        Ok(0) => continue,
                        Ok(per_channel) => {
                            self.scratch.truncate(per_channel * *channels);
                            return Ok(Some(DecodedChunk {
                                samples: std::mem::take(&mut self.scratch),
                                channels: *channels,
                                rate: *rate,
                                timestamp,
                            }));
                        }
                        Err(_) => continue,
                    }
                }
            }
        }
    }

    /// Seek to `seconds`, resetting the decoder as the format contract requires.
    pub fn seek(&mut self, seconds: f64) -> MhResult<f64> {
        if !self.info.seekable {
            return Err(MhError::Unsupported(
                "this stream cannot be seeked".to_string(),
            ));
        }

        let to = SeekTo::Time {
            time: Time::try_from_secs_f64(seconds.max(0.0))
                .ok_or_else(|| MhError::Other(format!("seek target {seconds} is out of range")))?,
            track_id: Some(self.track_id),
        };
        let seeked = self
            .reader
            .seek(SeekMode::Accurate, to)
            .map_err(|e| MhError::Other(format!("seek failed: {e}")))?;

        match &mut self.codec {
            Codec::Symphonia(d) => d.reset(),
            Codec::Opus {
                decoder,
                channels,
                rate,
            } => {
                **decoder = rusty_opus::OpusDecoder::new(*rate as i32, *channels)
                    .map_err(|e| MhError::Other(format!("Opus decoder reset failed: {e}")))?;
            }
            Codec::HeAac {
                decoder,
                asc,
                channels,
                core_rate,
                ..
            } => {
                if let Codec::HeAac { decoder: fresh, .. } =
                    build_he_aac(asc, *channels, *core_rate)?
                {
                    *decoder = fresh;
                }
            }
        }

        Ok(self
            .time_base
            .and_then(|tb| tb.calc_time(seeked.actual_ts))
            .map(|t| t.as_secs_f64())
            .unwrap_or(seconds))
    }
}

/// Flatten a decoded buffer of any sample format into interleaved f32.
///
/// `scratch` is reused across packets so steady-state decoding does not
/// allocate; it is handed to the caller and left empty for the next call.
fn interleave(
    buf: GenericAudioBufferRef<'_>,
    scratch: &mut Vec<f32>,
    timestamp: f64,
) -> DecodedChunk {
    let spec = buf.spec().clone();
    let channels = spec.channels().count();

    scratch.clear();
    scratch.resize(buf.frames() * channels, 0.0);
    buf.copy_to_slice_interleaved(&mut scratch[..]);

    DecodedChunk {
        samples: std::mem::take(scratch),
        channels,
        rate: spec.rate(),
        timestamp,
    }
}

/// Frames per channel carried by an Opus packet, read from its TOC byte
/// (RFC 6716 §3.1).
///
/// The Opus decoder takes the frame size as an argument and echoes it back
/// rather than reporting what it actually decoded, so passing a fixed guess
/// would silently stretch or truncate every packet.
fn opus_frames_per_channel(packet: &[u8], rate: u32) -> Option<usize> {
    let toc = *packet.first()?;
    let config = toc >> 3;

    let frame_us: u32 = match config {
        0..=11 => match config % 4 {
            0 => 10_000,
            1 => 20_000,
            2 => 40_000,
            _ => 60_000,
        },
        12..=15 => {
            if config % 2 == 0 {
                10_000
            } else {
                20_000
            }
        }
        16..=31 => match config % 4 {
            0 => 2_500,
            1 => 5_000,
            2 => 10_000,
            _ => 20_000,
        },
        _ => return None,
    };

    let frame_count = match toc & 0b11 {
        0 => 1,
        1 | 2 => 2,
        _ => u32::from(*packet.get(1)? & 0x3F),
    };
    if frame_count == 0 {
        return None;
    }

    Some((u64::from(frame_us) * u64::from(frame_count) * u64::from(rate) / 1_000_000) as usize)
}

/// Build the HE-AAC decoder, reporting the rate it will actually emit.
fn build_he_aac(asc: &[u8], channels: usize, core_rate: u32) -> MhResult<Codec> {
    let mut params = oxideav_core::CodecParameters::audio(oxideav_core::CodecId::new("aac"));
    params.extradata = asc.to_vec();
    params.sample_rate = Some(core_rate);
    params.channels = Some(channels as u16);

    let decoder = oxideav_aac::codec_decoder::make_decoder(&params)
        .map_err(|e| MhError::Unsupported(format!("HE-AAC decoder init failed: {e}")))?;

    Ok(Codec::HeAac {
        decoder,
        asc: asc.to_vec(),
        channels,
        rate: core_rate,
        core_rate,
    })
}

/// Snap a measured sample rate to the nearest rate AAC actually uses.
///
/// The measurement comes from integer frame counts over a container time
/// base, so it lands within a few Hz; anything further away is treated as
/// unreliable and ignored.
fn nearest_standard_rate(measured: u32) -> Option<u32> {
    const RATES: &[u32] = &[
        8_000, 11_025, 12_000, 16_000, 22_050, 24_000, 32_000, 44_100, 48_000, 64_000, 88_200,
        96_000,
    ];
    RATES
        .iter()
        .copied()
        .min_by_key(|r| r.abs_diff(measured))
        .filter(|r| r.abs_diff(measured) * 100 < *r)
}

/// Wrap a raw MPEG-4 access unit in an ADTS header.
///
/// MP4 stores bare access units while the HE-AAC decoder expects ADTS or
/// LOAS framing, so each packet is re-framed from the stream's ASC.
fn adts_wrap(asc: &[u8], au: &[u8], out: &mut Vec<u8>) -> bool {
    if asc.len() < 2 || au.len() > 0x1fff {
        return false;
    }
    let aot = asc[0] >> 3;
    let freq_idx = ((asc[0] & 0x07) << 1) | (asc[1] >> 7);
    let channel_cfg = (asc[1] >> 3) & 0x0f;
    let profile = aot.saturating_sub(1) & 0x03;
    let len = au.len() + 7;

    out.clear();
    out.reserve(len);
    out.extend_from_slice(&[
        0xff,
        0xf1,
        (profile << 6) | (freq_idx << 2) | (channel_cfg >> 2),
        ((channel_cfg & 0x03) << 6) | ((len >> 11) as u8 & 0x03),
        ((len >> 3) & 0xff) as u8,
        (((len & 0x07) << 5) as u8) | 0x1f,
        0xfc,
    ]);
    out.extend_from_slice(au);
    true
}

/// The bytes never arrived, as against not being a format we decode.
/// `player::source` raises a refused fetch as `io::Error::other`.
fn is_transport_failure(err: &symphonia::core::errors::Error) -> bool {
    matches!(
        err,
        symphonia::core::errors::Error::IoError(e)
            if e.kind() == std::io::ErrorKind::Other
    )
}

fn is_end_of_stream(err: &symphonia::core::errors::Error) -> bool {
    matches!(
        err,
        symphonia::core::errors::Error::IoError(e)
            if e.kind() == std::io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use symphonia::core::codecs::audio::well_known::{CODEC_ID_EAC3, CODEC_ID_PCM_S16LE};

    #[test]
    fn accepts_codecs_the_services_actually_serve() {
        for id in [
            CODEC_ID_FLAC,
            CODEC_ID_MP3,
            CODEC_ID_AAC,
            CODEC_ID_ALAC,
            CODEC_ID_VORBIS,
            CODEC_ID_PCM_S16LE,
        ] {
            assert!(
                ensure_supported(CodecId::Audio(id)).is_ok(),
                "expected {id:?} to be decodable"
            );
        }
    }

    #[test]
    fn accepts_opus_via_the_dedicated_decoder() {
        assert!(ensure_supported(CodecId::Audio(CODEC_ID_OPUS)).is_ok());
    }

    #[test]
    fn opus_frame_sizes_match_the_toc_byte() {
        assert_eq!(opus_frames_per_channel(&[0b0000_0000], 48_000), Some(480));
        assert_eq!(opus_frames_per_channel(&[0b0000_1000], 48_000), Some(960));
        assert_eq!(opus_frames_per_channel(&[0b0001_1000], 48_000), Some(2880));
        assert_eq!(opus_frames_per_channel(&[0b1000_0000], 48_000), Some(120));
        assert_eq!(opus_frames_per_channel(&[0b0000_1001], 48_000), Some(1920));
        assert_eq!(
            opus_frames_per_channel(&[0b0000_1011, 3], 48_000),
            Some(2880)
        );
        assert_eq!(opus_frames_per_channel(&[0b0000_1011], 48_000), None);
        assert_eq!(opus_frames_per_channel(&[], 48_000), None);
    }

    #[test]
    fn rejects_an_unknown_codec_with_a_named_error() {
        let err =
            ensure_supported(CodecId::Audio(CODEC_ID_EAC3)).expect_err("E-AC-3 has no decoder");
        assert!(err.to_string().contains("E-AC-3"), "got: {err}");
    }

    fn report_of(result: MhResult<StreamDecoder>) -> UndecodableStream {
        match result {
            Err(MhError::Undecodable(report)) => *report,
            Err(e) => panic!("expected an undecodable report, got: {e}"),
            Ok(_) => panic!("expected the stream to be rejected"),
        }
    }

    fn mp4_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((8 + body.len()) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }

    /// The shape of the Apple Music failure: a service stream (no filename, no
    /// extension) whose `stsd` holds two entries. The report has to say what
    /// the container was even though probing never got as far as a codec.
    #[test]
    fn a_rejected_service_stream_still_names_its_container() {
        let mut entry = vec![0u8; 36];
        entry[..4].copy_from_slice(&36u32.to_be_bytes());
        entry[4..8].copy_from_slice(b"mp4a");
        let mut stsd = vec![0u8; 4];
        stsd.extend_from_slice(&2u32.to_be_bytes());
        stsd.extend_from_slice(&entry);
        stsd.extend_from_slice(&entry);
        let stbl = mp4_box(b"stbl", &mp4_box(b"stsd", &stsd));
        let moov = mp4_box(
            b"moov",
            &mp4_box(b"trak", &mp4_box(b"mdia", &mp4_box(b"minf", &stbl))),
        );
        let bytes = [mp4_box(b"ftyp", b"dash\0\0\0\0iso6"), moov].concat();

        let url = "http://127.0.0.1:9/stream/52ccf318-ec13-4a93-88bf-0bf82fe7308a";
        let report = report_of(StreamDecoder::new(
            Box::new(std::io::Cursor::new(bytes)),
            None,
            Some(url),
        ));

        assert_eq!(report.url, url);
        assert_eq!(report.container.as_deref(), Some("MP4 (dash)"));
        assert_eq!(report.codec, None, "probing never reached a codec");
        assert!(
            report.detail.contains("more than 1 sample entry"),
            "{}",
            report.detail
        );
        assert!(report.app.starts_with("MediaHarbor "), "{}", report.app);
    }

    #[test]
    fn a_report_carries_everything_found_before_the_failure() {
        let found = Findings {
            url: Some("/music/x.m4a"),
            container: Some("MP4 (M4A)".into()),
            codec: Some(CODEC_ID_EAC3),
            sample_rate: Some(48_000),
            channels: Some(6),
        };
        let MhError::Undecodable(report) = found.undecodable("no decoder for E-AC-3") else {
            panic!("expected an undecodable report");
        };
        assert_eq!(report.codec.as_deref(), Some("E-AC-3"));
        assert_eq!(report.sample_rate, Some(48_000));
        assert_eq!(report.channels, Some(6));
        assert_eq!(report.detail, "no decoder for E-AC-3");
    }

    #[test]
    fn the_container_is_named_from_its_first_bytes() {
        let sniff = |b: &[u8]| sniff_container(b);
        assert_eq!(
            sniff(b"\0\0\0\x18ftypM4A \0\0\0\0").as_deref(),
            Some("MP4 (M4A)")
        );
        assert_eq!(sniff(b"fLaC\0\0\0\x22").as_deref(), Some("FLAC"));
        assert_eq!(
            sniff(&[0xFF, 0xF1, 0x50, 0x80]).as_deref(),
            Some("AAC (ADTS)")
        );
        assert_eq!(
            sniff(&[0xFF, 0xFB, 0x90, 0x64]).as_deref(),
            Some("MPEG audio")
        );
        assert_eq!(
            sniff(b"#EXTM3U\n").as_deref(),
            Some("HLS playlist, not audio")
        );
        assert_eq!(
            sniff(b"  <!DOCTYPE html>").as_deref(),
            Some("HTML/XML, not audio")
        );
        assert_eq!(sniff(b"{\"error\":1}").as_deref(), Some("JSON, not audio"));
        assert_eq!(sniff(b""), None, "a non-seekable source sniffs nothing");
    }

    #[test]
    fn an_unnamed_codec_is_still_identifiable() {
        assert!(codec_label(CODEC_ID_PCM_S16LE).starts_with("codec id 0x"));
    }
}
