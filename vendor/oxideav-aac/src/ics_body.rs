//! `individual_channel_stream()` body walker — ISO/IEC 14496-3 §4.4.6 /
//! Table 4.50.
//!
//! This module composes the existing per-tool parsers / writers
//! (`global_gain`, [`crate::ics_info`], [`crate::section_data`],
//! [`crate::scale_factor_data`], [`crate::pulse_data`],
//! [`crate::tns_data`], [`crate::gain_control_data`]) into the
//! Table 4.50 channel-element body, **up to but not including**
//! `spectral_data()`.
//!
//! ## Why "up to but not including"
//!
//! `spectral_data()` (Table 4.56) is the per-band Huffman-coded
//! quantised MDCT-coefficient block. Its walker lives in the
//! dedicated [`crate::spectral_data`] module (round 281): this body
//! walker stops at the bit position immediately after
//! `gain_control_data()` (or the dispatching
//! `gain_control_data_present` bit when the tool is omitted) and
//! surfaces that position as [`IcsBody::spectral_data_bit_offset`],
//! from which [`crate::spectral_data::SpectralData::parse`] consumes
//! the spectrum in place — see `tests/spectral_data.rs` for the
//! sequential composition. Keeping the two stages separate mirrors
//! the CPE shared-`ics_info` split: the caller owns the reader and
//! decides when to hand off.
//!
//! This is consistent with the round-200 README ("Phase 2 in
//! progress + channel-element body walker still pending") — the
//! Walker in [`crate::raw_data_block`] emits a `ChannelElement`
//! event but does not consume the body, so the caller has to
//! re-bind a [`BitReader`] to the body region and call this module
//! to parse the structural per-tool layout.
//!
//! ## Table 4.50 layout (the non-scalable branch)
//!
//! ```text
//! individual_channel_stream(common_window, scale_flag) {
//!     global_gain;                       8  uimsbf
//!     if (!common_window && !scale_flag) {
//!         ics_info();
//!     }
//!     section_data();
//!     scale_factor_data();
//!     if (!scale_flag) {
//!         pulse_data_present;            1  uimsbf
//!         if (pulse_data_present)  pulse_data();
//!         tns_data_present;              1  uimsbf
//!         if (tns_data_present)    tns_data();
//!         gain_control_data_present;     1  uimsbf
//!         if (gain_control_data_present) gain_control_data();
//!     }
//!     if (!aacSpectralDataResilienceFlag) {
//!         spectral_data();               // NOT covered here
//!     } else {
//!         length_of_reordered_spectral_data;
//!         length_of_longest_codeword;
//!         reordered_spectral_data();     // NOT covered here
//!     }
//! }
//! ```
//!
//! Per Table 4.50 the `common_window` flag is set by the surrounding
//! `channel_pair_element()` (Table 4.4) when the two channels of the
//! CPE share the `ics_info()`; in that case the *first* call to
//! `individual_channel_stream()` reads the shared `ics_info()` (the
//! caller of this module does that — by, say, invoking
//! [`crate::ics_info::IcsInfo::parse`] directly — and then calls
//! [`IcsBody::parse_with_ics_info`]). For the single-channel form
//! (SCE / LFE) `common_window == false` and the body reads its own
//! `ics_info()` inline; the caller invokes [`IcsBody::parse`] and the
//! module both reads `ics_info()` and surfaces it.
//!
//! `scale_flag` is set by scalable streams (AOT 6) when the
//! `aac_scalable_main_header()` carries side-info that already
//! dispatched the pulse / TNS / gain-control tools. Phase 2 does not
//! yet support the scalable extension; this module rejects
//! `scale_flag == true` with [`crate::Error::NotImplemented`] so the
//! existing SCE / CPE / LFE callers keep their bit-exact round-trip.
//!
//! ## What this module covers
//!
//! * [`IcsBody::parse`] — reads `global_gain`, the inline
//!   `ics_info()`, `section_data()`, `scale_factor_data()`, the three
//!   `*_present` dispatch bits, and the dispatched
//!   `pulse_data()` / `tns_data()` / `gain_control_data()` bodies.
//! * [`IcsBody::parse_with_ics_info`] — same minus the `ics_info()`
//!   read; the caller supplies the parsed [`crate::ics_info::IcsInfo`]
//!   that the CPE-shared-info path already produced.
//! * [`IcsBody::write`] — the symmetric writer that round-trips the
//!   parsed `IcsBody` back to a bit-exact Table 4.50 prefix
//!   (everything up to and including `gain_control_data_present` /
//!   its body). The `spectral_data()` portion is the caller's
//!   responsibility (typically `push_channel_body_bits` on a
//!   [`crate::raw_data_block::FrameAssembler`]).
//! * [`IcsBody::write_with_ics_info`] — same minus the `ics_info()`
//!   write.
//!
//! Field validity:
//!
//! * Pulse-data is only legal when `window_sequence != EIGHT_SHORT`
//!   per Table 4.50 / Table 4.7; the parser surfaces
//!   [`crate::Error::PulseDataEncodeInvalid`] on a violation, the
//!   writer rejects the same shape before emitting.
//! * Gain-control-data is only legal when `audioObjectType == 3`
//!   (SSR) per the §4.6.12 normative constraint; the parser does not
//!   enforce this (it surfaces the dispatching bit verbatim so a
//!   non-SSR stream with the bit set still round-trips) but the
//!   writer does, to keep the FrameAssembler emitting only
//!   conforming streams.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::asc::AacResilienceFlags;
use crate::gain_control_data::GainControlData;
use crate::ics_info::{IcsInfo, WindowSequence};
use crate::pulse_data::PulseData;
use crate::scale_factor_data::{ErScaleFactorData, ScaleFactorData};
use crate::section_data::SectionData;
use crate::tns_data::TnsData;
use crate::{Error, Result};

/// Field width of `global_gain` (Table 4.50).
pub const GLOBAL_GAIN_BITS: u32 = 8;

/// AOT value for AAC SSR (the only AOT that uses
/// `gain_control_data()`).
pub const AOT_AAC_SSR: u8 = 3;

/// Parsed `individual_channel_stream()` body per Table 4.50, up to
/// but not including `spectral_data()`.
///
/// The trailing `spectral_data()` block is the caller's
/// responsibility — see the module docs for the rationale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcsBody {
    /// `global_gain` — 8-bit `uimsbf` per Table 4.50. The seed for
    /// the §4.6.2.3.2 scalefactor DPCM accumulator
    /// (`last_sf = global_gain` at the top of every frame).
    pub global_gain: u8,
    /// `ics_info()` (Table 4.6). `None` only when
    /// [`IcsBody::parse_with_ics_info`] / [`IcsBody::write_with_ics_info`]
    /// were used and the caller's `IcsInfo` is held outside this
    /// struct (CPE shared-info form).
    pub ics_info: Option<IcsInfo>,
    /// `section_data()` (ISO/IEC 13818-7 §6.3 Table 17).
    pub section_data: SectionData,
    /// `scale_factor_data()` (Table 4.53, non-resilient branch).
    pub scale_factor_data: ScaleFactorData,
    /// `pulse_data_present` (1 bit). When `true`, [`Self::pulse_data`]
    /// carries the dispatched Table 4.7 record.
    pub pulse_data_present: bool,
    /// `pulse_data()` (Table 4.7). Populated when
    /// `pulse_data_present == true`.
    pub pulse_data: Option<PulseData>,
    /// `tns_data_present` (1 bit). When `true`, [`Self::tns_data`]
    /// carries the dispatched Table 4.54 record.
    pub tns_data_present: bool,
    /// `tns_data()` (Table 4.54). Populated when
    /// `tns_data_present == true`.
    pub tns_data: Option<TnsData>,
    /// `gain_control_data_present` (1 bit). When `true`,
    /// [`Self::gain_control_data`] carries the dispatched Table 4.12
    /// record.
    pub gain_control_data_present: bool,
    /// `gain_control_data()` (Table 4.12). Populated when
    /// `gain_control_data_present == true`.
    pub gain_control_data: Option<GainControlData>,
    /// Bit position of the *first* `spectral_data()` bit, measured
    /// from the start of this `individual_channel_stream()` body
    /// (i.e. the bit reader's position when [`IcsBody::parse`] was
    /// invoked is `0` here). Useful for callers that need to slice
    /// the spectrum block out of a parent buffer or hand it to a
    /// spectral-data parser without re-walking the body.
    pub spectral_data_bit_offset: u64,
    /// The error-resilient `scale_factor_data()` record (RVLC branch,
    /// Table 4.53) when the body was parsed via [`IcsBody::parse_er`] /
    /// [`IcsBody::parse_with_ics_info_er`] with
    /// `aacScalefactorDataResilienceFlag == 1`. `None` on the
    /// non-resilient path. The reconstructed absolute-delta records
    /// are mirrored into [`Self::scale_factor_data`] so the shared
    /// §4.6.2.3.2 accumulate pass consumes the body unchanged
    /// regardless of which branch produced it; this field preserves
    /// the extra RVLC backward seeds (`rev_global_gain`,
    /// `dpcm_*_last_position`).
    pub er_scale_factor_data: Option<ErScaleFactorData>,
    /// The §4.4.2.7 Table 4.50 spectral-resilience length fields,
    /// present only when the body was parsed via the ER path with
    /// `aacSpectralDataResilienceFlag == 1`:
    /// `(length_of_reordered_spectral_data, length_of_longest_codeword)`.
    /// The `reordered_spectral_data()` (HCR) payload that follows is
    /// the caller's responsibility (same contract as the non-resilient
    /// `spectral_data()` block); these two counts size that payload.
    pub reordered_spectral_lengths: Option<(u16, u8)>,
}

impl IcsBody {
    /// Parse a Table 4.50 channel-element body whose `ics_info()` is
    /// inline (the single-channel `SCE` / `LFE` form, or the
    /// non-shared `CPE` form).
    ///
    /// * `reader` — positioned at the first bit of the
    ///   `individual_channel_stream()` body (i.e. at `global_gain`).
    /// * `audio_object_type` — the surrounding ASC's effective AOT
    ///   (post SBR/PS unwrap). Drives the Table 4.6 / 4.55 predictor
    ///   branch and the SSR-only `gain_control_data` gate.
    /// * `sampling_frequency_index` — the surrounding ASC's
    ///   `samplingFrequencyIndex` (the *core* index for hierarchical
    ///   SBR / PS).
    /// * `scale_flag` — Table 4.50's outer `scale_flag` (set by
    ///   scalable AAC, AOT 6). The Phase 2 surface rejects
    ///   `scale_flag == true` with [`Error::NotImplemented`].
    ///
    /// Errors propagate from the underlying per-tool parsers:
    /// [`Error::UnexpectedEnd`] on bit-reader underflow,
    /// [`Error::IcsInfoUnsupportedSampleRateIndex`] on an out-of-range
    /// `fs_index`, [`Error::SectionDataOverrun`] on a non-conforming
    /// `section_data()`, [`Error::PulseDataEncodeInvalid`] when the
    /// stream sets `pulse_data_present == 1` on an
    /// `EIGHT_SHORT_SEQUENCE` (Table 4.50 Note 1).
    pub fn parse(
        reader: &mut BitReader<'_>,
        audio_object_type: u8,
        sampling_frequency_index: u8,
        scale_flag: bool,
    ) -> Result<Self> {
        // CPE-shared-info form is handled by parse_with_ics_info; the
        // public `parse` always reads its own ics_info, which matches
        // the SCE / LFE / non-common-window CPE case.
        Self::parse_inner(
            reader,
            audio_object_type,
            sampling_frequency_index,
            false,
            scale_flag,
        )
    }

    /// Parse a Table 4.50 channel-element body whose `ics_info()` was
    /// already consumed by the surrounding shared-info `CPE` form.
    ///
    /// The supplied `ics_info` drives the same `num_window_groups` /
    /// `max_sfb` / `window_sequence` dependencies the inline path
    /// would otherwise derive.
    ///
    /// `scale_flag` semantics mirror [`IcsBody::parse`]. The returned
    /// `IcsBody::ics_info` is `None` — the caller holds the shared
    /// `IcsInfo` outside the per-channel body.
    pub fn parse_with_ics_info(
        reader: &mut BitReader<'_>,
        ics_info: &IcsInfo,
        audio_object_type: u8,
        scale_flag: bool,
    ) -> Result<Self> {
        if scale_flag {
            return Err(Error::NotImplemented);
        }
        let start = reader.bit_position();
        let global_gain = read_u8(reader, GLOBAL_GAIN_BITS)?;
        let section_data = SectionData::parse(
            reader,
            ics_info.window_sequence,
            ics_info.num_window_groups,
            ics_info.max_sfb,
        )?;
        let scale_factor_data = ScaleFactorData::parse(reader, &section_data.sfb_cb)?;

        let tools = parse_tools(reader, ics_info, audio_object_type, start)?;

        Ok(IcsBody {
            global_gain,
            ics_info: None,
            section_data,
            scale_factor_data,
            pulse_data_present: tools.pulse_data_present,
            pulse_data: tools.pulse_data,
            tns_data_present: tools.tns_data_present,
            tns_data: tools.tns_data,
            gain_control_data_present: tools.gain_control_data_present,
            gain_control_data: tools.gain_control_data,
            spectral_data_bit_offset: tools.spectral_data_bit_offset,
            er_scale_factor_data: None,
            reordered_spectral_lengths: None,
        })
    }

    fn parse_inner(
        reader: &mut BitReader<'_>,
        audio_object_type: u8,
        sampling_frequency_index: u8,
        common_window: bool,
        scale_flag: bool,
    ) -> Result<Self> {
        if scale_flag {
            return Err(Error::NotImplemented);
        }
        let start = reader.bit_position();
        let global_gain = read_u8(reader, GLOBAL_GAIN_BITS)?;
        // Table 4.50: `if (!common_window && !scale_flag) ics_info();`
        // — `parse` is the !common_window path (the caller of
        // `parse_with_ics_info` covers the other branch).
        let ics_info = IcsInfo::parse(
            reader,
            audio_object_type,
            sampling_frequency_index,
            common_window,
        )?;
        let section_data = SectionData::parse(
            reader,
            ics_info.window_sequence,
            ics_info.num_window_groups,
            ics_info.max_sfb,
        )?;
        let scale_factor_data = ScaleFactorData::parse(reader, &section_data.sfb_cb)?;

        let tools = parse_tools(reader, &ics_info, audio_object_type, start)?;

        Ok(IcsBody {
            global_gain,
            ics_info: Some(ics_info),
            section_data,
            scale_factor_data,
            pulse_data_present: tools.pulse_data_present,
            pulse_data: tools.pulse_data,
            tns_data_present: tools.tns_data_present,
            tns_data: tools.tns_data,
            gain_control_data_present: tools.gain_control_data_present,
            gain_control_data: tools.gain_control_data,
            spectral_data_bit_offset: tools.spectral_data_bit_offset,
            er_scale_factor_data: None,
            reordered_spectral_lengths: None,
        })
    }

    /// Parse an **error-resilient** Table 4.50 channel-element body
    /// (the ER General Audio object types — AOTs 17 / 19 / 20 / 23 —
    /// whose ASC carries the [`AacResilienceFlags`] triplet).
    ///
    /// Differs from [`IcsBody::parse`] in three spec-driven ways
    /// (Table 4.50 / Table 4.52 / Table 4.53):
    ///
    /// * `section_data()` takes the [`SectionData::parse_er`] branch
    ///   when `resilience.section_data` is set (5-bit `sect_cb`).
    /// * `scale_factor_data()` takes the RVLC
    ///   [`ErScaleFactorData::parse`] branch when
    ///   `resilience.scalefactor_data` is set; the reconstructed
    ///   absolute-delta records are mirrored into
    ///   [`Self::scale_factor_data`] and the RVLC seeds are retained
    ///   in [`Self::er_scale_factor_data`].
    /// * the trailing `spectral_data()` is replaced — when
    ///   `resilience.spectral_data` is set — by the
    ///   `length_of_reordered_spectral_data` (14-bit) +
    ///   `length_of_longest_codeword` (6-bit) pair captured in
    ///   [`Self::reordered_spectral_lengths`]; the
    ///   `reordered_spectral_data()` (HCR) payload that follows is the
    ///   caller's responsibility, exactly as `spectral_data()` is on
    ///   the non-resilient path.
    pub fn parse_er(
        reader: &mut BitReader<'_>,
        audio_object_type: u8,
        sampling_frequency_index: u8,
        scale_flag: bool,
        resilience: AacResilienceFlags,
    ) -> Result<Self> {
        if scale_flag {
            return Err(Error::NotImplemented);
        }
        let start = reader.bit_position();
        let global_gain = read_u8(reader, GLOBAL_GAIN_BITS)?;
        let ics_info = IcsInfo::parse(reader, audio_object_type, sampling_frequency_index, false)?;
        let mut body = Self::finish_er_shared(
            reader,
            global_gain,
            &ics_info,
            audio_object_type,
            resilience,
            start,
        )?;
        body.ics_info = Some(ics_info);
        Ok(body)
    }

    /// Parse an error-resilient Table 4.50 body whose `ics_info()` was
    /// already consumed by the surrounding shared-info CPE form.
    ///
    /// ER analogue of [`IcsBody::parse_with_ics_info`]; the resilience
    /// branch semantics match [`IcsBody::parse_er`]. The returned
    /// `ics_info` is `None` (the caller holds the shared `IcsInfo`).
    pub fn parse_with_ics_info_er(
        reader: &mut BitReader<'_>,
        ics_info: &IcsInfo,
        audio_object_type: u8,
        scale_flag: bool,
        resilience: AacResilienceFlags,
    ) -> Result<Self> {
        if scale_flag {
            return Err(Error::NotImplemented);
        }
        let start = reader.bit_position();
        let global_gain = read_u8(reader, GLOBAL_GAIN_BITS)?;
        Self::finish_er_shared(
            reader,
            global_gain,
            ics_info,
            audio_object_type,
            resilience,
            start,
        )
    }

    /// Shared tail of the ER parse: `section_data()` (ER branch) →
    /// `scale_factor_data()` (RVLC branch) → tool dispatch → spectral
    /// resilience length fields. `ics_info` carries the geometry; the
    /// returned `IcsBody::ics_info` is `None` (the inline caller sets
    /// it afterwards from its owned value).
    fn finish_er_shared(
        reader: &mut BitReader<'_>,
        global_gain: u8,
        ics_info: &IcsInfo,
        audio_object_type: u8,
        resilience: AacResilienceFlags,
        start: u64,
    ) -> Result<Self> {
        let section_data = if resilience.section_data {
            SectionData::parse_er(
                reader,
                ics_info.window_sequence,
                ics_info.num_window_groups,
                ics_info.max_sfb,
            )?
        } else {
            SectionData::parse(
                reader,
                ics_info.window_sequence,
                ics_info.num_window_groups,
                ics_info.max_sfb,
            )?
        };

        let (scale_factor_data, er_scale_factor_data) = if resilience.scalefactor_data {
            let er =
                ErScaleFactorData::parse(reader, &section_data.sfb_cb, ics_info.window_sequence)?;
            (er.data.clone(), Some(er))
        } else {
            (ScaleFactorData::parse(reader, &section_data.sfb_cb)?, None)
        };

        let tools = parse_tools(reader, ics_info, audio_object_type, start)?;

        // Table 4.50 ER spectral branch: when
        // aacSpectralDataResilienceFlag is set, the body carries the
        // two HCR length fields in place of starting spectral_data().
        let reordered_spectral_lengths = if resilience.spectral_data {
            let len_reordered = reader.read_u32(14).map_err(|_| Error::UnexpectedEnd)? as u16;
            let len_longest = reader.read_u32(6).map_err(|_| Error::UnexpectedEnd)? as u8;
            Some((len_reordered, len_longest))
        } else {
            None
        };

        let spectral_data_bit_offset = reader.bit_position() - start;

        Ok(IcsBody {
            global_gain,
            ics_info: None,
            section_data,
            scale_factor_data,
            pulse_data_present: tools.pulse_data_present,
            pulse_data: tools.pulse_data,
            tns_data_present: tools.tns_data_present,
            tns_data: tools.tns_data,
            gain_control_data_present: tools.gain_control_data_present,
            gain_control_data: tools.gain_control_data,
            spectral_data_bit_offset,
            er_scale_factor_data,
            reordered_spectral_lengths,
        })
    }

    /// Write a Table 4.50 body whose `ics_info()` is inline.
    ///
    /// Mirrors [`IcsBody::parse`] — emits `global_gain`, `ics_info()`,
    /// `section_data()`, `scale_factor_data()`, then the three
    /// dispatching bits and their optional bodies. The trailing
    /// `spectral_data()` is the caller's responsibility.
    ///
    /// Returns [`Error::IcsInfoEncodeInvalid`] if [`Self::ics_info`]
    /// is `None` (use [`IcsBody::write_with_ics_info`] for the
    /// CPE-shared-info case); other errors propagate from the
    /// per-tool writers (e.g. [`Error::PulseDataEncodeInvalid`] when
    /// `pulse_data_present == true` on `EIGHT_SHORT_SEQUENCE`).
    pub fn write(
        &self,
        writer: &mut BitWriter,
        audio_object_type: u8,
        sampling_frequency_index: u8,
        scale_flag: bool,
    ) -> Result<()> {
        if scale_flag {
            return Err(Error::NotImplemented);
        }
        let ics_info = self.ics_info.as_ref().ok_or(Error::IcsInfoEncodeInvalid)?;
        writer.write_u32(u32::from(self.global_gain), GLOBAL_GAIN_BITS);
        ics_info.write(writer, audio_object_type, sampling_frequency_index, false)?;
        self.section_data
            .write(writer, ics_info.window_sequence, ics_info.max_sfb)?;
        self.scale_factor_data
            .write(writer, &self.section_data.sfb_cb)?;
        self.write_tools(writer, ics_info, audio_object_type)
    }

    /// Write a Table 4.50 body whose `ics_info()` was emitted
    /// separately by the surrounding shared-info `CPE` form.
    ///
    /// The supplied `ics_info` drives the same per-tool field
    /// dispatch the inline path would. The in-memory
    /// [`Self::ics_info`] field is ignored (and is expected to be
    /// `None` for round-trip consistency).
    pub fn write_with_ics_info(
        &self,
        writer: &mut BitWriter,
        ics_info: &IcsInfo,
        audio_object_type: u8,
        scale_flag: bool,
    ) -> Result<()> {
        if scale_flag {
            return Err(Error::NotImplemented);
        }
        writer.write_u32(u32::from(self.global_gain), GLOBAL_GAIN_BITS);
        self.section_data
            .write(writer, ics_info.window_sequence, ics_info.max_sfb)?;
        self.scale_factor_data
            .write(writer, &self.section_data.sfb_cb)?;
        self.write_tools(writer, ics_info, audio_object_type)
    }

    fn write_tools(
        &self,
        writer: &mut BitWriter,
        ics_info: &IcsInfo,
        audio_object_type: u8,
    ) -> Result<()> {
        // pulse_data_present + body.
        writer.write_bit(self.pulse_data_present);
        if self.pulse_data_present {
            // Table 4.50 Note 1: pulse_data is illegal on
            // EIGHT_SHORT_SEQUENCE (the pulse-escape fix-up needs the
            // long-window swb_offset_long table).
            if ics_info.window_sequence == WindowSequence::EightShort {
                return Err(Error::PulseDataEncodeInvalid);
            }
            let pd = self
                .pulse_data
                .as_ref()
                .ok_or(Error::PulseDataEncodeInvalid)?;
            pd.write(writer)?;
        } else if self.pulse_data.is_some() {
            // Slot populated while the dispatching bit is clear.
            return Err(Error::PulseDataEncodeInvalid);
        }

        // tns_data_present + body.
        writer.write_bit(self.tns_data_present);
        if self.tns_data_present {
            let td = self.tns_data.as_ref().ok_or(Error::TnsDataEncodeInvalid)?;
            td.write(writer, ics_info.window_sequence)?;
        } else if self.tns_data.is_some() {
            return Err(Error::TnsDataEncodeInvalid);
        }

        // gain_control_data_present + body. The §4.6.12 normative
        // constraint: AOT 3 (SSR) only.
        writer.write_bit(self.gain_control_data_present);
        if self.gain_control_data_present {
            if audio_object_type != AOT_AAC_SSR {
                return Err(Error::GainControlDataEncodeInvalid);
            }
            let gc = self
                .gain_control_data
                .as_ref()
                .ok_or(Error::GainControlDataEncodeInvalid)?;
            gc.write(writer, ics_info.window_sequence)?;
        } else if self.gain_control_data.is_some() {
            return Err(Error::GainControlDataEncodeInvalid);
        }
        Ok(())
    }
}

/// Helper: read an 8-bit `uimsbf` field.
fn read_u8(reader: &mut BitReader<'_>, bits: u32) -> Result<u8> {
    Ok(reader.read_u32(bits).map_err(|_| Error::UnexpectedEnd)? as u8)
}

/// Internal carrier for the pulse / tns / gain_control walk result.
struct ToolDispatch {
    pulse_data_present: bool,
    pulse_data: Option<PulseData>,
    tns_data_present: bool,
    tns_data: Option<TnsData>,
    gain_control_data_present: bool,
    gain_control_data: Option<GainControlData>,
    spectral_data_bit_offset: u64,
}

/// Helper: walk the pulse / tns / gain_control dispatch trio after
/// `scale_factor_data()` and return the resulting slots plus the
/// `spectral_data_bit_offset` (measured from `start`).
fn parse_tools(
    reader: &mut BitReader<'_>,
    ics_info: &IcsInfo,
    _audio_object_type: u8,
    start: u64,
) -> Result<ToolDispatch> {
    let pulse_data_present = reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
    let pulse_data = if pulse_data_present {
        // Table 4.50 Note 1: pulse_data is illegal on
        // EIGHT_SHORT_SEQUENCE. A conforming stream never sets the
        // flag in that case; surface the violation so callers can
        // reject the stream rather than crash downstream.
        if ics_info.window_sequence == WindowSequence::EightShort {
            return Err(Error::PulseDataEncodeInvalid);
        }
        Some(PulseData::parse(reader)?)
    } else {
        None
    };

    let tns_data_present = reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
    let tns_data = if tns_data_present {
        Some(TnsData::parse(reader, ics_info.window_sequence)?)
    } else {
        None
    };

    let gain_control_data_present = reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
    let gain_control_data = if gain_control_data_present {
        // Per §4.6.12 the gain_control_data tool is AOT-3 (SSR) only;
        // a conforming stream never sets the flag on any other AOT.
        // The parser surfaces the literal bits regardless of AOT —
        // the AOT-validity check is enforced on the writer side so
        // we can ingest hostile streams without panicking, and the
        // emitter side keeps us from emitting non-conforming streams.
        Some(GainControlData::parse(reader, ics_info.window_sequence)?)
    } else {
        None
    };

    let spectral_data_bit_offset = reader.bit_position() - start;
    Ok(ToolDispatch {
        pulse_data_present,
        pulse_data,
        tns_data_present,
        tns_data,
        gain_control_data_present,
        gain_control_data,
        spectral_data_bit_offset,
    })
}
