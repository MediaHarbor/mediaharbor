//! `section_data()` parser — ISO/IEC 14496-3 §4.4.6 / ISO/IEC
//! 13818-7 §6.3 Table 17.
//!
//! `section_data()` is the second tool inside
//! `individual_channel_stream()` (after `global_gain` and
//! `ics_info()`, before `scale_factor_data()`). It assigns one
//! Huffman codebook (`sect_cb`) to each *run* of scalefactor bands
//! (a "section") within each window group, using run-length coding
//! with an escape mechanism for sections longer than the field can
//! hold in one increment.
//!
//! This parser depends only on values already produced by
//! [`crate::ics_info::IcsInfo`]:
//!
//! * `num_window_groups` — the outer loop bound.
//! * `max_sfb` — the inner loop terminator (`while (k < max_sfb)`).
//! * `window_sequence == EIGHT_SHORT_SEQUENCE` — selects the
//!   3-bit (`sect_esc_val = 7`) versus 5-bit (`sect_esc_val = 31`)
//!   `sect_len_incr` field width.
//!
//! Crucially it carries **no Huffman codebook of its own**: every
//! field is fixed-width (`sect_cb` is 4 bits, `sect_len_incr` is
//! 3 or 5 bits), so the parser is a pure bit-walker. The Huffman
//! codebooks the `sect_cb` values *select* (the spectrum books 1-11
//! plus the scalefactor book) are consumed by later tools
//! (`scale_factor_data()`, `spectral_data()`), not here.
//!
//! ## Run-length escape coding (Table 17)
//!
//! For each window group `g`, starting at scalefactor band `k = 0`:
//!
//! 1. Read `sect_cb[g][i]` (4 bits).
//! 2. Set `sect_len = 0`. Read `sect_len_incr` (3 or 5 bits).
//!    While the value read equals `sect_esc_val`, add `sect_esc_val`
//!    to `sect_len` and read the next `sect_len_incr`. When a
//!    non-escape value is read, add it to `sect_len` and stop.
//! 3. The section covers bands `[k, k + sect_len)`. Record
//!    `sect_start[g][i] = k`, `sect_end[g][i] = k + sect_len`, and
//!    `sfb_cb[g][sfb] = sect_cb[g][i]` for every band in the run.
//! 4. Advance `k += sect_len`, `i += 1`. Repeat while `k < max_sfb`.
//!
//! `num_sec[g]` is the final value of `i` for the group.
//!
//! ## What is *not* in this round
//!
//! * No Huffman decode. The codebook indices are surfaced verbatim;
//!   the spectrum / scalefactor decoders consume them later.
//! * No `is_intensity()` / PNS classification. The
//!   [`Codebook`] enum exposes the semantic role of each value
//!   (`Intensity`, `IntensityInPhase`, `Noise`, `Esc`, …) for the
//!   benefit of `scale_factor_data()` / `spectral_data()`, but
//!   `section_data()` itself only records the raw `u8`.
//! * No validation that `sfb_cb` is fully populated to `max_sfb` in
//!   pathological streams — the parser surfaces a
//!   [`Error::SectionDataOverrun`] when a section would extend past
//!   `max_sfb` (which a conforming encoder never emits) and
//!   otherwise trusts the run lengths.
//!
//! ## Encode side (Phase 2: first writer primitive)
//!
//! [`SectionData::write`] is the inverse of [`SectionData::parse`]:
//! given the same `window_sequence` / `num_window_groups` / `max_sfb`
//! context the parser was invoked with, it emits the bit-exact
//! Table 17 syntax that the parser reads back. This is the AAC
//! crate's first encoder primitive — a bounded syntax-element
//! writer with no Huffman tables of its own, so the surface lives
//! entirely in the fixed-width `sect_cb` / `sect_len_incr` field
//! pair.
//!
//! The encode-side rule for the §6.3 escape is the inverse of the
//! decode-side accumulation:
//!
//! 1. While the remaining `sect_len` is **greater than or equal to**
//!    `sect_esc_val`, emit a `sect_len_incr` of `sect_esc_val` and
//!    subtract `sect_esc_val` from the remaining length. The
//!    "greater than or equal to" boundary is what forces a trailing
//!    non-escape `sect_len_incr == 0` after a length that lands
//!    exactly on a multiple of `sect_esc_val` — the parser loop
//!    keeps reading while `incr == sect_esc_val`, so the writer
//!    must terminate the run with a non-escape value (which can be
//!    zero) so the parser sees a `break` condition.
//! 2. Emit the residual `sect_len` (which is now strictly less than
//!    `sect_esc_val`) as a single non-escape `sect_len_incr`.
//!
//! [`SectionData::write`] validates that the supplied sections form
//! a contiguous run `0 → max_sfb` per group and that every
//! `sect_cb` and `sect_len` fits the wire field; encoder bugs upstream
//! that violate either invariant surface as
//! [`Error::SectionDataEncodeInvalid`].

use oxideav_core::bits::{BitReader, BitWriter};

use crate::ics_info::WindowSequence;
use crate::{Error, Result};

/// `ZERO_HCB` — section carries neither scalefactor nor spectral
/// data; the band is silent. ISO/IEC 13818-7 §9.2.2 / §11.3.2.
pub const ZERO_HCB: u8 = 0;

/// `FIRST_PAIR_HCB` — the first codebook whose dimension is 2
/// (a 2-tuple); books `< FIRST_PAIR_HCB` are 4-tuple (QUAD) books.
/// ISO/IEC 13818-7 §9.2.2.
pub const FIRST_PAIR_HCB: u8 = 5;

/// `ESC_HCB` — the spectrum escape codebook (book 11). Values whose
/// magnitude reaches the LAV use the §9.3 escape sequence for the
/// actual coefficient. ISO/IEC 13818-7 §9.2.2.
pub const ESC_HCB: u8 = 11;

/// `NOISE_HCB` — Perceptual Noise Substitution codebook (value 13).
/// An MPEG-4 extension (ISO/IEC 14496-3; the base ISO/IEC 13818-7
/// Table 59 marks value 13 *reserved* and adds PNS in its Annex B
/// Table B.1 extended `scale_factor_data()`). When a band's
/// `sfb_cb == NOISE_HCB` the band is noise-filled and its
/// "scalefactor" position carries the PNS energy delta instead.
pub const NOISE_HCB: u8 = 13;

/// `INTENSITY_HCB2` — out-of-phase intensity-stereo codebook
/// (value 14). ISO/IEC 13818-7 §9.2.2 / Table 59.
pub const INTENSITY_HCB2: u8 = 14;

/// `INTENSITY_HCB` — in-phase intensity-stereo codebook (value 15).
/// ISO/IEC 13818-7 §9.2.2 / Table 59.
pub const INTENSITY_HCB: u8 = 15;

/// Semantic classification of a 4-bit `sect_cb` value, per ISO/IEC
/// 13818-7 Table 59 (extended by the MPEG-4 PNS codebook 13).
///
/// `section_data()` records the raw `u8` in [`Section::codebook`];
/// this enum is a *view* over that value so downstream tools
/// (`scale_factor_data()` for the `is_intensity` / PNS branch,
/// `spectral_data()` for the dimension / signed / escape branch)
/// can dispatch without re-deriving the classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Codebook {
    /// `0` — `ZERO_HCB`: silent band, no scalefactor, no spectrum.
    Zero,
    /// `1..=4` — 4-tuple (QUAD) spectrum book. `signed` is `false`
    /// for books 1-2 (`unsigned_cb == 0`) and `true` for 3-4.
    Quad {
        /// Codebook number (1..=4).
        number: u8,
        /// `true` ⇔ the book is *unsigned* (`unsigned_cb[i] == 1`).
        unsigned: bool,
    },
    /// `5..=10` — 2-tuple (PAIR) spectrum book.
    Pair {
        /// Codebook number (5..=10).
        number: u8,
        /// `true` ⇔ the book is *unsigned* (`unsigned_cb[i] == 1`).
        unsigned: bool,
    },
    /// `11` — `ESC_HCB`: 2-tuple unsigned escape book.
    Esc,
    /// `12` — reserved (ISO/IEC 13818-7 Table 59).
    Reserved12,
    /// `13` — `NOISE_HCB`: Perceptual Noise Substitution (MPEG-4).
    Noise,
    /// `14` — `INTENSITY_HCB2`: out-of-phase intensity stereo.
    IntensityOutOfPhase,
    /// `15` — `INTENSITY_HCB`: in-phase intensity stereo.
    IntensityInPhase,
}

impl Codebook {
    /// Classify a raw 4-bit `sect_cb` value (0..=15).
    ///
    /// `unsigned_cb[]` per ISO/IEC 13818-7 Table 59: books 1, 2 are
    /// signed (`unsigned == false`); books 3, 4, 5*, 6*, 7, 8, 9,
    /// 10, 11 are unsigned. (*Books 5 and 6 are 2-tuple signed in
    /// Table 59 — see the per-number mapping below.)
    pub fn from_value(value: u8) -> Self {
        match value & 0x0f {
            0 => Codebook::Zero,
            // QUAD books (dimension 4): 1, 2 signed; 3, 4 unsigned.
            n @ 1..=4 => Codebook::Quad {
                number: n,
                unsigned: matches!(n, 3 | 4),
            },
            // PAIR books (dimension 2): 5, 6 signed; 7, 8, 9, 10
            // unsigned.
            n @ 5..=10 => Codebook::Pair {
                number: n,
                unsigned: matches!(n, 7..=10),
            },
            11 => Codebook::Esc,
            12 => Codebook::Reserved12,
            13 => Codebook::Noise,
            14 => Codebook::IntensityOutOfPhase,
            15 => Codebook::IntensityInPhase,
            _ => unreachable!("masked to 0..=15"),
        }
    }

    /// `true` ⇔ this codebook is an intensity-stereo book
    /// (`INTENSITY_HCB` or `INTENSITY_HCB2`). Mirrors the spec
    /// `is_intensity()` helper used by `scale_factor_data()`.
    pub fn is_intensity(self) -> bool {
        matches!(
            self,
            Codebook::IntensityInPhase | Codebook::IntensityOutOfPhase
        )
    }

    /// `true` ⇔ this is the PNS noise codebook (`NOISE_HCB`).
    pub fn is_noise(self) -> bool {
        matches!(self, Codebook::Noise)
    }

    /// `true` ⇔ this is `ZERO_HCB` (band carries no data).
    pub fn is_zero(self) -> bool {
        matches!(self, Codebook::Zero)
    }
}

/// One contiguous run of scalefactor bands sharing a codebook, as
/// produced by Table 17. `start`/`end` are scalefactor-band indices
/// (`end` is one past the last band, matching `sect_end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    /// `sect_cb[g][i]` — the raw 4-bit codebook value for this run.
    pub codebook: u8,
    /// `sect_start[g][i]` — first scalefactor band in the section.
    pub start: u8,
    /// `sect_end[g][i]` — one past the last band (`start +
    /// sect_len`).
    pub end: u8,
}

impl Section {
    /// Length of the section in scalefactor bands (`sect_len`).
    pub fn len(self) -> u8 {
        self.end - self.start
    }

    /// `true` ⇔ the section spans zero bands. A conforming encoder
    /// never emits a zero-length section, but the accessor is
    /// provided so the `clippy::len_without_is_empty` lint is
    /// satisfied and callers can defensively check.
    pub fn is_empty(self) -> bool {
        self.end == self.start
    }

    /// Semantic [`Codebook`] classification of [`Self::codebook`].
    pub fn codebook_kind(self) -> Codebook {
        Codebook::from_value(self.codebook)
    }
}

/// Parsed `section_data()` for one `individual_channel_stream()`.
///
/// The per-group section lists plus the flattened `sfb_cb[g][sfb]`
/// map are surfaced; `scale_factor_data()` (next round) consumes
/// `sfb_cb` to decide which bands carry a transmitted scalefactor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionData {
    /// `sect[g]` — the ordered sections of window group `g`. The
    /// outer index runs `0..num_window_groups`; `sect[g].len()` is
    /// `num_sec[g]`.
    pub sections: Vec<Vec<Section>>,
    /// `sfb_cb[g][sfb]` — the codebook assigned to scalefactor band
    /// `sfb` of group `g`, for `sfb in 0..max_sfb`. Flattened per
    /// group; the outer index runs `0..num_window_groups`.
    pub sfb_cb: Vec<Vec<u8>>,
}

impl SectionData {
    /// Parse a `section_data()` from the bit-reader.
    ///
    /// * `reader` — positioned immediately after `ics_info()` (well,
    ///   after `global_gain` + `ics_info()` in the full ICS, but
    ///   `section_data()` starts right where the caller leaves the
    ///   reader).
    /// * `window_sequence` — from the surrounding `ics_info()`;
    ///   selects the 3-bit vs 5-bit `sect_len_incr` field.
    /// * `num_window_groups` — from the surrounding `ics_info()`
    ///   derivations (`1` for long sequences).
    /// * `max_sfb` — from the surrounding `ics_info()`.
    ///
    /// Returns [`Error::SectionDataOverrun`] if a section run would
    /// extend past `max_sfb` (non-conforming stream), and
    /// [`Error::UnexpectedEnd`] on bit-reader underflow.
    pub fn parse(
        reader: &mut BitReader<'_>,
        window_sequence: WindowSequence,
        num_window_groups: u8,
        max_sfb: u8,
    ) -> Result<Self> {
        // Table 17: sect_esc_val and sect_len_incr field width.
        let (sect_esc_val, len_bits) = if window_sequence.is_eight_short() {
            ((1u32 << 3) - 1, 3u32) // 7, 3-bit field
        } else {
            ((1u32 << 5) - 1, 5u32) // 31, 5-bit field
        };

        let mut sections: Vec<Vec<Section>> = Vec::with_capacity(num_window_groups as usize);
        let mut sfb_cb: Vec<Vec<u8>> = Vec::with_capacity(num_window_groups as usize);

        for _g in 0..num_window_groups {
            let mut group_sections: Vec<Section> = Vec::new();
            let mut group_sfb_cb: Vec<u8> = vec![ZERO_HCB; max_sfb as usize];

            let mut k: u32 = 0;
            let max = max_sfb as u32;
            while k < max {
                let sect_cb = read_u8(reader, 4)?;

                // sect_len accumulation with escape coding.
                let mut sect_len: u32 = 0;
                loop {
                    let incr = reader
                        .read_u32(len_bits)
                        .map_err(|_| Error::UnexpectedEnd)?;
                    if incr == sect_esc_val {
                        sect_len += sect_esc_val;
                        // Re-read another sect_len_incr.
                        continue;
                    }
                    sect_len += incr;
                    break;
                }

                let start = k;
                let end = k + sect_len;
                if end > max {
                    return Err(Error::SectionDataOverrun);
                }
                for sfb in start..end {
                    group_sfb_cb[sfb as usize] = sect_cb;
                }
                group_sections.push(Section {
                    codebook: sect_cb,
                    start: start as u8,
                    end: end as u8,
                });
                k = end;
            }

            sections.push(group_sections);
            sfb_cb.push(group_sfb_cb);
        }

        Ok(SectionData { sections, sfb_cb })
    }

    /// Parse the error-resilient `section_data()` branch
    /// (`aacSectionDataResilienceFlag == 1`, Table 4.52).
    ///
    /// Two differences from the non-resilient [`SectionData::parse`]:
    ///
    /// * `sect_cb[g][i]` is read as a **5-bit** field (so it can carry
    ///   the §4.6.16.4 virtual codebooks 16..=31, the per-band VCB11
    ///   range derived from `ESC_HCB`) rather than 4 bits.
    /// * The `sect_len_incr` escape loop only runs when
    ///   `sect_cb < 11 || (sect_cb > 11 && sect_cb < 16)`; for
    ///   `sect_cb == 11` (`ESC_HCB`) or `sect_cb >= 16` (a virtual
    ///   codebook) the section length is fixed at `sect_len_incr = 1`
    ///   (one band) with no field on the wire. This is the Table 4.52
    ///   `else { sect_len_incr = 1; }` branch.
    ///
    /// The recovered `sfb_cb[g][sfb]` therefore carries the raw 5-bit
    /// `sect_cb` value (which may exceed `0x0f`); downstream tools that
    /// only understand the base §4.A.1 books must map a virtual `>= 16`
    /// codebook back onto `ESC_HCB` before dispatching — the value is
    /// preserved here so that mapping can stay one layer up.
    ///
    /// Returns [`Error::SectionDataOverrun`] on a run past `max_sfb`
    /// and [`Error::UnexpectedEnd`] on bit-reader underflow.
    pub fn parse_er(
        reader: &mut BitReader<'_>,
        window_sequence: WindowSequence,
        num_window_groups: u8,
        max_sfb: u8,
    ) -> Result<Self> {
        // Table 4.52: sect_esc_val / sect_len_incr field width are the
        // same as the non-resilient branch; only sect_cb widens to 5
        // bits and the escape loop is gated by the codebook value.
        let (sect_esc_val, len_bits) = if window_sequence.is_eight_short() {
            ((1u32 << 3) - 1, 3u32)
        } else {
            ((1u32 << 5) - 1, 5u32)
        };

        let mut sections: Vec<Vec<Section>> = Vec::with_capacity(num_window_groups as usize);
        let mut sfb_cb: Vec<Vec<u8>> = Vec::with_capacity(num_window_groups as usize);

        for _g in 0..num_window_groups {
            let mut group_sections: Vec<Section> = Vec::new();
            let mut group_sfb_cb: Vec<u8> = vec![ZERO_HCB; max_sfb as usize];

            let mut k: u32 = 0;
            let max = max_sfb as u32;
            while k < max {
                let sect_cb = read_u8(reader, 5)?;

                let mut sect_len: u32 = 0;
                if er_uses_escape_coding(sect_cb) {
                    loop {
                        let incr = reader
                            .read_u32(len_bits)
                            .map_err(|_| Error::UnexpectedEnd)?;
                        if incr == sect_esc_val {
                            sect_len += sect_esc_val;
                            continue;
                        }
                        sect_len += incr;
                        break;
                    }
                } else {
                    // Table 4.52 `else { sect_len_incr = 1; }` — one band,
                    // no field on the wire.
                    sect_len = 1;
                }

                let start = k;
                let end = k + sect_len;
                if end > max {
                    return Err(Error::SectionDataOverrun);
                }
                for sfb in start..end {
                    group_sfb_cb[sfb as usize] = sect_cb;
                }
                group_sections.push(Section {
                    codebook: sect_cb,
                    start: start as u8,
                    end: end as u8,
                });
                k = end;
            }

            sections.push(group_sections);
            sfb_cb.push(group_sfb_cb);
        }

        Ok(SectionData { sections, sfb_cb })
    }

    /// Encode the error-resilient `section_data()` branch, the inverse
    /// of [`SectionData::parse_er`].
    ///
    /// `sect_cb` is emitted as a 5-bit field; the `sect_len_incr`
    /// escape sequence is emitted only for codebooks that use escape
    /// coding (`< 11`, or `12..=15`). A `sect_cb == 11` / `>= 16`
    /// section must span exactly one band (the Table 4.52 fixed
    /// `sect_len_incr = 1`); a longer such section is rejected with
    /// [`Error::SectionDataEncodeInvalid`].
    pub fn write_er(
        &self,
        writer: &mut BitWriter,
        window_sequence: WindowSequence,
        max_sfb: u8,
    ) -> Result<()> {
        let (sect_esc_val, len_bits) = if window_sequence.is_eight_short() {
            (7u32, 3u32)
        } else {
            (31u32, 5u32)
        };

        for group_sections in &self.sections {
            if group_sections.is_empty() {
                if max_sfb != 0 {
                    return Err(Error::SectionDataEncodeInvalid);
                }
                continue;
            }
            if group_sections[0].start != 0 {
                return Err(Error::SectionDataEncodeInvalid);
            }
            for w in group_sections.windows(2) {
                if w[0].end != w[1].start {
                    return Err(Error::SectionDataEncodeInvalid);
                }
            }
            if group_sections.last().unwrap().end != max_sfb {
                return Err(Error::SectionDataEncodeInvalid);
            }

            for section in group_sections {
                // sect_cb is 5 bits in the ER branch.
                if section.codebook > 0x1f {
                    return Err(Error::SectionDataEncodeInvalid);
                }
                let sect_len = section.len() as u32;
                if sect_len == 0 {
                    return Err(Error::SectionDataEncodeInvalid);
                }

                writer.write_u32(section.codebook as u32, 5);

                if er_uses_escape_coding(section.codebook) {
                    let mut remaining = sect_len;
                    while remaining >= sect_esc_val {
                        writer.write_u32(sect_esc_val, len_bits);
                        remaining -= sect_esc_val;
                    }
                    writer.write_u32(remaining, len_bits);
                } else {
                    // Fixed sect_len_incr = 1 — the section must be a
                    // single band and carries no length field.
                    if sect_len != 1 {
                        return Err(Error::SectionDataEncodeInvalid);
                    }
                }
            }
        }

        Ok(())
    }

    /// `num_sec[g]` — number of sections in window group `g`.
    /// Returns `0` for an out-of-range group index.
    pub fn num_sec(&self, group: usize) -> usize {
        self.sections.get(group).map_or(0, Vec::len)
    }

    /// Encode `section_data()` onto `writer`, inverse of
    /// [`SectionData::parse`].
    ///
    /// * `writer` — receives the bit-exact Table 17 stream. The
    ///   writer position advances by `4 + (3|5) × (n_increments)` bits
    ///   per section (per the chosen `sect_esc_val` branch).
    /// * `window_sequence` — must match the value the surrounding
    ///   `ics_info()` carries; selects 3-bit / 5-bit `sect_len_incr`.
    /// * `max_sfb` — the band count the parser will be told. Every
    ///   per-group section list must cover bands `[0, max_sfb)`
    ///   exactly without gaps or overlaps.
    ///
    /// Returns [`Error::SectionDataEncodeInvalid`] if:
    ///
    /// * `self.sections.len()` doesn't equal the implicit
    ///   `num_window_groups` (taken from `self.sections.len()`).
    ///   `num_window_groups` itself isn't a parameter — it's read
    ///   off `self.sections` so a caller who constructed
    ///   [`SectionData`] in-memory cannot accidentally desync.
    /// * Any group's section list isn't contiguous from band `0`
    ///   to band `max_sfb` (start of first section != 0; end of
    ///   last section != `max_sfb`; or section `[i].end !=
    ///   sections[i+1].start`).
    /// * A `sect_cb` exceeds the 4-bit field width.
    /// * A `sect_len` of `0` appears (a conforming encoder never
    ///   emits empty sections, and the §6.3 escape can't terminate
    ///   a zero-length run with the parser's `break` semantics).
    pub fn write(
        &self,
        writer: &mut BitWriter,
        window_sequence: WindowSequence,
        max_sfb: u8,
    ) -> Result<()> {
        let (sect_esc_val, len_bits) = if window_sequence.is_eight_short() {
            (7u32, 3u32) // (1 << 3) - 1, 3-bit field
        } else {
            (31u32, 5u32) // (1 << 5) - 1, 5-bit field
        };

        for group_sections in &self.sections {
            // Empty section list is only valid when max_sfb == 0:
            // the parser's `while k < max_sfb` loop never enters.
            if group_sections.is_empty() {
                if max_sfb != 0 {
                    return Err(Error::SectionDataEncodeInvalid);
                }
                continue;
            }

            // Contiguity: first section starts at 0, sections chain
            // end[i] == start[i+1], last ends at max_sfb.
            if group_sections[0].start != 0 {
                return Err(Error::SectionDataEncodeInvalid);
            }
            for w in group_sections.windows(2) {
                if w[0].end != w[1].start {
                    return Err(Error::SectionDataEncodeInvalid);
                }
            }
            if group_sections.last().unwrap().end != max_sfb {
                return Err(Error::SectionDataEncodeInvalid);
            }

            for section in group_sections {
                // sect_cb is 4 bits; reject any out-of-range value.
                if section.codebook > 0x0f {
                    return Err(Error::SectionDataEncodeInvalid);
                }
                let sect_len = section.len() as u32;
                // A conforming encoder never emits a zero-length
                // section; the §6.3 termination relies on a non-
                // escape final increment, and the parser's outer
                // `while k < max_sfb` would then re-enter the loop
                // expecting another sect_cb. Reject up front.
                if sect_len == 0 {
                    return Err(Error::SectionDataEncodeInvalid);
                }

                writer.write_u32(section.codebook as u32, 4);

                // §6.3 escape: while remaining >= sect_esc_val,
                // emit sect_esc_val and subtract. The trailing
                // non-escape increment (which is in [0, sect_esc_val)
                // by construction) terminates the run. This is what
                // forces a literal `0` after a length that's an
                // exact multiple of sect_esc_val (e.g. sect_len=31
                // long branch → emit 31, then 0).
                let mut remaining = sect_len;
                while remaining >= sect_esc_val {
                    writer.write_u32(sect_esc_val, len_bits);
                    remaining -= sect_esc_val;
                }
                writer.write_u32(remaining, len_bits);
            }
        }

        Ok(())
    }
}

fn read_u8(reader: &mut BitReader<'_>, n: u32) -> Result<u8> {
    debug_assert!(n <= 8);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u8)
}

/// Table 4.52 escape-coding gate for the error-resilient
/// `section_data()` branch.
///
/// Escape coding (`sect_len_incr` loop) runs when
/// `sect_cb < 11 || (sect_cb > 11 && sect_cb < 16)`. For
/// `sect_cb == 11` (`ESC_HCB`) and `sect_cb >= 16` (the §4.6.16.4
/// virtual codebooks) the spec fixes `sect_len_incr = 1` and emits no
/// length field, so the section spans exactly one band.
fn er_uses_escape_coding(sect_cb: u8) -> bool {
    sect_cb < 11 || (sect_cb > 11 && sect_cb < 16)
}
