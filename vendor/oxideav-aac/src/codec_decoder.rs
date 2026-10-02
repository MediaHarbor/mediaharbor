//! `oxideav_core::Decoder` wiring for AAC-LC carried in ADTS.
//!
//! The crate's [`decode::StreamDecoder`](crate::decode::StreamDecoder)
//! already walks one ADTS frame's §4.4.2.1 `raw_data_block()` to
//! interleaved 16-bit PCM end-to-end, carrying every channel element's
//! §4.6.11 overlap-add / §4.6.7 LTP / §4.6.6 predictor state across
//! frames. This module adapts that path into the framework's packet-in /
//! frame-out [`oxideav_core::Decoder`] trait so containers (the MP4
//! `mp4a` object-type, the AVI / WAVEFORMATEX `0x00FF` raw-AAC tag, the
//! Matroska `A_AAC` CodecID, …) can route ADTS-framed AAC streams via the
//! registry.
//!
//! ## Trait-API adaptation
//!
//! The framework trait is *packet-in, frame-out*:
//!
//! * [`send_packet`](Decoder::send_packet) accepts one [`Packet`] whose
//!   `data` is **one or more complete ADTS frames** — each an ADTS
//!   fixed/variable header (+ optional 16-bit CRC) followed by its
//!   `aac_frame_length`-delimited `raw_data_block()`. A leading ID3v2 tag
//!   (the streaming-mux convention) is skipped. Every ADTS frame in the
//!   packet is decoded in order against the persistent
//!   [`StreamDecoder`](crate::decode::StreamDecoder), so the per-element
//!   filterbank / LTP / predictor state threads across packet boundaries
//!   exactly as it does across the frames of a contiguous stream.
//! * [`receive_frame`](Decoder::receive_frame) returns one
//!   [`AudioFrame`] per decoded ADTS frame: [`FRAME_LEN`] = 1024 samples
//!   per channel (the 1024-line transform family this crate covers),
//!   interleaved little-endian `i16` in element order ([`SampleFormat::S16`]).
//! * [`flush`](Decoder::flush) marks end-of-stream so subsequent
//!   `receive_frame` calls return [`Error::Eof`] once the pending queue
//!   drains.
//! * [`reset`](Decoder::reset) drops the persistent
//!   [`StreamDecoder`](crate::decode::StreamDecoder) (and with it all
//!   §4.6.11 overlap / §4.6.7 LTP / §4.6.6 predictor memory) so the next
//!   `send_packet` decodes as if it were the first — the trait contract
//!   for a stateful, overlap-add codec after a container seek.
//!
//! ## Output format
//!
//! The decoder emits **interleaved** S16 PCM in `Frame::Audio`:
//! `data.len() == 1`, the single plane holding
//! `samples_per_channel * channels * 2` little-endian `i16` bytes in the
//! §4.4.2.1 element order an SCE/LFE contributes one channel, a CPE two.
//! The §4.6.11 [`pcm`](crate::pcm) output stage has already applied the
//! §1.3 `NINT()` round-half-away-from-zero and the 16-bit saturation, so
//! this layer only widens each `i16` to its two little-endian bytes.
//!
//! ## Registration
//!
//! [`register_codecs`] installs the codec under id `"aac"` and claims the
//! container tags an AAC stream is looked up under: the MP4 object-type
//! `0x40` (`Audio ISO/IEC 14496-3`), the WAVEFORMATEX `0x00FF`
//! (raw AAC) and `0x1601` (MPEG-4 ADTS AAC), the `mp4a` / `aac ` FourCCs,
//! and the Matroska `A_AAC` CodecID. A probe scores the ADTS syncword on
//! the first packet so a genuine ADTS stream out-ranks a non-ADTS
//! claimant on a shared tag.
//!
//! ## Provenance
//!
//! Every byte-layout and clause reference is from ISO/IEC 13818-7 /
//! 14496-3 staged under `docs/audio/aac/`; the trait adaptation composes
//! the crate's own [`decode::StreamDecoder`](crate::decode::StreamDecoder)
//! with the framework surface and reads no external decoder.

use std::collections::VecDeque;

use oxideav_core::{
    AudioFrame, CodecCapabilities, CodecId, CodecInfo, CodecParameters, CodecRegistry, CodecTag,
    Confidence, Decoder, Error, Frame, Packet, ProbeContext, Result, SampleFormat,
};

use crate::adts::{AdtsHeader, ADTS_HEADER_BYTES_NO_CRC};
use crate::decode::{DecodedFrame, StreamDecoder};
use crate::latm::{LoasDecoder, AUDIO_SYNC_STREAM_SYNCWORD};

/// Codec id under which [`register_codecs`] installs this decoder.
pub const CODEC_ID_STR: &str = "aac";

/// MP4 object-type indicator for `Audio ISO/IEC 14496-3` (AAC). The OTI
/// every MP4 / ISO-BMFF `esds` AudioObject descriptor carries for an AAC
/// elementary stream.
pub const MP4_OBJECT_TYPE_AAC: u8 = 0x40;

/// WAVEFORMATEX `wFormatTag` for raw AAC (`WAVE_FORMAT_RAW_AAC1`).
pub const WAVE_FORMAT_RAW_AAC1: u16 = 0x00FF;

/// WAVEFORMATEX `wFormatTag` for MPEG-4 ADTS AAC (`WAVE_FORMAT_MPEG_ADTS_AAC`).
pub const WAVE_FORMAT_MPEG_ADTS_AAC: u16 = 0x1601;

/// Build a boxed AAC [`Decoder`] from `params`.
///
/// `params.sample_rate` and `params.channels` seed the returned
/// decoder's [`output_params`](AacDecoder)-equivalent stream description;
/// the real per-frame sample rate and channel count are re-derived from
/// each ADTS frame header on `send_packet`, so the values supplied here
/// are a hint only. The decoder is always built — AAC carries its full
/// configuration in-band (the ADTS header), so no parameter is mandatory.
pub fn make_decoder(params: &CodecParameters) -> Result<Box<dyn Decoder>> {
    let sample_rate = params.sample_rate.unwrap_or(44_100);
    let channels = params.channels.unwrap_or(2);

    let mut out_params = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
    out_params.sample_rate = Some(sample_rate);
    out_params.channels = Some(channels);
    out_params.sample_format = Some(SampleFormat::S16);

    Ok(Box::new(AacDecoder::new(
        CodecId::new(CODEC_ID_STR),
        out_params,
    )))
}

/// Packet-to-frame adaptor wrapping [`StreamDecoder`] in the framework
/// [`Decoder`] trait.
///
/// State carried across packets:
///
/// * `stream` — the persistent [`StreamDecoder`] whose per-element slots
///   thread the §4.6.11 overlap-add tail / §4.6.7 LTP history / §4.6.6
///   predictor state across the frames of the stream.
/// * `pending` queues the [`AudioFrame`]s produced by the last
///   `send_packet` (one per decoded ADTS frame); `receive_frame` pops the
///   front.
/// * `eof` — set by [`Decoder::flush`]; once `pending` drains and `eof`
///   is set, `receive_frame` returns [`Error::Eof`].
pub struct AacDecoder {
    codec_id: CodecId,
    output: CodecParameters,
    stream: StreamDecoder,
    loas: LoasDecoder,
    /// The transport syntax detected from the first non-empty packet:
    /// raw ADTS (`0xFFF` syncword) or LOAS `AudioSyncStream` (`0x2B7`
    /// syncword). `None` until the first packet picks one; once set, every
    /// later packet is routed the same way.
    transport: Option<Transport>,
    pending: VecDeque<AudioFrame>,
    eof: bool,
}

/// The carrier syntax an [`AacDecoder`] auto-detects on its first packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    /// Raw ADTS frames (`0xFFF` 12-bit syncword), routed through
    /// [`StreamDecoder::decode_frame`].
    Adts,
    /// LOAS `AudioSyncStream` (`0x2B7` 11-bit syncword), routed through
    /// [`LoasDecoder::decode_all`].
    Loas,
}

impl std::fmt::Debug for AacDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AacDecoder")
            .field("codec_id", &self.codec_id)
            .field("transport", &self.transport)
            .field("pending", &self.pending.len())
            .field("eof", &self.eof)
            .finish()
    }
}

impl AacDecoder {
    fn new(codec_id: CodecId, output: CodecParameters) -> Self {
        Self {
            codec_id,
            output,
            stream: StreamDecoder::new(),
            loas: LoasDecoder::new(),
            transport: None,
            pending: VecDeque::new(),
            eof: false,
        }
    }

    /// The parameter set this decoder advertises for its output stream.
    /// Updated from each decoded ADTS frame header so a caller reading it
    /// after the first packet sees the on-the-wire sample rate / channel
    /// count rather than the at-construction hints.
    pub fn output_params(&self) -> &CodecParameters {
        &self.output
    }

    /// Convert one [`DecodedFrame`]'s interleaved `i16` PCM to an
    /// interleaved-S16 [`AudioFrame`] (single plane, little-endian).
    fn decoded_to_audio(decoded: &DecodedFrame, pts: Option<i64>) -> AudioFrame {
        let mut bytes = Vec::with_capacity(decoded.pcm.len() * 2);
        for &s in &decoded.pcm {
            bytes.extend_from_slice(&s.to_le_bytes());
        }
        AudioFrame {
            // Per-channel sample count from the interleaved buffer:
            // 1024 for the plain AAC path, 2048 for an SBR (HE-AAC)
            // dual-rate frame. A fill-only frame (`channels == 0`)
            // carries no samples.
            samples: decoded.pcm.len().checked_div(decoded.channels).unwrap_or(0) as u32,
            pts,
            data: vec![bytes],
        }
    }

    /// Queue a decoded frame's PCM and refresh the advertised output
    /// params; a fill-only frame (`channels == 0`) produces no audio.
    fn queue_decoded(&mut self, decoded: &DecodedFrame, pts: Option<i64>) -> bool {
        if decoded.channels > 0 {
            self.output.sample_rate = Some(decoded.sample_rate);
            self.output.channels = Some(decoded.channels as u16);
            self.pending.push_back(Self::decoded_to_audio(decoded, pts));
            true
        } else {
            false
        }
    }

    /// Route an ADTS-framed packet (`data` already ID3-stripped) through
    /// the [`StreamDecoder`], queuing one [`AudioFrame`] per ADTS frame.
    fn send_adts(&mut self, data: &[u8], pts: Option<i64>) -> Result<()> {
        let mut pos = 0usize;
        let mut produced_any = false;
        while pos + ADTS_HEADER_BYTES_NO_CRC <= data.len() {
            let (header, payload_offset) = AdtsHeader::parse(&data[pos..])
                .map_err(|e| Error::other(format!("oxideav-aac: adts header: {e}")))?;
            let frame_len = header.aac_frame_length as usize;
            if frame_len < payload_offset || pos + frame_len > data.len() {
                return Err(Error::other(
                    "oxideav-aac: ADTS frame length overruns packet",
                ));
            }
            let body = &data[pos + payload_offset..pos + frame_len];
            let decoded = self
                .stream
                .decode_frame(&header, body)
                .map_err(|e| Error::other(format!("oxideav-aac: decode_frame: {e}")))?;
            produced_any |= self.queue_decoded(&decoded, pts);
            pos += frame_len;
        }

        if !produced_any && pos == 0 {
            return Err(Error::other(
                "oxideav-aac: packet held no complete ADTS frame",
            ));
        }
        Ok(())
    }

    /// Route a LOAS `AudioSyncStream` packet (`data` already ID3-stripped)
    /// through the [`LoasDecoder`], queuing one [`AudioFrame`] per
    /// recovered access unit. A packet may carry one or several LOAS sync
    /// frames; the persistent [`LoasDecoder`] threads the
    /// `StreamMuxConfig` (and per-stream decode state) across packets.
    fn send_loas(&mut self, data: &[u8], pts: Option<i64>) -> Result<()> {
        let decoded_frames = self
            .loas
            .decode_all(data)
            .map_err(|e| Error::other(format!("oxideav-aac: loas decode: {e}")))?;
        let mut produced_any = false;
        for decoded in &decoded_frames {
            produced_any |= self.queue_decoded(decoded, pts);
        }
        if !produced_any && decoded_frames.is_empty() {
            return Err(Error::other(
                "oxideav-aac: packet held no complete LOAS sync frame",
            ));
        }
        Ok(())
    }
}

impl Decoder for AacDecoder {
    fn codec_id(&self) -> &CodecId {
        &self.codec_id
    }

    fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        if self.eof {
            return Err(Error::other("oxideav-aac: cannot send_packet after flush"));
        }

        let data = skip_id3v2(&packet.data);

        // Pick the carrier from the first non-empty packet, then route
        // every later packet the same way.
        let transport = match self.transport {
            Some(t) => t,
            None => {
                let Some(t) = detect_transport(data) else {
                    return Err(Error::other(
                        "oxideav-aac: packet has neither an ADTS nor a LOAS syncword",
                    ));
                };
                self.transport = Some(t);
                t
            }
        };

        match transport {
            Transport::Adts => self.send_adts(data, packet.pts),
            Transport::Loas => self.send_loas(data, packet.pts),
        }
    }

    fn receive_frame(&mut self) -> Result<Frame> {
        if let Some(audio) = self.pending.pop_front() {
            return Ok(Frame::Audio(audio));
        }
        if self.eof {
            return Err(Error::Eof);
        }
        Err(Error::NeedMore)
    }

    fn flush(&mut self) -> Result<()> {
        self.eof = true;
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        // Drop every per-element overlap / LTP / predictor slot (for both
        // carriers) so the next send_packet decodes from a clean state,
        // and re-arm transport auto-detection.
        self.stream = StreamDecoder::new();
        self.loas = LoasDecoder::new();
        self.transport = None;
        self.pending.clear();
        self.eof = false;
        Ok(())
    }
}

/// Detect the AAC carrier syntax from the first bytes of a packet
/// (already ID3v2-stripped).
///
/// * ADTS — 12-bit `0xFFF` syncword: `byte0 == 0xFF` and the top four
///   bits of `byte1` are set.
/// * LOAS `AudioSyncStream` — 11-bit `0x2B7` syncword: the first 11 bits
///   equal `0x2B7` (`byte0 == 0x56`, top three bits of `byte1` set).
///
/// Returns `None` when neither syncword matches.
fn detect_transport(data: &[u8]) -> Option<Transport> {
    if data.len() < 2 {
        return None;
    }
    if data[0] == 0xFF && (data[1] & 0xF0) == 0xF0 {
        return Some(Transport::Adts);
    }
    // 0x2B7 = 0b010_1011_0111: byte0 = 0b0101_0110 = 0x56, byte1 top 3 =
    // 0b111. Confirm via the 11-bit syncword constant.
    let first11 = (u32::from(data[0]) << 3) | (u32::from(data[1]) >> 5);
    if first11 == AUDIO_SYNC_STREAM_SYNCWORD {
        return Some(Transport::Loas);
    }
    None
}

/// Skip a leading ID3v2 tag (`"ID3"` + 6-byte header + syncsafe size +
/// optional footer) if present; otherwise return the input unchanged.
/// Mirrors [`crate::decode`]'s stream-level skip so a packet that carries
/// a leading tag (the streaming-mux convention) decodes cleanly.
fn skip_id3v2(data: &[u8]) -> &[u8] {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return data;
    }
    let size = data[6..10]
        .iter()
        .fold(0usize, |acc, &b| (acc << 7) | usize::from(b & 0x7f));
    let footer = if data[5] & 0x10 != 0 { 10 } else { 0 };
    let total = 10 + size + footer;
    if total >= data.len() {
        data
    } else {
        &data[total..]
    }
}

/// Probe the [`ADTS syncword`](crate::adts::ADTS_SYNCWORD) on the first
/// packet to disambiguate the shared container tags.
///
/// * Sync OK (and a parseable fixed header) → `1.0` (definitive ADTS AAC).
/// * Leading ID3v2 then sync OK             → `1.0` (streaming-mux ADTS).
/// * Packet present but no ADTS sync        → `0.2` (not us, but the same
///   tag also covers non-ADTS — raw `raw_data_block()` / LATM — carriage
///   we can still attempt, so don't refuse outright).
/// * No packet hint                         → `0.5`.
fn probe_aac(ctx: &ProbeContext) -> Confidence {
    let Some(pkt) = ctx.packet else {
        return 0.5;
    };
    let pkt = skip_id3v2(pkt);
    if pkt.len() < 2 {
        return 0.2;
    }
    // 12-bit ADTS syncword 0xFFF: byte 0 == 0xFF and the top 4 bits of
    // byte 1 are 1. `AdtsHeader::parse` confirms the rest of the fixed
    // header is structurally valid before we commit to the definitive
    // score.
    if pkt[0] == 0xFF && (pkt[1] & 0xF0) == 0xF0 && AdtsHeader::parse(pkt).is_ok() {
        return 1.0;
    }
    // 11-bit LOAS AudioSyncStream syncword 0x2B7. A bare syncword match
    // is a strong-but-not-definitive AAC signal (the AudioMuxElement
    // body is validated on the first decode), so score it just below the
    // structurally-confirmed ADTS hit.
    if detect_transport(pkt) == Some(Transport::Loas) {
        return 0.9;
    }
    0.2
}

/// Install the AAC decoder factory into `reg`.
///
/// Claims the container tags an AAC elementary stream is routed under:
///
/// * **MP4 object-type `0x40`** — the `esds` AudioObject descriptor OTI
///   for `Audio ISO/IEC 14496-3`.
/// * **WAVEFORMATEX `0x00FF`** (`WAVE_FORMAT_RAW_AAC1`) and **`0x1601`**
///   (`WAVE_FORMAT_MPEG_ADTS_AAC`) — the Win32 `mmreg.h` raw-AAC and
///   ADTS-AAC format tags used by AVI / WAVE carriage.
/// * **FourCCs `mp4a` / `aac `** and the **Matroska `A_AAC`** CodecID.
///
/// The encoder factory is **not** wired: this crate has the per-tool
/// decode chain plus the bit-exact wire writers, but no rate-control /
/// psychoacoustic encoder. The §4.4.2.1 `raw_data_block()` assembler
/// (`raw_data_block::FrameAssembler`) is the encode-side surface that
/// would feed a future `make_encoder`.
///
/// The probe ([`probe_aac`]) scores the ADTS syncword so a genuine ADTS
/// stream out-ranks a non-ADTS claimant on any shared tag.
pub fn register_codecs(reg: &mut CodecRegistry) {
    let info = CodecInfo::new(CodecId::new(CODEC_ID_STR))
        .capabilities(
            CodecCapabilities::audio("aac")
                .with_decode()
                .with_lossy(true),
        )
        .decoder(make_decoder)
        .probe(probe_aac)
        .tags([
            CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC),
            CodecTag::wave_format(WAVE_FORMAT_RAW_AAC1),
            CodecTag::wave_format(WAVE_FORMAT_MPEG_ADTS_AAC),
            CodecTag::fourcc(b"mp4a"),
            CodecTag::fourcc(b"aac "),
            CodecTag::matroska("A_AAC"),
        ]);
    reg.register(info);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decode::FRAME_LEN;
    use oxideav_core::TimeBase;

    fn build_params(sample_rate: u32, channels: u16) -> CodecParameters {
        let mut p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        p.sample_rate = Some(sample_rate);
        p.channels = Some(channels);
        p.sample_format = Some(SampleFormat::S16);
        p
    }

    /// Read a fixture's whole `input.aac` byte buffer, or `None` when the
    /// workspace `docs/` tree is absent (standalone-crate CI checkouts).
    fn fixture_bytes(name: &str) -> Option<Vec<u8>> {
        let path = format!(
            "{}/../../docs/audio/aac/fixtures/{name}/input.aac",
            env!("CARGO_MANIFEST_DIR")
        );
        if !std::path::Path::new(&path).exists() {
            eprintln!("skip: staged ADTS fixture not present at {path}");
            return None;
        }
        Some(std::fs::read(&path).expect("read staged ADTS fixture"))
    }

    /// Slice a raw-ADTS byte buffer into one packet per ADTS frame, the
    /// way a demuxer would emit them on the wire.
    fn split_into_packets(bytes: &[u8]) -> Vec<Packet> {
        let bytes = skip_id3v2(bytes);
        let tb = TimeBase::new(1, 44_100);
        let mut packets = Vec::new();
        let mut pos = 0usize;
        let mut pts: i64 = 0;
        while pos + ADTS_HEADER_BYTES_NO_CRC <= bytes.len() {
            let Ok((header, _)) = AdtsHeader::parse(&bytes[pos..]) else {
                break;
            };
            let fl = header.aac_frame_length as usize;
            if fl == 0 || pos + fl > bytes.len() {
                break;
            }
            let mut pkt = Packet::new(0, tb, bytes[pos..pos + fl].to_vec());
            pkt.pts = Some(pts);
            packets.push(pkt);
            pts += FRAME_LEN as i64;
            pos += fl;
        }
        packets
    }

    /// Read a fixture's whole `input.<ext>` byte buffer, or `None` when
    /// the workspace `docs/` tree is absent.
    fn fixture_bytes_ext(name: &str, ext: &str) -> Option<Vec<u8>> {
        let path = format!(
            "{}/../../docs/audio/aac/fixtures/{name}/input.{ext}",
            env!("CARGO_MANIFEST_DIR")
        );
        if !std::path::Path::new(&path).exists() {
            eprintln!("skip: staged fixture not present at {path}");
            return None;
        }
        Some(std::fs::read(&path).expect("read staged fixture"))
    }

    #[test]
    fn detect_transport_recognises_adts_and_loas() {
        // ADTS: 0xFFF syncword.
        assert_eq!(detect_transport(&[0xFF, 0xF1, 0x00]), Some(Transport::Adts));
        // LOAS AudioSyncStream: 0x2B7 in the first 11 bits → 0x56, top 3
        // bits of byte 1 set.
        assert_eq!(detect_transport(&[0x56, 0xE0, 0x00]), Some(Transport::Loas));
        // Neither.
        assert_eq!(detect_transport(&[0x00, 0x00]), None);
        assert_eq!(detect_transport(&[0xFF]), None);
    }

    #[test]
    fn loas_packet_decodes_through_trait() {
        let Some(buf) = fixture_bytes_ext("aac-latm-stream", "latm") else {
            return;
        };
        // Feed the whole LOAS buffer as one packet (a demuxer that hands
        // the elementary stream in bulk).
        let mut pkt = Packet::new(0, TimeBase::new(1, 44_100), buf.clone());
        pkt.pts = Some(0);

        let mut dec = make_decoder(&build_params(44_100, 2)).expect("decoder");
        dec.send_packet(&pkt).expect("send_packet (loas)");

        let mut frames = 0usize;
        let mut samples_total = 0usize;
        while let Ok(Frame::Audio(a)) = dec.receive_frame() {
            assert_eq!(a.samples as usize, FRAME_LEN);
            // interleaved stereo → FRAME_LEN * 2 channels * 2 bytes.
            assert_eq!(a.data[0].len(), FRAME_LEN * 2 * 2);
            frames += 1;
            samples_total += a.data[0].len() / 2;
        }
        assert!(frames > 0, "LOAS packet produced no frames");
        // 32 access units × 1024 × 2 channels.
        assert_eq!(samples_total, 65_536);
    }

    #[test]
    fn loas_trait_matches_loas_decoder_pcm() {
        let Some(buf) = fixture_bytes_ext("aac-latm-stream", "latm") else {
            return;
        };
        // Reference: bare LoasDecoder.
        let mut reference = LoasDecoder::new();
        let ref_frames = reference.decode_all(&buf).expect("LoasDecoder");
        let mut ref_pcm: Vec<i16> = Vec::new();
        for f in &ref_frames {
            ref_pcm.extend_from_slice(&f.pcm);
        }

        // Trait path: one bulk packet.
        let mut pkt = Packet::new(0, TimeBase::new(1, 44_100), buf);
        pkt.pts = Some(0);
        let mut dec = make_decoder(&build_params(44_100, 2)).expect("decoder");
        dec.send_packet(&pkt).expect("send_packet");
        let mut trait_pcm: Vec<i16> = Vec::new();
        while let Ok(Frame::Audio(a)) = dec.receive_frame() {
            for c in a.data[0].chunks_exact(2) {
                trait_pcm.push(i16::from_le_bytes([c[0], c[1]]));
            }
        }
        assert_eq!(trait_pcm, ref_pcm, "LOAS trait diverged from LoasDecoder");
    }

    #[test]
    fn probe_scores_loas_sync() {
        // 0x2B7 syncword (0x56, top 3 bits of next byte set).
        let pkt = [0x56u8, 0xE0, 0x00, 0x00];
        let tag = CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!((probe_aac(&ctx) - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn make_decoder_builds_and_reports_id() {
        let dec = make_decoder(&build_params(44_100, 2)).expect("decoder builds");
        assert_eq!(dec.codec_id().as_str(), CODEC_ID_STR);
    }

    #[test]
    fn make_decoder_defaults_without_hints() {
        let p = CodecParameters::audio(CodecId::new(CODEC_ID_STR));
        let _ = make_decoder(&p).expect("default-params decoder builds");
    }

    #[test]
    fn receive_without_packet_is_need_more() {
        let mut dec = make_decoder(&build_params(44_100, 2)).unwrap();
        match dec.receive_frame() {
            Err(Error::NeedMore) => {}
            other => panic!("expected NeedMore, got {other:?}"),
        }
    }

    #[test]
    fn mono_fixture_decodes_one_frame_per_packet() {
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let packets = split_into_packets(&buf);
        assert!(!packets.is_empty(), "fixture yielded zero packets");

        let mut dec = make_decoder(&build_params(8_000, 1)).expect("decoder");
        let mut frames = 0usize;
        for pkt in &packets {
            dec.send_packet(pkt).expect("send_packet");
            loop {
                match dec.receive_frame() {
                    Ok(Frame::Audio(a)) => {
                        assert_eq!(a.samples as usize, FRAME_LEN);
                        assert_eq!(a.data.len(), 1, "interleaved single plane");
                        // mono → FRAME_LEN samples * 1 channel * 2 bytes.
                        assert_eq!(a.data[0].len(), FRAME_LEN * 2);
                        assert_eq!(a.pts, pkt.pts);
                        frames += 1;
                    }
                    Ok(other) => panic!("expected Audio, got {other:?}"),
                    Err(Error::NeedMore) => break,
                    Err(e) => panic!("receive_frame: {e}"),
                }
            }
        }
        assert_eq!(frames, packets.len(), "one frame per packet");
    }

    #[test]
    fn stereo_fixture_decodes_two_channel_planes() {
        let Some(buf) = fixture_bytes("aac-lc-intensity-stereo") else {
            return;
        };
        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&build_params(44_100, 2)).expect("decoder");
        dec.send_packet(&packets[0]).expect("send_packet 0");
        let Frame::Audio(a) = dec.receive_frame().expect("frame 0") else {
            panic!("expected AudioFrame");
        };
        assert_eq!(a.samples as usize, FRAME_LEN);
        // interleaved stereo → FRAME_LEN * 2 channels * 2 bytes.
        assert_eq!(a.data[0].len(), FRAME_LEN * 2 * 2);
    }

    #[test]
    fn trait_decode_matches_stream_decoder_pcm() {
        // The trait wrapper must produce byte-identical PCM to the
        // StreamDecoder it adapts (same persistent state, same order).
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let mut reference = StreamDecoder::new();
        let ref_frames = reference.decode_all(&buf).expect("reference decode_all");
        let mut ref_pcm: Vec<i16> = Vec::new();
        for f in &ref_frames {
            ref_pcm.extend_from_slice(&f.pcm);
        }

        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&build_params(8_000, 1)).expect("decoder");
        let mut trait_pcm: Vec<i16> = Vec::new();
        for pkt in &packets {
            dec.send_packet(pkt).expect("send_packet");
            while let Ok(Frame::Audio(a)) = dec.receive_frame() {
                for c in a.data[0].chunks_exact(2) {
                    trait_pcm.push(i16::from_le_bytes([c[0], c[1]]));
                }
            }
        }
        assert_eq!(
            trait_pcm, ref_pcm,
            "trait decode diverged from StreamDecoder"
        );
    }

    #[test]
    fn flush_then_receive_yields_eof_after_drain() {
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&build_params(8_000, 1)).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        dec.flush().unwrap();
        let _ = dec.receive_frame().expect("pending frame drains");
        match dec.receive_frame() {
            Err(Error::Eof) => {}
            other => panic!("expected Eof, got {other:?}"),
        }
    }

    #[test]
    fn send_after_flush_is_rejected() {
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let packets = split_into_packets(&buf);
        let mut dec = make_decoder(&build_params(8_000, 1)).unwrap();
        dec.flush().unwrap();
        assert!(dec.send_packet(&packets[0]).is_err());
    }

    #[test]
    fn reset_re_enables_send_and_restores_clean_state() {
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let packets = split_into_packets(&buf);

        // Decode the first frame fresh, capture its PCM.
        let mut dec = make_decoder(&build_params(8_000, 1)).unwrap();
        dec.send_packet(&packets[0]).unwrap();
        let Frame::Audio(first_clean) = dec.receive_frame().unwrap() else {
            panic!("audio");
        };

        // Advance a few frames (building overlap state), flush, reset.
        for pkt in packets.iter().take(4) {
            dec.send_packet(pkt).unwrap();
            while let Ok(Frame::Audio(_)) = dec.receive_frame() {}
        }
        dec.flush().unwrap();
        dec.reset().unwrap();

        // After reset the first frame decodes byte-identically again —
        // proving the overlap / state was wiped.
        dec.send_packet(&packets[0]).unwrap();
        let Frame::Audio(first_again) = dec.receive_frame().unwrap() else {
            panic!("audio");
        };
        assert_eq!(
            first_again.data, first_clean.data,
            "reset did not restore the initial decode state"
        );
    }

    #[test]
    fn multi_frame_packet_emits_one_audio_frame_each() {
        // A packet carrying two concatenated ADTS frames must yield two
        // AudioFrames (the streaming case where a demuxer batches frames).
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let packets = split_into_packets(&buf);
        assert!(packets.len() >= 2);
        let mut joined = packets[0].data.clone();
        joined.extend_from_slice(&packets[1].data);
        let mut pkt = Packet::new(0, TimeBase::new(1, 8_000), joined);
        pkt.pts = Some(0);

        let mut dec = make_decoder(&build_params(8_000, 1)).unwrap();
        dec.send_packet(&pkt).unwrap();
        let mut n = 0usize;
        while let Ok(Frame::Audio(_)) = dec.receive_frame() {
            n += 1;
        }
        assert_eq!(n, 2, "two ADTS frames in one packet → two AudioFrames");
    }

    // ───────────────────── probe + registration ─────────────────────

    /// Pack a minimal, structurally-valid 7-byte ADTS fixed/variable
    /// header (protection_absent, LC mono 44.1 kHz, `aac_frame_length`
    /// covering just the header) MSB-first so the probe vector cannot
    /// drift out of sync with `AdtsHeader::parse`.
    fn synth_adts_header() -> [u8; 7] {
        let mut bits: Vec<u8> = Vec::new();
        let mut push = |val: u32, n: u32| {
            for i in (0..n).rev() {
                bits.push(((val >> i) & 1) as u8);
            }
        };
        push(0xFFF, 12); // syncword
        push(0, 1); // ID = MPEG-4
        push(0, 2); // layer
        push(1, 1); // protection_absent
        push(1, 2); // profile = LC (AOT 2)
        push(4, 4); // sampling_frequency_index = 44100
        push(0, 1); // private_bit
        push(1, 3); // channel_configuration = mono
        push(0, 1); // original_copy
        push(0, 1); // home
        push(0, 1); // copyright_identification_bit
        push(0, 1); // copyright_identification_start
        push(7, 13); // aac_frame_length = 7 (header only)
        push(0x7FF, 11); // adts_buffer_fullness = VBR sentinel
        push(0, 2); // number_of_raw_data_blocks_in_frame - 1
        let mut out = [0u8; 7];
        for (i, chunk) in bits.chunks(8).enumerate() {
            let mut b = 0u8;
            for (j, &bit) in chunk.iter().enumerate() {
                b |= bit << (7 - j);
            }
            out[i] = b;
        }
        out
    }

    #[test]
    fn probe_scores_adts_sync() {
        let hdr = synth_adts_header();
        let tag = CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC);
        let ctx = ProbeContext::new(&tag).packet(&hdr);
        // Confirm the test vector is a well-formed ADTS header first.
        assert!(AdtsHeader::parse(&hdr).is_ok(), "test ADTS header invalid");
        assert!((probe_aac(&ctx) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_scores_low_for_non_adts() {
        let pkt = [0x00u8, 0x00, 0x00, 0x00];
        let tag = CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC);
        let ctx = ProbeContext::new(&tag).packet(&pkt);
        assert!(probe_aac(&ctx) < 0.5);
    }

    #[test]
    fn probe_default_without_packet() {
        let tag = CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC);
        let ctx = ProbeContext::new(&tag);
        assert!((probe_aac(&ctx) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn probe_uses_fixture_first_bytes() {
        let Some(buf) = fixture_bytes("aac-lc-mono-8000-16kbps-adts") else {
            return;
        };
        let tag = CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC);
        let ctx = ProbeContext::new(&tag).packet(&buf);
        assert!((probe_aac(&ctx) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn register_installs_decoder_factory() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        assert!(reg.has_decoder(&CodecId::new(CODEC_ID_STR)));
        let _ = reg
            .first_decoder(&build_params(44_100, 2))
            .expect("registry-built decoder");
    }

    #[test]
    fn register_claims_all_tags() {
        let mut reg = CodecRegistry::new();
        register_codecs(&mut reg);
        for tag in [
            CodecTag::mp4_object_type(MP4_OBJECT_TYPE_AAC),
            CodecTag::wave_format(WAVE_FORMAT_RAW_AAC1),
            CodecTag::wave_format(WAVE_FORMAT_MPEG_ADTS_AAC),
            CodecTag::fourcc(b"mp4a"),
            CodecTag::fourcc(b"aac "),
            CodecTag::matroska("A_AAC"),
        ] {
            let ctx = ProbeContext::new(&tag);
            assert_eq!(
                reg.resolve_tag_ref(&ctx).map(|c| c.as_str()),
                Some(CODEC_ID_STR),
                "tag {tag:?} did not resolve to aac",
            );
        }
    }
}
