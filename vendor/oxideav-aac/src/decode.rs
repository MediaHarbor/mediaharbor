//! Stream-level ADTS decode driver — raw_data_block walk to interleaved
//! 16-bit PCM.
//!
//! [`crate::element_decode::ElementDecoder`] decodes *one* channel
//! element per call and carries that element's §4.6.11 overlap-add tail
//! across frames. This module is the layer above it: it walks the
//! §4.4.2.1 `raw_data_block()` of one ADTS frame
//! ([`crate::raw_data_block::Walker`]), dispatches each `id_syn_ele`
//! onto a per-element-slot [`ElementDecoder`] (keyed by `(syntactic
//! element id, element_instance_tag)` so each element's filterbank state
//! is independent), composes the channel-element bodies via
//! [`crate::ics_body`] / [`crate::spectral_data`], and renders the
//! frame's per-channel time signals to the element-order interleaved
//! 16-bit PCM layout via [`crate::pcm`].
//!
//! Scope: AAC-LC (and the other General-Audio object types the
//! per-tool chain covers) carried in ADTS, single `raw_data_block` per
//! frame, the channel elements the staged-fixture encoders emit
//! (SCE / LFE / CPE, plus the consumed-and-ignored FIL / DSE /
//! PCE). A `coupling_channel_element()` (CCE) is now **fully consumed**
//! from the bitstream via [`crate::cce::CouplingChannelElement`] so a
//! CCE-bearing frame stays bit-aligned and its SCE / CPE channels still
//! decode; the §4.6.8.3.3 cross-element coupling (scaling the CCE
//! spectrum onto the addressed targets) is decoded but not yet applied,
//! so the CCE contributes no output channel of its own. SBR / PS
//! up-sampling and multi-`raw_data_block` ADTS frames remain out of
//! scope (the same limits the element driver carries).
//!
//! ## Provenance
//!
//! The §4.4.2.1 `raw_data_block()` walk, the §4.4.2.3 `channel_pair_
//! element()` `common_window` / `ms_mask_present` header, and the
//! §4.6.11 PCM output contract are from ISO/IEC 14496-3 / 13818-7 staged
//! under `docs/audio/aac/`. No part of the byte ordering or the element
//! dispatch comes from any external decoder.

use std::collections::HashMap;

use oxideav_core::bits::BitReader;

use crate::adts::AdtsHeader;
use crate::cce::CouplingChannelElement;
use crate::element_decode::{ChannelInput, CpeJointStereo, ElementDecoder};
use crate::extension_payload::{ExtensionPayload, ExtensionPayloadOrSbr};
use crate::ics_body::IcsBody;
use crate::ics_info::IcsInfo;
use crate::ms_stereo::MsMaskPresent;
use crate::pcm::interleave_s16;
use crate::raw_data_block::{Element, IdSynEle, Walker};
use crate::sbr_decoder::SbrDecoder;
use crate::sbr_extension::SbrExtensionData;
use crate::sbr_header::SbrHeader;
use crate::spectral_data::SpectralData;
use crate::{Error, Result};

/// The §4.6.11 per-frame sample count for the 1024-line transform
/// family (the only family this crate's `swb_offset` layout covers).
pub const FRAME_LEN: usize = 1024;

/// One decoded ADTS frame: the interleaved 16-bit PCM plus the geometry
/// needed to interpret it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedFrame {
    /// Interleaved 16-bit PCM, `channels` samples per time index. For a
    /// default `channelConfiguration` (Table 1.19, values 1–6) the
    /// channels are in the canonical [`crate::channel_map`] output order
    /// (e.g. 5.1 is `L, R, C, LFE, Ls, Rs`); for the unmapped configs
    /// (`0` PCE-defined, `7`) they stay in `raw_data_block` element order
    /// (an SCE/LFE contributes one channel, a CPE two). Length is
    /// `FRAME_LEN * channels` for the plain AAC path, or
    /// `2 * FRAME_LEN * channels` once the stream is SBR-active
    /// (HE-AAC dual-rate output).
    pub pcm: Vec<i16>,
    /// Number of interleaved channels this frame produced.
    pub channels: usize,
    /// The frame's sampling rate in Hz: the ADTS-signalled core rate,
    /// doubled once the stream is SBR-active.
    pub sample_rate: u32,
}

/// Stateful whole-stream ADTS decoder.
///
/// Holds one [`ElementDecoder`] per `(element-id, instance-tag)` slot so
/// every channel element's §4.6.11 overlap-add tail, §4.6.7 LTP history,
/// and §4.6.6 predictor state persist across the frames of the stream.
/// Construct one [`StreamDecoder`] per stream and feed it ADTS frames in
/// order via [`Self::decode_frame`], or hand it the whole byte buffer
/// via [`Self::decode_all`].
#[derive(Debug, Default)]
pub struct StreamDecoder {
    decoders: HashMap<(u8, u8), ElementDecoder>,
    /// One §4.6.18 SBR back-end per channel-element slot (HE-AAC).
    sbr: HashMap<(u8, u8), SbrDecoder>,
    /// The threaded previous `sbr_header()` per slot (the
    /// `bs_header_flag == 0` reuse path).
    sbr_prev_header: HashMap<(u8, u8), SbrHeader>,
    /// Latched once any frame carries SBR data: from then on every
    /// frame is emitted at the doubled (SBR) rate — SBR-less frames go
    /// through the §4.6.18.5 pure-upsampling path so the output rate
    /// never flaps.
    sbr_active: bool,
}

impl StreamDecoder {
    /// A fresh stream decoder with no element state.
    #[must_use]
    pub fn new() -> Self {
        StreamDecoder::default()
    }

    /// Decode one ADTS frame's `raw_data_block()` payload to interleaved
    /// 16-bit PCM.
    ///
    /// `header` is the parsed [`AdtsHeader`]; `payload` is the
    /// `raw_data_block()` bytes (the frame body *after* the
    /// fixed/variable header and the optional CRC — i.e. starting at the
    /// header's `payload_offset`). The channel elements update this
    /// decoder's per-slot state, so frames must be fed in stream order.
    ///
    /// A frame that yields no channel element (e.g. fill-only) returns a
    /// [`DecodedFrame`] with `channels == 0` and an empty `pcm`.
    pub fn decode_frame(&mut self, header: &AdtsHeader, payload: &[u8]) -> Result<DecodedFrame> {
        self.decode_raw_data_block(
            header.audio_object_type(),
            header.sampling_frequency_index,
            header.sample_rate(),
            header.channel_configuration,
            header.number_of_raw_data_blocks_in_frame,
            payload,
        )
    }

    /// Decode one `raw_data_block()` payload to interleaved 16-bit PCM,
    /// driven by an explicit `(audioObjectType, samplingFrequencyIndex,
    /// sampleRate)` configuration rather than an ADTS header.
    ///
    /// This is the transport-independent core that [`Self::decode_frame`]
    /// (ADTS) and the LATM/LOAS driver
    /// ([`crate::latm::LoasDecoder`]) both call: each recovers the AAC
    /// configuration from its own framing (the ADTS fixed header, or the
    /// LATM `AudioSpecificConfig`) and hands the same §4.4.2.1
    /// `raw_data_block()` bytes here. `aot` is the §1.6.2.1
    /// `audioObjectType` (already escaped past the ADTS `profile + 1`
    /// adjustment), `fs_index` is the Table 1.18
    /// `samplingFrequencyIndex`, `sample_rate` is the resolved rate the
    /// returned [`DecodedFrame`] reports, `channel_configuration` is the
    /// Table 1.19 default-layout selector that drives the §1.6.3.5
    /// element→speaker output reorder (see [`crate::channel_map`]), and
    /// `num_raw_data_blocks` is the resolved block count `N` (ADTS carries
    /// `N - 1`; LATM carries one block per payload, i.e. `N == 1`).
    pub fn decode_raw_data_block(
        &mut self,
        aot: u8,
        fs_index: u8,
        sample_rate: u32,
        channel_configuration: u8,
        num_raw_data_blocks: u8,
        payload: &[u8],
    ) -> Result<DecodedFrame> {
        let fs = fs_index;
        let mut reader = BitReader::new(payload);

        // Per channel-element outputs in element order: the decoded
        // core time signals plus any SBR extension payload that
        // followed the element in a FIL.
        struct ElementOut {
            key: (u8, u8),
            kind: IdSynEle,
            channels: Vec<Vec<f64>>,
            sbr: Option<Box<SbrExtensionData>>,
        }
        let mut elements: Vec<ElementOut> = Vec::new();
        let fs_sbr = sample_rate.saturating_mul(2);

        // `num_raw_data_blocks` is the resolved count `N`. The walker
        // returns `None` when the payload is exhausted before an explicit
        // END (real-world encoders pad the frame but do not always
        // round-trip a trailing END marker after the last element); treat
        // that as end-of-block, the same as an `Element::End`.
        'blocks: for _ in 0..num_raw_data_blocks {
            while let Some(elem) = Walker::new(&mut reader).next_element_keep_fill()? {
                match elem {
                    Element::ChannelElement {
                        kind: kind @ (IdSynEle::Sce | IdSynEle::Lfe),
                        element_instance_tag,
                    } => {
                        let body = IcsBody::parse(&mut reader, aot, fs, false)?;
                        let ics = body.ics_info.clone().ok_or(Error::ElementDecodeInvalid)?;
                        let spectral =
                            SpectralData::parse(&mut reader, &ics, &body.section_data, fs)?;
                        let ch = ChannelInput {
                            body: &body,
                            ics_info: &ics,
                            spectral: &spectral,
                        };
                        let key = (kind_id(kind), element_instance_tag);
                        let dec = self.decoders.entry(key).or_default();
                        elements.push(ElementOut {
                            key,
                            kind,
                            channels: vec![dec.decode_sce(&ch, aot, fs)?],
                            sbr: None,
                        });
                    }
                    Element::ChannelElement {
                        kind: IdSynEle::Cpe,
                        element_instance_tag,
                    } => {
                        let key = (kind_id(IdSynEle::Cpe), element_instance_tag);
                        let (l, r) = self.decode_cpe(&mut reader, aot, fs, element_instance_tag)?;
                        elements.push(ElementOut {
                            key,
                            kind: IdSynEle::Cpe,
                            channels: vec![l, r],
                            sbr: None,
                        });
                    }
                    Element::ChannelElement {
                        kind: IdSynEle::Cce,
                        element_instance_tag,
                    } => {
                        // §4.6.8.3 / Table 4.8: fully consume the coupling
                        // channel element from the bitstream (header +
                        // embedded single_channel_element + gain lists) so
                        // the walker stays bit-aligned for the following
                        // elements. The CCE contributes no output channel
                        // of its own; the §4.6.8.3.3 cross-element coupling
                        // (scale-and-add onto the addressed SCE/CPE
                        // targets) is not yet wired, so the decoded
                        // coupling spectrum is dropped — a stream that
                        // carries a CCE still decodes its SCE/CPE channels
                        // rather than aborting the whole frame.
                        let _cce = CouplingChannelElement::parse_after_tag(
                            &mut reader,
                            element_instance_tag,
                            aot,
                            fs,
                        )?;
                    }
                    Element::ChannelElement { kind, .. } => {
                        // Any other channel-element id has no decode path.
                        return Err(unsupported_element(kind));
                    }
                    Element::Fill { payload_bytes } => {
                        // The FIL body was left unconsumed: walk the
                        // Table 4.51 extension_payload() chain, routing
                        // any SBR payload onto the preceding channel
                        // element (§4.4.2.7: an SBR FIL directly follows
                        // the SCE/CPE it extends).
                        let target = elements
                            .last()
                            .filter(|el| matches!(el.kind, IdSynEle::Sce | IdSynEle::Cpe))
                            .map(|el| (el.kind, el.key));
                        if let Some(ext) =
                            self.consume_fill(&mut reader, payload_bytes, fs_sbr, target)?
                        {
                            if let Some(el) = elements.last_mut() {
                                el.sbr = Some(ext);
                            }
                        }
                    }
                    Element::Data { .. } | Element::ProgramConfig(_) => {}
                    Element::End => continue 'blocks,
                }
            }
        }

        // HE-AAC: once any frame carries SBR data the stream is emitted
        // at the doubled rate; frames without SBR go through the pure
        // upsampling path so the rate never flaps.
        if elements.iter().any(|e| e.sbr.is_some()) {
            self.sbr_active = true;
        }

        let mut channels: Vec<Vec<f64>> = Vec::new();
        let mut out_rate = sample_rate;
        if self.sbr_active {
            out_rate = fs_sbr;
            for el in &elements {
                let n_ch = el.channels.len();
                let dec = match self.sbr.entry(el.key) {
                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                    std::collections::hash_map::Entry::Vacant(v) => {
                        v.insert(SbrDecoder::new(fs_sbr, n_ch)?)
                    }
                };
                let core: Vec<&[f64]> = el.channels.iter().map(Vec::as_slice).collect();
                let up = match &el.sbr {
                    Some(ext) => dec.process_frame(ext, &core)?,
                    None => dec.upsample_frame(&core)?,
                };
                channels.extend(up);
            }
        } else {
            for el in elements {
                channels.extend(el.channels);
            }
        }

        // §1.6.3.5 / Table 1.19: a default `channelConfiguration` fixes
        // which loudspeaker each decoded element feeds. Reorder the
        // element-order channel buffers into the canonical interleaved
        // layout (a no-op for mono/stereo and for the configs this crate
        // does not yet reorder — see `channel_map`).
        let channels = crate::channel_map::reorder_channels(channel_configuration, channels);

        let pcm = interleave_s16(&channels)?;
        Ok(DecodedFrame {
            pcm,
            channels: channels.len(),
            sample_rate: out_rate,
        })
    }

    /// Walk a FIL element's Table 4.51 `extension_payload()` chain
    /// (the body was left unconsumed by
    /// [`Walker::next_element_keep_fill`]). `target` is the preceding
    /// SCE / CPE this FIL would extend (its `id_syn_ele` + slot key),
    /// or `None` when the FIL follows no channel element. Returns the
    /// decoded SBR payload, if any; the threaded `sbr_header()` reuse
    /// state is updated per slot.
    fn consume_fill(
        &mut self,
        reader: &mut BitReader<'_>,
        payload_bytes: u32,
        fs_sbr: u32,
        target: Option<(IdSynEle, (u8, u8))>,
    ) -> Result<Option<Box<SbrExtensionData>>> {
        let mut remaining = payload_bytes;
        let mut result = None;
        while remaining > 0 {
            match target {
                None => {
                    // No preceding channel element: only the non-SBR
                    // payload types are meaningful here.
                    let p = ExtensionPayload::parse(reader, remaining)?;
                    let n = p.byte_length().max(1);
                    remaining = remaining.saturating_sub(n);
                }
                Some((id_aac, slot)) => {
                    let prev = self.sbr_prev_header.get(&slot).copied();
                    match ExtensionPayload::parse_with_sbr(reader, remaining, id_aac, fs_sbr, prev)?
                    {
                        ExtensionPayloadOrSbr::Payload(p) => {
                            let n = p.byte_length().max(1);
                            remaining = remaining.saturating_sub(n);
                        }
                        ExtensionPayloadOrSbr::Sbr(ext) => {
                            self.sbr_prev_header.insert(slot, ext.header);
                            result = Some(ext);
                            remaining = 0;
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    /// Decode a whole raw-ADTS byte buffer to a vector of per-frame
    /// interleaved PCM.
    ///
    /// Skips a leading ID3v2 tag if present, then walks consecutive ADTS
    /// frames (`aac_frame_length`-delimited) to exhaustion. A truncated
    /// trailing frame (fewer bytes than its `aac_frame_length`) is
    /// rejected with [`Error::UnexpectedEnd`].
    pub fn decode_all(&mut self, data: &[u8]) -> Result<Vec<DecodedFrame>> {
        let data = skip_id3v2(data);
        let mut frames = Vec::new();
        let mut pos = 0usize;
        while pos + crate::adts::ADTS_HEADER_BYTES_NO_CRC <= data.len() {
            let (header, payload_offset) = AdtsHeader::parse(&data[pos..])?;
            let frame_len = header.aac_frame_length as usize;
            if frame_len < payload_offset || pos + frame_len > data.len() {
                return Err(Error::UnexpectedEnd);
            }
            let payload = &data[pos + payload_offset..pos + frame_len];
            frames.push(self.decode_frame(&header, payload)?);
            pos += frame_len;
        }
        Ok(frames)
    }

    /// Parse one CPE body (after the walker consumed its element-instance
    /// tag) and run it through the per-slot element decoder, returning
    /// the `(left, right)` channel time signals.
    fn decode_cpe(
        &mut self,
        reader: &mut BitReader<'_>,
        aot: u8,
        fs: u8,
        element_instance_tag: u8,
    ) -> Result<(Vec<f64>, Vec<f64>)> {
        let key = (kind_id(IdSynEle::Cpe), element_instance_tag);
        let common_window = reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
        if common_window {
            // §4.4.2.3: shared ics_info, then the Table 4.4 ms_mask.
            let ics = IcsInfo::parse(reader, aot, fs, true)?;
            let ms_bits = reader.read_u32(2).map_err(|_| Error::UnexpectedEnd)? as u8;
            let ms_mask_present = MsMaskPresent::from_bits(ms_bits)?;
            let mut ms_used: Vec<Vec<bool>> = Vec::new();
            if ms_mask_present == MsMaskPresent::Mask {
                for _g in 0..usize::from(ics.num_window_groups) {
                    let mut row = Vec::with_capacity(usize::from(ics.max_sfb));
                    for _sfb in 0..usize::from(ics.max_sfb) {
                        row.push(reader.read_bit().map_err(|_| Error::UnexpectedEnd)?);
                    }
                    ms_used.push(row);
                }
            }
            let left_body = IcsBody::parse_with_ics_info(reader, &ics, aot, false)?;
            let left_spectral = SpectralData::parse(reader, &ics, &left_body.section_data, fs)?;
            let right_body = IcsBody::parse_with_ics_info(reader, &ics, aot, false)?;
            let right_spectral = SpectralData::parse(reader, &ics, &right_body.section_data, fs)?;
            let left = ChannelInput {
                body: &left_body,
                ics_info: &ics,
                spectral: &left_spectral,
            };
            let right = ChannelInput {
                body: &right_body,
                ics_info: &ics,
                spectral: &right_spectral,
            };
            let joint = CpeJointStereo {
                ms_mask_present,
                ms_used,
            };
            let dec = self.decoders.entry(key).or_default();
            dec.decode_cpe(&left, &right, &joint, aot, fs)
        } else {
            // Non-shared CPE: each channel carries its own ics_info; no
            // M/S mask, so the joint-stereo tools do not run.
            let left_body = IcsBody::parse(reader, aot, fs, false)?;
            let left_ics = left_body
                .ics_info
                .clone()
                .ok_or(Error::ElementDecodeInvalid)?;
            let left_spectral =
                SpectralData::parse(reader, &left_ics, &left_body.section_data, fs)?;
            let right_body = IcsBody::parse(reader, aot, fs, false)?;
            let right_ics = right_body
                .ics_info
                .clone()
                .ok_or(Error::ElementDecodeInvalid)?;
            let right_spectral =
                SpectralData::parse(reader, &right_ics, &right_body.section_data, fs)?;
            let left = ChannelInput {
                body: &left_body,
                ics_info: &left_ics,
                spectral: &left_spectral,
            };
            let right = ChannelInput {
                body: &right_body,
                ics_info: &right_ics,
                spectral: &right_spectral,
            };
            let dec = self.decoders.entry(key).or_default();
            dec.decode_cpe(&left, &right, &CpeJointStereo::default(), aot, fs)
        }
    }
}

/// Map a channel-element `id_syn_ele` to the slot key's first component
/// (the element decoders are keyed independently per syntactic-element
/// id so an SCE tag 0 and a CPE tag 0 never collide).
fn kind_id(kind: IdSynEle) -> u8 {
    match kind {
        IdSynEle::Sce => 0,
        IdSynEle::Cpe => 1,
        IdSynEle::Lfe => 3,
        _ => 9,
    }
}

fn unsupported_element(kind: IdSynEle) -> Error {
    // CCE (coupling) has no decode path; surface the element-decode
    // failure mode rather than a parse error so the caller can tell a
    // structural-OK-but-unsupported element apart from a malformed one.
    let _ = kind;
    Error::ElementDecodeInvalid
}

/// Skip a leading ID3v2 tag (`"ID3"` + 6-byte header + syncsafe size +
/// optional footer) if present; otherwise return the input unchanged.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skip_id3v2_passes_through_non_id3() {
        let data = [0xFFu8, 0xF1, 0x00, 0x00];
        assert_eq!(skip_id3v2(&data), &data);
    }

    #[test]
    fn skip_id3v2_strips_a_tag() {
        // "ID3", ver 4.0, no flags, syncsafe size = 4 → 10 + 4 = 14
        // bytes of tag, then a sentinel payload byte.
        let mut data = vec![b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 4];
        data.extend_from_slice(&[0; 4]);
        data.push(0xAB);
        assert_eq!(skip_id3v2(&data), &[0xABu8]);
    }

    #[test]
    fn skip_id3v2_keeps_tag_when_size_overruns() {
        // A declared size larger than the buffer leaves the data as-is
        // rather than panicking.
        let data = vec![b'I', b'D', b'3', 4, 0, 0, 0x7f, 0x7f, 0x7f, 0x7f];
        assert_eq!(skip_id3v2(&data), &data[..]);
    }

    #[test]
    fn kind_id_separates_sce_and_cpe() {
        assert_ne!(kind_id(IdSynEle::Sce), kind_id(IdSynEle::Cpe));
        assert_ne!(kind_id(IdSynEle::Lfe), kind_id(IdSynEle::Cpe));
    }
}
