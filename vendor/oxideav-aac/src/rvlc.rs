//! Reversible Variable Length Coding (RVLC) codebooks — ISO/IEC
//! 14496-3 §4.6.16.2 (error-resilient AAC scalefactor coding).
//!
//! RVLC is the error-resilient plug-in replacement for the §4.6.3
//! noiseless coding of scalefactors. Instead of the Table 4.A.1
//! Huffman codebook (codebook 12, indices `0..=120`, DPCM range
//! `-60..=+60`), the error-resilient `scale_factor_data()` branch
//! (Table 4.53) codes the scalefactor / intensity-position /
//! noise-energy DPCM deltas with the **RVLC codebook** (Table 4.166)
//! — a small *symmetric* (palindromic) prefix code covering only the
//! deltas `-7..=+7`. The value `±7` is the `ESC_FLAG`: it signals
//! that an escape magnitude (Huffman-coded with the separate RVLC-ESC
//! codebook, Table 4.168) is to be *added to +7* (positive ESC) or
//! *subtracted from -7* (negative ESC) to recover the true delta.
//!
//! Two properties make RVLC error-resilient, and both are exercised
//! by this module:
//!
//! 1. **Symmetry / reversibility.** Every Table 4.166 codeword is a
//!    bit-palindrome, so the same codebook decodes a stream forwards
//!    *and* backwards. The encoder transmits `rev_global_gain` (the
//!    last scalefactor) and `length_of_rvlc_sf` (the bit length of
//!    the RVLC part) so a decoder that hits a bit error mid-stream
//!    can restart from the far end. This module provides the forward
//!    primitive — the §4.6.2.3.2 note that "the decoding process of
//!    the RVLC words is the same as for the Huffman codewords"
//!    means a clean stream decodes identically forwards, so the
//!    backward path is a recovery-only concern handled at the
//!    `scale_factor_data` driver level.
//! 2. **Sparse code → error detection.** The 4-bit-deep code tree
//!    has unused (asymmetric) leaves: Table 4.167 lists eight
//!    *forbidden* codewords that a conforming encoder never emits.
//!    Hitting one signals a bit error. [`rvlc_decode`] surfaces them
//!    as [`Error::RvlcForbiddenCodeword`].
//!
//! ## Provenance / cross-check
//!
//! The two codebooks are transcribed verbatim from the human-readable
//! ISO/IEC 14496-3:2009 normative tables:
//!
//! * [`RVLC_CB`] — Table 4.166 (`index`, `length`, `codeword`).
//! * [`RVLC_FORBIDDEN`] — Table 4.167 (asymmetric / forbidden
//!   `length`, `codeword`).
//! * [`RVLC_ESC_CB`] — Table 4.168 (RVLC escape Huffman, 54 entries).
//!
//! Each was independently cross-validated against the packed
//! binary-tree node tables staged under
//! `docs/audio/aac/tables/rvlc-codewords-huff-tree.csv` and
//! `rvlc-escape-huff-tree.csv`: decoding the tree recovers exactly
//! the 15 Table 4.166 codewords (leaf − 7 == index) plus the 8
//! Table 4.167 forbidden leaves, confirming both transcriptions.

use oxideav_core::bits::BitReader;

use crate::{Error, Result};

// =============================================================================
// Table 4.166 — RVLC codebook
// =============================================================================

/// The `ESC_FLAG` magnitude. A decoded RVLC delta of `±7` does not
/// stand for the literal value `±7` when escapes are present; it
/// flags that an escape magnitude follows (§4.6.16.2.1).
pub const RVLC_ESC_FLAG: i8 = 7;

/// Number of entries in the Table 4.166 RVLC codebook (deltas
/// `-7..=+7`, 15 entries).
pub const RVLC_CB_NUM_ENTRIES: usize = 15;

/// Maximum Table 4.166 codeword length (9 bits — the `±6` codewords).
pub const RVLC_CB_MAX_LEN: u32 = 9;

/// Table 4.166 — `(value, length_in_bits, codeword)` for the RVLC
/// codebook. `value` is the signed DPCM delta in `-7..=+7`;
/// `codeword` is right-aligned within the `u32` (MSB at bit
/// `length - 1`). Every codeword is a bit-palindrome (the symmetry
/// property §4.6.16.2.1 relies on).
const RVLC_CB: [(i8, u8, u32); RVLC_CB_NUM_ENTRIES] = [
    (-7, 7, 65),  // 1000001
    (-6, 9, 257), // 100000001
    (-5, 8, 129), // 10000001
    (-4, 6, 33),  // 100001
    (-3, 5, 17),  // 10001
    (-2, 4, 9),   // 1001
    (-1, 3, 5),   // 101
    (0, 1, 0),    // 0
    (1, 3, 7),    // 111
    (2, 5, 27),   // 11011
    (3, 6, 51),   // 110011
    (4, 7, 107),  // 1101011
    (5, 8, 195),  // 11000011
    (6, 9, 427),  // 110101011
    (7, 7, 99),   // 1100011
];

/// Table 4.167 — the eight *asymmetric* (forbidden) codewords as
/// `(length_in_bits, codeword)`. A conforming encoder never emits
/// these; a decode that lands on one signals a bit error
/// (§4.6.16.2.1 "some error detection is possible … because not all
/// nodes of the coding tree are used as codewords").
const RVLC_FORBIDDEN: [(u8, u32); 8] = [
    (6, 50),  // 110010
    (7, 96),  // 1100000
    (9, 256), // 100000000
    (8, 194), // 11000010
    (7, 98),  // 1100010
    (6, 52),  // 110100
    (9, 426), // 110101010
    (8, 212), // 11010100
];

/// Encode a signed RVLC delta in `-7..=+7` to its Table 4.166
/// codeword.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u32` (MSB at bit `length - 1`). Out-of-range `value`
/// produces [`Error::RvlcEncodeInvalid`].
///
/// The inverse of [`rvlc_decode`].
pub fn rvlc_encode(value: i8) -> Result<(u8, u32)> {
    for &(v, len, cw) in &RVLC_CB {
        if v == value {
            return Ok((len, cw));
        }
    }
    Err(Error::RvlcEncodeInvalid)
}

/// Decode one Table 4.166 RVLC codeword from `reader`, returning the
/// signed delta in `-7..=+7`.
///
/// Read MSB-first one bit at a time and prefix-match against the
/// codebook. If the accumulated bit pattern matches one of the
/// Table 4.167 forbidden codewords, return
/// [`Error::RvlcForbiddenCodeword`] (an error-detection event, not a
/// reader fault). Returns [`Error::UnexpectedEnd`] on reader
/// underflow.
///
/// A delta of `±7` is the `ESC_FLAG` (see [`RVLC_ESC_FLAG`]); the
/// caller decides whether an escape magnitude follows based on the
/// stream's `sf_escapes_present` flag.
pub fn rvlc_decode(reader: &mut BitReader<'_>) -> Result<i8> {
    let mut acc: u32 = 0;
    for len in 1..=RVLC_CB_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for &(value, entry_len, entry_cw) in &RVLC_CB {
            if u32::from(entry_len) == len && entry_cw == acc {
                return Ok(value);
            }
        }
        for &(f_len, f_cw) in &RVLC_FORBIDDEN {
            if u32::from(f_len) == len && f_cw == acc {
                return Err(Error::RvlcForbiddenCodeword);
            }
        }
    }
    // Every 9-bit prefix is either a valid codeword, a forbidden
    // codeword, or a prefix of one of those; the RVLC tree is fully
    // populated to depth 9, so a 9-bit walk always terminates in one
    // of the two arms above. The guard keeps the return type `!`-free.
    Err(Error::RvlcForbiddenCodeword)
}

// =============================================================================
// Table 4.168 — RVLC escape Huffman codebook
// =============================================================================

/// Number of entries in the Table 4.168 RVLC-ESC Huffman codebook
/// (54 entries, indices `0..=53`).
pub const RVLC_ESC_NUM_ENTRIES: usize = 54;

/// Maximum Table 4.168 codeword length (20 bits).
pub const RVLC_ESC_MAX_LEN: u32 = 20;

/// Table 4.168 — `(length_in_bits, codeword)` per escape index
/// `0..=53`. `codeword` is right-aligned in the `u32`.
///
/// The escape *index* is the magnitude added to the `ESC_FLAG`: a
/// positive escape recovers `+7 + index`, a negative escape recovers
/// `-7 - index` (§4.6.16.2.1). Indices `0` and `1` (the two
/// shortest, 2-bit codewords) correspond to magnitudes 0 and 1.
const RVLC_ESC_CB: [(u8, u32); RVLC_ESC_NUM_ENTRIES] = [
    (2, 2),       // 0
    (2, 0),       // 1
    (3, 6),       // 2
    (3, 2),       // 3
    (4, 14),      // 4
    (5, 31),      // 5
    (5, 15),      // 6
    (5, 13),      // 7
    (6, 61),      // 8
    (6, 29),      // 9
    (6, 25),      // 10
    (6, 24),      // 11
    (7, 120),     // 12
    (7, 56),      // 13
    (8, 242),     // 14
    (8, 114),     // 15
    (9, 486),     // 16
    (9, 230),     // 17
    (10, 974),    // 18
    (10, 463),    // 19
    (11, 1950),   // 20
    (11, 1951),   // 21
    (11, 925),    // 22
    (12, 1848),   // 23
    (14, 7399),   // 24
    (13, 3698),   // 25
    (15, 14797),  // 26
    (20, 473482), // 27
    (20, 473483), // 28
    (20, 473484), // 29
    (20, 473485), // 30
    (20, 473486), // 31
    (20, 473487), // 32
    (20, 473488), // 33
    (20, 473489), // 34
    (20, 473490), // 35
    (20, 473491), // 36
    (20, 473492), // 37
    (20, 473493), // 38
    (20, 473494), // 39
    (20, 473495), // 40
    (20, 473496), // 41
    (20, 473497), // 42
    (20, 473498), // 43
    (20, 473499), // 44
    (20, 473500), // 45
    (20, 473501), // 46
    (20, 473502), // 47
    (20, 473503), // 48
    (19, 236736), // 49
    (19, 236737), // 50
    (19, 236738), // 51
    (19, 236739), // 52
    (19, 236740), // 53
];

/// Encode an RVLC escape magnitude (`0..=53`) to its Table 4.168
/// Huffman codeword.
///
/// Returns `(length_in_bits, codeword)` right-aligned in the `u32`.
/// An out-of-range magnitude produces [`Error::RvlcEncodeInvalid`].
///
/// The inverse of [`rvlc_esc_decode`].
pub fn rvlc_esc_encode(magnitude: u8) -> Result<(u8, u32)> {
    RVLC_ESC_CB
        .get(magnitude as usize)
        .copied()
        .ok_or(Error::RvlcEncodeInvalid)
}

/// Decode one Table 4.168 RVLC-ESC Huffman codeword from `reader`,
/// returning the escape magnitude index `0..=53`.
///
/// Read MSB-first one bit at a time and prefix-match. Returns
/// [`Error::UnexpectedEnd`] on reader underflow and
/// [`Error::RvlcEscInvalid`] if the 20-bit walk matches no entry
/// (a bit error inside the escape part).
pub fn rvlc_esc_decode(reader: &mut BitReader<'_>) -> Result<u8> {
    let mut acc: u32 = 0;
    for len in 1..=RVLC_ESC_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in RVLC_ESC_CB.iter().enumerate() {
            if u32::from(entry_len) == len && entry_cw == acc {
                return Ok(idx as u8);
            }
        }
    }
    Err(Error::RvlcEscInvalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::bits::BitWriter;

    fn encode_rvlc(value: i8) -> Vec<u8> {
        let (len, cw) = rvlc_encode(value).unwrap();
        let mut w = BitWriter::new();
        w.write_u32(cw, u32::from(len));
        // Pad to a byte so the BitReader has whole bytes to read.
        w.align_to_byte_zero();
        w.finish()
    }

    #[test]
    fn rvlc_codebook_roundtrips_every_value() {
        for value in -7..=7 {
            let bytes = encode_rvlc(value);
            let mut r = BitReader::new(&bytes);
            assert_eq!(rvlc_decode(&mut r).unwrap(), value, "value {value}");
        }
    }

    #[test]
    fn rvlc_codewords_are_palindromes() {
        // The §4.6.16.2.1 symmetry property: every codeword reads the
        // same forwards and backwards. This is what enables backward
        // decoding of the RVLC part.
        for &(value, len, cw) in &RVLC_CB {
            let mut forward = 0u32;
            for i in 0..len {
                let bit = (cw >> i) & 1;
                forward = (forward << 1) | bit;
            }
            assert_eq!(forward, cw, "value {value} codeword not a palindrome");
        }
    }

    #[test]
    fn rvlc_codebook_is_prefix_free() {
        for &(_, li, ci) in &RVLC_CB {
            for &(_, lj, cj) in &RVLC_CB {
                if (li, ci) == (lj, cj) {
                    continue;
                }
                // ci is a prefix of cj iff the high `li` bits of cj
                // (a `lj`-bit codeword) equal ci.
                if li <= lj {
                    let shifted = cj >> (lj - li);
                    assert_ne!(shifted, ci, "({li},{ci}) is a prefix of ({lj},{cj})");
                }
            }
        }
    }

    #[test]
    fn forbidden_codewords_are_detected() {
        for &(len, cw) in &RVLC_FORBIDDEN {
            let mut w = BitWriter::new();
            w.write_u32(cw, u32::from(len));
            w.align_to_byte_zero();
            let bytes = w.finish();
            let mut r = BitReader::new(&bytes);
            assert!(
                matches!(rvlc_decode(&mut r), Err(Error::RvlcForbiddenCodeword)),
                "forbidden codeword ({len},{cw}) not detected"
            );
        }
    }

    #[test]
    fn forbidden_codewords_disjoint_from_valid() {
        for &(fl, fc) in &RVLC_FORBIDDEN {
            for &(_, vl, vc) in &RVLC_CB {
                assert!(
                    !(fl == vl && fc == vc),
                    "forbidden ({fl},{fc}) collides with a valid codeword"
                );
            }
        }
    }

    #[test]
    fn rvlc_esc_codebook_roundtrips_every_magnitude() {
        for magnitude in 0u8..RVLC_ESC_NUM_ENTRIES as u8 {
            let (len, cw) = rvlc_esc_encode(magnitude).unwrap();
            let mut w = BitWriter::new();
            w.write_u32(cw, u32::from(len));
            w.align_to_byte_zero();
            let bytes = w.finish();
            let mut r = BitReader::new(&bytes);
            assert_eq!(
                rvlc_esc_decode(&mut r).unwrap(),
                magnitude,
                "mag {magnitude}"
            );
        }
    }

    #[test]
    fn rvlc_esc_codebook_is_prefix_free() {
        for &(li, ci) in &RVLC_ESC_CB {
            for &(lj, cj) in &RVLC_ESC_CB {
                if (li, ci) == (lj, cj) {
                    continue;
                }
                if li <= lj {
                    let shifted = cj >> (lj - li);
                    assert_ne!(shifted, ci, "esc ({li},{ci}) is a prefix of ({lj},{cj})");
                }
            }
        }
    }

    #[test]
    fn rvlc_encode_rejects_out_of_range() {
        assert!(matches!(rvlc_encode(8), Err(Error::RvlcEncodeInvalid)));
        assert!(matches!(rvlc_encode(-8), Err(Error::RvlcEncodeInvalid)));
        assert!(matches!(
            rvlc_esc_encode(RVLC_ESC_NUM_ENTRIES as u8),
            Err(Error::RvlcEncodeInvalid)
        ));
    }

    #[test]
    fn esc_flag_is_seven() {
        // Table 4.166 maps +7 and -7 to the shortest of the
        // extreme magnitudes; the ESC_FLAG constant must agree.
        assert_eq!(RVLC_ESC_FLAG, 7);
        assert!(rvlc_encode(RVLC_ESC_FLAG).is_ok());
        assert!(rvlc_encode(-RVLC_ESC_FLAG).is_ok());
    }
}
