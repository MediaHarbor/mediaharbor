//! Spectrum Huffman codebook parameters and the §4.6.3.3 index →
//! n-tuple translation.
//!
//! ISO/IEC 14496-3 §4.6.3 / Table 4.95 enumerates the AAC spectrum
//! Huffman codebooks. Each codebook is identified by a number `i ∈
//! 0..=11` (plus the §4.6.3.1 non-spectral books 12..=15 and the
//! ISO/IEC 14496-3 Annex 4.6.3.3 extension books 16..=31) and carries
//! four parameters used by the §4.6.3.3 spectrum-translation
//! pseudocode:
//!
//! | column         | meaning |
//! |----------------|---------|
//! | `unsigned_cb`  | `0` ⇔ codeword indices encode a signed centred range `-LAV..=+LAV`; `1` ⇔ unsigned `0..=LAV` with explicit sign bits |
//! | `dimension`    | `2` (PAIR books) or `4` (QUAD books) — number of spectral coefficients per codeword |
//! | `LAV`          | largest absolute value the book can represent directly (without the ESC sequence) |
//! | spec table     | which `Table 4.A.x` lists the Huffman codes (not consumed by this module — see "Scope" below) |
//!
//! ## What this module covers
//!
//! * The [`Table495Row`] struct — the four normative columns of
//!   Table 4.95 for one codebook number.
//! * The [`TABLE_4_95`] static — the row for every codebook number
//!   in `0..=31`, sourced from ISO/IEC 14496-3:2001(E) §4.6.3.1
//!   Table 4.95. Rows for `12` (reserved), `13` (PNS), `14`
//!   (out-of-phase intensity), and `15` (in-phase intensity) carry
//!   `None` for `unsigned_cb`, `dimension`, and `lav` — those four
//!   indices do not carry spectral data so the §4.6.3.3 translation
//!   does not apply.
//! * [`table_4_95`] — a safe accessor that returns the row for a
//!   given codebook number (0..=31).
//! * [`decode_index_to_tuple`] — the §4.6.3.3 pseudocode that
//!   translates a Huffman codeword index `idx` (the first column of
//!   Table 4.A.2 through Table 4.A.12) into a `dim`-tuple of
//!   quantised spectral coefficients. For unsigned books, the
//!   returned tuple carries non-negative magnitudes whose signs are
//!   restored by the per-coefficient sign bits that follow the
//!   codeword on the wire.
//! * [`encode_tuple_to_index`] — the inverse of
//!   `decode_index_to_tuple`. Given a `dim`-tuple of quantised
//!   coefficients valid for the codebook (i.e. respecting the
//!   `signed`/`unsigned` convention and the LAV cap), returns the
//!   matching codeword index that an encoder would emit before the
//!   Huffman compression layer.
//! * [`apply_sign_bits`] — folds the per-non-zero-coefficient sign
//!   bits from §4.6.3.3 onto an unsigned-codebook decoded tuple.
//! * [`derive_sign_bits`] — the inverse: extracts the sign bits an
//!   encoder must emit for an unsigned-codebook signed tuple.
//! * [`decode_esc_value`] — the §4.6.3.3 escape sequence for
//!   codebook 11 (`ESC_HCB`). Given an `escape_prefix` length (the
//!   run of 1-bits before the separator 0) and the `(N + 4)`-bit
//!   `escape_word`, returns the absolute magnitude
//!   `2^(N + 4) + escape_word`.
//! * [`encode_esc_value`] — the inverse: given an absolute magnitude
//!   `>= LAV = 16`, returns the `(prefix_len, escape_word_bits,
//!   escape_word)` triple an encoder must emit.
//! * [`MAX_QUANT`] = `8191` — the maximum absolute amplitude any
//!   spectrum codebook 11 can represent, per §4.6.1.3.
//!
//! ## What this module does *not* cover
//!
//! * The Huffman tables themselves (Tables 4.A.2 through 4.A.12 +
//!   the AAC-LD / ER variants). Those translate a codeword
//!   *bit-pattern* into the `idx` consumed by [`decode_index_to_tuple`].
//!   The Huffman trees are a separate clean-room transcription that
//!   will land in a follow-up round.
//! * The §4.4.6 `spectral_data()` wire walker — the function that
//!   loops over scalefactor bands and dispatches per-band onto the
//!   appropriate codebook. That walker will sit on top of this
//!   module and the (forthcoming) Huffman tables.
//! * Codebooks 16..=31 — the Table 4.95 tail (rows 16..=31, all
//!   reusing Table 4.A.12 with different ESC thresholds) are
//!   surfaced in [`TABLE_4_95`] for completeness but the §4.6.3.3
//!   index translation for these books needs the ESC threshold
//!   plumbed through the ESC sequence; the parser-facing accessors
//!   in this round handle the standard `0..=11` range and reject
//!   `12..=31` with [`Error::SpectralCodebookOutOfRange`]. The
//!   per-row LAV value already differs for `16..=31` because each
//!   row carries its own ESC threshold; the row data is correct,
//!   only the wire decoder is unwired.

use crate::section_data::Codebook;
use crate::{Error, Result};

/// Maximum absolute amplitude for a quantised spectral coefficient
/// (`x_quant`). ISO/IEC 14496-3 §4.6.1.3.
pub const MAX_QUANT: i32 = 8191;

/// One row of ISO/IEC 14496-3 Table 4.95 (Spectrum Huffman codebook
/// parameters). Carries the `unsigned_cb`, dimension, and LAV
/// columns; the "Codebook listed in Table" column is encoded as a
/// `Some(table_index)` (e.g. `Some(2)` for Table 4.A.2) when the
/// row references a Huffman codebook listing, and `None` for the
/// non-spectral books (0 / 12..=15).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Table495Row {
    /// Column 2 — `unsigned_cb[i]`. `None` for non-spectral books
    /// (0 / 12..=15).
    pub unsigned: Option<bool>,
    /// Column 3 — dimension (`2` or `4`). `None` for non-spectral
    /// books.
    pub dimension: Option<u8>,
    /// Column 4 — Largest Absolute Value the codebook can encode
    /// directly. For codebook `0` the value is `0` (the band carries
    /// no data so the maximum encoded magnitude is trivially 0). For
    /// `12..=15` the column is `None`. For `11` and `16..=31` the
    /// row carries the LAV after the ESC sequence is consumed; the
    /// in-band LAV (15) is fixed by the Huffman codebook shape — see
    /// [`Self::esc_threshold`] for the per-row ESC value.
    pub lav: Option<u32>,
    /// Column 4 trailing parenthesis — the per-row ESC threshold.
    /// `Some(8191)` for codebook 11, `Some(15)` for codebook 16
    /// (the "w/o ESC" row — the threshold is the in-band cap), and
    /// `Some(31)..=Some(2047)` for codebooks 17..=31. `None` for
    /// codebooks 1..=10 (no ESC sequence — the LAV is fully covered
    /// by the in-band Huffman table) and for the non-spectral books.
    pub esc_threshold: Option<u32>,
    /// Column 5 — the Table 4.A.x number that lists the Huffman
    /// codes. `Some(2)..=Some(12)` for codebooks 1..=11; the
    /// extension books 16..=31 all reuse Table 4.A.12 so the value
    /// is `Some(12)` for each of those. `None` for codebook 0 and
    /// 12..=15.
    pub huffman_table: Option<u8>,
}

impl Table495Row {
    /// `true` ⇔ the row carries `unsigned_cb == 1`. Convenience
    /// accessor that defaults to `false` for non-spectral books
    /// (where the column is `None`).
    pub fn is_unsigned(self) -> bool {
        matches!(self.unsigned, Some(true))
    }

    /// `true` ⇔ the codebook carries an ESC sequence (codebook 11
    /// and the extension books 16..=31).
    pub fn has_esc(self) -> bool {
        self.esc_threshold.is_some()
    }
}

/// Helper to build a row for a spectral codebook (`1..=11` and
/// `16..=31`).
const fn spec_row(
    unsigned: bool,
    dimension: u8,
    lav: u32,
    esc: Option<u32>,
    table: u8,
) -> Table495Row {
    Table495Row {
        unsigned: Some(unsigned),
        dimension: Some(dimension),
        lav: Some(lav),
        esc_threshold: esc,
        huffman_table: Some(table),
    }
}

/// Helper for non-spectral rows (`0`, `12..=15`).
const fn nonspec_row() -> Table495Row {
    Table495Row {
        unsigned: None,
        dimension: None,
        lav: None,
        esc_threshold: None,
        huffman_table: None,
    }
}

/// ISO/IEC 14496-3 §4.6.3.1 Table 4.95 — Spectrum Huffman codebook
/// parameters. Index by codebook number (`0..=31`).
///
/// Cross-check with ISO/IEC 14496-3:2001(E) page 113. Row-by-row:
///
/// | i  | unsigned | dim | LAV | ESC | table |
/// |----|----------|-----|-----|-----|-------|
/// | 0  | —        | —   | 0   | —   | —     |
/// | 1  | 0        | 4   | 1   | —   | 4.A.2 |
/// | 2  | 0        | 4   | 1   | —   | 4.A.3 |
/// | 3  | 1        | 4   | 2   | —   | 4.A.4 |
/// | 4  | 1        | 4   | 2   | —   | 4.A.5 |
/// | 5  | 0        | 2   | 4   | —   | 4.A.6 |
/// | 6  | 0        | 2   | 4   | —   | 4.A.7 |
/// | 7  | 1        | 2   | 7   | —   | 4.A.8 |
/// | 8  | 1        | 2   | 7   | —   | 4.A.9 |
/// | 9  | 1        | 2   | 12  | —   | 4.A.10|
/// | 10 | 1        | 2   | 12  | —   | 4.A.11|
/// | 11 | 1        | 2   | 16  | 8191| 4.A.12|
/// | 12 | —        | —   | —   | —   | reserved |
/// | 13 | —        | —   | —   | —   | PNS      |
/// | 14 | —        | —   | —   | —   | intensity out-of-phase |
/// | 15 | —        | —   | —   | —   | intensity in-phase     |
/// | 16 | 1        | 2   | 16  | 15  | 4.A.12 |
/// | 17 | 1        | 2   | 16  | 31  | 4.A.12 |
/// | 18 | 1        | 2   | 16  | 47  | 4.A.12 |
/// | 19 | 1        | 2   | 16  | 63  | 4.A.12 |
/// | 20 | 1        | 2   | 16  | 95  | 4.A.12 |
/// | 21 | 1        | 2   | 16  | 127 | 4.A.12 |
/// | 22 | 1        | 2   | 16  | 159 | 4.A.12 |
/// | 23 | 1        | 2   | 16  | 191 | 4.A.12 |
/// | 24 | 1        | 2   | 16  | 223 | 4.A.12 |
/// | 25 | 1        | 2   | 16  | 255 | 4.A.12 |
/// | 26 | 1        | 2   | 16  | 319 | 4.A.12 |
/// | 27 | 1        | 2   | 16  | 383 | 4.A.12 |
/// | 28 | 1        | 2   | 16  | 511 | 4.A.12 |
/// | 29 | 1        | 2   | 16  | 767 | 4.A.12 |
/// | 30 | 1        | 2   | 16  | 1023| 4.A.12 |
/// | 31 | 1        | 2   | 16  | 2047| 4.A.12 |
pub const TABLE_4_95: [Table495Row; 32] = [
    // 0: ZERO_HCB
    Table495Row {
        unsigned: None,
        dimension: None,
        lav: Some(0),
        esc_threshold: None,
        huffman_table: None,
    },
    // 1..=4 (QUAD)
    spec_row(false, 4, 1, None, 2),
    spec_row(false, 4, 1, None, 3),
    spec_row(true, 4, 2, None, 4),
    spec_row(true, 4, 2, None, 5),
    // 5..=10 (PAIR)
    spec_row(false, 2, 4, None, 6),
    spec_row(false, 2, 4, None, 7),
    spec_row(true, 2, 7, None, 8),
    spec_row(true, 2, 7, None, 9),
    spec_row(true, 2, 12, None, 10),
    spec_row(true, 2, 12, None, 11),
    // 11: ESC
    spec_row(true, 2, 16, Some(8191), 12),
    // 12..=15: non-spectral
    nonspec_row(),
    nonspec_row(),
    nonspec_row(),
    nonspec_row(),
    // 16: w/o ESC 15 (ESC threshold equals in-band LAV — the row
    // exists but the ESC sequence is never invoked because the LAV
    // cap is also 15).
    spec_row(true, 2, 16, Some(15), 12),
    // 17..=31: ESC books with increasing thresholds
    spec_row(true, 2, 16, Some(31), 12),
    spec_row(true, 2, 16, Some(47), 12),
    spec_row(true, 2, 16, Some(63), 12),
    spec_row(true, 2, 16, Some(95), 12),
    spec_row(true, 2, 16, Some(127), 12),
    spec_row(true, 2, 16, Some(159), 12),
    spec_row(true, 2, 16, Some(191), 12),
    spec_row(true, 2, 16, Some(223), 12),
    spec_row(true, 2, 16, Some(255), 12),
    spec_row(true, 2, 16, Some(319), 12),
    spec_row(true, 2, 16, Some(383), 12),
    spec_row(true, 2, 16, Some(511), 12),
    spec_row(true, 2, 16, Some(767), 12),
    spec_row(true, 2, 16, Some(1023), 12),
    spec_row(true, 2, 16, Some(2047), 12),
];

/// Safe accessor for [`TABLE_4_95`]. Returns
/// [`Error::SpectralCodebookOutOfRange`] for `codebook > 31`.
pub fn table_4_95(codebook: u8) -> Result<Table495Row> {
    if (codebook as usize) >= TABLE_4_95.len() {
        return Err(Error::SpectralCodebookOutOfRange(codebook));
    }
    Ok(TABLE_4_95[codebook as usize])
}

/// Translate a Huffman codeword index `idx` to a `dim`-tuple of
/// quantised spectral coefficients, per ISO/IEC 14496-3 §4.6.3.3.
///
/// The output buffer is the first `dim` entries of the returned
/// fixed-size array; the unused trailing entries are zero. For
/// `dim == 2` the meaningful entries are `[y, z]`; for `dim == 4`
/// they are `[w, x, y, z]`. The spec ordering is preserved
/// (low-frequency first within the n-tuple).
///
/// `codebook` must be one of:
///
/// * `1..=11` — standard spectrum books. The full §4.6.3.3 path is
///   exercised; ESC handling for `11` is not performed *inside* this
///   call (the caller dispatches on [`Table495Row::has_esc`] and
///   invokes [`decode_esc_value`] for each coefficient at the LAV
///   cap).
/// * `0` is rejected with [`Error::SpectralCodebookHasNoTuple`]
///   because the band carries no spectrum data.
/// * `12..=15` are rejected with
///   [`Error::SpectralCodebookHasNoTuple`] (non-spectral books).
/// * `16..=31` are accepted for the pseudocode mechanics but with
///   the same ESC-handling caveat as `11`.
///
/// An out-of-range `idx` (which can only happen when the caller's
/// Huffman tree is incoherent — a conforming Huffman decoder always
/// emits an in-range index) surfaces as
/// [`Error::SpectralCodebookIndexOutOfRange`]. The legal range is
/// `0..mod^dim` where `mod = lav + 1` (unsigned) or `2 * lav + 1`
/// (signed).
pub fn decode_index_to_tuple(codebook: u8, idx: u32) -> Result<[i32; 4]> {
    let row = table_4_95(codebook)?;
    let dim = row
        .dimension
        .ok_or(Error::SpectralCodebookHasNoTuple(codebook))?;
    let lav = row.lav.ok_or(Error::SpectralCodebookHasNoTuple(codebook))?;
    let unsigned = row.is_unsigned();

    let (modulus, offset) = if unsigned {
        (lav as i64 + 1, 0i64)
    } else {
        (2 * lav as i64 + 1, lav as i64)
    };

    // Range check: idx must be < modulus^dim.
    let mut max = 1i64;
    for _ in 0..dim {
        max = max.saturating_mul(modulus);
    }
    if (idx as i64) >= max {
        return Err(Error::SpectralCodebookIndexOutOfRange(codebook));
    }

    let mut out = [0i32; 4];
    let mut remaining = idx as i64;
    if dim == 4 {
        // §4.6.3.3 pseudocode:
        //   w = INT(idx / mod^3) - off
        //   x = INT(idx / mod^2) - off  (after removing the w slice)
        //   y = INT(idx / mod^1) - off  (after removing the x slice)
        //   z = idx - off               (the leftover scaled by mod^0)
        let m2 = modulus * modulus;
        let m3 = m2 * modulus;
        let w = remaining / m3 - offset;
        remaining -= (w + offset) * m3;
        let x = remaining / m2 - offset;
        remaining -= (x + offset) * m2;
        let y = remaining / modulus - offset;
        remaining -= (y + offset) * modulus;
        let z = remaining - offset;
        out[0] = w as i32;
        out[1] = x as i32;
        out[2] = y as i32;
        out[3] = z as i32;
    } else {
        // dim == 2: only y and z (in the lower two slots).
        let y = remaining / modulus - offset;
        remaining -= (y + offset) * modulus;
        let z = remaining - offset;
        out[0] = y as i32;
        out[1] = z as i32;
    }
    Ok(out)
}

/// Inverse of [`decode_index_to_tuple`]: given a `dim`-tuple of
/// quantised coefficients, returns the codeword index that maps to
/// it under the §4.6.3.3 translation.
///
/// `tuple` is the first `dim` entries of the input slice (`[w, x,
/// y, z]` for `dim == 4`, `[y, z]` for `dim == 2`); the unused
/// trailing entries are ignored. For unsigned codebooks every entry
/// must be in `0..=lav`; for signed codebooks every entry must be in
/// `-lav..=+lav`. Any value outside the valid range surfaces as
/// [`Error::SpectralCodebookTupleOutOfRange`].
pub fn encode_tuple_to_index(codebook: u8, tuple: &[i32]) -> Result<u32> {
    let row = table_4_95(codebook)?;
    let dim = row
        .dimension
        .ok_or(Error::SpectralCodebookHasNoTuple(codebook))?;
    let lav = row.lav.ok_or(Error::SpectralCodebookHasNoTuple(codebook))?;
    let unsigned = row.is_unsigned();

    if tuple.len() < dim as usize {
        return Err(Error::SpectralCodebookTupleOutOfRange(codebook));
    }

    let (modulus, offset) = if unsigned {
        (lav as i64 + 1, 0i64)
    } else {
        (2 * lav as i64 + 1, lav as i64)
    };
    let lav_i = lav as i32;

    let mut acc: i64 = 0;
    for &v in tuple.iter().take(dim as usize) {
        let valid = if unsigned {
            (0..=lav_i).contains(&v)
        } else {
            (-lav_i..=lav_i).contains(&v)
        };
        if !valid {
            return Err(Error::SpectralCodebookTupleOutOfRange(codebook));
        }
        acc = acc * modulus + (v as i64 + offset);
    }
    Ok(acc as u32)
}

/// Apply the §4.6.3.3 sign-bit fix-up to an unsigned-codebook
/// decoded tuple.
///
/// On the wire, an unsigned codebook (codebooks 3, 4, 7, 8, 9, 10,
/// 11, 16..=31) emits non-negative magnitudes; the actual sign of
/// each *non-zero* coefficient is carried in a separate sign bit
/// that immediately follows the Huffman codeword. The bit ordering
/// matches the spec's "lower frequency first" rule: for a QUAD book,
/// the sign for `w` (if `w != 0`) is first, then `x`, then `y`,
/// then `z`; for a PAIR book the order is `y`, then `z`.
///
/// `signs` must contain exactly one bit per non-zero coefficient in
/// `tuple`, in the spec-defined order. A `1` bit makes the
/// coefficient negative; a `0` leaves it positive.
///
/// On signed codebooks this is a no-op (signed books already carry
/// their sign in the codeword index). The caller is expected to
/// guard on [`Table495Row::is_unsigned`]; if invoked on a signed
/// codebook the function returns the input unchanged.
///
/// Returns [`Error::SpectralCodebookSignBitsMismatch`] when
/// `signs.len()` disagrees with the count of non-zero coefficients
/// in the unsigned-codebook tuple.
pub fn apply_sign_bits(codebook: u8, mut tuple: [i32; 4], signs: &[bool]) -> Result<[i32; 4]> {
    let row = table_4_95(codebook)?;
    if !row.is_unsigned() {
        // Signed codebooks already carry the sign in the codeword
        // index; this is a no-op. We still accept `signs.is_empty()`
        // and reject any non-empty `signs` to keep the API symmetric
        // — a caller that incorrectly sent sign bits for a signed
        // codebook is a bug worth surfacing.
        if !signs.is_empty() {
            return Err(Error::SpectralCodebookSignBitsMismatch(codebook));
        }
        return Ok(tuple);
    }
    let dim = row
        .dimension
        .ok_or(Error::SpectralCodebookHasNoTuple(codebook))? as usize;
    let nonzero = tuple.iter().take(dim).filter(|&&v| v != 0).count();
    if signs.len() != nonzero {
        return Err(Error::SpectralCodebookSignBitsMismatch(codebook));
    }
    let mut sign_it = signs.iter();
    for entry in tuple.iter_mut().take(dim) {
        if *entry != 0 {
            let neg = *sign_it.next().expect("count match");
            if neg {
                *entry = -*entry;
            }
        }
    }
    Ok(tuple)
}

/// Inverse of [`apply_sign_bits`]: given a signed tuple decoded
/// from an unsigned codebook, returns the sign-bit sequence the
/// encoder must emit (one bit per non-zero coefficient, low-to-high
/// frequency).
///
/// On signed codebooks returns an empty sign-bit vector.
pub fn derive_sign_bits(codebook: u8, tuple: &[i32]) -> Result<Vec<bool>> {
    let row = table_4_95(codebook)?;
    let dim = row
        .dimension
        .ok_or(Error::SpectralCodebookHasNoTuple(codebook))? as usize;
    if tuple.len() < dim {
        return Err(Error::SpectralCodebookTupleOutOfRange(codebook));
    }
    if !row.is_unsigned() {
        return Ok(Vec::new());
    }
    let mut bits = Vec::with_capacity(dim);
    for &v in tuple.iter().take(dim) {
        if v != 0 {
            bits.push(v < 0);
        }
    }
    Ok(bits)
}

/// Decode a §4.6.3.3 ESC sequence to its absolute magnitude.
///
/// The ESC sequence is emitted whenever a codebook-11 Huffman
/// codeword decodes to a 2-tuple coefficient at the in-band cap
/// (magnitude `16`). It consists of:
///
/// 1. `escape_prefix` — a run of `N` consecutive `1` bits.
/// 2. `escape_separator` — a single `0` bit.
/// 3. `escape_word` — `N + 4` bits, big-endian, carrying the
///    unsigned word value.
///
/// The decoded absolute magnitude is `2^(N + 4) + escape_word`.
///
/// `prefix_len` must be in `0..=9` so the magnitude does not exceed
/// [`MAX_QUANT`]. `escape_word` must fit `(N + 4)` bits.
/// Out-of-range arguments surface as
/// [`Error::SpectralCodebookEscOutOfRange`].
pub fn decode_esc_value(prefix_len: u32, escape_word: u32) -> Result<u32> {
    if prefix_len > 9 {
        return Err(Error::SpectralCodebookEscOutOfRange);
    }
    let word_bits = prefix_len + 4;
    if escape_word >= (1u32 << word_bits) {
        return Err(Error::SpectralCodebookEscOutOfRange);
    }
    let value = (1u32 << word_bits) + escape_word;
    if value as i32 > MAX_QUANT {
        return Err(Error::SpectralCodebookEscOutOfRange);
    }
    Ok(value)
}

/// Inverse of [`decode_esc_value`]: given an absolute magnitude
/// `>= 16` (the ESC threshold for codebook 11), returns the
/// `(prefix_len, escape_word)` pair the encoder must emit.
///
/// The mapping is `prefix_len = floor(log2(value)) - 4` and
/// `escape_word = value - 2^(prefix_len + 4)`. Values in
/// `0..=15` cannot be ESC-encoded (they are in-band) and surface as
/// [`Error::SpectralCodebookEscOutOfRange`]. Values greater than
/// [`MAX_QUANT`] also surface there.
pub fn encode_esc_value(value: u32) -> Result<(u32, u32)> {
    if value < 16 || value as i32 > MAX_QUANT {
        return Err(Error::SpectralCodebookEscOutOfRange);
    }
    // floor(log2(value)) — value is in 16..=8191, so log2 is in
    // 4..=12, and prefix_len = log2 - 4 is in 0..=8.
    let log = 31 - value.leading_zeros();
    let prefix_len = log - 4;
    let escape_word = value - (1u32 << (prefix_len + 4));
    Ok((prefix_len, escape_word))
}

/// Bridge to the existing [`Codebook`] enum: classifies a `sect_cb`
/// value (`0..=15`) into a semantic category. The wire-form
/// `sect_cb` field is 4 bits in the standard branch and 5 bits in
/// the ER-AAC resilience branch (Table 17), so [`Codebook`] only
/// covers `0..=15`; this re-export is a convenience so callers can
/// reach the existing classifier without importing
/// [`crate::section_data`] directly.
pub fn classify(sect_cb: u8) -> Codebook {
    Codebook::from_value(sect_cb)
}
