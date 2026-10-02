//! `spectral_data()` wire walker — ISO/IEC 14496-3 Table 4.56.
//!
//! This module is the §4.4.6 driver that the round-259 README named
//! as the next step after the Codebook 1..=11 table set completed:
//! it loops over the window groups and sections established by
//! [`crate::ics_info`] / [`crate::section_data`] and recovers the
//! quantised spectral coefficients `x_quant` by dispatching, per
//! section, onto the [`crate::spectrum_huffman`] codeword decoders
//! and the [`crate::spectral_codebook`] index/sign/ESC translation
//! helpers.
//!
//! ## Table 4.56 layout
//!
//! ```text
//! spectral_data() {
//!     for (g = 0; g < num_window_groups; g++) {
//!         for (i = 0; i < num_sec[g]; i++) {
//!             if (sect_cb[g][i] != ZERO_HCB &&
//!                 sect_cb[g][i] != NOISE_HCB &&
//!                 sect_cb[g][i] != INTENSITY_HCB &&
//!                 sect_cb[g][i] != INTENSITY_HCB2) {
//!                 for (k = sect_sfb_offset[g][sect_start[g][i]];
//!                      k < sect_sfb_offset[g][sect_end[g][i]];) {
//!                     if (sect_cb[g][i] < FIRST_PAIR_HCB) {
//!                         hcod[sect_cb[g][i]][w][x][y][z];   // 1..16 vlclbf
//!                         if (unsigned_cb[sect_cb[g][i]])
//!                             quad_sign_bits;                // 0..4 bslbf
//!                         k += QUAD_LEN;
//!                     } else {
//!                         hcod[sect_cb[g][i]][y][z];         // 1..15 vlclbf
//!                         if (unsigned_cb[sect_cb[g][i]])
//!                             pair_sign_bits;                // 0..2 bslbf
//!                         k += PAIR_LEN;
//!                         if (sect_cb[g][i] == ESC_HCB) {
//!                             if (y == ESC_FLAG) hcod_esc_y; // 5..21 vlclbf
//!                             if (z == ESC_FLAG) hcod_esc_z; // 5..21 vlclbf
//!                         }
//!                     }
//!                 }
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## `sect_sfb_offset` — §4.5.2.3.4
//!
//! The loop bounds come from the per-group coefficient offsets
//! `sect_sfb_offset[g][sfb]` derived in §4.5.2.3.4:
//!
//! * For the three long window sequences (`num_window_groups == 1`,
//!   `window_group_length[0] == 1`) the offsets are simply
//!   `swb_offset_long_window[fs_index][sfb]` for
//!   `sfb ∈ 0..=max_sfb`.
//! * For `EIGHT_SHORT_SEQUENCE` each group `g` spans
//!   `window_group_length[g]` grouped short windows whose spectral
//!   data is interleaved scalefactor-band by scalefactor-band
//!   (§4.5.2.3.5), so each *virtual* scalefactor band is
//!   `window_group_length[g]` times the Table 4.130-family
//!   scalefactor-window-band width:
//!   `sect_sfb_offset[g][i+1] = sect_sfb_offset[g][i] +
//!   (swb_offset_short[i+1] − swb_offset_short[i]) ×
//!   window_group_length[g]`.
//!
//! [`sect_sfb_offset`] exposes that derivation so follow-up tools
//! (the §4.6.3.3 `quant_to_spec()` deinterleaver, intensity / PNS
//! reconstruction) can reuse it.
//!
//! ## Coefficient storage
//!
//! [`SpectralData::x_quant`] holds one buffer per window group, in
//! the §4.5.2.3.5 *transmission* order: groups sequential, and
//! within a group the coefficients of all grouped short windows
//! interleaved per scalefactor band ("virtual" scalefactor bands).
//! Each group buffer is allocated at the full group span —
//! `window_group_length[g] × 128` for `EIGHT_SHORT_SEQUENCE`,
//! `1024` otherwise — with the bands above `max_sfb` (and every
//! `ZERO_HCB` / `NOISE_HCB` / intensity band) left at `0`, matching
//! the §4.5.2.3 "all spectral data associated with Huffman codebook
//! zero are omitted [and zeroed]" rule. De-interleaving into the
//! `spec[w][k]` window-major layout consumed by TNS / the filterbank
//! (the §4.6.3.3 `quant_to_spec()` pseudocode) is a follow-up tool.
//!
//! ## §4.6.3.3 per-codeword translation
//!
//! * The Huffman codeword index is translated to the n-tuple via
//!   [`crate::spectral_codebook::decode_index_to_tuple`].
//! * For unsigned codebooks (3, 4, 7..=11) the
//!   `quad_sign_bits` / `pair_sign_bits` field follows the codeword
//!   — one bit per non-zero coefficient, low frequency first, `1` =
//!   negative — applied via
//!   [`crate::spectral_codebook::apply_sign_bits`].
//! * For the ESC codebook (11) a decoded magnitude of `16`
//!   (`ESC_FLAG`) is not a literal value: a `hcod_esc_y` /
//!   `hcod_esc_z` escape sequence follows the sign bits (in `y`,
//!   `z` order) — an `escape_prefix` of `N` ones, a zero
//!   `escape_separator`, and an `(N + 4)`-bit `escape_word` —
//!   decoding to `2^(N+4) + escape_word` via
//!   [`crate::spectral_codebook::decode_esc_value`], with the sign
//!   carried by the already-parsed sign bit. §4.6.1.3 caps the
//!   magnitude at `MAX_QUANT` (8191), so `N ≤ 8` on a conforming
//!   stream.
//!
//! [`SpectralData::write`] is the symmetric encoder: it re-derives
//! the codeword index via
//! [`crate::spectral_codebook::encode_tuple_to_index`] (clamping
//! ESC-book magnitudes ≥ 16 to the in-band `ESC_FLAG`), emits the
//! sign bits via [`crate::spectral_codebook::derive_sign_bits`], and
//! appends the escape sequences via
//! [`crate::spectral_codebook::encode_esc_value`], producing a
//! bit-exact inverse of [`SpectralData::parse`].

use oxideav_core::bits::{BitReader, BitWriter};

use crate::ics_info::IcsInfo;
use crate::section_data::{Codebook, Section, SectionData};
use crate::spectral_codebook::{
    apply_sign_bits, decode_esc_value, decode_index_to_tuple, derive_sign_bits, encode_esc_value,
    encode_tuple_to_index, table_4_95, MAX_QUANT,
};
use crate::spectrum_huffman::{
    hcod10_decode, hcod10_write, hcod11_decode, hcod11_write, hcod1_decode, hcod1_write,
    hcod2_decode, hcod2_write, hcod3_decode, hcod3_write, hcod4_decode, hcod4_write, hcod5_decode,
    hcod5_write, hcod6_decode, hcod6_write, hcod7_decode, hcod7_write, hcod8_decode, hcod8_write,
    hcod9_decode, hcod9_write,
};
use crate::swb_offset::{
    long_window_offsets, short_window_offsets, LONG_WINDOW_LEN, SHORT_WINDOW_LEN,
};
use crate::{Error, Result};

/// `QUAD_LEN` — coefficients per codeword for the dim-4 books
/// (1..=4), per Table 4.56 / Table 4.151.
pub const QUAD_LEN: usize = 4;

/// `PAIR_LEN` — coefficients per codeword for the dim-2 books
/// (5..=11), per Table 4.56 / Table 4.151.
pub const PAIR_LEN: usize = 2;

/// `ESC_FLAG` — the in-band ESC-book magnitude (16) that signals a
/// following `hcod_esc_y` / `hcod_esc_z` escape sequence
/// (§4.6.3.3).
pub const ESC_FLAG: i32 = 16;

/// Derive `sect_sfb_offset[g][sfb]` (`sfb ∈ 0..=max_sfb`) per the
/// §4.5.2.3.4 pseudocode — the offset of the first coefficient of
/// each (virtual) scalefactor band within window group `g`'s
/// interleaved coefficient stream.
///
/// Returns one `max_sfb + 1`-entry offset vector per window group.
/// For the long window sequences this is a single group mirroring
/// `swb_offset_long_window[fs_index]`; for `EIGHT_SHORT_SEQUENCE`
/// each group scales the Table 4.130-family band widths by
/// `window_group_length[g]`.
///
/// Errors:
///
/// * [`Error::IcsInfoUnsupportedSampleRateIndex`] — `fs_index`
///   outside the `0..=11` SWB-table range.
/// * [`Error::SpectralDataInvalid`] — `max_sfb` exceeds the
///   `num_swb` of the active window sequence (the §4.5.2.3.4 loops
///   index `swb_offset[max_sfb]`, which only exists up to
///   `num_swb`).
pub fn sect_sfb_offset(ics_info: &IcsInfo, fs_index: u8) -> Result<Vec<Vec<u32>>> {
    let max_sfb = ics_info.max_sfb as usize;
    if ics_info.window_sequence.is_eight_short() {
        let swb = short_window_offsets(fs_index)?;
        // `swb` has num_swb + 1 entries; band widths exist for
        // sfb < num_swb only.
        if max_sfb + 1 > swb.len() {
            return Err(Error::SpectralDataInvalid);
        }
        let mut per_group = Vec::with_capacity(ics_info.num_window_groups as usize);
        for g in 0..ics_info.num_window_groups as usize {
            let wgl = u32::from(ics_info.window_group_length[g]);
            let mut offsets = Vec::with_capacity(max_sfb + 1);
            let mut offset = 0u32;
            offsets.push(offset);
            for i in 0..max_sfb {
                let width = u32::from(swb[i + 1] - swb[i]) * wgl;
                offset += width;
                offsets.push(offset);
            }
            per_group.push(offsets);
        }
        Ok(per_group)
    } else {
        let swb = long_window_offsets(fs_index)?;
        if max_sfb + 1 > swb.len() {
            return Err(Error::SpectralDataInvalid);
        }
        let offsets = swb[..=max_sfb].iter().map(|&o| u32::from(o)).collect();
        Ok(vec![offsets])
    }
}

/// Quantised spectral coefficients recovered from (or destined for)
/// a Table 4.56 `spectral_data()` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpectralData {
    /// `x_quant`, one buffer per window group, in the §4.5.2.3.5
    /// transmission (interleaved) order. `x_quant[g].len() ==
    /// window_group_length[g] × 128` for `EIGHT_SHORT_SEQUENCE`,
    /// `1024` otherwise; bands at or above `max_sfb` and bands whose
    /// section codebook carries no spectrum (`ZERO_HCB`,
    /// `NOISE_HCB`, intensity) are `0`.
    pub x_quant: Vec<Vec<i32>>,
}

impl SpectralData {
    /// Length of one window group's coefficient buffer.
    fn group_len(ics_info: &IcsInfo, g: usize) -> usize {
        if ics_info.window_sequence.is_eight_short() {
            ics_info.window_group_length[g] as usize * SHORT_WINDOW_LEN as usize
        } else {
            LONG_WINDOW_LEN as usize
        }
    }

    /// Parse a Table 4.56 `spectral_data()` block.
    ///
    /// * `reader` — positioned at the first `spectral_data()` bit
    ///   (the position [`crate::ics_body::IcsBody`] surfaces as
    ///   `spectral_data_bit_offset`).
    /// * `ics_info` — the channel's parsed `ics_info()` (drives
    ///   `num_window_groups` / `window_group_length` /
    ///   `window_sequence`).
    /// * `section_data` — the channel's parsed `section_data()`
    ///   (drives the per-section codebook dispatch and the
    ///   `sect_start` / `sect_end` loop bounds).
    /// * `fs_index` — `samplingFrequencyIndex` selecting the
    ///   Table 4.129-family `swb_offset` tables.
    ///
    /// Errors:
    ///
    /// * [`Error::UnexpectedEnd`] — bit-reader underflow inside a
    ///   codeword, sign-bit field, or escape sequence.
    /// * [`Error::SpectralDataInvalid`] — structural violations: see
    ///   [`sect_sfb_offset`], a `section_data` group count that
    ///   disagrees with `ics_info`, a section carrying the reserved
    ///   codebook 12, or a section span that is not a whole number
    ///   of n-tuples.
    /// * [`Error::SpectralCodebookEscOutOfRange`] — an escape
    ///   sequence whose decoded magnitude exceeds `MAX_QUANT`
    ///   (8191) per §4.6.1.3.
    pub fn parse(
        reader: &mut BitReader<'_>,
        ics_info: &IcsInfo,
        section_data: &SectionData,
        fs_index: u8,
    ) -> Result<Self> {
        let offsets = sect_sfb_offset(ics_info, fs_index)?;
        let num_groups = ics_info.num_window_groups as usize;
        if section_data.sections.len() != num_groups {
            return Err(Error::SpectralDataInvalid);
        }

        let mut x_quant = Vec::with_capacity(num_groups);
        for (g, group_offsets) in offsets.iter().enumerate() {
            let mut buf = vec![0i32; Self::group_len(ics_info, g)];
            for sec in &section_data.sections[g] {
                let (cb, dim) = match section_codebook(sec)? {
                    Some(pair) => pair,
                    None => continue,
                };
                let start = group_offsets[sec.start as usize] as usize;
                let end = group_offsets[sec.end as usize] as usize;
                debug_assert!(end <= buf.len(), "offsets bounded by group span");
                let mut k = start;
                while k < end {
                    if k + dim > end {
                        return Err(Error::SpectralDataInvalid);
                    }
                    let idx = decode_codeword(reader, cb)?;
                    let tuple = decode_index_to_tuple(cb, idx)?;
                    let tuple = read_and_apply_signs(reader, cb, dim, tuple)?;
                    for (j, &v) in tuple.iter().take(dim).enumerate() {
                        buf[k + j] = if cb == 11 && v.abs() == ESC_FLAG {
                            // §4.6.3.3: escape sequences follow the
                            // sign bits, in y then z order; the sign
                            // bit already parsed applies to the
                            // escaped magnitude.
                            let mag = read_escape_sequence(reader)? as i32;
                            if v < 0 {
                                -mag
                            } else {
                                mag
                            }
                        } else {
                            v
                        };
                    }
                    k += dim;
                }
            }
            x_quant.push(buf);
        }
        Ok(SpectralData { x_quant })
    }

    /// Write a Table 4.56 `spectral_data()` block — the bit-exact
    /// inverse of [`SpectralData::parse`] under the same `ics_info`
    /// / `section_data` / `fs_index`.
    ///
    /// Errors:
    ///
    /// * [`Error::SpectralDataInvalid`] — same structural checks as
    ///   the parser.
    /// * [`Error::SpectralDataEncodeInvalid`] — group buffer count
    ///   or lengths disagreeing with the `ics_info` grouping, or a
    ///   non-zero coefficient in a band that transmits no spectrum
    ///   (`ZERO_HCB` / `NOISE_HCB` / intensity sections, or at and
    ///   above `max_sfb`).
    /// * [`Error::SpectralCodebookTupleOutOfRange`] — a coefficient
    ///   magnitude exceeding the section codebook's LAV (for the
    ///   ESC book, propagated as
    ///   [`Error::SpectralCodebookEscOutOfRange`] above
    ///   `MAX_QUANT`).
    pub fn write(
        &self,
        writer: &mut BitWriter,
        ics_info: &IcsInfo,
        section_data: &SectionData,
        fs_index: u8,
    ) -> Result<()> {
        let offsets = sect_sfb_offset(ics_info, fs_index)?;
        let num_groups = ics_info.num_window_groups as usize;
        if section_data.sections.len() != num_groups {
            return Err(Error::SpectralDataInvalid);
        }
        if self.x_quant.len() != num_groups {
            return Err(Error::SpectralDataEncodeInvalid);
        }

        for (g, group_offsets) in offsets.iter().enumerate() {
            let buf = &self.x_quant[g];
            if buf.len() != Self::group_len(ics_info, g) {
                return Err(Error::SpectralDataEncodeInvalid);
            }
            // Bands that transmit no spectrum must hold zeros:
            // everything not covered by a spectrum-carrying section.
            let mut covered = vec![false; buf.len()];
            for sec in &section_data.sections[g] {
                if section_codebook(sec)?.is_none() {
                    continue;
                }
                let start = group_offsets[sec.start as usize] as usize;
                let end = group_offsets[sec.end as usize] as usize;
                covered[start..end].fill(true);
            }
            if buf.iter().zip(covered.iter()).any(|(&v, &c)| v != 0 && !c) {
                return Err(Error::SpectralDataEncodeInvalid);
            }

            for sec in &section_data.sections[g] {
                let (cb, dim) = match section_codebook(sec)? {
                    Some(pair) => pair,
                    None => continue,
                };
                let start = group_offsets[sec.start as usize] as usize;
                let end = group_offsets[sec.end as usize] as usize;
                let mut k = start;
                while k < end {
                    if k + dim > end {
                        return Err(Error::SpectralDataInvalid);
                    }
                    write_tuple(writer, cb, dim, &buf[k..k + dim])?;
                    k += dim;
                }
            }
        }
        Ok(())
    }
}

/// Classify a section for the Table 4.56 dispatch: `Ok(None)` for
/// the codebooks that transmit no spectral data (`ZERO_HCB`,
/// `NOISE_HCB`, `INTENSITY_HCB`, `INTENSITY_HCB2`),
/// `Ok(Some((cb, dim)))` for the spectrum books 1..=11, and
/// [`Error::SpectralDataInvalid`] for the reserved codebook 12
/// (which the Table 4.56 condition does not exclude but which has
/// no Huffman table to dispatch onto).
fn section_codebook(sec: &Section) -> Result<Option<(u8, usize)>> {
    match sec.codebook_kind() {
        Codebook::Zero
        | Codebook::Noise
        | Codebook::IntensityInPhase
        | Codebook::IntensityOutOfPhase => Ok(None),
        Codebook::Quad { number, .. } => Ok(Some((number, QUAD_LEN))),
        Codebook::Pair { number, .. } => Ok(Some((number, PAIR_LEN))),
        Codebook::Esc => Ok(Some((11, PAIR_LEN))),
        Codebook::Reserved12 => Err(Error::SpectralDataInvalid),
    }
}

/// Dispatch one `hcod[cb]` codeword decode onto the per-book
/// decoder (Tables 4.A.2 … 4.A.12).
fn decode_codeword(reader: &mut BitReader<'_>, cb: u8) -> Result<u32> {
    match cb {
        1 => hcod1_decode(reader),
        2 => hcod2_decode(reader),
        3 => hcod3_decode(reader),
        4 => hcod4_decode(reader),
        5 => hcod5_decode(reader),
        6 => hcod6_decode(reader),
        7 => hcod7_decode(reader),
        8 => hcod8_decode(reader),
        9 => hcod9_decode(reader),
        10 => hcod10_decode(reader),
        11 => hcod11_decode(reader),
        _ => Err(Error::SpectralDataInvalid),
    }
}

/// Dispatch one `hcod[cb]` codeword write onto the per-book writer.
fn write_codeword(writer: &mut BitWriter, cb: u8, idx: u32) -> Result<()> {
    match cb {
        1 => hcod1_write(writer, idx),
        2 => hcod2_write(writer, idx),
        3 => hcod3_write(writer, idx),
        4 => hcod4_write(writer, idx),
        5 => hcod5_write(writer, idx),
        6 => hcod6_write(writer, idx),
        7 => hcod7_write(writer, idx),
        8 => hcod8_write(writer, idx),
        9 => hcod9_write(writer, idx),
        10 => hcod10_write(writer, idx),
        11 => hcod11_write(writer, idx),
        _ => Err(Error::SpectralDataInvalid),
    }
}

/// For unsigned codebooks, read the `quad_sign_bits` /
/// `pair_sign_bits` field (one bit per non-zero coefficient, low
/// frequency first, `1` = negative) and apply it to the magnitude
/// tuple per §4.6.3.3. Signed codebooks pass through unchanged.
fn read_and_apply_signs(
    reader: &mut BitReader<'_>,
    cb: u8,
    dim: usize,
    tuple: [i32; 4],
) -> Result<[i32; 4]> {
    let row = table_4_95(cb)?;
    if !row.is_unsigned() {
        return Ok(tuple);
    }
    let nonzero = tuple.iter().take(dim).filter(|&&v| v != 0).count();
    let mut signs = Vec::with_capacity(nonzero);
    for _ in 0..nonzero {
        signs.push(reader.read_bit().map_err(|_| Error::UnexpectedEnd)?);
    }
    apply_sign_bits(cb, tuple, &signs)
}

/// Read one `hcod_esc_y` / `hcod_esc_z` escape sequence per
/// §4.6.3.3: an `escape_prefix` of `N` ones, a zero
/// `escape_separator`, and an `(N + 4)`-bit `escape_word`, decoding
/// to `2^(N+4) + escape_word`. §4.6.1.3 caps the magnitude at
/// `MAX_QUANT`, bounding `N ≤ 8`; a prefix run past `N == 9` cannot
/// decode in range and is rejected without consuming further bits.
fn read_escape_sequence(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut prefix_len = 0u32;
    while reader.read_bit().map_err(|_| Error::UnexpectedEnd)? {
        prefix_len += 1;
        if prefix_len > 9 {
            return Err(Error::SpectralCodebookEscOutOfRange);
        }
    }
    let escape_word = reader
        .read_u32(prefix_len + 4)
        .map_err(|_| Error::UnexpectedEnd)?;
    decode_esc_value(prefix_len, escape_word)
}

/// Write one n-tuple: the Huffman codeword, the sign bits (unsigned
/// books), and the escape sequences (ESC book, magnitudes ≥ 16).
fn write_tuple(writer: &mut BitWriter, cb: u8, dim: usize, coeffs: &[i32]) -> Result<()> {
    // Build the in-band tuple: for the ESC book, magnitudes >= 16
    // are clamped to the ESC_FLAG (signed, so the sign survives for
    // derive_sign_bits); §4.6.1.3 bounds the true magnitude at
    // MAX_QUANT.
    let mut tuple = [0i32; 4];
    for (slot, &v) in tuple.iter_mut().zip(coeffs.iter()) {
        if cb == 11 && v.abs() >= ESC_FLAG {
            if v.abs() > MAX_QUANT {
                return Err(Error::SpectralCodebookEscOutOfRange);
            }
            *slot = v.signum() * ESC_FLAG;
        } else {
            *slot = v;
        }
    }

    let row = table_4_95(cb)?;
    let index_tuple: Vec<i32> = if row.is_unsigned() {
        tuple.iter().take(dim).map(|v| v.abs()).collect()
    } else {
        tuple[..dim].to_vec()
    };
    let idx = encode_tuple_to_index(cb, &index_tuple)?;
    write_codeword(writer, cb, idx)?;

    if row.is_unsigned() {
        for neg in derive_sign_bits(cb, &tuple[..dim])? {
            writer.write_bit(neg);
        }
    }

    if cb == 11 {
        for (&clamped, &v) in tuple.iter().zip(coeffs.iter()).take(dim) {
            if clamped.abs() == ESC_FLAG {
                let (prefix_len, escape_word) = encode_esc_value(v.unsigned_abs())?;
                for _ in 0..prefix_len {
                    writer.write_bit(true);
                }
                writer.write_bit(false);
                writer.write_u32(escape_word, prefix_len + 4);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::WindowSequence;
    use crate::section_data::ZERO_HCB;

    /// Build a long-window IcsInfo for fs_index 4 (44.1 kHz) with
    /// the given max_sfb.
    fn long_ics_info(max_sfb: u8) -> IcsInfo {
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::OnlyLong,
            window_shape: crate::ics_info::WindowShape::Sine,
            max_sfb,
            scale_factor_grouping: None,
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: false,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 1,
            num_window_groups: 1,
            window_group_length: vec![1],
            num_swb: crate::ics_info::NUM_SWB_LONG_WINDOW[4],
        }
    }

    /// Build an EIGHT_SHORT IcsInfo for fs_index 4 with the given
    /// grouping.
    fn short_ics_info(max_sfb: u8, window_group_length: Vec<u8>) -> IcsInfo {
        let num_window_groups = window_group_length.len() as u8;
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::EightShort,
            window_shape: crate::ics_info::WindowShape::Sine,
            max_sfb,
            scale_factor_grouping: Some(0),
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: false,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 8,
            num_window_groups,
            window_group_length,
            num_swb: crate::ics_info::NUM_SWB_SHORT_WINDOW[4],
        }
    }

    fn one_section(num_groups: usize, codebook: u8, max_sfb: u8) -> SectionData {
        let sections = (0..num_groups)
            .map(|_| {
                vec![Section {
                    codebook,
                    start: 0,
                    end: max_sfb,
                }]
            })
            .collect::<Vec<_>>();
        let sfb_cb = (0..num_groups)
            .map(|_| vec![codebook; max_sfb as usize])
            .collect::<Vec<_>>();
        SectionData { sections, sfb_cb }
    }

    fn round_trip(
        data: &SpectralData,
        ics_info: &IcsInfo,
        section_data: &SectionData,
        fs_index: u8,
    ) -> SpectralData {
        let mut writer = BitWriter::new();
        data.write(&mut writer, ics_info, section_data, fs_index)
            .expect("write");
        let bytes = writer.finish();
        let mut reader = BitReader::new(&bytes);
        SpectralData::parse(&mut reader, ics_info, section_data, fs_index).expect("parse")
    }

    #[test]
    fn sect_sfb_offset_long_mirrors_swb_table() {
        let info = long_ics_info(10);
        let offsets = sect_sfb_offset(&info, 4).expect("offsets");
        assert_eq!(offsets.len(), 1);
        let swb = long_window_offsets(4).expect("table");
        assert_eq!(offsets[0].len(), 11);
        for (i, &o) in offsets[0].iter().enumerate() {
            assert_eq!(o, u32::from(swb[i]));
        }
    }

    #[test]
    fn sect_sfb_offset_short_scales_by_group_length() {
        // Grouping 5 + 3: each virtual band is wgl × the Table
        // 4.130 band width.
        let info = short_ics_info(4, vec![5, 3]);
        let offsets = sect_sfb_offset(&info, 4).expect("offsets");
        assert_eq!(offsets.len(), 2);
        let swb = short_window_offsets(4).expect("table");
        for (g, wgl) in [(0usize, 5u32), (1, 3)] {
            for i in 0..4 {
                let width = u32::from(swb[i + 1] - swb[i]) * wgl;
                assert_eq!(offsets[g][i + 1] - offsets[g][i], width);
            }
        }
    }

    #[test]
    fn sect_sfb_offset_rejects_max_sfb_above_num_swb() {
        let mut info = long_ics_info(50);
        info.max_sfb = 50; // num_swb for fs 4 long is 49.
        assert!(matches!(
            sect_sfb_offset(&info, 4),
            Err(Error::SpectralDataInvalid)
        ));
    }

    #[test]
    fn all_zero_sections_consume_no_bits() {
        let info = long_ics_info(10);
        let sd = one_section(1, ZERO_HCB, 10);
        let mut reader = BitReader::new(&[0xff, 0xff]);
        let parsed = SpectralData::parse(&mut reader, &info, &sd, 4).expect("parse");
        assert_eq!(reader.bit_position(), 0);
        assert_eq!(parsed.x_quant.len(), 1);
        assert_eq!(parsed.x_quant[0].len(), 1024);
        assert!(parsed.x_quant[0].iter().all(|&v| v == 0));
    }

    #[test]
    fn quad_signed_book_round_trip() {
        let info = long_ics_info(2);
        let sd = one_section(1, 1, 2);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        // fs 4 long bands 0..2 cover coefficients 0..8.
        data.x_quant[0][..8].copy_from_slice(&[1, -1, 0, 1, -1, 0, 0, 1]);
        assert_eq!(round_trip(&data, &info, &sd, 4), data);
    }

    #[test]
    fn unsigned_pair_book_round_trip_with_signs() {
        let info = long_ics_info(2);
        let sd = one_section(1, 7, 2);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        data.x_quant[0][..8].copy_from_slice(&[7, -7, 0, 3, -1, 2, 0, -5]);
        assert_eq!(round_trip(&data, &info, &sd, 4), data);
    }

    #[test]
    fn esc_book_round_trip_with_escapes() {
        let info = long_ics_info(2);
        let sd = one_section(1, 11, 2);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        // In-band, half-ESC, full-ESC, extreme magnitudes.
        data.x_quant[0][..8].copy_from_slice(&[15, -15, 16, -16, 8191, -8191, 0, 100]);
        assert_eq!(round_trip(&data, &info, &sd, 4), data);
    }

    #[test]
    fn esc_magnitude_16_uses_escape_sequence_00000() {
        // §4.6.3.3 worked example: an escape_sequence of 00000
        // decodes as 16. Pin the wire layout for the tuple (16, 0):
        // index 16*17+0 = 272 → 9-bit 0x1c2, one sign bit (0), then
        // prefix-less escape 0 0000.
        let info = long_ics_info(1);
        let sd = one_section(1, 11, 1);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        data.x_quant[0][..4].copy_from_slice(&[16, 0, 0, 0]);
        let mut writer = BitWriter::new();
        data.write(&mut writer, &info, &sd, 4).expect("write");
        // Band 0 at fs 4 long spans 4 coefficients = 2 pair tuples:
        // (16, 0) then (0, 0). Codeword 0x1c2 (9 bits), sign 0,
        // escape 00000 (5 bits), then (0,0) codeword 0b0000 (4
        // bits). Total 9 + 1 + 5 + 4 = 19 bits.
        assert_eq!(writer.bit_position(), 19);
        let bytes = writer.finish();
        let mut reader = BitReader::new(&bytes);
        let parsed = SpectralData::parse(&mut reader, &info, &sd, 4).expect("parse");
        assert_eq!(reader.bit_position(), 19);
        assert_eq!(parsed, data);
    }

    #[test]
    fn short_grouped_round_trip() {
        // Two groups (5 + 3 windows); codebook 2 (signed quad) over
        // 4 virtual bands per group.
        let info = short_ics_info(4, vec![5, 3]);
        let sd = one_section(2, 2, 4);
        let offsets = sect_sfb_offset(&info, 4).expect("offsets");
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 5 * 128], vec![0i32; 3 * 128]],
        };
        for (g, group_offsets) in offsets.iter().enumerate() {
            let end = group_offsets[4] as usize;
            for k in 0..end {
                data.x_quant[g][k] = match k % 3 {
                    0 => 1,
                    1 => -1,
                    _ => 0,
                };
            }
        }
        assert_eq!(round_trip(&data, &info, &sd, 4), data);
    }

    #[test]
    fn parse_rejects_reserved_codebook_12() {
        let info = long_ics_info(2);
        let sd = one_section(1, 12, 2);
        let mut reader = BitReader::new(&[0x00; 8]);
        assert!(matches!(
            SpectralData::parse(&mut reader, &info, &sd, 4),
            Err(Error::SpectralDataInvalid)
        ));
    }

    #[test]
    fn parse_rejects_group_count_mismatch() {
        let info = long_ics_info(2);
        let sd = one_section(2, 1, 2); // two groups vs long's one
        let mut reader = BitReader::new(&[0x00; 8]);
        assert!(matches!(
            SpectralData::parse(&mut reader, &info, &sd, 4),
            Err(Error::SpectralDataInvalid)
        ));
    }

    #[test]
    fn parse_rejects_truncated_codeword() {
        let info = long_ics_info(2);
        let sd = one_section(1, 9, 2);
        // Codebook 9 max codeword is 15 bits; an all-ones byte is a
        // prefix of longer codewords, so a 1-byte buffer underflows.
        let mut reader = BitReader::new(&[0xff]);
        assert!(matches!(
            SpectralData::parse(&mut reader, &info, &sd, 4),
            Err(Error::UnexpectedEnd)
        ));
    }

    #[test]
    fn write_rejects_nonzero_outside_sections() {
        let info = long_ics_info(2);
        let sd = one_section(1, ZERO_HCB, 2);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        data.x_quant[0][0] = 1;
        let mut writer = BitWriter::new();
        assert!(matches!(
            data.write(&mut writer, &info, &sd, 4),
            Err(Error::SpectralDataEncodeInvalid)
        ));
    }

    #[test]
    fn write_rejects_nonzero_above_max_sfb() {
        let info = long_ics_info(2);
        let sd = one_section(1, 1, 2);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        data.x_quant[0][1023] = 1;
        let mut writer = BitWriter::new();
        assert!(matches!(
            data.write(&mut writer, &info, &sd, 4),
            Err(Error::SpectralDataEncodeInvalid)
        ));
    }

    #[test]
    fn write_rejects_magnitude_above_lav() {
        let info = long_ics_info(2);
        let sd = one_section(1, 1, 2); // codebook 1, LAV 1
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        data.x_quant[0][0] = 2;
        let mut writer = BitWriter::new();
        assert!(matches!(
            data.write(&mut writer, &info, &sd, 4),
            Err(Error::SpectralCodebookTupleOutOfRange(1))
        ));
    }

    #[test]
    fn write_rejects_esc_magnitude_above_max_quant() {
        let info = long_ics_info(2);
        let sd = one_section(1, 11, 2);
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        data.x_quant[0][0] = MAX_QUANT + 1;
        let mut writer = BitWriter::new();
        assert!(matches!(
            data.write(&mut writer, &info, &sd, 4),
            Err(Error::SpectralCodebookEscOutOfRange)
        ));
    }

    #[test]
    fn write_rejects_wrong_group_buffer_length() {
        let info = long_ics_info(2);
        let sd = one_section(1, 1, 2);
        let data = SpectralData {
            x_quant: vec![vec![0i32; 512]],
        };
        let mut writer = BitWriter::new();
        assert!(matches!(
            data.write(&mut writer, &info, &sd, 4),
            Err(Error::SpectralDataEncodeInvalid)
        ));
    }

    #[test]
    fn escape_prefix_run_past_9_rejected() {
        // 10 ones cannot start a conforming escape_sequence (the
        // decoded magnitude would exceed MAX_QUANT).
        let mut reader = BitReader::new(&[0xff, 0xff, 0xff]);
        assert!(matches!(
            read_escape_sequence(&mut reader),
            Err(Error::SpectralCodebookEscOutOfRange)
        ));
    }

    #[test]
    fn escape_sequence_examples_from_spec() {
        // §4.6.3.3: 00000 → 16, 01111 → 31, 1000000 → 32,
        // 1011111 → 63.
        for (bits, len, expect) in [
            (0b00000u32, 5u32, 16u32),
            (0b01111, 5, 31),
            (0b1000000, 7, 32),
            (0b1011111, 7, 63),
        ] {
            let mut writer = BitWriter::new();
            writer.write_u32(bits, len);
            let bytes = writer.finish();
            let mut reader = BitReader::new(&bytes);
            assert_eq!(read_escape_sequence(&mut reader).expect("esc"), expect);
            assert_eq!(reader.bit_position(), u64::from(len));
        }
    }

    #[test]
    fn multi_section_mixed_codebooks_round_trip() {
        // Bands 0..2 on book 1 (quad), 2..4 zero, 4..6 on book 11.
        let info = long_ics_info(6);
        let sections = vec![vec![
            Section {
                codebook: 1,
                start: 0,
                end: 2,
            },
            Section {
                codebook: ZERO_HCB,
                start: 2,
                end: 4,
            },
            Section {
                codebook: 11,
                start: 4,
                end: 6,
            },
        ]];
        let sfb_cb = vec![vec![1, 1, ZERO_HCB, ZERO_HCB, 11, 11]];
        let sd = SectionData { sections, sfb_cb };
        let mut data = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        // fs 4 long: bands are 4 wide here, so 0..8 book 1, 8..16
        // zero, 16..24 book 11.
        data.x_quant[0][..8].copy_from_slice(&[1, 0, -1, 0, 0, 1, 1, -1]);
        data.x_quant[0][16..24].copy_from_slice(&[20, -3, 0, 0, 1000, -16, 15, 0]);
        assert_eq!(round_trip(&data, &info, &sd, 4), data);
    }
}
