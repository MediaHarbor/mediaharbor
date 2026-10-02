//! `extension_payload()` parser + encoder primitive — ISO/IEC
//! 14496-3 §4.4.2.7 / Table 4.51 plus the DRC
//! `dynamic_range_info()` body (Table 4.52) and the
//! `excluded_channels()` helper (Table 4.53), with
//! `extension_type` values per Table 4.59 (and ISO/IEC 13818-7
//! Table 40, which extends the 14496-3 table with the SBR-data
//! values).
//!
//! `extension_payload()` is the structured body inside a FIL
//! element (`fill_element()`). The outer FIL surfaces a byte
//! count `cnt`; the `extension_payload(cnt)` reads exactly `cnt`
//! bytes — the first 4 bits select an `extension_type`, the
//! remaining bits carry the type-specific body. Three of the four
//! well-known `extension_type` values have fully fixed-width
//! Table 4.51 / 4.52 layouts and are implemented here:
//!
//! * `EXT_FILL` (`0b0000`) — bitstream filler. Body is
//!   `8 * (cnt - 1) + 4` `other_bits`. No normative value
//!   constraint per Table 4.51's `default` branch.
//! * `EXT_FILL_DATA` (`0b0001`) — bitstream data as filler.
//!   Body is a 4-bit `fill_nibble` (normatively `0b0000`)
//!   followed by `cnt - 1` × 8-bit `fill_byte` (each normatively
//!   `0b10100101`).
//! * `EXT_DYNAMIC_RANGE` (`0b1011`) — dynamic range control.
//!   Body is the Table 4.52 `dynamic_range_info()` block (see
//!   [`DynamicRangeInfo`]).
//!
//! The SBR-data extension types defined by ISO/IEC 13818-7 Table 40
//! are surfaced as [`Error::UnsupportedExtensionSbr`] by the default
//! [`ExtensionPayload::parse`] (so the byte-exact AAC-LC decode path
//! stays untouched). The dedicated [`ExtensionPayload::parse_with_sbr`]
//! entry instead routes them into the §4.4.2.8
//! [`crate::sbr_extension::SbrExtensionData`] side-info walker (the SBR
//! back-end DSP is still not applied):
//!
//! * `EXT_SBR_DATA` (`0b1101`).
//! * `EXT_SBR_DATA_CRC` (`0b1110`).
//!
//! All other (reserved) values surface as
//! [`Error::UnsupportedExtensionType`] carrying the literal 4-bit
//! value as read from the wire.
//!
//! ## Why a parser / writer pair, and why now
//!
//! The Phase 1 `raw_data_block()` walker (round 121) recognises
//! FIL but skips its payload bytes opaque. Round 160's
//! `FrameAssembler::push_fill` accepts an opaque payload byte
//! slice. Neither side decodes or encodes the structured
//! `extension_payload()` body — and the FIL element is where the
//! DRC metadata (per-band gain factors), encoder-identifier fill
//! bytes, and the SBR enhancement bytes ride. This module is
//! the §4.4.2.7 wire-level decode/encode for the three non-SBR
//! extension types whose body layouts are fully specified by
//! fixed-width fields (no Huffman, no spectral context). The
//! intent is that downstream rounds plug this module into
//! `FrameAssembler::push_fill` /
//! `Walker::next_element` to surface a typed `extension_payload`
//! per FIL element.
//!
//! ## Returned byte count
//!
//! Per Table 4.51, `extension_payload()` returns the byte count
//! it consumed. Table 4.52's `dynamic_range_info()` returns its
//! own byte count starting from `n = 1` (the leading byte
//! containing the 4-bit `extension_type` nibble plus four of the
//! body's "presence" flags); each subsequent 8-bit-wide field set
//! is `n++`. [`ExtensionPayload::parse`] and [`ExtensionPayload::write`]
//! both expose this byte count via the returned
//! [`ExtensionPayload::bytes_consumed`] / [`ExtensionPayload::byte_length`]
//! accessors.
//!
//! ## What this module does *not* cover
//!
//! * No application of the DRC `(dyn_rng_sgn, dyn_rng_ctl)` gain
//!   factors to the reconstructed audio. §4.5.2.13 specifies the
//!   companding curve; this module surfaces the raw fields only.
//! * No semantic validation of `pce_instance_tag` against the
//!   surrounding PCE (the surrounding PCE may not be known at
//!   parse time — e.g. when the DRC FIL precedes the PCE in
//!   independent-program multiplexes).
//! * The SBR-data extension types (Table 40
//!   `EXT_SBR_DATA` / `EXT_SBR_DATA_CRC`) are surfaced as
//!   [`Error::UnsupportedExtensionSbr`] — their bodies are the
//!   `sbr_extension_data()` syntax which needs the QMF / patching
//!   back-end. This module's writer / parser deliberately does
//!   *not* consume bits for these types so a future SBR round can
//!   take over without a wire-format incompatibility.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::{Error, Result};

/// Width in bits of the wire `extension_type` field. ISO/IEC
/// 14496-3 Table 4.51.
pub const EXTENSION_TYPE_BITS: u32 = 4;

/// Symbolic `extension_type` values per ISO/IEC 14496-3 Table 4.59
/// plus the ISO/IEC 13818-7 Table 40 SBR-data extensions.
///
/// Every variant maps to a single 4-bit wire value; the raw value
/// is exposed via [`ExtensionType::as_u8`] for round-tripping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionType {
    /// `EXT_FILL` (`0b0000`) — bitstream filler.
    Fill,
    /// `EXT_FILL_DATA` (`0b0001`) — bitstream data as filler.
    /// Normative payload: 4-bit `fill_nibble == 0b0000` followed
    /// by `cnt - 1` × 8-bit `fill_byte == 0b10100101`.
    FillData,
    /// `EXT_DYNAMIC_RANGE` (`0b1011`) — dynamic range control.
    /// Body is the Table 4.52 `dynamic_range_info()` block.
    DynamicRange,
    /// `EXT_SBR_DATA` (`0b1101`) — SBR enhancement (ISO/IEC
    /// 13818-7 Table 40). This crate does not parse the
    /// `sbr_extension_data()` body yet.
    SbrData,
    /// `EXT_SBR_DATA_CRC` (`0b1110`) — SBR enhancement with CRC
    /// (ISO/IEC 13818-7 Table 40). This crate does not parse the
    /// `sbr_extension_data()` body yet.
    SbrDataCrc,
}

impl ExtensionType {
    /// Map a 4-bit wire value (`0..=15`) to the corresponding
    /// [`ExtensionType`], or surface a structural error.
    ///
    /// Returns:
    ///
    /// * [`Error::UnsupportedExtensionSbr`] for `0b1101`
    ///   (`EXT_SBR_DATA`) and `0b1110` (`EXT_SBR_DATA_CRC`) — the
    ///   bodies are the SBR `sbr_extension_data()` syntax which
    ///   this crate does not parse.
    /// * [`Error::UnsupportedExtensionType`] carrying the raw
    ///   4-bit value for any other value not in
    ///   `{0b0000, 0b0001, 0b1011, 0b1101, 0b1110}`. Table 4.59 /
    ///   Table 40 list these as "reserved".
    pub fn from_bits(value: u8) -> Result<Self> {
        match value {
            0b0000 => Ok(ExtensionType::Fill),
            0b0001 => Ok(ExtensionType::FillData),
            0b1011 => Ok(ExtensionType::DynamicRange),
            0b1101 | 0b1110 => Err(Error::UnsupportedExtensionSbr(value)),
            other if other <= 0x0f => Err(Error::UnsupportedExtensionType(other)),
            // unreachable in practice — `read_u32(4)` produces 0..=15
            _ => Err(Error::UnsupportedExtensionType(value)),
        }
    }

    /// Like [`Self::from_bits`] but maps the two SBR wire values to
    /// their [`ExtensionType`] variants instead of an error, so the
    /// [`ExtensionPayload::parse_with_sbr`] entry can dispatch them into
    /// the SBR side-info walker. Reserved values still error.
    pub fn from_bits_allow_sbr(value: u8) -> Result<Self> {
        match value {
            0b0000 => Ok(ExtensionType::Fill),
            0b0001 => Ok(ExtensionType::FillData),
            0b1011 => Ok(ExtensionType::DynamicRange),
            0b1101 => Ok(ExtensionType::SbrData),
            0b1110 => Ok(ExtensionType::SbrDataCrc),
            other => Err(Error::UnsupportedExtensionType(other)),
        }
    }

    /// Convert back to the 4-bit wire value used by Table 4.51.
    pub fn as_u8(self) -> u8 {
        match self {
            ExtensionType::Fill => 0b0000,
            ExtensionType::FillData => 0b0001,
            ExtensionType::DynamicRange => 0b1011,
            ExtensionType::SbrData => 0b1101,
            ExtensionType::SbrDataCrc => 0b1110,
        }
    }
}

/// Normative `fill_byte` literal per ISO/IEC 14496-3 §4.4.2.7 /
/// Table 4.51 (`must be '10100101'`). Surfaced as a public constant
/// so callers and tests can refer to the same magic value.
pub const FILL_DATA_BYTE: u8 = 0b1010_0101;

/// Normative `fill_nibble` literal per ISO/IEC 14496-3 §4.4.2.7 /
/// Table 4.51 (`must be '0000'`).
pub const FILL_DATA_NIBBLE: u8 = 0b0000;

/// Parsed `extension_payload()` body (Table 4.51 dispatch).
///
/// The body always carries the byte count it consumed (the `n`
/// returned by Table 4.51) so the surrounding FIL `cnt` can be
/// decremented in lockstep with the spec.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionPayload {
    /// `EXT_FILL` — opaque filler. Carries the raw `8 * (cnt - 1) + 4`
    /// "other_bits" packed MSB-first into a byte vector. The last
    /// byte's low 4 bits are unused if `cnt > 0` (since the body
    /// is not a whole number of bytes).
    Fill {
        /// Total bytes consumed by this `extension_payload`,
        /// including the 4-bit `extension_type` nibble (so the
        /// useful body is `8 * (cnt - 1) + 4` bits).
        cnt: u32,
        /// `other_bits` packed MSB-first. Empty when `cnt == 1`
        /// (a 4-bit-only EXT_FILL whose body is 4 unused bits).
        other_bits: Vec<u8>,
    },
    /// `EXT_FILL_DATA` — normative-pattern filler. Carries the
    /// byte count (FIL `cnt`) so the body length is implicit.
    FillData {
        /// Total bytes consumed (the FIL `cnt`).
        cnt: u32,
    },
    /// `EXT_DYNAMIC_RANGE` — DRC metadata per Table 4.52.
    DynamicRange(DynamicRangeInfo),
}

/// The result of [`ExtensionPayload::parse_with_sbr`]: either a standard
/// (non-SBR) extension payload, or a decoded SBR side-info element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionPayloadOrSbr {
    /// A non-SBR extension payload (`EXT_FILL` / `EXT_FILL_DATA` /
    /// `EXT_DYNAMIC_RANGE`).
    Payload(ExtensionPayload),
    /// A decoded `sbr_extension_data()` (`EXT_SBR_DATA` /
    /// `EXT_SBR_DATA_CRC`). Boxed because the SBR side-info element is
    /// much larger than the other variants.
    Sbr(Box<crate::sbr_extension::SbrExtensionData>),
}

/// Parsed `dynamic_range_info()` body (Table 4.52). All fields are
/// surfaced verbatim from the wire; the §4.5.2.13 companding curve
/// that maps `(dyn_rng_sgn, dyn_rng_ctl)` pairs to dB attenuations
/// is *not* applied here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynamicRangeInfo {
    /// Optional PCE element-tag selector. `Some((tag, reserved))`
    /// when `pce_tag_present == 1`. Both fields are 4 bits.
    pub pce_tag: Option<PceTagFields>,
    /// Optional excluded-channels list. `Some(_)` when
    /// `excluded_chns_present == 1`.
    pub excluded_channels: Option<ExcludedChannels>,
    /// Optional per-band partitioning. `Some(_)` when
    /// `drc_bands_present == 1`. When `None`, the spec sets
    /// `drc_num_bands = 1` and there is a single
    /// `(dyn_rng_sgn[0], dyn_rng_ctl[0])` pair below.
    pub drc_bands: Option<DrcBands>,
    /// Optional 7-bit `prog_ref_level` reference level
    /// (`Some((level, reserved))` when `prog_ref_level_present
    /// == 1`). `reserved` is the trailing 1-bit reserved field.
    pub prog_ref_level: Option<ProgRefLevelFields>,
    /// Per-band `(dyn_rng_sgn, dyn_rng_ctl)` records, in wire
    /// order. Length equals the resolved `drc_num_bands`
    /// (`drc_bands.is_none()` ⇒ 1; otherwise
    /// `1 + drc_bands.band_incr`).
    pub bands: Vec<DrcBandRecord>,
}

/// 4-bit `pce_instance_tag` + 4-bit `drc_tag_reserved_bits` pair
/// per Table 4.52.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PceTagFields {
    /// `pce_instance_tag` — selects the surrounding PCE this DRC
    /// applies to. 4 bits.
    pub pce_instance_tag: u8,
    /// `drc_tag_reserved_bits` — 4 bits, value not constrained by
    /// the spec.
    pub reserved: u8,
}

/// `excluded_channels()` body (Table 4.53). Carries the resolved
/// `exclude_mask[]` bits packed MSB-first into a `Vec<bool>`. The
/// wire length is implied by the trailing
/// `additional_excluded_chns[n-1] == 0` flag — every 7
/// `exclude_mask` bits are followed by a 1-bit continuation flag,
/// repeating until the continuation flag reads 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExcludedChannels {
    /// `exclude_mask[i]` for `i = 0..(7 * n_groups)`, where
    /// `n_groups` is the number of 8-bit-wide groups consumed.
    pub exclude_mask: Vec<bool>,
}

/// `drc_band_incr` + `drc_bands_reserved_bits` + `drc_band_top[]`
/// payload per Table 4.52.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrcBands {
    /// `drc_band_incr` — 4 bits. Resolved
    /// `drc_num_bands = 1 + drc_band_incr`.
    pub band_incr: u8,
    /// `drc_bands_reserved_bits` — 4 bits, value not constrained
    /// by the spec.
    pub reserved: u8,
    /// `drc_band_top[i]` — 8 bits per band. Length equals
    /// `1 + band_incr`.
    pub band_top: Vec<u8>,
}

/// 7-bit `prog_ref_level` + 1-bit `prog_ref_level_reserved_bits`
/// pair per Table 4.52.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgRefLevelFields {
    /// `prog_ref_level` — 7 bits. Reference level for downstream
    /// loudness normalisation.
    pub level: u8,
    /// `prog_ref_level_reserved_bits` — 1 bit.
    pub reserved: bool,
}

/// Per-band `(dyn_rng_sgn, dyn_rng_ctl)` pair per Table 4.52.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrcBandRecord {
    /// `dyn_rng_sgn[i]` — 1-bit sign. `true` ⇒ negative gain (cut).
    pub dyn_rng_sgn: bool,
    /// `dyn_rng_ctl[i]` — 7-bit magnitude in 0.25 dB steps per
    /// §4.5.2.13. Surfaced verbatim here.
    pub dyn_rng_ctl: u8,
}

impl ExtensionPayload {
    /// Parse an `extension_payload(cnt)` from `reader`.
    ///
    /// `cnt` is the FIL element's payload byte count after the
    /// §4.4.2.7 `esc_count` escape resolution (the same value the
    /// existing [`crate::raw_data_block::Walker`] computes via
    /// `read_fill_count`). `cnt == 0` is rejected as
    /// [`Error::ExtensionPayloadInvalid`] — Table 4.51's
    /// `extension_type` field itself is 4 bits, so a zero-byte FIL
    /// has no room for it.
    pub fn parse(reader: &mut BitReader<'_>, cnt: u32) -> Result<Self> {
        if cnt == 0 {
            return Err(Error::ExtensionPayloadInvalid);
        }
        let raw = read_u8(reader, EXTENSION_TYPE_BITS)?;
        let ty = ExtensionType::from_bits(raw)?;
        match ty {
            ExtensionType::Fill => parse_fill(reader, cnt),
            ExtensionType::FillData => parse_fill_data(reader, cnt),
            ExtensionType::DynamicRange => parse_dynamic_range(reader, cnt),
            // `from_bits` already converted these to errors.
            ExtensionType::SbrData | ExtensionType::SbrDataCrc => unreachable!(),
        }
    }

    /// Parse an `extension_payload(cnt)`, routing the two SBR extension
    /// types (`EXT_SBR_DATA` / `EXT_SBR_DATA_CRC`) into the
    /// [`crate::sbr_extension::SbrExtensionData`] side-info walker rather
    /// than rejecting them.
    ///
    /// Unlike [`Self::parse`] (which surfaces
    /// [`Error::UnsupportedExtensionSbr`] for the SBR types so the
    /// byte-exact AAC-LC decode path stays untouched), this entry decodes
    /// the SBR bitstream side info: the §4.4.2.8 `sbr_extension_data()`
    /// header + element framing keyed off the surrounding channel
    /// element. The SBR back-end DSP (QMF / HF patching / envelope
    /// adjustment) is still not applied — this only recovers the decoded
    /// side info.
    ///
    /// * `id_aac` — the AAC core element this FIL follows
    ///   ([`crate::raw_data_block::IdSynEle::Sce`] / `Cpe`); selects the
    ///   single- vs pair-element `sbr_data()` dispatch.
    /// * `fs_sbr` — the SBR internal sample rate (twice the core rate).
    /// * `prev_header` — the threaded previous `sbr_header()` for the
    ///   `bs_header_flag == 0` reuse path (`None` on the first payload).
    ///
    /// A non-SBR extension type returns
    /// [`ExtensionPayloadOrSbr::Payload`] with the same body
    /// [`Self::parse`] would produce.
    pub fn parse_with_sbr(
        reader: &mut BitReader<'_>,
        cnt: u32,
        id_aac: crate::raw_data_block::IdSynEle,
        fs_sbr: u32,
        prev_header: Option<crate::sbr_header::SbrHeader>,
    ) -> Result<ExtensionPayloadOrSbr> {
        if cnt == 0 {
            return Err(Error::ExtensionPayloadInvalid);
        }
        let raw = read_u8(reader, EXTENSION_TYPE_BITS)?;
        let ty = ExtensionType::from_bits_allow_sbr(raw)?;
        match ty {
            ExtensionType::Fill => Ok(ExtensionPayloadOrSbr::Payload(parse_fill(reader, cnt)?)),
            ExtensionType::FillData => Ok(ExtensionPayloadOrSbr::Payload(parse_fill_data(
                reader, cnt,
            )?)),
            ExtensionType::DynamicRange => Ok(ExtensionPayloadOrSbr::Payload(parse_dynamic_range(
                reader, cnt,
            )?)),
            ExtensionType::SbrData | ExtensionType::SbrDataCrc => {
                let crc_flag = ty == ExtensionType::SbrDataCrc;
                let sbr = crate::sbr_extension::SbrExtensionData::parse(
                    reader,
                    id_aac,
                    crc_flag,
                    fs_sbr,
                    Some(cnt),
                    prev_header,
                )?;
                Ok(ExtensionPayloadOrSbr::Sbr(Box::new(sbr)))
            }
        }
    }

    /// Encode an `extension_payload()` body onto `writer` — the
    /// bit-exact inverse of [`ExtensionPayload::parse`].
    ///
    /// Returns the byte count consumed (matching Table 4.51's
    /// returned `n`). Surfaces caller-side field violations as
    /// [`Error::ExtensionPayloadInvalid`].
    pub fn write(&self, writer: &mut BitWriter) -> Result<u32> {
        match self {
            ExtensionPayload::Fill { cnt, other_bits } => write_fill(writer, *cnt, other_bits),
            ExtensionPayload::FillData { cnt } => write_fill_data(writer, *cnt),
            ExtensionPayload::DynamicRange(drc) => write_dynamic_range(writer, drc),
        }
    }

    /// Total byte count this `extension_payload` consumed on the
    /// wire — Table 4.51's returned `n`.
    pub fn byte_length(&self) -> u32 {
        match self {
            ExtensionPayload::Fill { cnt, .. } => *cnt,
            ExtensionPayload::FillData { cnt } => *cnt,
            ExtensionPayload::DynamicRange(drc) => drc.byte_length(),
        }
    }
}

impl DynamicRangeInfo {
    /// Byte count this DRC body consumes — Table 4.52's returned
    /// `n`, including the 4-bit `extension_type` nibble that the
    /// outer `extension_payload()` writes immediately before the
    /// DRC body.
    pub fn byte_length(&self) -> u32 {
        // Start from n = 1 (the leading byte containing the 4-bit
        // extension_type + 4 presence flags).
        let mut n: u32 = 1;
        if self.pce_tag.is_some() {
            n += 1;
        }
        if let Some(ex) = &self.excluded_channels {
            // Each group is 7 mask bits + 1 continuation bit = 1 byte.
            n += excluded_group_count(ex.exclude_mask.len()) as u32;
        }
        if let Some(b) = &self.drc_bands {
            // drc_band_incr + reserved = 1 byte, then 1 byte per
            // drc_band_top entry.
            n += 1 + b.band_top.len() as u32;
        }
        if self.prog_ref_level.is_some() {
            n += 1;
        }
        // 1 byte per (dyn_rng_sgn + dyn_rng_ctl).
        n += self.bands.len() as u32;
        n
    }

    /// Resolved `drc_num_bands` per Table 4.52. Always equals
    /// `bands.len()`.
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }
}

// ===================================================================
// EXT_FILL parser / writer
// ===================================================================

fn parse_fill(reader: &mut BitReader<'_>, cnt: u32) -> Result<ExtensionPayload> {
    // Table 4.51 default branch:
    //   for (i = 0; i < 8*(cnt-1) + 4; i++) other_bits[i];
    // 4 of those bits are already consumed (the extension_type
    // nibble — except wait, no: the 8*(cnt-1)+4 count is the bits
    // AFTER the extension_type. Re-reading the spec carefully —
    // Table 4.51 reads extension_type FIRST, then enters the
    // switch; the default branch's loop counts the body AFTER the
    // type nibble. The total bits consumed is then
    // 4 + 8*(cnt-1) + 4 = 8 * cnt — consistent with returning cnt.
    let body_bits = 8u32
        .checked_mul(cnt.saturating_sub(1))
        .ok_or(Error::ExtensionPayloadInvalid)?
        .checked_add(4)
        .ok_or(Error::ExtensionPayloadInvalid)?;
    let other_bits = read_packed_bits(reader, body_bits)?;
    Ok(ExtensionPayload::Fill { cnt, other_bits })
}

fn write_fill(writer: &mut BitWriter, cnt: u32, other_bits: &[u8]) -> Result<u32> {
    if cnt == 0 {
        return Err(Error::ExtensionPayloadInvalid);
    }
    let body_bits = 8u32
        .checked_mul(cnt.saturating_sub(1))
        .ok_or(Error::ExtensionPayloadInvalid)?
        .checked_add(4)
        .ok_or(Error::ExtensionPayloadInvalid)?;
    let expected_bytes = (body_bits as usize).div_ceil(8);
    if other_bits.len() != expected_bytes {
        return Err(Error::ExtensionPayloadInvalid);
    }
    writer.write_u32(ExtensionType::Fill.as_u8() as u32, EXTENSION_TYPE_BITS);
    write_packed_bits(writer, other_bits, body_bits)?;
    Ok(cnt)
}

// ===================================================================
// EXT_FILL_DATA parser / writer
// ===================================================================

fn parse_fill_data(reader: &mut BitReader<'_>, cnt: u32) -> Result<ExtensionPayload> {
    // Table 4.51:
    //   fill_nibble;                 4 bits  /* must be '0000' */
    //   for (i = 0; i < cnt - 1; i++)
    //       fill_byte[i];            8 bits  /* must be '10100101' */
    let nibble = read_u8(reader, 4)?;
    if nibble != FILL_DATA_NIBBLE {
        return Err(Error::ExtensionPayloadInvalid);
    }
    let body_bytes = cnt.saturating_sub(1) as usize;
    for _ in 0..body_bytes {
        let b = read_u8(reader, 8)?;
        if b != FILL_DATA_BYTE {
            return Err(Error::ExtensionPayloadInvalid);
        }
    }
    Ok(ExtensionPayload::FillData { cnt })
}

fn write_fill_data(writer: &mut BitWriter, cnt: u32) -> Result<u32> {
    if cnt == 0 {
        return Err(Error::ExtensionPayloadInvalid);
    }
    writer.write_u32(ExtensionType::FillData.as_u8() as u32, EXTENSION_TYPE_BITS);
    writer.write_u32(FILL_DATA_NIBBLE as u32, 4);
    let body_bytes = cnt.saturating_sub(1) as usize;
    for _ in 0..body_bytes {
        writer.write_u32(FILL_DATA_BYTE as u32, 8);
    }
    Ok(cnt)
}

// ===================================================================
// EXT_DYNAMIC_RANGE parser / writer
// ===================================================================

fn parse_dynamic_range(reader: &mut BitReader<'_>, cnt: u32) -> Result<ExtensionPayload> {
    let pce_tag_present = read_bit(reader)?;
    let pce_tag = if pce_tag_present {
        let pce_instance_tag = read_u8(reader, 4)?;
        let reserved = read_u8(reader, 4)?;
        Some(PceTagFields {
            pce_instance_tag,
            reserved,
        })
    } else {
        None
    };

    let excluded_chns_present = read_bit(reader)?;
    let excluded_channels = if excluded_chns_present {
        Some(parse_excluded_channels(reader)?)
    } else {
        None
    };

    let drc_bands_present = read_bit(reader)?;
    let drc_bands = if drc_bands_present {
        let band_incr = read_u8(reader, 4)?;
        let reserved = read_u8(reader, 4)?;
        let num_bands = 1usize + band_incr as usize;
        let mut band_top = Vec::with_capacity(num_bands);
        for _ in 0..num_bands {
            band_top.push(read_u8(reader, 8)?);
        }
        Some(DrcBands {
            band_incr,
            reserved,
            band_top,
        })
    } else {
        None
    };

    let prog_ref_level_present = read_bit(reader)?;
    let prog_ref_level = if prog_ref_level_present {
        let level = read_u8(reader, 7)?;
        let reserved = read_bit(reader)?;
        Some(ProgRefLevelFields { level, reserved })
    } else {
        None
    };

    let num_bands = drc_bands
        .as_ref()
        .map(|b| 1 + b.band_incr as usize)
        .unwrap_or(1);
    let mut bands = Vec::with_capacity(num_bands);
    for _ in 0..num_bands {
        let dyn_rng_sgn = read_bit(reader)?;
        let dyn_rng_ctl = read_u8(reader, 7)?;
        bands.push(DrcBandRecord {
            dyn_rng_sgn,
            dyn_rng_ctl,
        });
    }

    let drc = DynamicRangeInfo {
        pce_tag,
        excluded_channels,
        drc_bands,
        prog_ref_level,
        bands,
    };
    if drc.byte_length() != cnt {
        // The dispatching FIL `cnt` and the derived Table 4.52 `n`
        // must agree byte-for-byte — Table 4.52 normatively
        // returns the byte count to the caller. A mismatch
        // indicates a malformed bitstream.
        return Err(Error::ExtensionPayloadInvalid);
    }
    Ok(ExtensionPayload::DynamicRange(drc))
}

fn write_dynamic_range(writer: &mut BitWriter, drc: &DynamicRangeInfo) -> Result<u32> {
    // Caller-side invariant checks (every numeric field cap from
    // Table 4.52).
    if let Some(p) = &drc.pce_tag {
        if p.pce_instance_tag > 0x0f || p.reserved > 0x0f {
            return Err(Error::ExtensionPayloadInvalid);
        }
    }
    if let Some(b) = &drc.drc_bands {
        if b.band_incr > 0x0f || b.reserved > 0x0f {
            return Err(Error::ExtensionPayloadInvalid);
        }
        if b.band_top.len() != 1 + b.band_incr as usize {
            return Err(Error::ExtensionPayloadInvalid);
        }
    }
    if let Some(p) = &drc.prog_ref_level {
        if p.level > 0x7f {
            return Err(Error::ExtensionPayloadInvalid);
        }
    }
    let expected_bands = drc
        .drc_bands
        .as_ref()
        .map(|b| 1 + b.band_incr as usize)
        .unwrap_or(1);
    if drc.bands.len() != expected_bands {
        return Err(Error::ExtensionPayloadInvalid);
    }
    for r in &drc.bands {
        if r.dyn_rng_ctl > 0x7f {
            return Err(Error::ExtensionPayloadInvalid);
        }
    }

    writer.write_u32(
        ExtensionType::DynamicRange.as_u8() as u32,
        EXTENSION_TYPE_BITS,
    );

    writer.write_bit(drc.pce_tag.is_some());
    if let Some(p) = &drc.pce_tag {
        writer.write_u32(p.pce_instance_tag as u32, 4);
        writer.write_u32(p.reserved as u32, 4);
    }

    writer.write_bit(drc.excluded_channels.is_some());
    if let Some(ex) = &drc.excluded_channels {
        write_excluded_channels(writer, ex)?;
    }

    writer.write_bit(drc.drc_bands.is_some());
    if let Some(b) = &drc.drc_bands {
        writer.write_u32(b.band_incr as u32, 4);
        writer.write_u32(b.reserved as u32, 4);
        for &top in &b.band_top {
            writer.write_u32(top as u32, 8);
        }
    }

    writer.write_bit(drc.prog_ref_level.is_some());
    if let Some(p) = &drc.prog_ref_level {
        writer.write_u32(p.level as u32, 7);
        writer.write_bit(p.reserved);
    }

    for r in &drc.bands {
        writer.write_bit(r.dyn_rng_sgn);
        writer.write_u32(r.dyn_rng_ctl as u32, 7);
    }

    Ok(drc.byte_length())
}

// ===================================================================
// excluded_channels() helper (Table 4.53)
// ===================================================================

fn parse_excluded_channels(reader: &mut BitReader<'_>) -> Result<ExcludedChannels> {
    // Table 4.53: each iteration reads 7 exclude_mask bits + 1
    // additional_excluded_chns continuation flag = 1 byte. Stop
    // when the continuation flag reads 0.
    let mut exclude_mask = Vec::new();
    loop {
        for _ in 0..7 {
            exclude_mask.push(read_bit(reader)?);
        }
        let cont = read_bit(reader)?;
        if !cont {
            break;
        }
    }
    Ok(ExcludedChannels { exclude_mask })
}

fn write_excluded_channels(writer: &mut BitWriter, ex: &ExcludedChannels) -> Result<()> {
    if ex.exclude_mask.is_empty() || ex.exclude_mask.len() % 7 != 0 {
        // Table 4.53 emits exclude_mask bits in fixed groups of 7;
        // any non-multiple-of-7 length cannot round-trip through
        // [`parse_excluded_channels`].
        return Err(Error::ExtensionPayloadInvalid);
    }
    let groups = ex.exclude_mask.len() / 7;
    for g in 0..groups {
        for i in 0..7 {
            writer.write_bit(ex.exclude_mask[g * 7 + i]);
        }
        // The continuation flag is 1 for every group except the
        // last, which carries 0 to terminate.
        let last = g + 1 == groups;
        writer.write_bit(!last);
    }
    Ok(())
}

/// Resolved byte count for an `excluded_channels()` body carrying
/// the given total `exclude_mask` bit count. Exposed so callers can
/// pre-size `cnt` without round-tripping through
/// [`DynamicRangeInfo::byte_length`].
pub fn excluded_group_count(exclude_mask_len: usize) -> usize {
    // The spec emits 7-bit groups; the byte count equals the group
    // count (each group is 7 mask bits + 1 continuation bit).
    exclude_mask_len.div_ceil(7)
}

// ===================================================================
// Helpers
// ===================================================================

fn read_packed_bits(reader: &mut BitReader<'_>, n_bits: u32) -> Result<Vec<u8>> {
    let n_bytes = (n_bits as usize).div_ceil(8);
    let mut out = vec![0u8; n_bytes];
    let mut remaining = n_bits;
    let mut idx = 0;
    while remaining >= 8 {
        out[idx] = read_u8(reader, 8)?;
        idx += 1;
        remaining -= 8;
    }
    if remaining > 0 {
        // Pack the trailing partial byte into the top bits of the
        // last output byte.
        let partial = read_u8(reader, remaining)?;
        out[idx] = partial << (8 - remaining);
    }
    Ok(out)
}

fn write_packed_bits(writer: &mut BitWriter, bytes: &[u8], n_bits: u32) -> Result<()> {
    let mut remaining = n_bits;
    let mut idx = 0;
    while remaining >= 8 {
        writer.write_u32(bytes[idx] as u32, 8);
        idx += 1;
        remaining -= 8;
    }
    if remaining > 0 {
        // The trailing partial byte stores its bits in the top
        // `remaining` bits; recover them with a right-shift.
        let partial = bytes[idx] >> (8 - remaining);
        writer.write_u32(partial as u32, remaining);
    }
    Ok(())
}

fn read_u8(reader: &mut BitReader<'_>, n: u32) -> Result<u8> {
    debug_assert!(n <= 8);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u8)
}

fn read_bit(reader: &mut BitReader<'_>) -> Result<bool> {
    reader.read_bit().map_err(|_| Error::UnexpectedEnd)
}
