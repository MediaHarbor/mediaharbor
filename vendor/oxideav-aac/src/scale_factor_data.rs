//! `scale_factor_data()` parser + encoder primitive — ISO/IEC 14496-3
//! §4.4.6 / Table 4.53 (non-resilient branch) plus §4.6.3 / Table 4.A.1
//! ("Scalefactor Huffman Codebook" — codebook 12).
//!
//! `scale_factor_data()` is the third tool inside
//! `individual_channel_stream()` (after `global_gain` and
//! `section_data()`, before `pulse_data_present` /
//! `pulse_data()`). For every `(g, sfb)` whose
//! [`section_data`](crate::section_data) classifier picked a non-zero
//! codebook, this tool emits one differentially-coded value (a DPCM
//! delta in the range `-60..=+60`) using the 121-entry Table 4.A.1
//! Huffman codebook. The exception is the **first** Perceptual Noise
//! Substitution (PNS) band of the frame, whose energy delta is sent
//! as a literal 9-bit signed value — every subsequent PNS band falls
//! back to the Huffman path.
//!
//! ## Wire layout (Table 4.53, non-resilient branch)
//!
//! ```text
//! scale_factor_data() {
//!     noise_pcm_flag = 1
//!     for (g = 0; g < num_window_groups; g++) {
//!         for (sfb = 0; sfb < max_sfb; sfb++) {
//!             if (sfb_cb[g][sfb] != ZERO_HCB) {
//!                 if (is_intensity(g, sfb)) {
//!                     hcod_sf[dpcm_is_position[g][sfb]];   1..19 bits
//!                 } else if (is_noise(g, sfb)) {
//!                     if (noise_pcm_flag) {
//!                         noise_pcm_flag = 0
//!                         dpcm_noise_nrg[g][sfb];          9 bits (PCM)
//!                     } else {
//!                         hcod_sf[dpcm_noise_nrg[g][sfb]]; 1..19 bits
//!                     }
//!                 } else {
//!                     hcod_sf[dpcm_sf[g][sfb]];            1..19 bits
//!                 }
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! Three observations the parser and writer both rely on:
//!
//! 1. The outer `(g, sfb)` traversal is **driven by**
//!    [`section_data::SectionData::sfb_cb`](crate::section_data::SectionData::sfb_cb)
//!    — the parser must already know which bands carry a value before
//!    it can decide between "skip", "Huffman value", or "9-bit PCM
//!    energy". The wire stream carries no per-band header that would
//!    let it self-synchronise.
//! 2. The DPCM range is `-60..=+60` (Table 4.150). The Huffman
//!    codebook (Table 4.A.1) has 121 entries indexed `0..=120`; an
//!    `index_offset` of `-60` recovers the signed delta. The codeword
//!    for index 60 (delta 0) is the single bit `0`.
//! 3. `noise_pcm_flag` is **frame-scoped** (not group-scoped): it
//!    starts at `1` at the top of `scale_factor_data()` and clears the
//!    first time a PNS band is emitted, regardless of which window
//!    group or scalefactor band that is.
//!
//! ## What this module covers
//!
//! * [`ScaleFactorData::parse`] — read a non-resilient Table 4.53
//!   block given the surrounding `sfb_cb[g][sfb]` map. Surfaces the
//!   raw transmitted `dpcm_sf` / `dpcm_is_position` deltas and the
//!   `dpcm_noise_nrg` magnitudes verbatim.
//! * [`ScaleFactorData::write`] — the inverse: serialise a
//!   [`ScaleFactorData`] bit-for-bit. Surfaces caller-side structural
//!   bugs (delta out of range, PCM energy out of range, missing /
//!   surplus per-band entry versus the `sfb_cb` map) as
//!   [`Error::ScaleFactorDataEncodeInvalid`].
//! * [`accumulate`] — the §4.6.2.3.2 / §4.6.8.1.4 / §4.6.13 DPCM
//!   accumulator (decoder side). Runs the three independent tracks
//!   forward: spectrum scalefactors (`last_sf = global_gain`),
//!   intensity stereo positions (`last_is = 0`), and PNS noise
//!   energies (`last_nrg = global_gain - NOISE_OFFSET - 256`).
//!   Returns absolute `(sf, is_pos, noise_nrg)` per band.
//! * [`differentiate`] — the symmetric inverse (encoder side). Takes
//!   absolute per-band quantities from rate-allocation and produces
//!   the [`ScaleFactorData`] the bit-exact writer expects. Validates
//!   that every spectrum / intensity / PNS-subsequent delta fits
//!   Table 4.150's `-60..=+60`, and that the first PNS band's
//!   seed fits the 9-bit `uimsbf` Table 4.53 field.
//! * [`hcod_sf_encode`] / [`hcod_sf_decode`] — public Table 4.A.1
//!   accessors for callers (Auditor harnesses, fixture cross-checks)
//!   that need the codebook directly without going through the full
//!   `scale_factor_data()` driver.
//!
//! ## Three-track DPCM (spec ambiguity, resolved per §4.6.8 / §4.6.13)
//!
//! The §4.6.2.3.2 illustrative pseudocode declares **one** accumulator
//! `last_sf = global_gain` and lumps PNS (`NOISE_HCB`) bands into it
//! alongside spectrum bands. This pseudocode predates MPEG-4's PNS
//! feature (it is identical in 13818-7 §11.3.2 where no PNS exists)
//! and conflicts with the surrounding §4.6.8.1.4 + §4.6.13 wording,
//! which states explicitly that "differential decoding is done
//! separately between scalefactors, intensity stereo positions and
//! noise energies" with each track having its own running register
//! and its own initial-condition seed.
//!
//! This module implements the three-track interpretation:
//! intensity bands seed at `last_is = 0`, PNS bands seed at
//! `last_nrg = global_gain - NOISE_OFFSET - 256` (with the first
//! PNS band's 9-bit literal added directly to `last_nrg`), spectrum
//! bands seed at `last_sf = global_gain`. The §4.6.2.3.2 pseudocode's
//! single-track form is not used because the §4.6.8 / §4.6.13
//! prose-level requirement of independence cannot be honoured under
//! a single track that mixes spectrum and PNS deltas.
//!
//! ## What this module does *not* cover
//!
//! * The §4.4.6 error-resilient branch (`aacScalefactorDataResilienceFlag
//!   == 1` → RVLC with `rev_global_gain`, `length_of_rvlc_sf`,
//!   `sf_concealment`, `length_of_rvlc_escapes`, etc.) — the in-memory
//!   structure here is the non-resilient flavour. ER AAC-LD / scalable
//!   profiles that flip the resilience flag will need a sibling
//!   `scale_factor_data_rvlc()` module.
//! * The §4.6.2.3.3 / §4.6.8 / §4.6.13 reconstruction steps that
//!   actually *consume* the absolute values: `get_scale_factor_gain
//!   = 2^(0.25 * (sf - SF_OFFSET))`, the IS rescaling sign-flip per
//!   `ms_used`, and the PNS random-vector energy rescaling. Those
//!   are per-AOT IMDCT back-end concerns that need spectral-context
//!   state this module does not own.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::ics_info::WindowSequence;
use crate::section_data::{INTENSITY_HCB, INTENSITY_HCB2, NOISE_HCB, ZERO_HCB};
use crate::{Error, Result};

// =============================================================================
// Table 4.A.1 — Scalefactor Huffman Codebook (codebook 12)
// =============================================================================
//
// Per Table 4.150, the codebook covers indices 0..=120 with
// `index_offset = -60`, producing DPCM values in `-60..=+60`. The
// table is reproduced verbatim from ISO/IEC 14496-3 §4.A.1 / Table
// 4.A.1 with every length / codeword cross-checked against the
// 13818-7 §11.3.2 / Table 11.3 listing (the two specifications carry
// the same table for backwards bitstream compatibility).
//
// Format: `(length_in_bits, codeword_value)`. Codewords are stored
// right-aligned (the MSB of the wire codeword sits at bit
// `length - 1`), exactly as the Table 4.A.1 hexadecimal column
// presents them.

/// `index_offset` for the scalefactor codebook per Table 4.150
/// (`-60`, surfaced as a signed type because the DPCM range is
/// `-60..=+60`).
pub const SF_INDEX_OFFSET: i8 = -60;

/// `dpcm_noise_nrg` PCM seed width — Table 4.53 `dpcm_noise_nrg`
/// row (9 bits, `uimsbf` in the spec which the §4.6.13 decoder
/// re-interprets as a signed 9-bit delta).
pub const NOISE_PCM_BITS: u32 = 9;

/// Number of entries in Table 4.A.1 (`121`, indices `0..=120`).
pub const HCOD_SF_NUM_ENTRIES: usize = 121;

/// Maximum codeword length emitted by Table 4.A.1 (19 bits).
pub const HCOD_SF_MAX_LEN: u32 = 19;

/// Table 4.A.1 — `(length_in_bits, codeword)` per index `0..=120`.
///
/// Codewords are right-aligned within the `u32`. To emit one bit-for-
/// bit, write `codeword` as `length` bits MSB-first.
const HCOD_SF: [(u8, u32); HCOD_SF_NUM_ENTRIES] = [
    (18, 0x3ffe8), // 0
    (18, 0x3ffe6), // 1
    (18, 0x3ffe7), // 2
    (18, 0x3ffe5), // 3
    (19, 0x7fff5), // 4
    (19, 0x7fff1), // 5
    (19, 0x7ffed), // 6
    (19, 0x7fff6), // 7
    (19, 0x7ffee), // 8
    (19, 0x7ffef), // 9
    (19, 0x7fff0), // 10
    (19, 0x7fffc), // 11
    (19, 0x7fffd), // 12
    (19, 0x7ffff), // 13
    (19, 0x7fffe), // 14
    (19, 0x7fff7), // 15
    (19, 0x7fff8), // 16
    (19, 0x7fffb), // 17
    (19, 0x7fff9), // 18
    (18, 0x3ffe4), // 19
    (19, 0x7fffa), // 20
    (18, 0x3ffe3), // 21
    (17, 0x1ffef), // 22
    (17, 0x1fff0), // 23
    (16, 0x0fff5), // 24
    (17, 0x1ffee), // 25
    (16, 0x0fff2), // 26
    (16, 0x0fff3), // 27
    (16, 0x0fff4), // 28
    (16, 0x0fff1), // 29
    (15, 0x07ff6), // 30
    (15, 0x07ff7), // 31
    (14, 0x03ff9), // 32
    (14, 0x03ff5), // 33
    (14, 0x03ff7), // 34
    (14, 0x03ff3), // 35
    (14, 0x03ff6), // 36
    (14, 0x03ff2), // 37
    (13, 0x01ff7), // 38
    (13, 0x01ff5), // 39
    (12, 0x00ff9), // 40
    (12, 0x00ff7), // 41
    (12, 0x00ff6), // 42
    (11, 0x007f9), // 43
    (12, 0x00ff4), // 44
    (11, 0x007f8), // 45
    (10, 0x003f9), // 46
    (10, 0x003f7), // 47
    (10, 0x003f5), // 48
    (9, 0x001f8),  // 49
    (9, 0x001f7),  // 50
    (8, 0x000fa),  // 51
    (8, 0x000f8),  // 52
    (8, 0x000f6),  // 53
    (7, 0x00079),  // 54
    (6, 0x0003a),  // 55
    (6, 0x00038),  // 56
    (5, 0x0001a),  // 57
    (4, 0x0000b),  // 58
    (3, 0x00004),  // 59
    (1, 0x00000),  // 60 — delta 0, single bit `0`
    (4, 0x0000a),  // 61
    (4, 0x0000c),  // 62
    (5, 0x0001b),  // 63
    (6, 0x00039),  // 64
    (6, 0x0003b),  // 65
    (7, 0x00078),  // 66
    (7, 0x0007a),  // 67
    (8, 0x000f7),  // 68
    (8, 0x000f9),  // 69
    (9, 0x001f6),  // 70
    (9, 0x001f9),  // 71
    (10, 0x003f4), // 72
    (10, 0x003f6), // 73
    (10, 0x003f8), // 74
    (11, 0x007f5), // 75
    (11, 0x007f4), // 76
    (11, 0x007f6), // 77
    (11, 0x007f7), // 78
    (12, 0x00ff5), // 79
    (12, 0x00ff8), // 80
    (13, 0x01ff4), // 81
    (13, 0x01ff6), // 82
    (13, 0x01ff8), // 83
    (14, 0x03ff8), // 84
    (14, 0x03ff4), // 85
    (16, 0x0fff0), // 86
    (15, 0x07ff4), // 87
    (16, 0x0fff6), // 88
    (15, 0x07ff5), // 89
    (18, 0x3ffe2), // 90
    (19, 0x7ffd9), // 91
    (19, 0x7ffda), // 92
    (19, 0x7ffdb), // 93
    (19, 0x7ffdc), // 94
    (19, 0x7ffdd), // 95
    (19, 0x7ffde), // 96
    (19, 0x7ffd8), // 97
    (19, 0x7ffd2), // 98
    (19, 0x7ffd3), // 99
    (19, 0x7ffd4), // 100
    (19, 0x7ffd5), // 101
    (19, 0x7ffd6), // 102
    (19, 0x7fff2), // 103
    (19, 0x7ffdf), // 104
    (19, 0x7ffe7), // 105
    (19, 0x7ffe8), // 106
    (19, 0x7ffe9), // 107
    (19, 0x7ffea), // 108
    (19, 0x7ffeb), // 109
    (19, 0x7ffe6), // 110
    (19, 0x7ffe0), // 111
    (19, 0x7ffe1), // 112
    (19, 0x7ffe2), // 113
    (19, 0x7ffe3), // 114
    (19, 0x7ffe4), // 115
    (19, 0x7ffe5), // 116
    (19, 0x7ffd7), // 117
    (19, 0x7ffec), // 118
    (19, 0x7fff4), // 119
    (19, 0x7fff3), // 120
];

/// Encode a signed DPCM delta in `-60..=+60` to the wire Huffman
/// codeword for Table 4.A.1.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u32` (MSB at bit `length - 1`). Out-of-range `dpcm`
/// produces [`Error::ScaleFactorDataEncodeInvalid`].
///
/// The inverse of [`hcod_sf_decode`].
pub fn hcod_sf_encode(dpcm: i8) -> Result<(u8, u32)> {
    let idx = (dpcm as i32) - (SF_INDEX_OFFSET as i32);
    if !(0..HCOD_SF_NUM_ENTRIES as i32).contains(&idx) {
        return Err(Error::ScaleFactorDataEncodeInvalid);
    }
    Ok(HCOD_SF[idx as usize])
}

/// Decode one Table 4.A.1 Huffman codeword from `reader`, returning
/// the signed DPCM delta in `-60..=+60`.
///
/// The decoder is a straight prefix-match: read one bit at a time,
/// look it up in a flat table. The table is small (121 entries, max
/// length 19 bits) so a single linear scan per bit-extend is
/// sufficient and avoids the cost / complexity of a multi-level
/// lookup acceleration table. Returns [`Error::UnexpectedEnd`] on
/// reader underflow.
///
/// The codebook is a **complete** prefix code (Kraft equality:
/// `Σ 2^(19-L_i) = 2^19`), so every fully-read 19-bit sequence is
/// guaranteed to match some entry — the bottom of the loop is
/// unreachable provided `reader` produces 19 bits without
/// underflowing. A purely-defensive `unreachable!()` guards the
/// loop fall-through; it has been verified at compile-time as
/// dead code by the [`hcod_sf_decode_is_complete`](#) regression
/// test that exhaustively walks all `2^19` 19-bit prefixes.
pub fn hcod_sf_decode(reader: &mut BitReader<'_>) -> Result<i8> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD_SF_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        // Linear scan: cost is bounded by HCOD_SF_NUM_ENTRIES * 19.
        for (idx, &(entry_len, entry_cw)) in HCOD_SF.iter().enumerate() {
            if u32::from(entry_len) == len && entry_cw == acc {
                return Ok((idx as i8) + SF_INDEX_OFFSET);
            }
        }
    }
    // Unreachable: the codebook is a complete prefix code over
    // 19 bits (Kraft equality = 524288), so the inner loop must
    // hit for at least one `len <= 19`. The guard is here so the
    // compiler doesn't infer a non-`!` return path.
    unreachable!("HCOD_SF is a complete 19-bit prefix code; the 19-bit walk must match");
}

// =============================================================================
// Per-band record
// =============================================================================

/// One transmitted per-band record.
///
/// The variant is selected by [`crate::section_data::SectionData::sfb_cb`]:
/// `Dpcm` for ordinary spectrum books (1..=11, plus PNS book 13
/// after the first), `Intensity` for books 14 / 15, `NoisePcm` for
/// the **first** PNS band of the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScaleFactorEntry {
    /// `hcod_sf[dpcm_sf[g][sfb]]` — Huffman DPCM delta for a band
    /// whose codebook is a non-zero spectrum book (1..=11).
    Dpcm(i8),
    /// `hcod_sf[dpcm_is_position[g][sfb]]` — Huffman DPCM delta for
    /// an intensity-stereo band (codebook 14 or 15).
    Intensity(i8),
    /// `dpcm_noise_nrg[g][sfb]` 9-bit PCM seed — emitted **only**
    /// for the first PNS band (codebook 13) of the frame. The value
    /// is the raw 9-bit wire bits (the §4.6.13 reconstruction
    /// converts the unsigned wire pattern to a signed `-256..=+255`
    /// energy delta).
    NoisePcm(u16),
    /// `hcod_sf[dpcm_noise_nrg[g][sfb]]` — Huffman DPCM delta for a
    /// PNS band after the first.
    NoiseDpcm(i8),
}

/// Parsed `scale_factor_data()` payload (non-resilient branch).
///
/// `entries` is grouped per window group: `entries[g][i]` is the
/// `i`-th transmitted per-band record for group `g`, in wire
/// (low-frequency-first) order. The mapping back to scalefactor
/// bands is recovered by walking
/// [`SectionData::sfb_cb`](crate::section_data::SectionData::sfb_cb)
/// and skipping `ZERO_HCB` bands — the same walk the parser
/// performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScaleFactorData {
    /// `entries[g]` — the per-band records of window group `g` in
    /// wire order. `entries.len()` equals `sfb_cb.len()`
    /// (`num_window_groups`).
    pub entries: Vec<Vec<ScaleFactorEntry>>,
}

impl ScaleFactorData {
    /// Parse a non-resilient `scale_factor_data()` from `reader`.
    ///
    /// * `reader` — positioned at the first bit of the
    ///   `scale_factor_data()` block (immediately after
    ///   `section_data()`).
    /// * `sfb_cb` — the per-`(g, sfb)` codebook map produced by
    ///   [`section_data::SectionData::parse`](crate::section_data::SectionData::parse).
    ///   Outer length is `num_window_groups`; each inner slice is
    ///   `max_sfb` entries.
    ///
    /// Returns [`Error::UnexpectedEnd`] on reader underflow. The
    /// codebook is a complete 19-bit prefix code so a fully-read
    /// Huffman value is guaranteed to match an entry.
    pub fn parse(reader: &mut BitReader<'_>, sfb_cb: &[Vec<u8>]) -> Result<Self> {
        let mut noise_pcm_flag = true;
        let mut entries: Vec<Vec<ScaleFactorEntry>> = Vec::with_capacity(sfb_cb.len());
        for group in sfb_cb {
            let mut group_entries: Vec<ScaleFactorEntry> = Vec::new();
            for &cb in group {
                if cb == ZERO_HCB {
                    continue;
                }
                let entry = if is_intensity(cb) {
                    let dpcm = hcod_sf_decode(reader)?;
                    ScaleFactorEntry::Intensity(dpcm)
                } else if is_noise(cb) {
                    if noise_pcm_flag {
                        noise_pcm_flag = false;
                        let pcm = reader
                            .read_u32(NOISE_PCM_BITS)
                            .map_err(|_| Error::UnexpectedEnd)?
                            as u16;
                        ScaleFactorEntry::NoisePcm(pcm)
                    } else {
                        let dpcm = hcod_sf_decode(reader)?;
                        ScaleFactorEntry::NoiseDpcm(dpcm)
                    }
                } else {
                    let dpcm = hcod_sf_decode(reader)?;
                    ScaleFactorEntry::Dpcm(dpcm)
                };
                group_entries.push(entry);
            }
            entries.push(group_entries);
        }
        Ok(ScaleFactorData { entries })
    }

    /// Encode `scale_factor_data()` onto `writer`, the inverse of
    /// [`ScaleFactorData::parse`].
    ///
    /// * `writer` — receives the bit-exact Table 4.53 stream.
    /// * `sfb_cb` — the same codebook map the matching parse call
    ///   would receive. Drives the variant the writer expects at
    ///   each band.
    ///
    /// Returns [`Error::ScaleFactorDataEncodeInvalid`] if:
    ///
    /// * `self.entries.len()` does not equal `sfb_cb.len()`.
    /// * A group's `entries` count does not match the number of
    ///   non-zero-codebook bands in the matching `sfb_cb` group.
    /// * The variant at index `i` does not match the codebook
    ///   classification of the `i`-th non-zero band
    ///   (e.g. [`ScaleFactorEntry::Intensity`] paired with a
    ///   spectrum book, or [`ScaleFactorEntry::NoisePcm`] paired
    ///   with a non-PNS band, or — for the second PNS band onward —
    ///   [`ScaleFactorEntry::NoisePcm`] re-used after
    ///   `noise_pcm_flag` has cleared).
    /// * A `Dpcm` / `Intensity` / `NoiseDpcm` delta falls outside
    ///   `-60..=+60`.
    /// * A `NoisePcm` value exceeds the 9-bit field cap
    ///   (`> 0x1ff`).
    pub fn write(&self, writer: &mut BitWriter, sfb_cb: &[Vec<u8>]) -> Result<()> {
        if self.entries.len() != sfb_cb.len() {
            return Err(Error::ScaleFactorDataEncodeInvalid);
        }
        let mut noise_pcm_flag = true;
        for (group_entries, group_cb) in self.entries.iter().zip(sfb_cb.iter()) {
            // Walk both in lockstep: the entries list and the
            // non-zero subsequence of sfb_cb must match position-by-
            // position. Surfacing a mismatch is the same error
            // regardless of cause (length vs variant mismatch).
            let mut entry_iter = group_entries.iter();
            for &cb in group_cb {
                if cb == ZERO_HCB {
                    continue;
                }
                let entry = entry_iter
                    .next()
                    .ok_or(Error::ScaleFactorDataEncodeInvalid)?;
                match (entry, cb) {
                    (ScaleFactorEntry::Intensity(dpcm), cb) if is_intensity(cb) => {
                        let (len, cw) = hcod_sf_encode(*dpcm)?;
                        writer.write_u32(cw, u32::from(len));
                    }
                    (ScaleFactorEntry::NoisePcm(pcm), cb) if is_noise(cb) => {
                        if !noise_pcm_flag {
                            // PNS seed already consumed earlier;
                            // a second NoisePcm is wire-illegal.
                            return Err(Error::ScaleFactorDataEncodeInvalid);
                        }
                        if u32::from(*pcm) >= (1u32 << NOISE_PCM_BITS) {
                            return Err(Error::ScaleFactorDataEncodeInvalid);
                        }
                        noise_pcm_flag = false;
                        writer.write_u32(u32::from(*pcm), NOISE_PCM_BITS);
                    }
                    (ScaleFactorEntry::NoiseDpcm(dpcm), cb) if is_noise(cb) => {
                        if noise_pcm_flag {
                            // First PNS band of the frame must use
                            // the 9-bit PCM seed, not the Huffman
                            // delta — caller skipped the seed.
                            return Err(Error::ScaleFactorDataEncodeInvalid);
                        }
                        let (len, cw) = hcod_sf_encode(*dpcm)?;
                        writer.write_u32(cw, u32::from(len));
                    }
                    (ScaleFactorEntry::Dpcm(dpcm), cb) if !is_intensity(cb) && !is_noise(cb) => {
                        let (len, cw) = hcod_sf_encode(*dpcm)?;
                        writer.write_u32(cw, u32::from(len));
                    }
                    _ => return Err(Error::ScaleFactorDataEncodeInvalid),
                }
            }
            // Extra entries beyond the non-zero codebook subsequence
            // would silently shift the wire layout — reject.
            if entry_iter.next().is_some() {
                return Err(Error::ScaleFactorDataEncodeInvalid);
            }
        }
        Ok(())
    }
}

// =============================================================================
// §4.6.2.3.2 / §4.6.8.1.4 / §4.6.13 DPCM accumulators
// =============================================================================
//
// `scale_factor_data()` transmits *differential* values. Recovering the
// absolute per-band quantities the per-AOT IMDCT / intensity-stereo /
// PNS back-ends consume requires accumulating the DPCM deltas against
// initial-condition seeds. There are **three** independent tracks:
//
// 1. **Spectrum scalefactors** (codebooks 1..=11): per ISO/IEC 14496-3
//    §4.6.2.3.2 / ISO/IEC 13818-7 §11.3.2, accumulator initial value
//    `last_sf = global_gain`; per-band `sf[g][sfb] = dpcm_sf +
//    last_sf; last_sf = sf[g][sfb]`. Range `0..=255` (clause note;
//    the 13818-7 wording matches).
//
// 2. **Intensity stereo positions** (codebooks 14, 15): per
//    §4.6.8.1.4, initial `last_is = 0`; per-band `is_pos[g][sfb] =
//    dpcm_is_position + last_is; last_is = is_pos[g][sfb]`. The
//    §4.6.8.1.4 text is explicit that intensity-position differential
//    decoding is "done separately" from the scalefactor track, with
//    the seed starting at zero rather than `global_gain`.
//
// 3. **PNS noise energies** (codebook 13): per §4.6.13, initial
//    `last_nrg = global_gain - NOISE_OFFSET - 256` (`NOISE_OFFSET ==
//    90`); the first PNS band carries a 9-bit `uimsbf` literal
//    `dpcm_noise_nrg` (added to `last_nrg` directly), each
//    subsequent PNS band carries a Huffman delta in `-60..=+60`.
//    Per-band `noise_nrg[g][sfb] = dpcm_noise_nrg + last_nrg;
//    last_nrg = noise_nrg[g][sfb]`. The §4.6.13 text is explicit
//    that PNS energies are "done separately" from both other tracks.
//
// The three-track presentation in §4.6.8 / §4.6.13 takes precedence
// over the §4.6.2.3.2 illustrative pseudocode (which predates PNS
// in 13818-7 and conflates the spectrum + PNS tracks under a single
// `last_sf` register). The "done separately" wording in §4.6.8.1.4
// and §4.6.13 is unambiguous; this crate honours it.
//
// `accumulate(sfd, sfb_cb, global_gain)` runs all three tracks
// forward (decoder side) to recover absolute `(sf, is_pos,
// noise_nrg)`. `differentiate(abs, sfb_cb, global_gain)` is its
// inverse (encoder side, fed by the rate-allocation stage's
// absolute-value output).

/// `NOISE_OFFSET` per §4.6.13 — added to the PNS energy seed to
/// position the running `last_nrg` register relative to
/// `global_gain`.
pub const NOISE_OFFSET: i32 = 90;

/// One absolute per-band record, the result of running the §4.6.2.3.2
/// / §4.6.8.1.4 / §4.6.13 DPCM accumulators forward over a
/// [`ScaleFactorData`] together with `global_gain`.
///
/// The variant matches the [`ScaleFactorEntry`] variant of the
/// corresponding transmitted record but carries the absolute value
/// the per-AOT back-end consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsoluteScaleFactorEntry {
    /// Absolute spectrum-band scalefactor `sf[g][sfb] ∈ 0..=255` —
    /// the gain applied to the spectral coefficients of this
    /// scalefactor band per §4.6.2.3.3.
    Sf(u8),
    /// Absolute intensity stereo position `is_pos[g][sfb] ∈
    /// -60..=+60` accumulated — the value the §4.6.8.2 IS decoder
    /// consumes. The track seeds at 0 and accumulates `-60..=+60`
    /// deltas, so the absolute value's reachable range is in
    /// principle unbounded; conforming streams keep it within the
    /// signed 8-bit window.
    IsPos(i16),
    /// Absolute noise energy `noise_nrg[g][sfb]` — the value the
    /// §4.6.13 noise-substitution back-end consumes. Tracked as
    /// `i32` because the seed is `global_gain - NOISE_OFFSET - 256`
    /// (which can be negative for small `global_gain`) and the
    /// running accumulator may dip negative before the first PNS
    /// band lands a positive 9-bit delta.
    NoiseNrg(i32),
}

/// Absolute per-band quantities recovered by running the §4.6.2.3.2
/// / §4.6.8.1.4 / §4.6.13 DPCM accumulators forward over a
/// [`ScaleFactorData`].
///
/// Outer length equals `sfb_cb.len()` (`num_window_groups`); inner
/// `entries[g]` length matches the `entries[g]` of the source
/// [`ScaleFactorData`] (the non-`ZERO_HCB` band count of the
/// matching `sfb_cb[g]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsoluteScaleFactors {
    /// `entries[g]` — the per-band absolute records of window group
    /// `g` in wire (low-frequency-first) order. Variant order
    /// follows the per-band codebook classification in the matching
    /// `sfb_cb[g]`, skipping `ZERO_HCB` bands.
    pub entries: Vec<Vec<AbsoluteScaleFactorEntry>>,
}

/// Run the §4.6.2.3.2 / §4.6.8.1.4 / §4.6.13 DPCM accumulators
/// forward over `sfd` to recover absolute scalefactors, intensity
/// stereo positions, and PNS noise energies (decoder side).
///
/// * `sfd` — the transmitted DPCM record set returned by
///   [`ScaleFactorData::parse`].
/// * `sfb_cb` — the per-`(g, sfb)` codebook map produced by
///   [`crate::section_data::SectionData::parse`].
/// * `global_gain` — the 8-bit `global_gain` element transmitted
///   immediately before `section_data()` in
///   `individual_channel_stream()`.
///
/// Returns [`Error::ScaleFactorAccumulatorInvalid`] if the
/// per-group entry layout in `sfd` does not match the non-`ZERO_HCB`
/// codebook classification of the matching `sfb_cb` group, or if a
/// Sf-track running value escapes the `0..=255` spec range (Note
/// after §4.6.2.3.2 pseudocode).
pub fn accumulate(
    sfd: &ScaleFactorData,
    sfb_cb: &[Vec<u8>],
    global_gain: u8,
) -> Result<AbsoluteScaleFactors> {
    if sfd.entries.len() != sfb_cb.len() {
        return Err(Error::ScaleFactorAccumulatorInvalid);
    }
    let mut last_sf: i32 = i32::from(global_gain);
    let mut last_is: i32 = 0;
    let mut last_nrg: i32 = i32::from(global_gain) - NOISE_OFFSET - 256;
    let mut noise_pcm_flag = true;
    let mut out: Vec<Vec<AbsoluteScaleFactorEntry>> = Vec::with_capacity(sfb_cb.len());
    for (group_entries, group_cb) in sfd.entries.iter().zip(sfb_cb.iter()) {
        let mut entry_iter = group_entries.iter();
        let mut group_out: Vec<AbsoluteScaleFactorEntry> = Vec::new();
        for &cb in group_cb {
            if cb == ZERO_HCB {
                continue;
            }
            let entry = entry_iter
                .next()
                .ok_or(Error::ScaleFactorAccumulatorInvalid)?;
            let abs_entry = match (entry, cb) {
                (ScaleFactorEntry::Intensity(dpcm), cb) if is_intensity(cb) => {
                    last_is += i32::from(*dpcm);
                    AbsoluteScaleFactorEntry::IsPos(last_is as i16)
                }
                (ScaleFactorEntry::NoisePcm(pcm), cb) if is_noise(cb) => {
                    if !noise_pcm_flag {
                        return Err(Error::ScaleFactorAccumulatorInvalid);
                    }
                    noise_pcm_flag = false;
                    last_nrg += i32::from(*pcm);
                    AbsoluteScaleFactorEntry::NoiseNrg(last_nrg)
                }
                (ScaleFactorEntry::NoiseDpcm(dpcm), cb) if is_noise(cb) => {
                    if noise_pcm_flag {
                        return Err(Error::ScaleFactorAccumulatorInvalid);
                    }
                    last_nrg += i32::from(*dpcm);
                    AbsoluteScaleFactorEntry::NoiseNrg(last_nrg)
                }
                (ScaleFactorEntry::Dpcm(dpcm), cb) if !is_intensity(cb) && !is_noise(cb) => {
                    last_sf += i32::from(*dpcm);
                    if !(0..=255).contains(&last_sf) {
                        return Err(Error::ScaleFactorAccumulatorInvalid);
                    }
                    AbsoluteScaleFactorEntry::Sf(last_sf as u8)
                }
                _ => return Err(Error::ScaleFactorAccumulatorInvalid),
            };
            group_out.push(abs_entry);
        }
        if entry_iter.next().is_some() {
            return Err(Error::ScaleFactorAccumulatorInvalid);
        }
        out.push(group_out);
    }
    Ok(AbsoluteScaleFactors { entries: out })
}

/// Run the §4.6.2.3.2 / §4.6.8.1.4 / §4.6.13 DPCM accumulators
/// backward (encoder side): convert absolute per-band quantities
/// produced by rate-allocation into the transmitted DPCM record set
/// the bit-exact `scale_factor_data()` writer expects.
///
/// This is the symmetric inverse of [`accumulate`]:
/// `accumulate(differentiate(abs, sfb_cb, gg)?, sfb_cb, gg) == abs`
/// on every well-formed input.
///
/// * `abs` — the absolute per-band records from rate-allocation
///   (`Sf` for spectrum bands, `IsPos` for intensity bands,
///   `NoiseNrg` for PNS bands).
/// * `sfb_cb` — per-band codebook map from `section_data()`.
/// * `global_gain` — the 8-bit element the wire stream carries
///   immediately before `section_data()` (a free parameter the
///   encoder picks; conforming choice is the first spectrum band's
///   absolute `sf` to make the first delta `0`).
///
/// Returns [`Error::ScaleFactorAccumulatorInvalid`] if outer / inner
/// shape disagrees with `sfb_cb`, if an entry variant does not match
/// its band's codebook, if a spectrum / intensity / PNS-subsequent
/// delta `cur - prev` falls outside Table 4.150's `-60..=+60`, or
/// if the first PNS band's initial `dpcm_noise_nrg` magnitude does
/// not fit the 9-bit `uimsbf` Table 4.53 field (`0..=511`).
pub fn differentiate(
    abs: &AbsoluteScaleFactors,
    sfb_cb: &[Vec<u8>],
    global_gain: u8,
) -> Result<ScaleFactorData> {
    if abs.entries.len() != sfb_cb.len() {
        return Err(Error::ScaleFactorAccumulatorInvalid);
    }
    let mut last_sf: i32 = i32::from(global_gain);
    let mut last_is: i32 = 0;
    let mut last_nrg: i32 = i32::from(global_gain) - NOISE_OFFSET - 256;
    let mut noise_pcm_flag = true;
    let mut out: Vec<Vec<ScaleFactorEntry>> = Vec::with_capacity(sfb_cb.len());
    for (group_abs, group_cb) in abs.entries.iter().zip(sfb_cb.iter()) {
        let mut abs_iter = group_abs.iter();
        let mut group_out: Vec<ScaleFactorEntry> = Vec::new();
        for &cb in group_cb {
            if cb == ZERO_HCB {
                continue;
            }
            let abs_entry = abs_iter
                .next()
                .ok_or(Error::ScaleFactorAccumulatorInvalid)?;
            let entry = match (abs_entry, cb) {
                (AbsoluteScaleFactorEntry::IsPos(cur), cb) if is_intensity(cb) => {
                    let delta = i32::from(*cur) - last_is;
                    if !(-60..=60).contains(&delta) {
                        return Err(Error::ScaleFactorAccumulatorInvalid);
                    }
                    last_is = i32::from(*cur);
                    ScaleFactorEntry::Intensity(delta as i8)
                }
                (AbsoluteScaleFactorEntry::NoiseNrg(cur), cb) if is_noise(cb) => {
                    if noise_pcm_flag {
                        let delta = *cur - last_nrg;
                        if !(0..=511).contains(&delta) {
                            return Err(Error::ScaleFactorAccumulatorInvalid);
                        }
                        noise_pcm_flag = false;
                        last_nrg = *cur;
                        ScaleFactorEntry::NoisePcm(delta as u16)
                    } else {
                        let delta = *cur - last_nrg;
                        if !(-60..=60).contains(&delta) {
                            return Err(Error::ScaleFactorAccumulatorInvalid);
                        }
                        last_nrg = *cur;
                        ScaleFactorEntry::NoiseDpcm(delta as i8)
                    }
                }
                (AbsoluteScaleFactorEntry::Sf(cur), cb) if !is_intensity(cb) && !is_noise(cb) => {
                    let delta = i32::from(*cur) - last_sf;
                    if !(-60..=60).contains(&delta) {
                        return Err(Error::ScaleFactorAccumulatorInvalid);
                    }
                    last_sf = i32::from(*cur);
                    ScaleFactorEntry::Dpcm(delta as i8)
                }
                _ => return Err(Error::ScaleFactorAccumulatorInvalid),
            };
            group_out.push(entry);
        }
        if abs_iter.next().is_some() {
            return Err(Error::ScaleFactorAccumulatorInvalid);
        }
        out.push(group_out);
    }
    Ok(ScaleFactorData { entries: out })
}

// =============================================================================
// Error-resilient `scale_factor_data()` — Table 4.53 RVLC branch (§4.6.16.2)
// =============================================================================
//
// When the GASpecificConfig sets `aacScalefactorDataResilienceFlag == 1`,
// `scale_factor_data()` takes the RVLC branch: the Table 4.A.1 Huffman
// codebook is replaced by the Table 4.166 symmetric RVLC codebook (see
// [`crate::rvlc`]) and three extra wire fields wrap the band loop so a
// decoder can recover from bit errors by decoding backwards:
//
//   * `sf_concealment` (1 bit) — concealment hint, decode-irrelevant
//     for an error-free stream (§4.6.16.2.2).
//   * `rev_global_gain` (8 bits) — the *last* scalefactor, the start
//     value for backward DPCM decoding.
//   * `length_of_rvlc_sf` (11 bits if `EIGHT_SHORT_SEQUENCE` else 9) —
//     the bit length of the RVLC part (the band loop + the optional
//     `dpcm_is_last_position`), used to seek to the backward start.
//   * `sf_escapes_present` (1 bit) + `length_of_rvlc_escapes` (8 bits)
//     — the optional escape sub-stream, present iff any band's RVLC
//     delta reached the ESC_FLAG (`±7`).
//   * `dpcm_is_last_position` (RVLC, present iff intensity used) — the
//     symmetric backward seed for the intensity-position track.
//   * `dpcm_noise_last_position` (9 bits, present iff PNS used) — the
//     symmetric backward seed for the PNS-energy track.
//
// Forward decoding is the focus here. Per §4.6.2.3.2, "the decoding
// process of the RVLC words is the same as for the Huffman
// codewords" — so once the RVLC deltas are recovered (and folded with
// their escapes), the *same* [`accumulate`] three-track DPCM forward
// pass reconstructs the absolute scalefactors. The `rev_global_gain`
// / `dpcm_*_last_position` seeds and the `length_of_*` fields are the
// backward-recovery scaffolding; this module surfaces them verbatim
// (and validates the two length fields against the bits actually
// consumed, an in-band conformance check) so a future recovery path
// can use them, but forward decode keys off `global_gain` exactly as
// the non-resilient branch does.
//
// Escape folding (§4.6.16.2.1): a base RVLC delta of `+7` means the
// true delta is `+7 + esc`; a base delta of `-7` means `-7 - esc`,
// where `esc` is the Table 4.168 escape magnitude. The escapes are a
// *separate pass* over the same band walk, after the whole RVLC part.

/// A parsed error-resilient `scale_factor_data()` block — Table 4.53
/// RVLC branch (`aacScalefactorDataResilienceFlag == 1`).
///
/// `data` carries the *reconstructed* per-band DPCM records (RVLC base
/// delta with any escape already folded in), so it feeds [`accumulate`]
/// unchanged. The remaining fields are the §4.6.16.2 backward-decoding
/// scaffolding, surfaced verbatim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErScaleFactorData {
    /// `sf_concealment` (1 bit) — concealment hint; not needed to
    /// decode an error-free stream.
    pub sf_concealment: bool,
    /// `rev_global_gain` (8 bits) — last scalefactor, the backward
    /// DPCM start value.
    pub rev_global_gain: u8,
    /// The reconstructed per-band records (escapes folded in),
    /// identical in shape to the non-resilient
    /// [`ScaleFactorData`] so [`accumulate`] consumes it directly.
    pub data: ScaleFactorData,
    /// `dpcm_is_last_position` — backward seed for the intensity
    /// track. `Some` iff at least one intensity band was present.
    pub dpcm_is_last_position: Option<i16>,
    /// `dpcm_noise_last_position` (9-bit `uimsbf`) — backward seed
    /// for the PNS track. `Some` iff at least one PNS band was
    /// present.
    pub dpcm_noise_last_position: Option<u16>,
}

/// `length_of_rvlc_sf` field width — 11 bits for
/// `EIGHT_SHORT_SEQUENCE`, 9 bits otherwise (§4.6.16.2.2).
fn length_of_rvlc_sf_bits(window_sequence: WindowSequence) -> u32 {
    if window_sequence.is_eight_short() {
        11
    } else {
        9
    }
}

/// `length_of_rvlc_escapes` field width — always 8 bits
/// (§4.6.16.2.2).
const LENGTH_OF_RVLC_ESCAPES_BITS: u32 = 8;

/// Fold a Table 4.168 escape magnitude into a base RVLC `±ESC_FLAG`
/// delta (§4.6.16.2.1): a positive base recovers `+7 + esc`, a
/// negative base recovers `-7 - esc`. Both extremes stay within the
/// `-60..=+60` DPCM range, so the result fits `i8`.
fn fold_escape(base: i8, esc_magnitude: u8) -> i8 {
    if base >= 0 {
        crate::rvlc::RVLC_ESC_FLAG + esc_magnitude as i8
    } else {
        -crate::rvlc::RVLC_ESC_FLAG - esc_magnitude as i8
    }
}

impl ErScaleFactorData {
    /// Parse an error-resilient `scale_factor_data()` from `reader`
    /// (Table 4.53, RVLC branch).
    ///
    /// * `reader` — positioned at the first bit of the block (the
    ///   `sf_concealment` flag).
    /// * `sfb_cb` — the per-`(g, sfb)` codebook map from
    ///   [`section_data`](crate::section_data).
    /// * `window_sequence` — selects the `length_of_rvlc_sf` field
    ///   width (11 vs 9 bits).
    ///
    /// The two `length_of_*` fields are validated against the bits
    /// actually consumed; a mismatch surfaces
    /// [`Error::RvlcScaleFactorDataInvalid`] (an in-band conformance
    /// check). A forbidden RVLC codeword surfaces
    /// [`Error::RvlcForbiddenCodeword`]; reader underflow surfaces
    /// [`Error::UnexpectedEnd`].
    pub fn parse(
        reader: &mut BitReader<'_>,
        sfb_cb: &[Vec<u8>],
        window_sequence: WindowSequence,
    ) -> Result<Self> {
        let sf_concealment = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)? != 0;
        let rev_global_gain = reader.read_u32(8).map_err(|_| Error::UnexpectedEnd)? as u8;
        let len_rvlc_sf = u64::from(
            reader
                .read_u32(length_of_rvlc_sf_bits(window_sequence))
                .map_err(|_| Error::UnexpectedEnd)?,
        );

        // ---- RVLC part (Table 4.53): base deltas + ESC bookkeeping.
        let rvlc_start = reader.bit_position();
        let mut intensity_used = false;
        let mut noise_used = false;
        // base[g] mirrors entries[g]; esc_band flags which records
        // need an escape fold in the second pass.
        let mut base: Vec<Vec<ScaleFactorEntry>> = Vec::with_capacity(sfb_cb.len());
        // `(group, index_within_group)` of each record that is at
        // ESC_FLAG and must read an escape (in band-walk order). The
        // first-PNS-PCM record is never escaped.
        let mut esc_records: Vec<(usize, usize)> = Vec::new();
        for (g, group) in sfb_cb.iter().enumerate() {
            let mut group_entries: Vec<ScaleFactorEntry> = Vec::new();
            for &cb in group {
                if cb == ZERO_HCB {
                    continue;
                }
                let idx_in_group = group_entries.len();
                let entry = if is_intensity(cb) {
                    intensity_used = true;
                    let d = crate::rvlc::rvlc_decode(reader)?;
                    if d.abs() == crate::rvlc::RVLC_ESC_FLAG {
                        esc_records.push((g, idx_in_group));
                    }
                    ScaleFactorEntry::Intensity(d)
                } else if is_noise(cb) {
                    if !noise_used {
                        noise_used = true;
                        let pcm = reader
                            .read_u32(NOISE_PCM_BITS)
                            .map_err(|_| Error::UnexpectedEnd)?
                            as u16;
                        ScaleFactorEntry::NoisePcm(pcm)
                    } else {
                        let d = crate::rvlc::rvlc_decode(reader)?;
                        if d.abs() == crate::rvlc::RVLC_ESC_FLAG {
                            esc_records.push((g, idx_in_group));
                        }
                        ScaleFactorEntry::NoiseDpcm(d)
                    }
                } else {
                    let d = crate::rvlc::rvlc_decode(reader)?;
                    if d.abs() == crate::rvlc::RVLC_ESC_FLAG {
                        esc_records.push((g, idx_in_group));
                    }
                    ScaleFactorEntry::Dpcm(d)
                };
                group_entries.push(entry);
            }
            base.push(group_entries);
        }

        // `dpcm_is_last_position` (RVLC) closes the RVLC part if any
        // intensity band was present.
        let mut is_last_base: Option<i8> = None;
        if intensity_used {
            let d = crate::rvlc::rvlc_decode(reader)?;
            is_last_base = Some(d);
        }

        // The RVLC part length must match the transmitted field.
        let rvlc_consumed = reader.bit_position() - rvlc_start;
        if rvlc_consumed != len_rvlc_sf {
            return Err(Error::RvlcScaleFactorDataInvalid);
        }

        // ---- Escape part (Table 4.53): optional second pass.
        let sf_escapes_present = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)? != 0;
        let mut is_last_esc: Option<u8> = None;
        if sf_escapes_present {
            let len_escapes = u64::from(
                reader
                    .read_u32(LENGTH_OF_RVLC_ESCAPES_BITS)
                    .map_err(|_| Error::UnexpectedEnd)?,
            );
            let esc_start = reader.bit_position();
            // Read one escape per recorded ESC_FLAG band, in walk order,
            // and fold it into the base delta.
            for &(g, i) in &esc_records {
                let mag = crate::rvlc::rvlc_esc_decode(reader)?;
                let folded = match base[g][i] {
                    ScaleFactorEntry::Dpcm(d) => ScaleFactorEntry::Dpcm(fold_escape(d, mag)),
                    ScaleFactorEntry::Intensity(d) => {
                        ScaleFactorEntry::Intensity(fold_escape(d, mag))
                    }
                    ScaleFactorEntry::NoiseDpcm(d) => {
                        ScaleFactorEntry::NoiseDpcm(fold_escape(d, mag))
                    }
                    // A NoisePcm record is never recorded as an escape.
                    ScaleFactorEntry::NoisePcm(_) => {
                        return Err(Error::RvlcScaleFactorDataInvalid);
                    }
                };
                base[g][i] = folded;
            }
            // `dpcm_is_last_position` escape closes the escape part.
            if let Some(d) = is_last_base {
                if d.abs() == crate::rvlc::RVLC_ESC_FLAG {
                    let mag = crate::rvlc::rvlc_esc_decode(reader)?;
                    is_last_esc = Some(mag);
                }
            }
            let esc_consumed = reader.bit_position() - esc_start;
            if esc_consumed != len_escapes {
                return Err(Error::RvlcScaleFactorDataInvalid);
            }
        }

        // ---- PNS backward seed.
        //
        // Table 4.53 resets `noise_used = 0` immediately before
        // `sf_escapes_present` and re-derives it inside the escape
        // loop's `if (!noise_used)` arm. The terminal
        // `if (noise_used) dpcm_noise_last_position` therefore fires
        // *only* when both a PNS band is present **and**
        // `sf_escapes_present == 1` (the escape loop — and its
        // `noise_used = 1` — is wholly inside `if (sf_escapes_present)`).
        // A PNS frame with no escapes carries no `dpcm_noise_last_position`.
        let dpcm_noise_last_position = if noise_used && sf_escapes_present {
            Some(
                reader
                    .read_u32(NOISE_PCM_BITS)
                    .map_err(|_| Error::UnexpectedEnd)? as u16,
            )
        } else {
            None
        };

        // Fold the intensity-last backward seed (with its escape).
        let dpcm_is_last_position = is_last_base.map(|d| {
            let folded = match is_last_esc {
                Some(mag) => fold_escape(d, mag),
                None => d,
            };
            i16::from(folded)
        });

        Ok(ErScaleFactorData {
            sf_concealment,
            rev_global_gain,
            data: ScaleFactorData { entries: base },
            dpcm_is_last_position,
            dpcm_noise_last_position,
        })
    }

    /// Encode an error-resilient `scale_factor_data()` onto `writer`,
    /// the inverse of [`ErScaleFactorData::parse`].
    ///
    /// The records in `self.data` carry the *final* DPCM deltas
    /// (escapes already folded). The writer re-splits each delta whose
    /// magnitude exceeds `±6` into a base `±7` RVLC codeword plus a
    /// Table 4.168 escape magnitude, regenerates the `length_of_*`
    /// fields from the bits emitted, and sets `sf_escapes_present`
    /// when any escape is needed.
    ///
    /// Returns [`Error::RvlcScaleFactorDataInvalid`] on a structural
    /// mismatch (record/codebook shape, escape magnitude out of the
    /// Table 4.168 domain, or a backward-seed field overflow).
    pub fn write(
        &self,
        writer: &mut BitWriter,
        sfb_cb: &[Vec<u8>],
        window_sequence: WindowSequence,
    ) -> Result<()> {
        if self.data.entries.len() != sfb_cb.len() {
            return Err(Error::RvlcScaleFactorDataInvalid);
        }

        writer.write_u32(u32::from(self.sf_concealment), 1);
        writer.write_u32(u32::from(self.rev_global_gain), 8);

        // Build the RVLC part into a scratch writer first so its bit
        // length is known for `length_of_rvlc_sf`. Collect the escape
        // magnitudes (in walk order) for the second pass.
        let mut rvlc_part = BitWriter::new();
        let mut escapes: Vec<u8> = Vec::new();
        let mut intensity_used = false;
        let mut noise_used = false;

        for (group_entries, group_cb) in self.data.entries.iter().zip(sfb_cb.iter()) {
            let mut entry_iter = group_entries.iter();
            for &cb in group_cb {
                if cb == ZERO_HCB {
                    continue;
                }
                let entry = entry_iter.next().ok_or(Error::RvlcScaleFactorDataInvalid)?;
                match (entry, cb) {
                    (ScaleFactorEntry::Intensity(d), cb) if is_intensity(cb) => {
                        intensity_used = true;
                        write_rvlc_delta(&mut rvlc_part, *d, &mut escapes)?;
                    }
                    (ScaleFactorEntry::NoisePcm(pcm), cb) if is_noise(cb) => {
                        if noise_used {
                            return Err(Error::RvlcScaleFactorDataInvalid);
                        }
                        if u32::from(*pcm) >= (1u32 << NOISE_PCM_BITS) {
                            return Err(Error::RvlcScaleFactorDataInvalid);
                        }
                        noise_used = true;
                        rvlc_part.write_u32(u32::from(*pcm), NOISE_PCM_BITS);
                    }
                    (ScaleFactorEntry::NoiseDpcm(d), cb) if is_noise(cb) => {
                        if !noise_used {
                            return Err(Error::RvlcScaleFactorDataInvalid);
                        }
                        write_rvlc_delta(&mut rvlc_part, *d, &mut escapes)?;
                    }
                    (ScaleFactorEntry::Dpcm(d), cb) if !is_intensity(cb) && !is_noise(cb) => {
                        write_rvlc_delta(&mut rvlc_part, *d, &mut escapes)?;
                    }
                    _ => return Err(Error::RvlcScaleFactorDataInvalid),
                }
            }
            if entry_iter.next().is_some() {
                return Err(Error::RvlcScaleFactorDataInvalid);
            }
        }

        // `dpcm_is_last_position` (RVLC) closes the RVLC part.
        let mut is_last_escape: Option<u8> = None;
        match (intensity_used, self.dpcm_is_last_position) {
            (true, Some(d)) => {
                let d8 = i8::try_from(d).map_err(|_| Error::RvlcScaleFactorDataInvalid)?;
                let mut tail: Vec<u8> = Vec::new();
                write_rvlc_delta(&mut rvlc_part, d8, &mut tail)?;
                is_last_escape = tail.into_iter().next();
            }
            (true, None) | (false, Some(_)) => {
                // Intensity presence must agree with the seed presence.
                return Err(Error::RvlcScaleFactorDataInvalid);
            }
            (false, None) => {}
        }

        let len_rvlc_sf = rvlc_part.bit_position();
        let field_bits = length_of_rvlc_sf_bits(window_sequence);
        if len_rvlc_sf >= (1u64 << field_bits) {
            return Err(Error::RvlcScaleFactorDataInvalid);
        }
        writer.write_u32(len_rvlc_sf as u32, field_bits);
        append_bits(writer, len_rvlc_sf, &rvlc_part.finish());

        // ---- Escape part.
        let any_escape = !escapes.is_empty() || is_last_escape.is_some();
        writer.write_u32(u32::from(any_escape), 1);
        if any_escape {
            let mut esc_part = BitWriter::new();
            for &mag in &escapes {
                let (len, cw) = crate::rvlc::rvlc_esc_encode(mag)?;
                esc_part.write_u32(cw, u32::from(len));
            }
            if let Some(mag) = is_last_escape {
                let (len, cw) = crate::rvlc::rvlc_esc_encode(mag)?;
                esc_part.write_u32(cw, u32::from(len));
            }
            let len_escapes = esc_part.bit_position();
            if len_escapes >= (1u64 << LENGTH_OF_RVLC_ESCAPES_BITS) {
                return Err(Error::RvlcScaleFactorDataInvalid);
            }
            writer.write_u32(len_escapes as u32, LENGTH_OF_RVLC_ESCAPES_BITS);
            append_bits(writer, len_escapes, &esc_part.finish());
        }

        // ---- PNS backward seed.
        //
        // Per Table 4.53 the terminal `dpcm_noise_last_position` is
        // present only when a PNS band exists **and**
        // `sf_escapes_present == 1` (the spec re-derives `noise_used`
        // inside the escape loop, which only runs when escapes are
        // present). So the seed must be `Some` exactly when
        // `noise_used && any_escape`, and `None` otherwise — any other
        // combination cannot be represented on the wire.
        let expect_noise_seed = noise_used && any_escape;
        match (expect_noise_seed, self.dpcm_noise_last_position) {
            (true, Some(pcm)) => {
                if u32::from(pcm) >= (1u32 << NOISE_PCM_BITS) {
                    return Err(Error::RvlcScaleFactorDataInvalid);
                }
                writer.write_u32(u32::from(pcm), NOISE_PCM_BITS);
            }
            (false, None) => {}
            _ => return Err(Error::RvlcScaleFactorDataInvalid),
        }
        Ok(())
    }
}

/// Split a final DPCM delta into an RVLC base codeword plus (if the
/// magnitude exceeds `±6`) an escape magnitude pushed onto `escapes`.
/// The base RVLC codeword is emitted onto `part`.
fn write_rvlc_delta(part: &mut BitWriter, delta: i8, escapes: &mut Vec<u8>) -> Result<()> {
    let flag = crate::rvlc::RVLC_ESC_FLAG; // 7
    if delta.abs() < flag {
        // Fits the RVLC codebook directly (no escape, magnitude ≤ 6).
        let (len, cw) = crate::rvlc::rvlc_encode(delta)?;
        part.write_u32(cw, u32::from(len));
    } else {
        // Magnitude ≥ 7: base codeword is the signed ESC_FLAG, the
        // remainder is the escape magnitude.
        let (base, mag) = if delta >= 0 {
            (flag, (delta - flag) as u8)
        } else {
            (-flag, (-delta - flag) as u8)
        };
        let (len, cw) = crate::rvlc::rvlc_encode(base)?;
        part.write_u32(cw, u32::from(len));
        if mag as usize >= crate::rvlc::RVLC_ESC_NUM_ENTRIES {
            return Err(Error::RvlcScaleFactorDataInvalid);
        }
        escapes.push(mag);
    }
    Ok(())
}

/// Append the first `total` bits of `bytes` (a zero-padded
/// `BitWriter::finish()` output) to `dst`, MSB-first, preserving bit
/// position. The trailing zero pad bits past `total` are ignored.
fn append_bits(dst: &mut BitWriter, total: u64, bytes: &[u8]) {
    let mut remaining = total;
    let mut byte_idx = 0usize;
    while remaining >= 8 {
        dst.write_u32(u32::from(bytes[byte_idx]), 8);
        byte_idx += 1;
        remaining -= 8;
    }
    if remaining > 0 {
        // The final partial byte is MSB-aligned in the finished buffer.
        let last = bytes[byte_idx];
        let value = u32::from(last) >> (8 - remaining);
        dst.write_u32(value, remaining as u32);
    }
}

/// Internal: `cb` is an intensity codebook (14 or 15).
fn is_intensity(cb: u8) -> bool {
    cb == INTENSITY_HCB || cb == INTENSITY_HCB2
}

/// Internal: `cb` is the PNS codebook (13).
fn is_noise(cb: u8) -> bool {
    cb == NOISE_HCB
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Spot-check a handful of Table 4.A.1 rows the way the spec
    /// presents them: `index 60 → 1 bit, codeword 0`; `index 59 →
    /// 3 bits, codeword 4`; `index 61 → 4 bits, codeword 0xa`.
    #[test]
    fn hcod_sf_table_known_rows() {
        assert_eq!(HCOD_SF[60], (1, 0x0));
        assert_eq!(HCOD_SF[59], (3, 0x4));
        assert_eq!(HCOD_SF[61], (4, 0xa));
        assert_eq!(HCOD_SF[0], (18, 0x3ffe8));
        assert_eq!(HCOD_SF[120], (19, 0x7fff3));
    }

    /// The codebook is prefix-free (no codeword is a prefix of any
    /// other). Verified once here as a regression guard against typos
    /// in the Table 4.A.1 transcription above.
    #[test]
    fn hcod_sf_table_is_prefix_free() {
        for (i, &(li, vi)) in HCOD_SF.iter().enumerate() {
            for (j, &(lj, vj)) in HCOD_SF.iter().enumerate() {
                if i == j || lj < li {
                    continue;
                }
                let lo = u32::from(lj - li);
                let prefix = vj >> lo;
                assert_ne!(
                    prefix, vi,
                    "entry {} (L={}, v={:x}) is prefix of entry {} (L={}, v={:x})",
                    i, li, vi, j, lj, vj
                );
            }
        }
    }

    /// `index_offset = -60`: encoding `dpcm = 0` selects index 60,
    /// the single-bit `0` codeword.
    #[test]
    fn encode_dpcm_zero_is_single_bit() {
        let (len, cw) = hcod_sf_encode(0).unwrap();
        assert_eq!(len, 1);
        assert_eq!(cw, 0);
    }

    /// Boundary values: `-60` and `+60` are the endpoints of the
    /// DPCM range; anything outside is rejected.
    #[test]
    fn encode_dpcm_boundaries() {
        assert!(hcod_sf_encode(-60).is_ok());
        assert!(hcod_sf_encode(60).is_ok());
        assert_eq!(
            hcod_sf_encode(-61),
            Err(Error::ScaleFactorDataEncodeInvalid)
        );
        assert_eq!(hcod_sf_encode(61), Err(Error::ScaleFactorDataEncodeInvalid));
    }

    /// Every entry of the table round-trips: encode then decode
    /// recovers the original DPCM value.
    #[test]
    fn hcod_sf_roundtrip_every_entry() {
        for dpcm in -60i8..=60 {
            let (len, cw) = hcod_sf_encode(dpcm).unwrap();
            let mut bw = BitWriter::new();
            bw.write_u32(cw, u32::from(len));
            let bits_written = bw.bit_position();
            let buf = bw.finish();
            let mut br = BitReader::new(&buf);
            let recovered = hcod_sf_decode(&mut br).unwrap();
            assert_eq!(recovered, dpcm);
            // Reader must consume exactly `len` bits.
            assert_eq!(br.bit_position(), bits_written);
        }
    }

    // -------------------------------------------------------------------------
    // Error-resilient (RVLC) `scale_factor_data()` — Table 4.53 / §4.6.16.2
    // -------------------------------------------------------------------------

    /// Round-trip an ER block (no intensity / no PNS, all spectrum
    /// bands within the RVLC ±6 range — no escapes) and confirm the
    /// writer regenerates exactly what the parser read back.
    #[test]
    fn er_roundtrip_spectrum_only_no_escapes() {
        // Two groups, codebook 2 (spectrum) on every band.
        let sfb_cb = vec![vec![2u8, 2, 2], vec![2u8, 2]];
        let block = ErScaleFactorData {
            sf_concealment: true,
            rev_global_gain: 137,
            data: ScaleFactorData {
                entries: vec![
                    vec![
                        ScaleFactorEntry::Dpcm(0),
                        ScaleFactorEntry::Dpcm(3),
                        ScaleFactorEntry::Dpcm(-5),
                    ],
                    vec![ScaleFactorEntry::Dpcm(6), ScaleFactorEntry::Dpcm(-6)],
                ],
            },
            dpcm_is_last_position: None,
            dpcm_noise_last_position: None,
        };
        let mut w = BitWriter::new();
        block
            .write(&mut w, &sfb_cb, WindowSequence::OnlyLong)
            .unwrap();
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let parsed = ErScaleFactorData::parse(&mut r, &sfb_cb, WindowSequence::OnlyLong).unwrap();
        assert_eq!(parsed, block);
    }

    /// A delta whose magnitude exceeds ±6 must round-trip through the
    /// base-`±7` + escape split (§4.6.16.2.1) and set
    /// `sf_escapes_present`.
    #[test]
    fn er_roundtrip_with_escapes() {
        let sfb_cb = vec![vec![3u8, 3, 3]];
        let block = ErScaleFactorData {
            sf_concealment: false,
            rev_global_gain: 200,
            data: ScaleFactorData {
                entries: vec![vec![
                    ScaleFactorEntry::Dpcm(7),   // +7 + 0 escape
                    ScaleFactorEntry::Dpcm(-20), // -7 - 13 escape
                    ScaleFactorEntry::Dpcm(60),  // +7 + 53 escape (max)
                ]],
            },
            dpcm_is_last_position: None,
            dpcm_noise_last_position: None,
        };
        let mut w = BitWriter::new();
        block
            .write(&mut w, &sfb_cb, WindowSequence::EightShort)
            .unwrap();
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let parsed = ErScaleFactorData::parse(&mut r, &sfb_cb, WindowSequence::EightShort).unwrap();
        assert_eq!(parsed, block);
    }

    /// An ER block with both intensity and PNS bands round-trips,
    /// exercising `dpcm_is_last_position`, the first-PNS 9-bit PCM
    /// seed, a subsequent PNS RVLC delta, and `dpcm_noise_last_position`.
    /// The Table 4.53 terminal `dpcm_noise_last_position` is present
    /// only when `sf_escapes_present == 1`, so this block carries an
    /// escape (`NoiseDpcm(10)` → base +7 + magnitude 3).
    #[test]
    fn er_roundtrip_intensity_and_pns() {
        // band codebooks: spectrum(2), intensity(15), pns(13), pns(13).
        let sfb_cb = vec![vec![2u8, INTENSITY_HCB2, NOISE_HCB, NOISE_HCB]];
        let block = ErScaleFactorData {
            sf_concealment: true,
            rev_global_gain: 100,
            data: ScaleFactorData {
                entries: vec![vec![
                    ScaleFactorEntry::Dpcm(2),
                    ScaleFactorEntry::Intensity(-3),
                    ScaleFactorEntry::NoisePcm(0x1a5), // 9-bit PCM seed
                    ScaleFactorEntry::NoiseDpcm(10),   // escape → sf_escapes_present
                ]],
            },
            dpcm_is_last_position: Some(5),
            dpcm_noise_last_position: Some(0x0c2),
        };
        let mut w = BitWriter::new();
        block
            .write(&mut w, &sfb_cb, WindowSequence::OnlyLong)
            .unwrap();
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let parsed = ErScaleFactorData::parse(&mut r, &sfb_cb, WindowSequence::OnlyLong).unwrap();
        assert_eq!(parsed, block);
    }

    /// A PNS frame whose deltas all fit the RVLC ±6 range emits no
    /// escapes (`sf_escapes_present == 0`), so per Table 4.53 the
    /// terminal `dpcm_noise_last_position` is **absent** — the parser
    /// recovers `None` for it. The writer rejects a `Some` seed in
    /// that escapeless case as unrepresentable.
    #[test]
    fn er_pns_without_escapes_has_no_noise_seed() {
        let sfb_cb = vec![vec![NOISE_HCB, NOISE_HCB]];
        let block = ErScaleFactorData {
            sf_concealment: false,
            rev_global_gain: 80,
            data: ScaleFactorData {
                entries: vec![vec![
                    ScaleFactorEntry::NoisePcm(0x010),
                    ScaleFactorEntry::NoiseDpcm(3), // within ±6 → no escape
                ]],
            },
            dpcm_is_last_position: None,
            dpcm_noise_last_position: None,
        };
        let mut w = BitWriter::new();
        block
            .write(&mut w, &sfb_cb, WindowSequence::OnlyLong)
            .unwrap();
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let parsed = ErScaleFactorData::parse(&mut r, &sfb_cb, WindowSequence::OnlyLong).unwrap();
        assert_eq!(parsed, block);
        assert_eq!(parsed.dpcm_noise_last_position, None);

        // A Some seed in the escapeless case is unrepresentable.
        let bad = ErScaleFactorData {
            dpcm_noise_last_position: Some(0x0aa),
            ..block
        };
        let mut bw = BitWriter::new();
        assert!(matches!(
            bad.write(&mut bw, &sfb_cb, WindowSequence::OnlyLong),
            Err(Error::RvlcScaleFactorDataInvalid)
        ));
    }

    /// The headline §4.6.2.3.2 equivalence: an RVLC-coded scalefactor
    /// stream and the Huffman-coded stream carrying the *same* DPCM
    /// deltas accumulate to identical absolute scalefactors. This is
    /// what "the decoding process of the RVLC words is the same as
    /// for the Huffman codewords" means in practice.
    #[test]
    fn er_forward_decode_matches_huffman_path() {
        let sfb_cb = vec![vec![2u8, 2, 2, 2]];
        let global_gain = 120u8;
        // Identical DPCM records for both paths.
        let entries = vec![vec![
            ScaleFactorEntry::Dpcm(0),
            ScaleFactorEntry::Dpcm(5),
            ScaleFactorEntry::Dpcm(-30), // forces an escape on the RVLC side
            ScaleFactorEntry::Dpcm(2),
        ]];
        let sfd = ScaleFactorData {
            entries: entries.clone(),
        };

        // Huffman path: write + parse + accumulate.
        let mut hw = BitWriter::new();
        sfd.write(&mut hw, &sfb_cb).unwrap();
        let hbytes = hw.finish();
        let mut hr = BitReader::new(&hbytes);
        let hsfd = ScaleFactorData::parse(&mut hr, &sfb_cb).unwrap();
        let habs = accumulate(&hsfd, &sfb_cb, global_gain).unwrap();

        // RVLC path: write + parse the ER block, then accumulate the
        // reconstructed records with the SAME global_gain.
        let er = ErScaleFactorData {
            sf_concealment: false,
            rev_global_gain: 0,
            data: ScaleFactorData { entries },
            dpcm_is_last_position: None,
            dpcm_noise_last_position: None,
        };
        let mut ew = BitWriter::new();
        er.write(&mut ew, &sfb_cb, WindowSequence::OnlyLong)
            .unwrap();
        let ebytes = ew.finish();
        let mut er_reader = BitReader::new(&ebytes);
        let parsed_er =
            ErScaleFactorData::parse(&mut er_reader, &sfb_cb, WindowSequence::OnlyLong).unwrap();
        let eabs = accumulate(&parsed_er.data, &sfb_cb, global_gain).unwrap();

        assert_eq!(habs, eabs, "RVLC forward decode must equal Huffman path");
    }

    /// A corrupted RVLC bit pattern that lands on a Table 4.167
    /// forbidden codeword surfaces the in-band error-detection event.
    #[test]
    fn er_forbidden_codeword_is_detected() {
        // Forbidden codeword (6 bits, 0b110010) followed by padding.
        let mut w = BitWriter::new();
        w.write_u32(0, 1); // sf_concealment
        w.write_u32(0, 8); // rev_global_gain
        w.write_u32(6, 9); // length_of_rvlc_sf == 6 bits
        w.write_u32(0b110010, 6); // the forbidden codeword
        let bytes = w.finish();
        let sfb_cb = vec![vec![2u8]];
        let mut r = BitReader::new(&bytes);
        assert!(matches!(
            ErScaleFactorData::parse(&mut r, &sfb_cb, WindowSequence::OnlyLong),
            Err(Error::RvlcForbiddenCodeword)
        ));
    }

    /// A `length_of_rvlc_sf` that disagrees with the bits actually
    /// consumed is an in-band conformance failure.
    #[test]
    fn er_length_mismatch_rejected() {
        // Build a valid block then corrupt the length field.
        let sfb_cb = vec![vec![2u8, 2]];
        let block = ErScaleFactorData {
            sf_concealment: false,
            rev_global_gain: 50,
            data: ScaleFactorData {
                entries: vec![vec![ScaleFactorEntry::Dpcm(1), ScaleFactorEntry::Dpcm(-1)]],
            },
            dpcm_is_last_position: None,
            dpcm_noise_last_position: None,
        };
        let mut w = BitWriter::new();
        block
            .write(&mut w, &sfb_cb, WindowSequence::OnlyLong)
            .unwrap();
        let mut bytes = w.finish();
        // The length_of_rvlc_sf field sits at bit offset 9 (after the
        // 1-bit sf_concealment + 8-bit rev_global_gain), 9 bits wide.
        // Flip its low bit to desync the consumed-bit check.
        // bits 9..18 → spans bytes 1 (bits 1..8) and 2 (bits 0..1).
        bytes[2] ^= 0x40; // flip a bit inside the length field region
        let mut r = BitReader::new(&bytes);
        // Either a length mismatch or a forbidden codeword — both are
        // valid in-band rejections of the corrupted stream.
        let res = ErScaleFactorData::parse(&mut r, &sfb_cb, WindowSequence::OnlyLong);
        assert!(res.is_err(), "corrupted length field must be rejected");
    }
}
