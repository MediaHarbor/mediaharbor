//! `AudioSpecificConfig` parser.
//!
//! ISO/IEC 14496-3 §1.6.2.1 Table 1.15 defines the canonical
//! `AudioSpecificConfig()` (ASC) as the out-of-band descriptor for
//! an MPEG-4 audio elementary stream. It carries
//! `audioObjectType`, `samplingFrequencyIndex` (and the 24-bit
//! escape rate when index is `0xf`), `channelConfiguration`, and a
//! per-AOT body (Table 1.17).
//!
//! Phase 1 parses the wrapper plus the body for **AOTs that route
//! to `GASpecificConfig`** (§4.4.1 Table 4.1) — the General Audio
//! branch covering all AAC variants: 1 (Main), 2 (LC), 3 (SSR), 4
//! (LTP), 6 (scalable), 7 (TwinVQ), 17 (ER AAC LC), 19 (ER AAC
//! LTP), 20 (ER AAC scalable), 21 (ER TwinVQ), 22 (ER BSAC), 23
//! (ER AAC LD). The hierarchical SBR (AOT 5) and PS (AOT 29)
//! outer-wrappers are also recognised: the parser reads the inner
//! `samplingFrequencyIndex` + (re-read) `audioObjectType` and
//! records `sbr_present` / `ps_present` so a later HE-AAC round can
//! drive SBR setup off the parsed ASC.
//!
//! All other AOTs return [`Error::UnsupportedAot`] so the spec
//! gap is explicit at the call site.
//!
//! ## What round 192 adds
//!
//! * The Table 1.15 trailing `syncExtensionType == 0x2b7` probe used
//!   for *backward-compatible* implicit SBR / PS signalling in the
//!   AudioSpecificConfig (§1.6.5, §1.6.6). After the per-AOT body
//!   and `epConfig`, when `extensionAudioObjectType != 5` and the
//!   carrier has `>= 16` bits remaining, the parser reads an 11-bit
//!   `syncExtensionType` value: if it equals `0x2b7` it consumes a
//!   nested `GetAudioObjectType()` and (when the resolved extension
//!   AOT is `5`) the `sbrPresentFlag`, optional
//!   `extensionSamplingFrequencyIndex` (with the same 24-bit escape
//!   as the outer ASC), and a second 11-bit `syncExtensionType`
//!   gated on `>= 12` further bits — if it equals `0x548` the
//!   `psPresentFlag` follows. The AOT-22 (ER BSAC) extension branch
//!   is also parsed: `sbrPresentFlag` (+ optional
//!   `extensionSamplingFrequencyIndex`) then a mandatory 4-bit
//!   `extensionChannelConfiguration`. The probe result lands in
//!   [`AudioSpecificConfig::trailing_sbr_probe`] as
//!   [`SbrExtensionProbe`]; when the probe resolves SBR or PS,
//!   `asc.sbr_present` / `asc.ps_present` are updated to reflect
//!   the implicit signalling. This entry point is exposed as
//!   [`AudioSpecificConfig::parse_bits_bounded`] for carriers that
//!   know the ASC bit length (LATM `StreamMuxConfig`, esds AudioObj
//!   descriptor); the byte-slice [`AudioSpecificConfig::parse`]
//!   computes the bound automatically. The original bit-level
//!   [`AudioSpecificConfig::parse_bits`] keeps its no-probe
//!   semantics so existing callers that pass a BitReader carrying
//!   trailing carrier bytes are not surprised by a stray 11-bit
//!   match.
//!
//! ## What is *not* parsed yet
//!
//! * `AOT 5` / `AOT 29` *implicit-extension* path **via the FIL
//!   extension_payload**: when the outer AOT is 2 (LC) and the
//!   SBR/PS extension is announced via the FIL `extension_payload`
//!   inside the raw_data_block stream (not the ASC trailing probe),
//!   the ASC alone does not carry the information — the decoder
//!   must look at the FIL stream. Round 192 only resolves the
//!   *ASC-side* implicit signalling (the Table 1.15
//!   `syncExtensionType == 0x2b7` probe). When neither signalling
//!   form is present, the ASC parser correctly records
//!   `sbr_present = false` / `ps_present = false` because no ASC
//!   bit said otherwise.
//!
//! ## What round 177 adds
//!
//! * `GASpecificConfig` `extensionFlag == 1` body (Table 4.1):
//!   AOT 22 (ER BSAC) emits a 5-bit `numOfSubFrame` + 11-bit
//!   `layer_length`; AOTs 17 / 19 / 20 / 23 emit the 1-bit
//!   `aacSectionDataResilienceFlag` + 1-bit
//!   `aacScalefactorDataResilienceFlag` + 1-bit
//!   `aacSpectralDataResilienceFlag` triplet; every AOT closes the
//!   body with a 1-bit `extensionFlag3` (the Version 3 body behind it
//!   is reserved per the spec's own "tbd in version 3" comment, so the
//!   bit is surfaced but the body is rejected with
//!   [`Error::UnsupportedAscExtensionFlag3`] when set).
//! * `epConfig` for ER object types (Table 1.15) — the 2-bit
//!   `epConfig` field that follows the AOT body for AOTs 17, 19, 20,
//!   21, 22, 23, 24, 25, 26, 27, 39. `epConfig == 2` or
//!   `epConfig == 3` further triggers the
//!   `ErrorProtectionSpecificConfig()` body, which Phase 1 does not
//!   parse — the ASC parser surfaces
//!   [`Error::UnsupportedEpConfig`] in that case rather than
//!   silently returning a partial ASC.

use oxideav_core::bits::BitReader;

use crate::adts::ADTS_SAMPLE_RATES_HZ;
use crate::pce::Pce;
use crate::{Error, Result};

/// Outer `audioObjectType` values for which the ASC body is
/// `GASpecificConfig` per Table 1.17.
const GA_AOTS: &[u8] = &[1, 2, 3, 4, 6, 7, 17, 19, 20, 21, 22, 23];

/// AOTs that signal SBR (5) or SBR + PS (29) as an outer wrapper
/// around an inner GA AOT (typically 2 = LC). The ASC walks the
/// extension sample-rate/index and re-reads `GetAudioObjectType`
/// before dispatching to the inner body.
const SBR_AOT: u8 = 5;
const PS_AOT: u8 = 29;

/// AOTs whose `GASpecificConfig` extension-flag body emits the 5-bit
/// `numOfSubFrame` + 11-bit `layer_length` pair (Table 4.1).
const GA_EXTENSION_NUM_OF_SUBFRAME_AOTS: &[u8] = &[22];

/// AOTs whose `GASpecificConfig` extension-flag body emits the three
/// error-resilience flags (Table 4.1).
const GA_EXTENSION_RESILIENCE_AOTS: &[u8] = &[17, 19, 20, 23];

/// AOTs whose ASC trailing body carries the 2-bit `epConfig` field
/// (Table 1.15 outer `switch (audioObjectType)` for the ER object
/// types).
const EP_CONFIG_AOTS: &[u8] = &[17, 19, 20, 21, 22, 23, 24, 25, 26, 27, 39];

/// Outer 11-bit `syncExtensionType` marker that introduces the Table
/// 1.15 trailing implicit-SBR signalling block.
pub const SYNC_EXTENSION_TYPE_SBR: u16 = 0x2b7;

/// Inner 11-bit `syncExtensionType` marker that introduces the
/// `psPresentFlag` inside the SBR (`extensionAudioObjectType == 5`)
/// branch of the Table 1.15 trailing probe.
pub const SYNC_EXTENSION_TYPE_PS: u16 = 0x548;

/// Width of the `syncExtensionType` field (Table 1.15).
pub const SYNC_EXTENSION_TYPE_BITS: u32 = 11;

/// `extensionAudioObjectType` value that signals HE-AAC SBR inside
/// the trailing probe (Table 1.15).
pub const TRAILING_EXTENSION_AOT_SBR: u8 = 5;

/// `extensionAudioObjectType` value that signals ER BSAC inside the
/// trailing probe (Table 1.15).
pub const TRAILING_EXTENSION_AOT_BSAC: u8 = 22;

/// Length of the IMDCT frame in samples for a GA AOT, controlled by
/// `frameLengthFlag`. ISO/IEC 14496-3 §4.4.1 semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLength {
    /// 1024 samples per channel (frameLengthFlag = 0, all GA AOTs
    /// except SSR / ELD).
    Long1024,
    /// 960 samples per channel (frameLengthFlag = 1, all GA AOTs
    /// except SSR / ELD).
    Long960,
}

impl FrameLength {
    /// Resolved sample count per output channel.
    pub fn samples(self) -> u32 {
        match self {
            FrameLength::Long1024 => 1024,
            FrameLength::Long960 => 960,
        }
    }
}

/// Parsed `GASpecificConfig` body (Table 4.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaSpecificConfig {
    /// Resolved frame length (1024 vs 960 lines).
    pub frame_length: FrameLength,
    /// `dependsOnCoreCoder` bit. `false` for plain AAC LC.
    pub depends_on_core_coder: bool,
    /// `coreCoderDelay` (14 bits, only present when
    /// `dependsOnCoreCoder == 1`).
    pub core_coder_delay: Option<u16>,
    /// `extensionFlag` bit. Shall be `false` for AOTs 1, 2, 3, 4,
    /// 6, 7; shall be `true` for AOTs 17, 19, 20, 21, 22, 23.
    pub extension_flag: bool,
    /// Inline `program_config_element()` (only present when the
    /// surrounding ASC's `channelConfiguration == 0`).
    pub pce: Option<Pce>,
    /// `layerNr` (3 bits, only present when AOT ∈ {6, 20}).
    pub layer_nr: Option<u8>,
    /// Parsed extension-flag body (only populated when
    /// `extension_flag == true`).
    pub extension_body: Option<GaExtensionBody>,
}

/// Parsed body of the `if (extensionFlag)` branch of `GASpecificConfig`
/// (Table 4.1). Carries AOT-dependent subfields plus the always-present
/// `extensionFlag3` bit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GaExtensionBody {
    /// `numOfSubFrame` (5 bits) + `layer_length` (11 bits). Only
    /// present when `audioObjectType == 22` (ER BSAC).
    pub bsac_layer: Option<BsacLayerSpec>,
    /// Error-resilience triplet. Only present when
    /// `audioObjectType ∈ {17, 19, 20, 23}` (ER AAC LC / ER AAC LTP /
    /// ER AAC scalable / ER AAC LD).
    pub resilience: Option<AacResilienceFlags>,
    /// `extensionFlag3` (1 bit). Always present at the tail of the
    /// extension-flag body. ISO/IEC 14496-3:2009 reserves the body
    /// behind this flag with the comment "tbd in version 3"; Phase 1
    /// surfaces the bit but rejects the body itself with
    /// [`Error::UnsupportedAscExtensionFlag3`] when the flag is set.
    pub extension_flag3: bool,
}

/// `numOfSubFrame` + `layer_length` pair from Table 4.1, only emitted
/// when the surrounding `audioObjectType == 22` (ER BSAC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BsacLayerSpec {
    /// 5-bit `numOfSubFrame` field.
    pub num_of_sub_frame: u8,
    /// 11-bit `layer_length` field.
    pub layer_length: u16,
}

/// `aacSection / Scalefactor / Spectral DataResilienceFlag` triplet from
/// Table 4.1, only emitted when the surrounding `audioObjectType ∈
/// {17, 19, 20, 23}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AacResilienceFlags {
    /// `aacSectionDataResilienceFlag`. Routes `section_data()` through
    /// the §4.4.6 RVLC branch in a downstream round.
    pub section_data: bool,
    /// `aacScalefactorDataResilienceFlag`. Routes `scale_factor_data()`
    /// through the §4.4.6 RVLC branch in a downstream round.
    pub scalefactor_data: bool,
    /// `aacSpectralDataResilienceFlag`. Routes `spectral_data()` through
    /// the §4.4.6 HCR / reordered branch in a downstream round.
    pub spectral_data: bool,
}

/// Result of the Table 1.15 trailing `syncExtensionType == 0x2b7`
/// implicit-SBR / PS / BSAC-extension probe (§1.6.5).
///
/// Only ever populated when the ASC parser reaches the trailing-bits
/// branch — i.e. the outer `audioObjectType` is **not** the
/// hierarchical SBR wrapper (5) or PS wrapper (29) (those already
/// emit `sbr_present` / `ps_present` from their explicit-signalling
/// path), at least 16 bits remain in the ASC carrier, and the next
/// 11 bits equal [`SYNC_EXTENSION_TYPE_SBR`] (`0x2b7`).
///
/// `extension_audio_object_type` is the resolved nested AOT
/// (`GetAudioObjectType()` after the `0x2b7` sync). Round 192
/// implements the bodies for `extension_audio_object_type == 5`
/// (HE-AAC SBR with the optional `0x548` PS sub-probe) and
/// `extension_audio_object_type == 22` (ER BSAC); any other resolved
/// extension AOT surfaces as
/// [`crate::Error::UnsupportedTrailingExtensionAot`] at parse time
/// (the body bit-layout is not defined by Table 1.15 for those).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SbrExtensionProbe {
    /// Resolved `extensionAudioObjectType` immediately after the
    /// 11-bit `syncExtensionType == 0x2b7` marker. Currently
    /// constrained to `5` (HE-AAC) or `22` (ER BSAC).
    pub extension_audio_object_type: u8,
    /// `sbrPresentFlag` (1 bit). Present for both the `ext_aot == 5`
    /// and `ext_aot == 22` branches.
    pub sbr_present_flag: bool,
    /// `extensionSamplingFrequencyIndex` (4 bits). Only present when
    /// `sbr_present_flag == true`; when the wire value is `0xf` the
    /// 24-bit `extensionSamplingFrequency` escape follows and the
    /// resolved rate lands in
    /// [`SbrExtensionProbe::extension_sample_rate`].
    pub extension_sampling_frequency_index: Option<u8>,
    /// Resolved extension sample rate in Hz (Table 1.18 lookup, or
    /// the 24-bit escape value when `extension_sampling_frequency_index
    /// == Some(0xf)`).
    pub extension_sample_rate: Option<u32>,
    /// `psPresentFlag` (1 bit). Only present when the SBR (`ext_aot
    /// == 5`) branch ran, at least 12 further bits were available, and
    /// the second 11-bit `syncExtensionType` equalled
    /// [`SYNC_EXTENSION_TYPE_PS`] (`0x548`).
    pub ps_present_flag: Option<bool>,
    /// `extensionChannelConfiguration` (4 bits). Only present when
    /// the resolved extension AOT is `22` (ER BSAC).
    pub extension_channel_configuration: Option<u8>,
}

/// Parsed `AudioSpecificConfig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSpecificConfig {
    /// Outer `audioObjectType` *as encoded on the wire* (before any
    /// SBR/PS unwrap). For HE-AAC v1 signalled hierarchically this
    /// is `5`; for HE-AAC v2 it is `29`.
    pub outer_aot: u8,
    /// Inner / effective `audioObjectType` after unwrapping the
    /// AOT-5 (SBR) and AOT-29 (PS) hierarchical containers. For
    /// plain AAC-LC this equals `outer_aot`.
    pub aot: u8,
    /// 4-bit `samplingFrequencyIndex` (the *core* index — for
    /// hierarchical HE-AAC this is the inner AAC's index, half the
    /// SBR output rate).
    pub sampling_frequency_index: u8,
    /// Resolved core sample rate. Resolves
    /// [`AudioSpecificConfig::sampling_frequency_index`] via
    /// Table 1.18, or reads the explicit 24-bit
    /// `samplingFrequency` field when the index is `0xf`.
    pub sample_rate: u32,
    /// `channelConfiguration` (4 bits). `0` ⇔ defined by an inline
    /// PCE inside `GASpecificConfig`.
    pub channel_configuration: u8,
    /// `true` ⇔ the ASC explicitly signalled SBR (outer AOT 5 or
    /// 29). Does **not** capture implicit SBR signalling carried in
    /// the FIL `extension_payload` of the AAC bitstream.
    pub sbr_present: bool,
    /// `true` ⇔ the ASC explicitly signalled PS (outer AOT 29).
    pub ps_present: bool,
    /// `extensionSamplingFrequencyIndex` (only present when
    /// `outer_aot ∈ {5, 29}`).
    pub extension_sampling_frequency_index: Option<u8>,
    /// Resolved extension sample rate (SBR output rate). Present
    /// when `extension_sampling_frequency_index` is set.
    pub extension_sample_rate: Option<u32>,
    /// `extensionChannelConfiguration` (only present when
    /// `outer_aot ∈ {5, 29}` *and* the inner AOT is `22` =
    /// ER BSAC).
    pub extension_channel_configuration: Option<u8>,
    /// Parsed body for the inner AOT. For GA AOTs this is
    /// populated; for other AOTs (which Phase 1 rejects with
    /// [`Error::UnsupportedAot`]) this is never returned.
    pub ga_body: GaSpecificConfig,
    /// `epConfig` (2 bits) for the ER object types listed in the
    /// Table 1.15 outer `switch (audioObjectType)` (AOTs 17, 19, 20,
    /// 21, 22, 23, 24, 25, 26, 27, 39). `None` for every other AOT.
    /// When the field is `2` or `3`, the spec mandates parsing the
    /// trailing `ErrorProtectionSpecificConfig()` body — Phase 1
    /// does **not** parse that body and surfaces
    /// [`Error::UnsupportedEpConfig`] at the call site.
    pub ep_config: Option<u8>,
    /// Result of the Table 1.15 trailing `syncExtensionType == 0x2b7`
    /// implicit-SBR probe (§1.6.5). Only ever populated when the
    /// outer `audioObjectType` is not the explicit SBR (5) or PS
    /// (29) wrapper, the carrier had at least 16 bits remaining
    /// after the per-AOT body + `epConfig`, and the next 11 bits
    /// equalled [`SYNC_EXTENSION_TYPE_SBR`]. When the probe resolves
    /// SBR or PS, [`AudioSpecificConfig::sbr_present`] /
    /// [`AudioSpecificConfig::ps_present`] are also updated to
    /// reflect the implicit signalling. Only populated by
    /// [`AudioSpecificConfig::parse`] (which knows the byte-slice
    /// bound) and the new [`AudioSpecificConfig::parse_bits_bounded`]
    /// entry point; the older
    /// [`AudioSpecificConfig::parse_bits`] keeps its no-probe
    /// semantics.
    pub trailing_sbr_probe: Option<SbrExtensionProbe>,
}

impl AudioSpecificConfig {
    /// Parse an `AudioSpecificConfig` from `data`. Returns the
    /// resolved ASC and the bit-length consumed (so the caller can
    /// skip the rest of the carrier — `esds` payload, LATM
    /// StreamMuxConfig, etc.).
    ///
    /// The byte-slice bound is also forwarded into the Table 1.15
    /// trailing `syncExtensionType == 0x2b7` implicit-SBR probe
    /// (§1.6.5), so the bit-length returned here already reflects
    /// any consumed trailing-probe fields.
    pub fn parse(data: &[u8]) -> Result<(Self, u64)> {
        let mut reader = BitReader::new(data);
        let asc_bit_length = (data.len() as u64).saturating_mul(8);
        let asc = Self::parse_bits_bounded(&mut reader, 0, asc_bit_length)?;
        Ok((asc, reader.bit_position()))
    }

    /// Parse from a pre-existing [`BitReader`] given the
    /// `origin_bit_offset` (the absolute bit position of the start
    /// of the ASC) and an explicit `asc_bit_length` (the total
    /// bit-length of the ASC inside the carrier, as conveyed by
    /// e.g. LATM `StreamMuxConfig`'s `audioSpecificConfig` length
    /// field). The trailing Table 1.15 `syncExtensionType == 0x2b7`
    /// probe consumes bits up to that bound.
    pub fn parse_bits_bounded(
        reader: &mut BitReader<'_>,
        origin_bit_offset: u64,
        asc_bit_length: u64,
    ) -> Result<Self> {
        let start_bit = reader.bit_position();
        let mut asc = Self::parse_bits_core(reader, origin_bit_offset)?;
        let consumed = reader.bit_position().saturating_sub(start_bit);
        // The Table 1.15 trailing-probe guard `extensionAudioObjectType
        // != 5` translates into "skip the probe when the explicit
        // hierarchical SBR (outer AOT 5) or PS (outer AOT 29) wrapper
        // already established `extensionAudioObjectType == 5`". For
        // every other outer AOT the spec defaults
        // `extensionAudioObjectType = 0` (per §1.6.5), so the
        // `!= 5` predicate is satisfied and the probe runs.
        let already_hierarchical_sbr = asc.outer_aot == SBR_AOT || asc.outer_aot == PS_AOT;
        if !already_hierarchical_sbr && consumed < asc_bit_length {
            let remaining = asc_bit_length - consumed;
            if let Some(probe) = parse_trailing_sbr_probe(reader, remaining)? {
                if probe.extension_audio_object_type == TRAILING_EXTENSION_AOT_SBR {
                    if probe.sbr_present_flag {
                        asc.sbr_present = true;
                        asc.extension_sampling_frequency_index =
                            probe.extension_sampling_frequency_index;
                        asc.extension_sample_rate = probe.extension_sample_rate;
                    }
                    if probe.ps_present_flag == Some(true) {
                        asc.ps_present = true;
                    }
                } else if probe.extension_audio_object_type == TRAILING_EXTENSION_AOT_BSAC {
                    if probe.sbr_present_flag {
                        asc.sbr_present = true;
                        asc.extension_sampling_frequency_index =
                            probe.extension_sampling_frequency_index;
                        asc.extension_sample_rate = probe.extension_sample_rate;
                    }
                    asc.extension_channel_configuration = probe.extension_channel_configuration;
                }
                asc.trailing_sbr_probe = Some(probe);
            }
        }
        Ok(asc)
    }

    /// Parse from a pre-existing [`BitReader`] given the
    /// `origin_bit_offset` (the absolute bit position of the start
    /// of the ASC). Used by carriers that embed an ASC inside a
    /// wider bit-stream — LATM `StreamMuxConfig` being the obvious
    /// case, where the ASC starts at a non-byte-aligned position
    /// relative to the LATM packet's first bit. The
    /// `origin_bit_offset` is forwarded into PCE parsing so the
    /// Table 4.2 `byte_alignment()` note is honoured.
    ///
    /// This entry point does **not** invoke the Table 1.15 trailing
    /// `syncExtensionType == 0x2b7` implicit-SBR probe: the
    /// `BitReader` may carry trailing carrier bytes that are not
    /// part of the ASC, and probing into them would mis-interpret
    /// garbage as a `0x2b7` marker. Carriers that know the exact
    /// ASC bit-length should call
    /// [`AudioSpecificConfig::parse_bits_bounded`] instead.
    pub fn parse_bits(reader: &mut BitReader<'_>, origin_bit_offset: u64) -> Result<Self> {
        Self::parse_bits_core(reader, origin_bit_offset)
    }

    fn parse_bits_core(reader: &mut BitReader<'_>, origin_bit_offset: u64) -> Result<Self> {
        // Outer audioObjectType + samplingFrequencyIndex (+ escape)
        let outer_aot = read_aot(reader)?;
        let sampling_frequency_index = read_u8(reader, 4)?;
        let core_sample_rate = if sampling_frequency_index == 0xf {
            read_u32(reader, 24)?
        } else {
            resolve_sample_rate_index(sampling_frequency_index)?
        };
        let channel_configuration = read_u8(reader, 4)?;

        // Hierarchical SBR / PS unwrap.
        let mut sbr_present = false;
        let mut ps_present = false;
        let mut ext_sfi = None;
        let mut ext_rate = None;
        let mut ext_chan_cfg = None;
        let mut effective_aot = outer_aot;

        if outer_aot == SBR_AOT || outer_aot == PS_AOT {
            sbr_present = true;
            if outer_aot == PS_AOT {
                ps_present = true;
            }
            let sfi = read_u8(reader, 4)?;
            let rate = if sfi == 0xf {
                read_u32(reader, 24)?
            } else {
                resolve_sample_rate_index(sfi)?
            };
            ext_sfi = Some(sfi);
            ext_rate = Some(rate);
            effective_aot = read_aot(reader)?;
            if effective_aot == 22 {
                ext_chan_cfg = Some(read_u8(reader, 4)?);
            }
        }

        // Body dispatch — Phase 1 only handles GA.
        if !GA_AOTS.contains(&effective_aot) {
            return Err(Error::UnsupportedAot(effective_aot));
        }
        let ga_body = parse_ga_specific_config(
            reader,
            channel_configuration,
            effective_aot,
            origin_bit_offset,
        )?;

        // Table 1.15 outer `switch (audioObjectType)` — `epConfig`
        // for ER object types. `epConfig == 2 || epConfig == 3`
        // triggers the `ErrorProtectionSpecificConfig()` body which
        // Phase 1 does not parse.
        let ep_config = if EP_CONFIG_AOTS.contains(&effective_aot) {
            let v = read_u8(reader, 2)?;
            if v == 2 || v == 3 {
                return Err(Error::UnsupportedEpConfig(v));
            }
            Some(v)
        } else {
            None
        };

        Ok(AudioSpecificConfig {
            outer_aot,
            aot: effective_aot,
            sampling_frequency_index,
            sample_rate: core_sample_rate,
            channel_configuration,
            sbr_present,
            ps_present,
            extension_sampling_frequency_index: ext_sfi,
            extension_sample_rate: ext_rate,
            extension_channel_configuration: ext_chan_cfg,
            ga_body,
            ep_config,
            trailing_sbr_probe: None,
        })
    }

    /// Number of audio channels implied by the
    /// `channelConfiguration` (Table 1.19); `0` means "defined by
    /// PCE" and returns the PCE-derived count.
    pub fn channel_count(&self) -> usize {
        match self.channel_configuration {
            0 => self
                .ga_body
                .pce
                .as_ref()
                .map(Pce::channel_count)
                .unwrap_or(0),
            1 => 1,
            2 => 2,
            3 => 3,
            4 => 4,
            5 => 5,
            6 => 6, // 5.1 — LFE counts as one channel
            7 => 8, // 7.1 — LFE counts as one channel
            _ => 0,
        }
    }
}

fn parse_ga_specific_config(
    reader: &mut BitReader<'_>,
    channel_configuration: u8,
    aot: u8,
    origin_bit_offset: u64,
) -> Result<GaSpecificConfig> {
    // Table 4.1 — GASpecificConfig.
    let frame_length_flag = read_bit(reader)?;
    let frame_length = if frame_length_flag {
        FrameLength::Long960
    } else {
        FrameLength::Long1024
    };
    let depends_on_core_coder = read_bit(reader)?;
    let core_coder_delay = if depends_on_core_coder {
        Some(read_u32(reader, 14)? as u16)
    } else {
        None
    };
    let extension_flag = read_bit(reader)?;

    let pce = if channel_configuration == 0 {
        Some(Pce::parse(reader, origin_bit_offset)?)
    } else {
        None
    };

    let layer_nr = if aot == 6 || aot == 20 {
        Some(read_u8(reader, 3)?)
    } else {
        None
    };

    let extension_body = if extension_flag {
        Some(parse_ga_extension_body(reader, aot)?)
    } else {
        None
    };

    Ok(GaSpecificConfig {
        frame_length,
        depends_on_core_coder,
        core_coder_delay,
        extension_flag,
        pce,
        layer_nr,
        extension_body,
    })
}

/// Parse the `if (extensionFlag)` body of `GASpecificConfig()` per
/// Table 4.1. Subfield gating mirrors the AOT lists in the spec
/// listing exactly: `numOfSubFrame` / `layer_length` only for
/// `audioObjectType == 22`; the resilience triplet only for
/// `audioObjectType ∈ {17, 19, 20, 23}`; `extensionFlag3` always.
fn parse_ga_extension_body(reader: &mut BitReader<'_>, aot: u8) -> Result<GaExtensionBody> {
    let bsac_layer = if GA_EXTENSION_NUM_OF_SUBFRAME_AOTS.contains(&aot) {
        let num_of_sub_frame = read_u8(reader, 5)?;
        let layer_length = read_u32(reader, 11)? as u16;
        Some(BsacLayerSpec {
            num_of_sub_frame,
            layer_length,
        })
    } else {
        None
    };

    let resilience = if GA_EXTENSION_RESILIENCE_AOTS.contains(&aot) {
        let section_data = read_bit(reader)?;
        let scalefactor_data = read_bit(reader)?;
        let spectral_data = read_bit(reader)?;
        Some(AacResilienceFlags {
            section_data,
            scalefactor_data,
            spectral_data,
        })
    } else {
        None
    };

    let extension_flag3 = read_bit(reader)?;
    if extension_flag3 {
        return Err(Error::UnsupportedAscExtensionFlag3);
    }

    Ok(GaExtensionBody {
        bsac_layer,
        resilience,
        extension_flag3,
    })
}

/// Probe the Table 1.15 trailing `syncExtensionType == 0x2b7` /
/// `0x548` chain for implicit SBR / PS / BSAC-extension signalling
/// (§1.6.5, §1.6.6).
///
/// Returns `Ok(None)` if any of the following holds (each is a
/// normative "no implicit signalling present" outcome — never an
/// error):
///
/// * Fewer than `SYNC_EXTENSION_TYPE_BITS + 5 = 16` bits remain
///   (the spec's outer `bits_to_decode() >= 16` guard).
/// * The next 11 bits are not [`SYNC_EXTENSION_TYPE_SBR`] (0x2b7).
///
/// When the outer 0x2b7 marker fires but the resolved
/// `extensionAudioObjectType` is neither `5` nor `22`, the parser
/// returns [`Error::UnsupportedTrailingExtensionAot`] — Table 1.15
/// does not specify a body layout for any other extension AOT and
/// the bit-reader cannot advance.
///
/// The `remaining_bits` parameter is the upper bound of bits the
/// probe is allowed to consume from the carrier (typically the
/// ASC's `bits_to_decode()`). The function never reads more than
/// `remaining_bits` bits; an UnexpectedEnd surfaces if a sub-field
/// extends past it.
fn parse_trailing_sbr_probe(
    reader: &mut BitReader<'_>,
    remaining_bits: u64,
) -> Result<Option<SbrExtensionProbe>> {
    // Outer §1.6.2.1 guard: at least 16 bits required to even
    // attempt the probe (`syncExtensionType` + the minimum 5-bit
    // `GetAudioObjectType()` base it gates).
    if remaining_bits < (SYNC_EXTENSION_TYPE_BITS as u64 + 5) {
        return Ok(None);
    }
    let sync = read_u32(reader, SYNC_EXTENSION_TYPE_BITS)? as u16;
    if sync != SYNC_EXTENSION_TYPE_SBR {
        return Ok(None);
    }

    let extension_audio_object_type = read_aot(reader)?;
    match extension_audio_object_type {
        TRAILING_EXTENSION_AOT_SBR => parse_trailing_sbr_branch(reader, remaining_bits),
        TRAILING_EXTENSION_AOT_BSAC => parse_trailing_bsac_branch(reader),
        other => Err(Error::UnsupportedTrailingExtensionAot(other)),
    }
}

/// `extensionAudioObjectType == 5` body of the trailing probe:
/// `sbrPresentFlag` + optional `extensionSamplingFrequencyIndex` /
/// `extensionSamplingFrequency` + optional second `syncExtensionType
/// == 0x548` + `psPresentFlag` (Table 1.15).
fn parse_trailing_sbr_branch(
    reader: &mut BitReader<'_>,
    initial_remaining_bits: u64,
) -> Result<Option<SbrExtensionProbe>> {
    let sbr_present_flag = read_bit(reader)?;
    let mut extension_sampling_frequency_index = None;
    let mut extension_sample_rate = None;
    let mut ps_present_flag = None;
    if sbr_present_flag {
        let sfi = read_u8(reader, 4)?;
        let rate = if sfi == 0xf {
            read_u32(reader, 24)?
        } else {
            resolve_sample_rate_index(sfi)?
        };
        extension_sampling_frequency_index = Some(sfi);
        extension_sample_rate = Some(rate);
        // §1.6.2.1 inner guard: at least 12 further bits required
        // to attempt the PS sub-probe (11-bit syncExtensionType +
        // 1-bit psPresentFlag).
        let consumed_so_far = SYNC_EXTENSION_TYPE_BITS as u64
            + 5 // GetAudioObjectType base
            + 1 // sbrPresentFlag
            + 4 // extensionSamplingFrequencyIndex
            + if sfi == 0xf { 24 } else { 0 };
        let still_available = initial_remaining_bits.saturating_sub(consumed_so_far);
        if still_available >= 12 {
            let inner_sync = read_u32(reader, SYNC_EXTENSION_TYPE_BITS)? as u16;
            if inner_sync == SYNC_EXTENSION_TYPE_PS {
                ps_present_flag = Some(read_bit(reader)?);
            }
        }
    }
    Ok(Some(SbrExtensionProbe {
        extension_audio_object_type: TRAILING_EXTENSION_AOT_SBR,
        sbr_present_flag,
        extension_sampling_frequency_index,
        extension_sample_rate,
        ps_present_flag,
        extension_channel_configuration: None,
    }))
}

/// `extensionAudioObjectType == 22` body of the trailing probe:
/// `sbrPresentFlag` + optional `extensionSamplingFrequencyIndex` /
/// `extensionSamplingFrequency` + mandatory
/// `extensionChannelConfiguration` (Table 1.15).
fn parse_trailing_bsac_branch(reader: &mut BitReader<'_>) -> Result<Option<SbrExtensionProbe>> {
    let sbr_present_flag = read_bit(reader)?;
    let mut extension_sampling_frequency_index = None;
    let mut extension_sample_rate = None;
    if sbr_present_flag {
        let sfi = read_u8(reader, 4)?;
        let rate = if sfi == 0xf {
            read_u32(reader, 24)?
        } else {
            resolve_sample_rate_index(sfi)?
        };
        extension_sampling_frequency_index = Some(sfi);
        extension_sample_rate = Some(rate);
    }
    let extension_channel_configuration = Some(read_u8(reader, 4)?);
    Ok(Some(SbrExtensionProbe {
        extension_audio_object_type: TRAILING_EXTENSION_AOT_BSAC,
        sbr_present_flag,
        extension_sampling_frequency_index,
        extension_sample_rate,
        ps_present_flag: None,
        extension_channel_configuration,
    }))
}

/// Table 1.16 — `GetAudioObjectType()`. 5-bit base, with the `31`
/// escape unlocking a 6-bit extension.
fn read_aot(reader: &mut BitReader<'_>) -> Result<u8> {
    let base = read_u8(reader, 5)?;
    if base == 31 {
        let ext = read_u8(reader, 6)?;
        // Per spec the result is `32 + audioObjectTypeExt`. AOTs
        // above 41 are not defined in ISO/IEC 14496-3:2009; the
        // parser preserves the wire value and the body dispatch
        // will reject it.
        let aot = 32u16 + ext as u16;
        if aot > u8::MAX as u16 {
            return Err(Error::UnsupportedAot(0));
        }
        Ok(aot as u8)
    } else {
        Ok(base)
    }
}

fn resolve_sample_rate_index(idx: u8) -> Result<u32> {
    if (idx as usize) >= ADTS_SAMPLE_RATES_HZ.len() {
        return Err(Error::AdtsReservedSampleRateIndex);
    }
    Ok(ADTS_SAMPLE_RATES_HZ[idx as usize])
}

fn read_u8(reader: &mut BitReader<'_>, n: u32) -> Result<u8> {
    debug_assert!(n <= 8);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u8)
}

fn read_u32(reader: &mut BitReader<'_>, n: u32) -> Result<u32> {
    reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)
}

fn read_bit(reader: &mut BitReader<'_>) -> Result<bool> {
    reader.read_bit().map_err(|_| Error::UnexpectedEnd)
}
