//! `ics_info()` parser — ISO/IEC 14496-3 §4.4.6 Table 4.6.
//!
//! `ics_info()` carries the per-channel window-shape / window-sequence
//! decision plus the `max_sfb` (number of scalefactor bands actually
//! coded), scale-factor grouping mask for `EIGHT_SHORT_SEQUENCE`, and
//! either the MPEG-2 frequency-domain predictor side-info (AOT 1
//! Main) or the LTP `ltp_data_present` flag(s) (every other GA AOT
//! that's not 3 = SSR — SSR uses `gain_control_data()` instead of
//! prediction).
//!
//! This parser is the **start** of Phase 2 (channel-element body
//! parsing). It does not consume `global_gain`, `section_data()`,
//! `scale_factor_data()`, `pulse_data()`, `tns_data()`,
//! `gain_control_data()`, or `spectral_data()` — those land in
//! later Phase 2 rounds. `ltp_data()` (Table 4.55) **is** parsed
//! when `ltp_data_present == 1`, because it is dispatched from
//! inside the Table 4.6 syntax itself; deferring it would leave
//! `IcsInfo` in an indeterminate bit-position.
//!
//! ## Derived values
//!
//! Beyond the literal wire fields the parser surfaces the
//! §4.5.2.3.4 / §4.5.2.6.2.4 derivations:
//!
//! * `num_windows` — `8` for `EIGHT_SHORT_SEQUENCE`, `1` otherwise.
//! * `num_window_groups` — `1` for long sequences; for
//!   `EIGHT_SHORT_SEQUENCE` it is the number of groups implied by
//!   the 7-bit `scale_factor_grouping` mask. The first short
//!   window always starts a new group; for windows 1..=7 a `1` bit
//!   at position `6 − i` (so bit 6 controls grouping of window 1,
//!   …, bit 0 controls window 7) merges window `i+1` into the
//!   current group, a `0` opens a new group. This matches the
//!   spec's `bit_set(scale_factor_grouping, 6 − i)` pseudo-code.
//! * `window_group_length[g]` — number of short windows in group
//!   `g`. Sum is always 8.
//! * `num_swb` — `num_swb_long_window[fs_index]` for long, or
//!   `num_swb_short_window[fs_index]` for `EIGHT_SHORT_SEQUENCE`.
//!   Sample-rate count tables ([`NUM_SWB_LONG_WINDOW`],
//!   [`NUM_SWB_SHORT_WINDOW`]) cover the 12 valid ADTS
//!   `sampling_frequency_index` values 0..=11.
//!
//! ## What is *not* in this round
//!
//! * `swb_offset_long_window[]` / `swb_offset_short_window[]`
//!   tables — only the *count* of scalefactor bands is needed to
//!   step through `ics_info()`. Spectral decoding (Phase 2 mid)
//!   will pull in the offset tables.
//! * `sect_sfb_offset[g][section]` — derived from the offset
//!   tables, not from `ics_info` proper; landed alongside
//!   `section_data()` in a later round.
//! * The `aac_section_data_resilience_flag` /
//!   `aac_scalefactor_data_resilience_flag` /
//!   `aac_spectral_data_resilience_flag` extension chain (ER AOTs).
//!   Surfaced by `GASpecificConfig` `extensionFlag == 1` parsing
//!   that itself is a Phase 1 follow-up.
//!
//! ## Predictor / LTP dispatch (Table 4.6)
//!
//! When `window_sequence != EIGHT_SHORT_SEQUENCE`, an extra
//! `predictor_data_present` bit follows `max_sfb`. The branch
//! taken when that bit is 1 depends on `audioObjectType`:
//!
//! * `audioObjectType == 1` (Main) — read `predictor_reset` (1 bit);
//!   if set, read `predictor_reset_group_number` (5 bits); then read
//!   `prediction_used[sfb]` for `sfb in 0..min(max_sfb, PRED_SFB_MAX)`.
//!   `PRED_SFB_MAX` is sample-rate dependent (see
//!   [`PRED_SFB_MAX`]).
//! * Any other AOT (LC, SSR, LTP, scalable, TwinVQ, ER variants) —
//!   read `ltp_data_present` (1 bit); if set, parse `ltp_data()`
//!   per Table 4.55. If the surrounding element is a CPE with
//!   `common_window == 1`, a *second* `ltp_data_present` (+
//!   optional `ltp_data()`) follows for the paired channel.
//!
//! The spec attaches a normative caveat: for plain LC streams the
//! `predictor_data_present` bit is required to be 0 by ISO/IEC
//! 14496-3 §1.5.1.1 (AOT 2 does not own a predictor). The parser
//! enforces nothing here — it surfaces whatever the wire said and
//! lets a higher-layer validator decide.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::{Error, Result};

/// Sentinel for the `EIGHT_SHORT_SEQUENCE` window-sequence value.
/// Exposed as a `pub const` so consumers can compare without
/// matching against [`WindowSequence`].
pub const EIGHT_SHORT_SEQUENCE: u8 = 2;

/// `window_sequence` enumeration — ISO/IEC 14496-3 §4.5.2.3.1.1 /
/// Table 4.128.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WindowSequence {
    /// `0` — one 1024-sample (or 960-sample if `frameLengthFlag`)
    /// MDCT covering the full frame.
    OnlyLong = 0,
    /// `1` — long MDCT with a start window on the right half.
    /// Always preceded by `OnlyLong` and followed by
    /// `EightShort` in a transient-onset transition.
    LongStart = 1,
    /// `2` — eight 128-sample MDCTs; `scale_factor_grouping` and
    /// `num_window_groups` are meaningful here.
    EightShort = 2,
    /// `3` — long MDCT with a stop window on the left half. Tail
    /// of an `EightShort` burst.
    LongStop = 3,
}

impl WindowSequence {
    /// Map a 2-bit wire value (0..=3) to the corresponding variant.
    pub fn from_bits(bits: u8) -> Self {
        match bits & 0b11 {
            0 => WindowSequence::OnlyLong,
            1 => WindowSequence::LongStart,
            2 => WindowSequence::EightShort,
            _ => WindowSequence::LongStop,
        }
    }

    /// `true` ⇔ `EIGHT_SHORT_SEQUENCE`.
    pub fn is_eight_short(self) -> bool {
        matches!(self, WindowSequence::EightShort)
    }
}

/// `window_shape` enumeration — ISO/IEC 14496-3 §4.5.2.3.1.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WindowShape {
    /// `0` — sine window. Default for AAC-LC.
    Sine = 0,
    /// `1` — Kaiser-Bessel-derived (KBD) window.
    Kbd = 1,
}

impl WindowShape {
    /// Map a 1-bit wire value (0..=1) to the variant.
    pub fn from_bit(bit: bool) -> Self {
        if bit {
            WindowShape::Kbd
        } else {
            WindowShape::Sine
        }
    }
}

/// `predictor_data()` body (Table 4.6, Main branch). Only the Main
/// AOT (`audioObjectType == 1`) ever instantiates this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictorData {
    /// `predictor_reset` bit.
    pub reset: bool,
    /// `predictor_reset_group_number` (5 bits) — only present when
    /// `reset == true`. Identifies which group of predictors to
    /// re-initialise this frame.
    pub reset_group_number: Option<u8>,
    /// `prediction_used[sfb]` for `sfb in 0..min(max_sfb,
    /// PRED_SFB_MAX[fs_index])`. Each entry is a single bit.
    pub prediction_used: Vec<bool>,
}

/// `ltp_data()` body (Table 4.55).
///
/// Two variants are distinguished by `audioObjectType == 23`
/// (`ER_AAC_LD`), which carries a delta-coded `ltp_lag_update` /
/// `ltp_lag` pair instead of an unconditional 11-bit `ltp_lag`.
/// For `EIGHT_SHORT_SEQUENCE` in the non-LD branch,
/// `ltp_long_used[]` is **absent** per the spec — the parser
/// emits an empty `long_used` vec in that case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LtpData {
    /// `ltp_lag_update` bit. Only present for `audioObjectType ==
    /// 23` (LD); `None` for every other AOT.
    pub lag_update: Option<bool>,
    /// `ltp_lag`. For LD this is 10 bits and may be absent when
    /// `lag_update == false`; for non-LD this is 11 bits and is
    /// always present.
    pub lag: Option<u16>,
    /// `ltp_coef` (3 bits) — index into the 8-entry LTP
    /// coefficient codebook.
    pub coef: u8,
    /// `ltp_long_used[sfb]` for `sfb in 0..min(max_sfb,
    /// MAX_LTP_LONG_SFB)`. Empty when the non-LD AOT is using
    /// `EIGHT_SHORT_SEQUENCE` (spec omits the loop in that case).
    pub long_used: Vec<bool>,
}

/// Per-Table 4.55 maximum number of scalefactor bands carrying
/// `ltp_long_used[]`. ISO/IEC 14496-3 §4.6.7.2.
pub const MAX_LTP_LONG_SFB: usize = 40;

/// Per-Table 4.6 / Table 62 (ISO/IEC 13818-7 §13.3.1)
/// sample-rate-dependent `PRED_SFB_MAX` constant. Indexed by
/// ADTS `sampling_frequency_index` 0..=11.
///
/// |  idx  |   rate (Hz)  | PRED_SFB_MAX |
/// |-------|--------------|--------------|
/// |  0    | 96 000       | 33           |
/// |  1    | 88 200       | 33           |
/// |  2    | 64 000       | 38           |
/// |  3    | 48 000       | 40           |
/// |  4    | 44 100       | 40           |
/// |  5    | 32 000       | 40           |
/// |  6    | 24 000       | 41           |
/// |  7    | 22 050       | 41           |
/// |  8    | 16 000       | 37           |
/// |  9    | 12 000       | 37           |
/// | 10    | 11 025       | 37           |
/// | 11    |  8 000       | 34           |
pub const PRED_SFB_MAX: [u8; 12] = [33, 33, 38, 40, 40, 40, 41, 41, 37, 37, 37, 34];

/// `num_swb_long_window[fs_index]` for the canonical 1024-line
/// long window — ISO/IEC 14496-3 Tables 4.129 / 4.131 / 4.132 /
/// 4.134 / 4.136 / 4.138 / 4.140, distilled to the count column.
///
/// |  idx  |   rate (Hz)  | num_swb |  source        |
/// |-------|--------------|---------|----------------|
/// |  0    | 96 000       | 41      | Table 4.140    |
/// |  1    | 88 200       | 41      | Table 4.140    |
/// |  2    | 64 000       | 47      | Table 4.138    |
/// |  3    | 48 000       | 49      | Table 4.129    |
/// |  4    | 44 100       | 49      | Table 4.129    |
/// |  5    | 32 000       | 51      | Table 4.131    |
/// |  6    | 24 000       | 47      | Table 4.136    |
/// |  7    | 22 050       | 47      | Table 4.136    |
/// |  8    | 16 000       | 43      | Table 4.134    |
/// |  9    | 12 000       | 43      | Table 4.134    |
/// | 10    | 11 025       | 43      | Table 4.134    |
/// | 11    |  8 000       | 40      | Table 4.132    |
pub const NUM_SWB_LONG_WINDOW: [u8; 12] = [41, 41, 47, 49, 49, 51, 47, 47, 43, 43, 43, 40];

/// `num_swb_short_window[fs_index]` for the canonical 128-line
/// short window — ISO/IEC 14496-3 Tables 4.130 / 4.133 / 4.135 /
/// 4.137 / 4.139 / 4.141.
///
/// |  idx  |   rate (Hz)  | num_swb |  source        |
/// |-------|--------------|---------|----------------|
/// |  0    | 96 000       | 12      | Table 4.141    |
/// |  1    | 88 200       | 12      | Table 4.141    |
/// |  2    | 64 000       | 12      | Table 4.139    |
/// |  3    | 48 000       | 14      | Table 4.130    |
/// |  4    | 44 100       | 14      | Table 4.130    |
/// |  5    | 32 000       | 14      | Table 4.130    |
/// |  6    | 24 000       | 15      | Table 4.137    |
/// |  7    | 22 050       | 15      | Table 4.137    |
/// |  8    | 16 000       | 15      | Table 4.135    |
/// |  9    | 12 000       | 15      | Table 4.135    |
/// | 10    | 11 025       | 15      | Table 4.135    |
/// | 11    |  8 000       | 15      | Table 4.133    |
pub const NUM_SWB_SHORT_WINDOW: [u8; 12] = [12, 12, 12, 14, 14, 14, 15, 15, 15, 15, 15, 15];

/// Parsed `ics_info()` (Table 4.6) plus the §4.5.2.3.4 derivations
/// that depend on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IcsInfo {
    /// `ics_reserved_bit` — spec mandates `0`; the parser surfaces
    /// the wire value without enforcement (some encoders set it
    /// even though they shouldn't).
    pub ics_reserved_bit: bool,
    /// `window_sequence` (2 bits, Table 4.128).
    pub window_sequence: WindowSequence,
    /// `window_shape` (1 bit, Table 4.129 reference).
    pub window_shape: WindowShape,
    /// `max_sfb` — 4 bits in the `EIGHT_SHORT_SEQUENCE` branch,
    /// 6 bits in every other branch.
    pub max_sfb: u8,
    /// `scale_factor_grouping` (7 bits) — only present when
    /// `window_sequence == EIGHT_SHORT_SEQUENCE`. Bit `6 − i`
    /// controls whether window `i + 1` joins the current group
    /// (`1`) or opens a new group (`0`) for `i in 0..7`.
    pub scale_factor_grouping: Option<u8>,
    /// `predictor_data_present` (1 bit) — only present when
    /// `window_sequence != EIGHT_SHORT_SEQUENCE`.
    pub predictor_data_present: bool,
    /// Main-AOT `predictor_data()` body (Table 4.6 Main branch).
    /// Populated when `predictor_data_present == true` and
    /// `audioObjectType == 1`.
    pub predictor_data: Option<PredictorData>,
    /// First `ltp_data_present` bit — read when
    /// `predictor_data_present == true` and `audioObjectType !=
    /// 1`. `false` if not read.
    pub ltp_data_present: bool,
    /// Channel's own `ltp_data()` body — populated when
    /// `ltp_data_present == true`.
    pub ltp_data: Option<LtpData>,
    /// `common_window`-paired channel `ltp_data_present` bit —
    /// only read when the caller passed `common_window == true`
    /// AND `predictor_data_present == true` AND `audioObjectType
    /// != 1`. `None` if not present.
    pub ltp_data_present_pair: Option<bool>,
    /// `ltp_data()` body for the paired channel — populated when
    /// `ltp_data_present_pair == Some(true)`.
    pub ltp_data_pair: Option<LtpData>,

    // Derived fields (§4.5.2.3.4) — populated unconditionally.
    /// Number of MDCT windows in this frame (`8` for short, `1`
    /// otherwise).
    pub num_windows: u8,
    /// Number of window-groups after scale-factor grouping. Always
    /// `1` for long sequences; for `EIGHT_SHORT_SEQUENCE` it is in
    /// `1..=8` per the [`Self::scale_factor_grouping`] mask.
    pub num_window_groups: u8,
    /// Number of windows in each group; `window_group_length[g]`
    /// for `g in 0..num_window_groups`. Sum is always
    /// `num_windows`.
    pub window_group_length: Vec<u8>,
    /// Total scalefactor window bands for this frame —
    /// `NUM_SWB_LONG_WINDOW[fs_index]` for long sequences,
    /// `NUM_SWB_SHORT_WINDOW[fs_index]` for short sequences.
    pub num_swb: u8,
}

impl IcsInfo {
    /// Parse a single `ics_info()` from the bit-reader.
    ///
    /// * `audio_object_type` — the surrounding ASC's effective
    ///   `audioObjectType` (post SBR/PS unwrap). Used to pick
    ///   between the Main / LTP predictor branches.
    /// * `sampling_frequency_index` — the surrounding ASC's
    ///   `samplingFrequencyIndex` (the *core* index for hierarchical
    ///   SBR/PS — ics_info follows the inner AAC framerate, not the
    ///   SBR output rate). Must be in `0..=11` (the 24-bit
    ///   explicit-rate escape from §1.6.2.1 is not supported here
    ///   because the SWB tables are indexed by the standard 12
    ///   rates).
    /// * `common_window` — `true` ⇔ the surrounding element is a
    ///   `channel_pair_element()` with the shared-info form
    ///   (`common_window == 1` per Table 4.5); controls whether
    ///   the second `ltp_data_present` (+ optional second
    ///   `ltp_data()`) is consumed.
    pub fn parse(
        reader: &mut BitReader<'_>,
        audio_object_type: u8,
        sampling_frequency_index: u8,
        common_window: bool,
    ) -> Result<Self> {
        let fs_index = sampling_frequency_index as usize;
        if fs_index >= NUM_SWB_LONG_WINDOW.len() {
            return Err(Error::IcsInfoUnsupportedSampleRateIndex(
                sampling_frequency_index,
            ));
        }

        let ics_reserved_bit = read_bit(reader)?;
        let window_sequence_bits = read_u8(reader, 2)?;
        let window_sequence = WindowSequence::from_bits(window_sequence_bits);
        let window_shape = WindowShape::from_bit(read_bit(reader)?);

        let mut scale_factor_grouping = None;
        let mut predictor_data_present = false;
        let mut predictor_data = None;
        let mut ltp_data_present = false;
        let mut ltp_data = None;
        let mut ltp_data_present_pair = None;
        let mut ltp_data_pair = None;

        let max_sfb;
        if window_sequence.is_eight_short() {
            max_sfb = read_u8(reader, 4)?;
            scale_factor_grouping = Some(read_u8(reader, 7)?);
        } else {
            max_sfb = read_u8(reader, 6)?;
            predictor_data_present = read_bit(reader)?;
            if predictor_data_present {
                if audio_object_type == 1 {
                    // Main predictor side info.
                    let reset = read_bit(reader)?;
                    let reset_group_number = if reset {
                        Some(read_u8(reader, 5)?)
                    } else {
                        None
                    };
                    let pred_sfb_max = PRED_SFB_MAX[fs_index] as u16;
                    let n = core::cmp::min(max_sfb as u16, pred_sfb_max) as usize;
                    let mut prediction_used = Vec::with_capacity(n);
                    for _ in 0..n {
                        prediction_used.push(read_bit(reader)?);
                    }
                    predictor_data = Some(PredictorData {
                        reset,
                        reset_group_number,
                        prediction_used,
                    });
                } else {
                    // LTP / other GA AOTs.
                    ltp_data_present = true;
                    ltp_data = Some(parse_ltp_data(
                        reader,
                        audio_object_type,
                        window_sequence,
                        max_sfb,
                    )?);
                    if common_window {
                        let pair_flag = read_bit(reader)?;
                        ltp_data_present_pair = Some(pair_flag);
                        if pair_flag {
                            ltp_data_pair = Some(parse_ltp_data(
                                reader,
                                audio_object_type,
                                window_sequence,
                                max_sfb,
                            )?);
                        }
                    }
                }
            } else if common_window && audio_object_type != 1 {
                // Spec note: when predictor_data_present == 0, the
                // second ltp_data_present bit is also not
                // transmitted (Table 4.6 only enters the LTP
                // branch when predictor_data_present == 1). The
                // pair-channel flag therefore stays absent.
            }
        }

        // §4.5.2.3.4 derivations.
        let (num_windows, num_window_groups, window_group_length, num_swb) =
            derive_window_grouping(window_sequence, scale_factor_grouping, fs_index);

        Ok(IcsInfo {
            ics_reserved_bit,
            window_sequence,
            window_shape,
            max_sfb,
            scale_factor_grouping,
            predictor_data_present,
            predictor_data,
            ltp_data_present,
            ltp_data,
            ltp_data_present_pair,
            ltp_data_pair,
            num_windows,
            num_window_groups,
            window_group_length,
            num_swb,
        })
    }

    /// Encode `ics_info()` onto `writer`, the inverse of
    /// [`IcsInfo::parse`].
    ///
    /// The writer mirrors Table 4.6 verbatim — `ics_reserved_bit`
    /// (1 bit), `window_sequence` (2 bits), `window_shape` (1 bit),
    /// then either `max_sfb` (4 bits) + `scale_factor_grouping`
    /// (7 bits) for `EIGHT_SHORT_SEQUENCE`, or `max_sfb` (6 bits) +
    /// `predictor_data_present` (1 bit) plus the per-AOT
    /// predictor / LTP body for every other window sequence.
    ///
    /// The `audio_object_type` / `sampling_frequency_index` /
    /// `common_window` parameters must match the values the parser
    /// was (or would be) invoked with. They drive the branch the
    /// encoder takes for the Main vs LTP predictor body and the
    /// `prediction_used[]` cap (`PRED_SFB_MAX[fs_index]` for AOT 1).
    ///
    /// Returns [`Error::IcsInfoEncodeInvalid`] if the in-memory
    /// [`IcsInfo`] violates a wire-field invariant:
    ///
    /// * `max_sfb` exceeds its field width
    ///   (`> 15` for `EIGHT_SHORT_SEQUENCE`, `> 63` otherwise).
    /// * `scale_factor_grouping` is `None` for `EIGHT_SHORT_SEQUENCE`,
    ///   `Some(_)` otherwise, or its value exceeds 7 bits.
    /// * `predictor_data_present == true` for `EIGHT_SHORT_SEQUENCE`
    ///   (Table 4.6 omits the bit on the short branch).
    /// * `predictor_data` is `Some` while `audio_object_type != 1`,
    ///   or `None` while the predictor bit is set with AOT 1.
    /// * Predictor `reset_group_number` doesn't match
    ///   `reset.is_some()` parity, or exceeds 5 bits.
    /// * Predictor `prediction_used.len()` differs from `min(max_sfb,
    ///   PRED_SFB_MAX[fs_index])`.
    /// * LTP body fields (lag width, `coef`, `long_used[]` length) do
    ///   not satisfy Table 4.55 (delegated to [`write_ltp_data`]).
    /// * The paired-channel LTP slot is populated while
    ///   `common_window == false`, or while `predictor_data_present
    ///   == false`, or while `audio_object_type == 1`.
    /// * `sampling_frequency_index` is outside `0..=11`.
    pub fn write(
        &self,
        writer: &mut BitWriter,
        audio_object_type: u8,
        sampling_frequency_index: u8,
        common_window: bool,
    ) -> Result<()> {
        let fs_index = sampling_frequency_index as usize;
        if fs_index >= NUM_SWB_LONG_WINDOW.len() {
            return Err(Error::IcsInfoEncodeInvalid);
        }

        writer.write_bit(self.ics_reserved_bit);
        writer.write_u32(self.window_sequence as u32 & 0b11, 2);
        writer.write_u32(self.window_shape as u32 & 0b1, 1);

        if self.window_sequence.is_eight_short() {
            if self.max_sfb > 0x0f {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            let mask = self
                .scale_factor_grouping
                .ok_or(Error::IcsInfoEncodeInvalid)?;
            if mask > 0x7f {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            // EIGHT_SHORT branch has neither predictor_data_present
            // nor any LTP body — reject populated slots before they
            // silently round-trip into a non-conforming stream.
            if self.predictor_data_present
                || self.predictor_data.is_some()
                || self.ltp_data_present
                || self.ltp_data.is_some()
                || self.ltp_data_present_pair.is_some()
                || self.ltp_data_pair.is_some()
            {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            writer.write_u32(self.max_sfb as u32, 4);
            writer.write_u32(mask as u32, 7);
        } else {
            if self.max_sfb > 0x3f {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            if self.scale_factor_grouping.is_some() {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            writer.write_u32(self.max_sfb as u32, 6);
            writer.write_bit(self.predictor_data_present);

            if self.predictor_data_present {
                if audio_object_type == 1 {
                    // Main predictor side info.
                    if self.ltp_data_present
                        || self.ltp_data.is_some()
                        || self.ltp_data_present_pair.is_some()
                        || self.ltp_data_pair.is_some()
                    {
                        return Err(Error::IcsInfoEncodeInvalid);
                    }
                    let pd = self
                        .predictor_data
                        .as_ref()
                        .ok_or(Error::IcsInfoEncodeInvalid)?;
                    // reset_group_number parity matches reset bit.
                    if pd.reset != pd.reset_group_number.is_some() {
                        return Err(Error::IcsInfoEncodeInvalid);
                    }
                    let pred_sfb_max = PRED_SFB_MAX[fs_index] as u16;
                    let expected = core::cmp::min(self.max_sfb as u16, pred_sfb_max) as usize;
                    if pd.prediction_used.len() != expected {
                        return Err(Error::IcsInfoEncodeInvalid);
                    }
                    writer.write_bit(pd.reset);
                    if let Some(g) = pd.reset_group_number {
                        if g > 0x1f {
                            return Err(Error::IcsInfoEncodeInvalid);
                        }
                        writer.write_u32(g as u32, 5);
                    }
                    for &b in &pd.prediction_used {
                        writer.write_bit(b);
                    }
                } else {
                    // LTP / non-Main branch.
                    if self.predictor_data.is_some() || !self.ltp_data_present {
                        return Err(Error::IcsInfoEncodeInvalid);
                    }
                    let ltp = self.ltp_data.as_ref().ok_or(Error::IcsInfoEncodeInvalid)?;
                    write_ltp_data(
                        writer,
                        ltp,
                        audio_object_type,
                        self.window_sequence,
                        self.max_sfb,
                    )?;
                    if common_window {
                        let pair_flag = self
                            .ltp_data_present_pair
                            .ok_or(Error::IcsInfoEncodeInvalid)?;
                        writer.write_bit(pair_flag);
                        if pair_flag {
                            let ltp2 = self
                                .ltp_data_pair
                                .as_ref()
                                .ok_or(Error::IcsInfoEncodeInvalid)?;
                            write_ltp_data(
                                writer,
                                ltp2,
                                audio_object_type,
                                self.window_sequence,
                                self.max_sfb,
                            )?;
                        } else if self.ltp_data_pair.is_some() {
                            return Err(Error::IcsInfoEncodeInvalid);
                        }
                    } else if self.ltp_data_present_pair.is_some() || self.ltp_data_pair.is_some() {
                        return Err(Error::IcsInfoEncodeInvalid);
                    }
                }
            } else {
                // predictor_data_present == 0: no predictor / LTP body
                // is emitted at all (Table 4.6 only enters either
                // branch under the predictor bit). Reject populated
                // slots so a stale in-memory structure cannot
                // silently desync from the wire.
                if self.predictor_data.is_some()
                    || self.ltp_data_present
                    || self.ltp_data.is_some()
                    || self.ltp_data_present_pair.is_some()
                    || self.ltp_data_pair.is_some()
                {
                    return Err(Error::IcsInfoEncodeInvalid);
                }
            }
        }

        Ok(())
    }
}

/// `ltp_data()` per Table 4.55. Public to allow standalone unit
/// tests; in normal use it is invoked indirectly via
/// [`IcsInfo::parse`].
pub fn parse_ltp_data(
    reader: &mut BitReader<'_>,
    audio_object_type: u8,
    window_sequence: WindowSequence,
    max_sfb: u8,
) -> Result<LtpData> {
    if audio_object_type == 23 {
        // ER_AAC_LD branch.
        let lag_update = read_bit(reader)?;
        let lag = if lag_update {
            Some(read_u16(reader, 10)?)
        } else {
            None
        };
        let coef = read_u8(reader, 3)?;
        let n = core::cmp::min(max_sfb as usize, MAX_LTP_LONG_SFB);
        let mut long_used = Vec::with_capacity(n);
        for _ in 0..n {
            long_used.push(read_bit(reader)?);
        }
        Ok(LtpData {
            lag_update: Some(lag_update),
            lag,
            coef,
            long_used,
        })
    } else {
        let lag = read_u16(reader, 11)?;
        let coef = read_u8(reader, 3)?;
        let long_used = if window_sequence.is_eight_short() {
            Vec::new()
        } else {
            let n = core::cmp::min(max_sfb as usize, MAX_LTP_LONG_SFB);
            let mut v = Vec::with_capacity(n);
            for _ in 0..n {
                v.push(read_bit(reader)?);
            }
            v
        };
        Ok(LtpData {
            lag_update: None,
            lag: Some(lag),
            coef,
            long_used,
        })
    }
}

/// Encode an `ltp_data()` (Table 4.55) body onto `writer`, the
/// inverse of [`parse_ltp_data`].
///
/// Mirrors the parser's two branches:
///
/// * `audio_object_type == 23` (ER AAC LD) — write `ltp_lag_update`
///   (1 bit); if set, write `ltp_lag` (10 bits); then `ltp_coef`
///   (3 bits); then `ltp_long_used[sfb]` for `sfb in 0..min(max_sfb,
///   MAX_LTP_LONG_SFB)`.
/// * Every other AOT — write `ltp_lag` (11 bits, always), `ltp_coef`
///   (3 bits), then `ltp_long_used[]` *unless* the surrounding
///   `ics_info()` says `EIGHT_SHORT_SEQUENCE` (the spec omits the
///   loop in that case).
///
/// Returns [`Error::IcsInfoEncodeInvalid`] when the in-memory
/// [`LtpData`] is inconsistent with the AOT or `window_sequence`
/// context (e.g. `lag_update == Some(_)` for a non-LD AOT, missing
/// `lag` for an LD `lag_update == true` slot, `coef > 7`, `lag`
/// exceeding its field width, or `long_used.len()` not matching
/// `min(max_sfb, MAX_LTP_LONG_SFB)` in the loop branch).
pub fn write_ltp_data(
    writer: &mut BitWriter,
    ltp: &LtpData,
    audio_object_type: u8,
    window_sequence: WindowSequence,
    max_sfb: u8,
) -> Result<()> {
    if ltp.coef > 0x07 {
        return Err(Error::IcsInfoEncodeInvalid);
    }
    if audio_object_type == 23 {
        let lag_update = ltp.lag_update.ok_or(Error::IcsInfoEncodeInvalid)?;
        writer.write_bit(lag_update);
        if lag_update {
            let lag = ltp.lag.ok_or(Error::IcsInfoEncodeInvalid)?;
            if lag > 0x3ff {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            writer.write_u32(lag as u32, 10);
        } else if ltp.lag.is_some() {
            return Err(Error::IcsInfoEncodeInvalid);
        }
        writer.write_u32(ltp.coef as u32, 3);
        let expected = core::cmp::min(max_sfb as usize, MAX_LTP_LONG_SFB);
        if ltp.long_used.len() != expected {
            return Err(Error::IcsInfoEncodeInvalid);
        }
        for &b in &ltp.long_used {
            writer.write_bit(b);
        }
    } else {
        if ltp.lag_update.is_some() {
            return Err(Error::IcsInfoEncodeInvalid);
        }
        let lag = ltp.lag.ok_or(Error::IcsInfoEncodeInvalid)?;
        if lag > 0x7ff {
            return Err(Error::IcsInfoEncodeInvalid);
        }
        writer.write_u32(lag as u32, 11);
        writer.write_u32(ltp.coef as u32, 3);
        if window_sequence.is_eight_short() {
            if !ltp.long_used.is_empty() {
                return Err(Error::IcsInfoEncodeInvalid);
            }
        } else {
            let expected = core::cmp::min(max_sfb as usize, MAX_LTP_LONG_SFB);
            if ltp.long_used.len() != expected {
                return Err(Error::IcsInfoEncodeInvalid);
            }
            for &b in &ltp.long_used {
                writer.write_bit(b);
            }
        }
    }
    Ok(())
}

/// Compute (`num_windows`, `num_window_groups`,
/// `window_group_length`, `num_swb`) per ISO/IEC 14496-3
/// §4.5.2.3.4. Exposed publicly so encoder-side code or
/// pre-section_data setup can compute the same derivations
/// without re-parsing an `ics_info`.
pub fn derive_window_grouping(
    window_sequence: WindowSequence,
    scale_factor_grouping: Option<u8>,
    fs_index: usize,
) -> (u8, u8, Vec<u8>, u8) {
    if !window_sequence.is_eight_short() {
        return (1, 1, vec![1], NUM_SWB_LONG_WINDOW[fs_index]);
    }
    // EIGHT_SHORT_SEQUENCE: scale_factor_grouping must be present
    // per Table 4.6. derive_window_grouping treats a missing mask
    // as the "no grouping" form (one group per window) so it
    // remains a pure function; callers that go through
    // IcsInfo::parse always supply the mask.
    let mask = scale_factor_grouping.unwrap_or(0);
    let mut groups: Vec<u8> = vec![1];
    for i in 0..7u32 {
        // bit_set(mask, 6 - i) — most-right bit is bit 0.
        let bit = (mask >> (6 - i as u8)) & 1;
        if bit == 0 {
            groups.push(1);
        } else {
            let last = groups.last_mut().expect("at least one group");
            *last += 1;
        }
    }
    let num_window_groups = groups.len() as u8;
    (8, num_window_groups, groups, NUM_SWB_SHORT_WINDOW[fs_index])
}

fn read_u8(reader: &mut BitReader<'_>, n: u32) -> Result<u8> {
    debug_assert!(n <= 8);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u8)
}

fn read_u16(reader: &mut BitReader<'_>, n: u32) -> Result<u16> {
    debug_assert!(n <= 16);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u16)
}

fn read_bit(reader: &mut BitReader<'_>) -> Result<bool> {
    reader.read_bit().map_err(|_| Error::UnexpectedEnd)
}
