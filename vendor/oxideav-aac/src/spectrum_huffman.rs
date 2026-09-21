//! Spectrum Huffman codebook **wire** layer — ISO/IEC 14496-3
//! §4.6.3 + Annex 4.A (Tables 4.A.2 … 4.A.12).
//!
//! Round 213 landed [`crate::spectral_codebook`] — the §4.6.3.3 index ↔
//! n-tuple translation, the §4.6.3 sign-bit fix-up, and the codebook-11
//! ESC sequence. That module does **not** carry the Huffman codeword
//! tables themselves: it operates on the *index* the wire bitstream
//! decodes to, leaving the codeword ↔ index mapping for this module
//! to own.
//!
//! Round 219 landed the first of the eleven spectrum Huffman
//! codebooks — **Table 4.A.2, "Spectrum Huffman Codebook 1"**. Round
//! 226 added the second — **Table 4.A.3, "Spectrum Huffman Codebook
//! 2"**. Round 231 added the third — **Table 4.A.4, "Spectrum Huffman
//! Codebook 3"**. Round 234 added the fourth — **Table 4.A.5, "Spectrum
//! Huffman Codebook 4"**. Round 238 added the fifth — **Table 4.A.6,
//! "Spectrum Huffman Codebook 5"** — the first **pair** (`dim = 2`)
//! book and the first book to widen its codewords to 13 bits. Round
//! 241 adds the sixth — **Table 4.A.7, "Spectrum Huffman Codebook
//! 6"** — the second pair book, sharing the Codebook 5 Table 4.95
//! row shape (`signed`, `dim = 2`, `LAV = 4`) but tightening the
//! codeword ceiling back down to 11 bits. Round 244 adds the
//! seventh — **Table 4.A.8, "Spectrum Huffman Codebook 7"** — the
//! first **unsigned pair** book (Table 4.95 row 7: `unsigned_cb = 1`,
//! `dim = 2`, `LAV = 7`), widening the per-coefficient magnitude
//! range to `0..=7` and parking the §4.6.3.3 zero-tuple `(0, 0)` at
//! index 0 with a single-bit `0` codeword. Round 250 adds the
//! eighth — **Table 4.A.9, "Spectrum Huffman Codebook 8"** — the
//! second **unsigned pair** book, sharing the Codebook 7 Table 4.95
//! row shape (`unsigned_cb = 1`, `dim = 2`, `LAV = 7` → 64 entries
//! indexed `0..=63`) but tightening the codeword ceiling down to
//! 10 bits and migrating the shortest codeword off the §4.6.3.3
//! zero-tuple `(0, 0)` at index 0 (which now carries a 5-bit
//! `0b01110`) onto the interior tuple `(1, 1)` at index 9 (which
//! carries the 3-bit `0b000`). Round 253 adds the ninth — **Table
//! 4.A.10, "Spectrum Huffman Codebook 9"** — the first
//! **expanded-LAV unsigned pair** book (Table 4.95 row 9:
//! `unsigned_cb = 1`, `dim = 2`, `LAV = 12`), exercising the
//! §4.6.3.3 universe expansion to a `(12 + 1)^2 = 13^2 = 169`-entry
//! lattice indexed `0..=168` with each `(y, z)` coefficient in
//! `0..=12`. Codebook 9 parks the §4.6.3.3 zero-tuple `(0, 0)` at
//! index 0 with a single-bit `0` codeword (matching the head-
//! placement of Codebook 7) and pins the far corner `(12, 12)` at
//! index 168 with a 15-bit `0x7fff` — the widest codeword among
//! the non-ESC spectrum books.
//! Codebooks 1 and 2 share the same Table 4.95
//! row shape (`signed`, `dim = 4`, `LAV = 1` → `3^4 = 81` entries
//! indexed `0..=80`); Codebooks 3 and 4 share the unsigned dim-4
//! shape (Table 4.95 rows 3 and 4 both: `unsigned_cb = 1`, `dim = 4`,
//! `LAV = 2` → `3^4 = 81` entries indexed `0..=80`, with sign bits
//! following the Huffman codeword for every non-zero coefficient per
//! §4.6.3.3); Codebooks 5 and 6 share the signed pair shape
//! (Table 4.95 rows 5 and 6 both: `unsigned_cb = 0`, `dim = 2`,
//! `LAV = 4` → `(2 * 4 + 1)^2 = 9^2 = 81` entries indexed `0..=80`,
//! each tuple coefficient in `-4..=+4`, signed-book so no sign-bit
//! suffix is required after the codeword). Codebook 7 is the first
//! unsigned pair book (Table 4.95 row 7: `unsigned_cb = 1`, `dim = 2`,
//! `LAV = 7` → `(7 + 1)^2 = 8^2 = 64` entries indexed `0..=63`, each
//! tuple coefficient in `0..=7`, sign-bit suffix follows the codeword
//! for each non-zero coefficient per §4.6.3.3); Codebook 8 shares
//! the same unsigned dim-2 LAV-7 shape (Table 4.95 row 8 column-for-
//! column matches row 7 except for the `Codebook listed in Table`
//! cell pointing at Table 4.A.9). Codebook 9 (Table 4.95 row 9)
//! widens the per-coefficient ceiling to `LAV = 12` — the §4.6.3.3
//! universe grows from `8 × 8 = 64` to `13 × 13 = 169` entries —
//! making it the largest of the non-ESC spectrum books.
//! Round 255 adds the tenth — **Table 4.A.11, "Spectrum Huffman
//! Codebook 10"** — the second **expanded-LAV unsigned pair** book
//! (Table 4.95 row 10: `unsigned_cb = 1`, `dim = 2`, `LAV = 12` →
//! 169 entries indexed `0..=168`, the same `13 × 13` universe
//! Codebook 9 covers). Codebook 10 trades Codebook 9's
//! zero-tuple-at-the-1-bit-head distribution for a flatter codeword
//! profile: the zero-tuple `(0, 0)` at index 0 now carries a 6-bit
//! `0b100010` (`0x22`), the shortest slot (4 bits, codeword `0b0000`)
//! migrates onto the interior `(1, 1)` tuple at index 14, and the
//! codeword ceiling pulls down from Codebook 9's 15 bits to **12
//! bits** — matching the head-displacement pattern Codebook 8 uses
//! relative to Codebook 7 (one row lifted from the 1-bit slot,
//! shortest codeword moved off the zero-tuple) but at the wider
//! `LAV = 12` universe.
//! Round 259 adds the eleventh — **Table 4.A.12, "Spectrum Huffman
//! Codebook 11"** — the only **ESC** spectrum book (Table 4.95
//! row 11: `unsigned_cb = 1`, `dim = 2`, `LAV = 16` with an ESC
//! threshold of `8191` — the §4.6.1.3 `x_quant` ceiling). The
//! §4.6.3.3 in-band universe widens to a `17 × 17 = 289`-entry
//! lattice indexed `0..=288` with each `(y, z)` coefficient in
//! `0..=16`; a coefficient value of `16` in either slot is the
//! §4.6.3.3 `escape_flag` whose actual magnitude is reconstructed
//! from the `escape_sequence` (`escape_prefix` of N `1`s, a `0`
//! `escape_separator`, and an `(N + 4)`-bit `escape_word`) bridged
//! by [`crate::spectral_codebook::decode_esc_value`] /
//! [`crate::spectral_codebook::encode_esc_value`] — both already
//! landed in round 213, separate from the Huffman codeword this
//! module carries. Codebook 11 parks the zero-tuple `(0, 0)` at
//! index 0 with the shortest 4-bit codeword `0b0000`, shares that
//! 4-bit floor with the interior `(1, 1)` pair at index 18 (the
//! second 4-bit slot, codeword `0b0001`), pins the half-ESC tuples
//! `(0, 16)` and `(16, 0)` to 10-bit `0x38e` (index 16) and 9-bit
//! `0x1c2` (index 272), and parks the full-ESC corner `(16, 16)`
//! at index 288 with the surprisingly short 5-bit `0b00100`
//! (`0x04`) — the wire layout extends with two sign bits and two
//! escape sequences for that corner, so the Huffman codeword
//! itself stays short. The codeword ceiling matches Codebook 10's
//! 12 bits — exactly six rows reach it (indices 12, 14, 15, 255,
//! 269, 270) — because Codebook 11 pushes its tail distribution
//! out of the Huffman table and into the §4.6.3 ESC sequence.
//! With Codebook 11 the per-codebook AAC spectrum Huffman tables
//! are complete (Tables 4.A.2 through 4.A.12 all land in this
//! module); the next step is the §4.4.6 `spectral_data()` wire
//! walker that loops over scalefactor bands and dispatches per-band
//! onto the codebook chosen by `section_data()`.
//!
//! ## Codebook 1 invariants (Table 4.A.2)
//!
//! | property               | value     | source                       |
//! |------------------------|-----------|------------------------------|
//! | dimension              | 4         | Table 4.95 row 1, column 3   |
//! | `unsigned_cb`          | 0 (signed)| Table 4.95 row 1, column 2   |
//! | LAV                    | 1         | Table 4.95 row 1, column 4   |
//! | entry count            | `3^4 = 81`| `(2 * 1 + 1)^4` per §4.6.3.3 |
//! | maximum codeword length| 11 bits   | Table 4.A.2 column 2 maximum |
//! | shortest codeword      | 1 bit     | Table 4.A.2 row 40 (index 40)|
//! | shortest codeword value| `0`       | Table 4.A.2 row 40           |
//! | Kraft equality         | 2048 = 2¹¹| see [`hcod1_is_complete`]    |
//!
//! Index 40 is `(w, x, y, z) = (0, 0, 0, 0)` per §4.6.3.3 — the
//! zero-tuple gets the single-bit codeword because zero-tuples are
//! the modal spectrum n-tuple in any non-silent frame.
//!
//! ## Wire representation in memory
//!
//! Codewords are stored right-aligned within a `u16`: the MSB of the
//! wire codeword sits at bit `length − 1`, the LSB at bit `0`. To emit
//! bit-for-bit, [`hcod1_encode`] returns `(length, codeword)` and the
//! caller passes them straight to
//! [`oxideav_core::bits::BitWriter::write_u32`].
//!
//! ## Codebook 2 invariants (Table 4.A.3)
//!
//! | property               | value     | source                       |
//! |------------------------|-----------|------------------------------|
//! | dimension              | 4         | Table 4.95 row 2, column 3   |
//! | `unsigned_cb`          | 0 (signed)| Table 4.95 row 2, column 2   |
//! | LAV                    | 1         | Table 4.95 row 2, column 4   |
//! | entry count            | `3^4 = 81`| `(2 * 1 + 1)^4` per §4.6.3.3 |
//! | maximum codeword length| 9 bits    | Table 4.A.3 column 2 maximum |
//! | shortest codeword      | 3 bits    | Table 4.A.3 row 40 (index 40)|
//! | shortest codeword value| `0`       | Table 4.A.3 row 40           |
//! | Kraft equality         | 512 = 2⁹  | see [`hcod2_is_complete`]    |
//!
//! Codebook 2 covers the same `3^4 = 81` signed 4-tuple universe as
//! Codebook 1, with each coefficient in `(-1, 0, +1)`. The encoder
//! chooses between the two books per-section based on
//! `section_data()`'s `sect_cb` field; the choice reflects which book
//! gives the shorter overall bit count for the section's tuple
//! statistics. Index 40 is `(w, x, y, z) = (0, 0, 0, 0)` in both
//! books; in Codebook 2 it carries the 3-bit codeword `0b000`
//! (vs the single bit `0` in Codebook 1).
//!
//! ## Codebook 3 invariants (Table 4.A.4)
//!
//! | property               | value      | source                       |
//! |------------------------|------------|------------------------------|
//! | dimension              | 4          | Table 4.95 row 3, column 3   |
//! | `unsigned_cb`          | 1 (unsigned)| Table 4.95 row 3, column 2  |
//! | LAV                    | 2          | Table 4.95 row 3, column 4   |
//! | entry count            | `3^4 = 81` | `(2 + 1)^4` per §4.6.3.3     |
//! | maximum codeword length| 16 bits    | Table 4.A.4 column 2 maximum |
//! | shortest codeword      | 1 bit      | Table 4.A.4 row 0 (index 0)  |
//! | shortest codeword value| `0`        | Table 4.A.4 row 0            |
//! | Kraft equality         | 65536 = 2¹⁶| see [`hcod3_is_complete`]    |
//!
//! Codebook 3 is the first *unsigned* spectrum book: the Huffman
//! codeword conveys the magnitude n-tuple (each coefficient in
//! `0..=LAV = 0..=2`) and each non-zero coefficient is followed by a
//! single sign bit per §4.6.3.3 (the sign bits travel in
//! low-frequency-first order: `w`, `x`, `y`, `z`). The zero-tuple
//! `(0, 0, 0, 0)` is at *index 0* (not 40 as in the signed books)
//! because the unsigned modulus-3 polynomial puts all-zero at the
//! origin; it carries the single bit codeword `0`. The §4.6.3.3
//! sign-bit suffix is exposed by
//! [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
//! [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
//! and is *not* part of the Huffman codeword itself — this module's
//! `hcod3_encode` / `hcod3_decode` cover the codeword only.
//!
//! ## Codebook 4 invariants (Table 4.A.5)
//!
//! | property               | value      | source                       |
//! |------------------------|------------|------------------------------|
//! | dimension              | 4          | Table 4.95 row 4, column 3   |
//! | `unsigned_cb`          | 1 (unsigned)| Table 4.95 row 4, column 2  |
//! | LAV                    | 2          | Table 4.95 row 4, column 4   |
//! | entry count            | `3^4 = 81` | `(2 + 1)^4` per §4.6.3.3     |
//! | maximum codeword length| 12 bits    | Table 4.A.5 column 2 maximum |
//! | shortest codeword      | 4 bits     | Table 4.A.5 row 40 (index 40)|
//! | shortest codeword value| `0`        | Table 4.A.5 row 40           |
//! | Kraft equality         | 4096 = 2¹² | see [`hcod4_is_complete`]    |
//!
//! Codebook 4 shares Codebook 3's unsigned dim-4 LAV-2 tuple universe
//! (Table 4.95 row 4 is identical to row 3 except for the `Codebook
//! listed in Table` column) but uses a different per-row Huffman
//! length tuning for a different encoder target-statistics. Where
//! Codebook 3 puts the zero-tuple at index 0 with a single-bit
//! codeword and lets the magnitude-2 tuples climb to a 16-bit
//! maximum, Codebook 4 puts the zero-tuple at the *same* §4.6.3.3
//! polynomial position (index 0 maps the unsigned `(0, 0, 0, 0)`
//! tuple via the `((w*3 + x)*3 + y)*3 + z` evaluation with no offset)
//! — but the codeword assignment lifts the zero-tuple to a 4-bit
//! codeword (`0b0111`) and parks the *shortest* codeword (4 bits
//! `0b0000`) at **index 40** instead. The maximum codeword length is
//! **12 bits** (vs 16 for Codebook 3), and two distinct rows reach
//! that length: index 62 (`0xfff`) and index 74 (`0xffe`). The
//! shorter overall code length distribution makes Codebook 4 a
//! better fit for sections whose magnitude statistics are flatter
//! across the `(0, 0, 0, 0) .. (2, 2, 2, 2)` range than Codebook 3's
//! zero-heavy target. The §4.6.3.3 sign-bit suffix is again exposed
//! by [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits)
//! / [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
//! and is *not* part of the Huffman codeword itself — this module's
//! `hcod4_encode` / `hcod4_decode` cover the codeword only.
//!
//! ## Codebook 5 invariants (Table 4.A.6)
//!
//! | property               | value      | source                       |
//! |------------------------|------------|------------------------------|
//! | dimension              | 2 (pair)   | Table 4.95 row 5, column 3   |
//! | `unsigned_cb`          | 0 (signed) | Table 4.95 row 5, column 2   |
//! | LAV                    | 4          | Table 4.95 row 5, column 4   |
//! | entry count            | `9^2 = 81` | `(2 * 4 + 1)^2` per §4.6.3.3 |
//! | maximum codeword length| 13 bits    | Table 4.A.6 column 2 maximum |
//! | shortest codeword      | 1 bit      | Table 4.A.6 row 40 (index 40)|
//! | shortest codeword value| `0`        | Table 4.A.6 row 40           |
//! | Kraft equality         | 8192 = 2¹³ | see [`hcod5_is_complete`]    |
//!
//! Codebook 5 is the first **pair** book — the §4.6.3.3 translation
//! consumes two coefficients per Huffman codeword (`(y, z)`) rather
//! than four (`(w, x, y, z)`) — and the first book to widen the
//! per-coefficient quantised range to `-4..=+4` (LAV = 4). The pair
//! universe stays at 81 entries because `(2 * 4 + 1)^2 = 9^2 = 81`
//! coincides with the dim-4 LAV-1 / LAV-2 universes of Codebooks
//! 1..=4. Index 40 carries the §4.6.3.3 zero-tuple `(0, 0)` — the
//! `(modulus = 9, offset = 4)` polynomial evaluation puts the
//! origin at the centre of the index range, not at the edges as in
//! the unsigned books (Codebooks 3 and 4 placed `(0, 0, 0, 0)` at
//! index 0). The shortest codeword (1 bit `0`) parks at index 40
//! — the same zero-tuple position as Codebook 1 (whose dim-4 origin
//! also lands at the row-40 centre via the same signed-book
//! polynomial). The maximum codeword length is **13 bits** — one
//! more than Codebook 4's 12-bit ceiling and three less than
//! Codebook 3's 16-bit reach — and exactly four rows occupy the
//! 13-bit ceiling: indices 0, 8, 72, and 80 (the four corners
//! `(-4, -4)`, `(-4, +4)`, `(+4, -4)`, `(+4, +4)` of the
//! `9 × 9` signed pair lattice). Because Codebook 5 is **signed**,
//! the §4.6.3.3 sign-bit suffix is *not* emitted after the
//! codeword — every coefficient's sign is baked into the index
//! itself via the `offset = LAV = 4` shift.
//!
//! ## Codebook 6 invariants (Table 4.A.7)
//!
//! | property               | value      | source                       |
//! |------------------------|------------|------------------------------|
//! | dimension              | 2 (pair)   | Table 4.95 row 6, column 3   |
//! | `unsigned_cb`          | 0 (signed) | Table 4.95 row 6, column 2   |
//! | LAV                    | 4          | Table 4.95 row 6, column 4   |
//! | entry count            | `9^2 = 81` | `(2 * 4 + 1)^2` per §4.6.3.3 |
//! | maximum codeword length| 11 bits    | Table 4.A.7 column 2 maximum |
//! | shortest codeword      | 4 bits     | Table 4.A.7 row 40 (index 40)|
//! | shortest codeword value| `0`        | Table 4.A.7 row 40           |
//! | Kraft equality         | 2048 = 2¹¹| see [`hcod6_is_complete`]    |
//!
//! ## Codebook 7 invariants (Table 4.A.8)
//!
//! | property               | value      | source                       |
//! |------------------------|------------|------------------------------|
//! | dimension              | 2 (pair)   | Table 4.95 row 7, column 3   |
//! | `unsigned_cb`          | 1 (unsigned)| Table 4.95 row 7, column 2  |
//! | LAV                    | 7          | Table 4.95 row 7, column 4   |
//! | entry count            | `8^2 = 64` | `(7 + 1)^2` per §4.6.3.3     |
//! | maximum codeword length| 12 bits    | Table 4.A.8 column 2 maximum |
//! | shortest codeword      | 1 bit      | Table 4.A.8 row 0 (index 0)  |
//! | shortest codeword value| `0`        | Table 4.A.8 row 0            |
//! | Kraft equality         | 4096 = 2¹²| see [`hcod7_is_complete`]    |
//!
//! Codebook 7 is the first **unsigned pair** spectrum book — the
//! §4.6.3.3 translation consumes two coefficients per Huffman codeword
//! (`(y, z)`) with each coefficient in `0..=LAV = 0..=7`. The pair
//! universe has `(7 + 1)^2 = 64` entries indexed `0..=63`, a notable
//! drop from the 81-entry universe of Codebooks 1..=6 — the higher
//! per-coefficient ceiling (LAV = 7 vs LAV = 1, 2, 4 in the earlier
//! books) trades dimensionality for range. Like the unsigned dim-4
//! books (Codebooks 3 and 4) the zero-tuple sits at *index 0* (not
//! 40 as in the signed books); the unsigned polynomial
//! `idx = y * (LAV + 1) + z = y * 8 + z` puts all-zero at the origin
//! and the maximum tuple `(7, 7)` at index 63. The single-bit
//! codeword `0` parks at index 0 — the same shortest-codeword position
//! as Codebook 3. The §4.6.3.3 sign-bit suffix applies after every
//! non-zero coefficient (the sign bits travel in low-frequency-first
//! order: `y`, `z`); the suffix is exposed by
//! [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
//! [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
//! and is *not* part of the Huffman codeword itself — this module's
//! `hcod7_encode` / `hcod7_decode` cover the codeword only.
//!
//! ## Codebook 8 invariants (Table 4.A.9)
//!
//! | property               | value      | source                       |
//! |------------------------|------------|------------------------------|
//! | dimension              | 2 (pair)   | Table 4.95 row 8, column 3   |
//! | `unsigned_cb`          | 1 (unsigned)| Table 4.95 row 8, column 2  |
//! | LAV                    | 7          | Table 4.95 row 8, column 4   |
//! | entry count            | `8^2 = 64` | `(7 + 1)^2` per §4.6.3.3     |
//! | maximum codeword length| 10 bits    | Table 4.A.9 column 2 maximum |
//! | shortest codeword      | 3 bits     | Table 4.A.9 row 9 (index 9)  |
//! | shortest codeword value| `0`        | Table 4.A.9 row 9            |
//! | Kraft equality         | 1024 = 2¹⁰| see [`hcod8_is_complete`]    |
//!
//! Codebook 8 shares Codebook 7's unsigned pair tuple universe
//! (Table 4.95 row 8 is identical to row 7 except for the `Codebook
//! listed in Table` column) but uses a different per-row Huffman
//! length tuning. Where Codebook 7 pins the §4.6.3.3 zero-tuple
//! `(0, 0)` to index 0 with the single-bit codeword `0` and lets
//! the upper-right quadrant of the lattice climb to a 12-bit
//! ceiling, Codebook 8 lifts the zero-tuple at index 0 to a 5-bit
//! `0b01110` and migrates the shortest codeword (3 bits `0b000`) to
//! **index 9** — the unsigned-polynomial position of the interior
//! tuple `(y, z) = (1, 1)` (`idx = 1 * 8 + 1 = 9`). The maximum
//! codeword length is **10 bits**; exactly four rows reach the
//! ceiling: indices 7 (`0x3fe`), 47 (`0x3fc`), 56 (`0x3fd`), and
//! 63 (`0x3ff`) — the rarest pair magnitudes (one or two
//! coefficients at the LAV cap). The flatter, lower-ceiling
//! codeword distribution makes Codebook 8 a better fit for sections
//! whose magnitude statistics put weight on the `(1, 1)` interior
//! rather than the `(0, 0)` zero-tuple corner Codebook 7
//! optimises. Because Codebook 8 is unsigned, the §4.6.3.3 sign-bit
//! suffix follows the Huffman codeword on the wire — one sign bit
//! per non-zero coefficient, low-frequency-first — and is exposed
//! by [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits)
//! / [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
//! and is *not* part of the Huffman codeword itself — this module's
//! `hcod8_encode` / `hcod8_decode` cover the codeword only.
//!
//! ## Codebook 9 invariants (Table 4.A.10)
//!
//! | property               | value         | source                        |
//! |------------------------|---------------|-------------------------------|
//! | dimension              | 2 (pair)      | Table 4.95 row 9, column 3    |
//! | `unsigned_cb`          | 1 (unsigned)  | Table 4.95 row 9, column 2    |
//! | LAV                    | 12            | Table 4.95 row 9, column 4    |
//! | entry count            | `13^2 = 169`  | `(12 + 1)^2` per §4.6.3.3     |
//! | maximum codeword length| 15 bits       | Table 4.A.10 column 2 maximum |
//! | shortest codeword      | 1 bit         | Table 4.A.10 row 0 (index 0)  |
//! | shortest codeword value| `0`           | Table 4.A.10 row 0            |
//! | Kraft equality         | 32768 = 2¹⁵   | see [`hcod9_is_complete`]     |
//!
//! Codebook 9 is the first **expanded-LAV pair** spectrum book — it
//! steps away from the `8 × 8` unsigned pair lattice Codebooks 7 and
//! 8 share and widens the per-coefficient ceiling from `7` to `12`,
//! producing a `13 × 13 = 169`-entry universe indexed `0..=168` with
//! each `(y, z)` coefficient in `0..=12`. The §4.6.3.3 unsigned
//! polynomial `idx = y * (LAV + 1) + z = y * 13 + z` parks the
//! zero-tuple `(0, 0)` at index 0 — the same head placement
//! Codebook 7 uses — and pins the maximum tuple `(12, 12)` at index
//! 168 (the far corner of the `13 × 13` unsigned lattice). The
//! single-bit codeword `0` lives at index 0, matching the
//! shortest-slot placement Codebook 7 also uses for its zero-tuple.
//! The maximum codeword length is **15 bits** — a 5-bit jump up
//! from Codebook 8's 10-bit ceiling and the widest non-ESC spectrum
//! codeword in the entire Annex 4.A book set — reflecting the
//! `169 / 64 ≈ 2.6×` universe expansion that widens the
//! distribution's tail. Exactly four rows reach the 15-bit ceiling:
//! indices 142 (`0x7ffc`), 154 (`0x7ffd`), 155 (`0x7ffe`), and 168
//! (`0x7fff`) — the rarest pair magnitudes, sitting near the
//! `LAV = 12` cap. The table is a **complete** 15-bit prefix code
//! (Kraft equality `Σ 2^(15 − L) = 32768 = 2¹⁵`), exhaustively
//! verified by walking every 15-bit prefix and asserting each maps
//! to exactly one entry. Because Codebook 9 is unsigned, the
//! §4.6.3.3 sign-bit suffix follows the Huffman codeword on the
//! wire — one sign bit per non-zero coefficient, low-frequency-
//! first — and is exposed by
//! [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
//! [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
//! and is *not* part of the Huffman codeword itself — this module's
//! `hcod9_encode` / `hcod9_decode` cover the codeword only.
//!
//! ## Codebook 10 invariants (Table 4.A.11)
//!
//! | property               | value         | source                        |
//! |------------------------|---------------|-------------------------------|
//! | dimension              | 2 (pair)      | Table 4.95 row 10, column 3   |
//! | `unsigned_cb`          | 1 (unsigned)  | Table 4.95 row 10, column 2   |
//! | LAV                    | 12            | Table 4.95 row 10, column 4   |
//! | entry count            | `13^2 = 169`  | `(12 + 1)^2` per §4.6.3.3     |
//! | maximum codeword length| 12 bits       | Table 4.A.11 column 2 maximum |
//! | shortest codeword      | 4 bits        | Table 4.A.11 row 14 (index 14)|
//! | shortest codeword value| `0`           | Table 4.A.11 row 14           |
//! | Kraft equality         | 4096 = 2¹²    | see [`hcod10_is_complete`]    |
//!
//! Codebook 10 shares Codebook 9's expanded-LAV unsigned pair tuple
//! universe (Table 4.95 row 10 is identical to row 9 except for the
//! `Codebook listed in Table` column pointing at Table 4.A.11) but
//! uses a different per-row Huffman length tuning for a different
//! encoder target-statistics. Where Codebook 9 parks the §4.6.3.3
//! zero-tuple `(0, 0)` at index 0 with the single-bit `0` codeword
//! and lets the four rarest pair magnitudes climb to a 15-bit
//! ceiling, Codebook 10 keeps the zero-tuple at index 0 (the
//! §4.6.3.3 polynomial position is fixed by the tuple) but its
//! codeword swells to 6 bits (`0x22`), the shortest 4-bit slot
//! migrates onto the interior `(1, 1)` tuple at index 14 with
//! codeword `0b0000`, and the codeword ceiling pulls down to
//! **12 bits**. Exactly three rows reach the 4-bit floor (indices
//! 14, 15, 27 with codewords `0x0`, `0x1`, `0x2`) and exactly eight
//! rows reach the 12-bit ceiling (indices 12, 129, 142, 155, 165,
//! 166, 167, 168 with codewords `0xffd`, `0xffa`, `0xff9`, `0xffb`,
//! `0xff8`, `0xffe`, `0xffc`, `0xfff`) — the four corners and four
//! near-edges of the `13 × 13` unsigned lattice. The flatter,
//! pull-down distribution makes Codebook 10 a better fit for
//! sections whose magnitude statistics put more weight in the
//! `(1..=4, 1..=4)` interior than Codebook 9's
//! more-zero-tuple-heavy target. The encoder chooses between the
//! two books per-section via `section_data()`'s `sect_cb` field;
//! the §4.6.3.3 sign-bit suffix follows the Huffman codeword on the
//! wire — one sign bit per non-zero coefficient, low-frequency-
//! first — and is exposed by
//! [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
//! [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
//! and is *not* part of the Huffman codeword itself — this module's
//! `hcod10_encode` / `hcod10_decode` cover the codeword only.
//!
//! Codebook 6 shares Codebook 5's signed pair tuple universe
//! (Table 4.95 row 6 is identical to row 5 except for the `Codebook
//! listed in Table` column) but uses a different per-row Huffman
//! length tuning. Where Codebook 5 parks the single bit `0` at
//! index 40 and lets the four lattice corners reach a 13-bit
//! ceiling, Codebook 6 lifts the zero-tuple at index 40 to a 4-bit
//! `0b0000` and pulls the ceiling back to **11 bits**. Exactly four
//! rows reach the 11-bit ceiling: indices 0 (`0x7fe`), 8 (`0x7fd`),
//! 72 (`0x7ff`), and 80 (`0x7fc`) — the four `(±4, ±4)` corners of
//! the `9 × 9` signed pair lattice, the same four corner positions
//! Codebook 5 also pinned to its 13-bit ceiling. The shorter,
//! flatter codeword distribution makes Codebook 6 a better fit
//! for sections whose magnitude statistics put more weight in the
//! `(±1, ±1) .. (±3, ±3)` interior than Codebook 5's
//! more-zero-tuple-heavy target. The encoder chooses between the
//! two books per-section via `section_data()`'s `sect_cb` field;
//! the §4.6.3.3 sign bits remain inside the index for both books
//! because both are signed (`unsigned_cb = 0`).
//!
//! * The §4.6.3.3 index → n-tuple translation. That sits in
//!   [`crate::spectral_codebook::decode_index_to_tuple`] /
//!   [`crate::spectral_codebook::encode_tuple_to_index`].
//! * The ESC sequence (codebook 11 and the extension books 16..=31).
//!   That sits in [`crate::spectral_codebook::decode_esc_value`] /
//!   [`crate::spectral_codebook::encode_esc_value`].
//! * The §4.6.3 sign-bit suffix for unsigned codebooks. Codebook 1
//!   is *signed* so no sign bits follow the codeword; the
//!   [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits)
//!   path is exercised by unsigned codebooks (3, 4, 7..=11, 16..=31).
//! * The `spectral_data()` driver that loops over scalefactor bands
//!   and dispatches per-band onto the codebook chosen by
//!   `section_data()`. That driver will land once codebooks 2..=11
//!   are in place.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::{Error, Result};

// =============================================================================
// Table 4.A.2 — Spectrum Huffman Codebook 1
// =============================================================================
//
// 81 entries, indices 0..=80. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the wire
// codeword at bit `length − 1`). Reproduced verbatim from ISO/IEC
// 14496-3:2001(E) §4.A.1 Table 4.A.2 (page 193).
//
// The codebook is a complete prefix code: Σᵢ 2^(11 − Lᵢ) = 2048 = 2¹¹.
// This is exhaustively verified at compile time by the
// `hcod1_is_complete` regression test (which walks every 11-bit
// prefix and asserts each maps to exactly one index).

/// Number of entries in Table 4.A.2 (`81`, indices `0..=80`).
pub const HCOD1_NUM_ENTRIES: usize = 81;

/// Maximum codeword length emitted by Table 4.A.2 (11 bits).
pub const HCOD1_MAX_LEN: u32 = 11;

/// Table 4.A.2 — `(length_in_bits, codeword)` per index `0..=80`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD1: [(u8, u16); HCOD1_NUM_ENTRIES] = [
    (11, 0x7f8), // 0
    (9, 0x1f1),  // 1
    (11, 0x7fd), // 2
    (10, 0x3f5), // 3
    (7, 0x68),   // 4
    (10, 0x3f0), // 5
    (11, 0x7f7), // 6
    (9, 0x1ec),  // 7
    (11, 0x7f5), // 8
    (10, 0x3f1), // 9
    (7, 0x72),   // 10
    (10, 0x3f4), // 11
    (7, 0x74),   // 12
    (5, 0x11),   // 13
    (7, 0x76),   // 14
    (9, 0x1eb),  // 15
    (7, 0x6c),   // 16
    (10, 0x3f6), // 17
    (11, 0x7fc), // 18
    (9, 0x1e1),  // 19
    (11, 0x7f1), // 20
    (9, 0x1f0),  // 21
    (7, 0x61),   // 22
    (9, 0x1f6),  // 23
    (11, 0x7f2), // 24
    (9, 0x1ea),  // 25
    (11, 0x7fb), // 26
    (9, 0x1f2),  // 27
    (7, 0x69),   // 28
    (9, 0x1ed),  // 29
    (7, 0x77),   // 30
    (5, 0x17),   // 31
    (7, 0x6f),   // 32
    (9, 0x1e6),  // 33
    (7, 0x64),   // 34
    (9, 0x1e5),  // 35
    (7, 0x67),   // 36
    (5, 0x15),   // 37
    (7, 0x62),   // 38
    (5, 0x12),   // 39
    (1, 0x000),  // 40 — zero-tuple, single bit `0`
    (5, 0x14),   // 41
    (7, 0x65),   // 42
    (5, 0x16),   // 43
    (7, 0x6d),   // 44
    (9, 0x1e9),  // 45
    (7, 0x63),   // 46
    (9, 0x1e4),  // 47
    (7, 0x6b),   // 48
    (5, 0x13),   // 49
    (7, 0x71),   // 50
    (9, 0x1e3),  // 51
    (7, 0x70),   // 52
    (9, 0x1f3),  // 53
    (11, 0x7fe), // 54
    (9, 0x1e7),  // 55
    (11, 0x7f3), // 56
    (9, 0x1ef),  // 57
    (7, 0x60),   // 58
    (9, 0x1ee),  // 59
    (11, 0x7f0), // 60
    (9, 0x1e2),  // 61
    (11, 0x7fa), // 62
    (10, 0x3f3), // 63
    (7, 0x6a),   // 64
    (9, 0x1e8),  // 65
    (7, 0x75),   // 66
    (5, 0x10),   // 67
    (7, 0x73),   // 68
    (9, 0x1f4),  // 69
    (7, 0x6e),   // 70
    (10, 0x3f7), // 71
    (11, 0x7f6), // 72
    (9, 0x1e0),  // 73
    (11, 0x7f9), // 74
    (10, 0x3f2), // 75
    (7, 0x66),   // 76
    (9, 0x1f5),  // 77
    (11, 0x7ff), // 78
    (9, 0x1f7),  // 79
    (11, 0x7f4), // 80
];

/// Encode a Codebook 1 codeword index (`0..=80`) to the wire Huffman
/// codeword from Table 4.A.2.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=80` (the 81-entry `3^4` enumeration of every legal
/// signed 4-tuple with each coefficient in `-1..=+1`).
///
/// The inverse of [`hcod1_decode`].
pub fn hcod1_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD1
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(1))?;
    Ok(*entry)
}

/// Decode one Codebook 1 Huffman codeword from `reader`, returning
/// the codeword index in `0..=80`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 81-entry table. The table is
/// small (max codeword length 11 bits, 81 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 11 bits (Kraft
/// equality `Σᵢ 2^(11 − Lᵢ) = 2048 = 2¹¹`), so any 11-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 11 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod1_is_complete` regression test that exhaustively
/// walks all `2¹¹` 11-bit prefixes.
pub fn hcod1_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD1_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD1.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD1 is a complete 11-bit prefix code. The
    // `hcod1_is_complete` regression test verifies every 11-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD1 is a complete 11-bit prefix code; the 11-bit walk must match");
}

/// Write a Codebook 1 codeword to `writer` by index.
///
/// Convenience over `hcod1_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 80`.
pub fn hcod1_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod1_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.3 — Spectrum Huffman Codebook 2
// =============================================================================
//
// 81 entries, indices 0..=80. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the wire
// codeword at bit `length − 1`). Transcribed verbatim from ISO/IEC
// 14496-3:2001(E) §4.A.1 Table 4.A.3 (page 194).
//
// The codebook is a complete prefix code: Σᵢ 2^(9 − Lᵢ) = 512 = 2⁹.
// This is exhaustively verified by the `hcod2_is_complete` regression
// test (which walks every 9-bit prefix and asserts each maps to
// exactly one index).
//
// The signed-tuple universe is identical to Codebook 1's (3^4 = 81
// signed 4-tuples with each element in `-1..=+1`); the §4.6.3.3 index
// translation in [`crate::spectral_codebook`] is reused as-is.

/// Number of entries in Table 4.A.3 (`81`, indices `0..=80`).
pub const HCOD2_NUM_ENTRIES: usize = 81;

/// Maximum codeword length emitted by Table 4.A.3 (9 bits).
pub const HCOD2_MAX_LEN: u32 = 9;

/// Table 4.A.3 — `(length_in_bits, codeword)` per index `0..=80`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD2: [(u8, u16); HCOD2_NUM_ENTRIES] = [
    (9, 0x1f3), // 0
    (7, 0x6f),  // 1
    (9, 0x1fd), // 2
    (8, 0xeb),  // 3
    (6, 0x23),  // 4
    (8, 0xea),  // 5
    (9, 0x1f7), // 6
    (8, 0xe8),  // 7
    (9, 0x1fa), // 8
    (8, 0xf2),  // 9
    (6, 0x2d),  // 10
    (7, 0x70),  // 11
    (6, 0x20),  // 12
    (5, 0x06),  // 13
    (6, 0x2b),  // 14
    (7, 0x6e),  // 15
    (6, 0x28),  // 16
    (8, 0xe9),  // 17
    (9, 0x1f9), // 18
    (7, 0x66),  // 19
    (8, 0xf8),  // 20
    (8, 0xe7),  // 21
    (6, 0x1b),  // 22
    (8, 0xf1),  // 23
    (9, 0x1f4), // 24
    (7, 0x6b),  // 25
    (9, 0x1f5), // 26
    (8, 0xec),  // 27
    (6, 0x2a),  // 28
    (7, 0x6c),  // 29
    (6, 0x2c),  // 30
    (5, 0x0a),  // 31
    (6, 0x27),  // 32
    (7, 0x67),  // 33
    (6, 0x1a),  // 34
    (8, 0xf5),  // 35
    (6, 0x24),  // 36
    (5, 0x08),  // 37
    (6, 0x1f),  // 38
    (5, 0x09),  // 39
    (3, 0x000), // 40 — zero-tuple, 3-bit codeword `0`
    (5, 0x07),  // 41
    (6, 0x1d),  // 42
    (5, 0x0b),  // 43
    (6, 0x30),  // 44
    (8, 0xef),  // 45
    (6, 0x1c),  // 46
    (7, 0x64),  // 47
    (6, 0x1e),  // 48
    (5, 0x0c),  // 49
    (6, 0x29),  // 50
    (8, 0xf3),  // 51
    (6, 0x2f),  // 52
    (8, 0xf0),  // 53
    (9, 0x1fc), // 54
    (7, 0x71),  // 55
    (9, 0x1f2), // 56
    (8, 0xf4),  // 57
    (6, 0x21),  // 58
    (8, 0xe6),  // 59
    (8, 0xf7),  // 60
    (7, 0x68),  // 61
    (9, 0x1f8), // 62
    (8, 0xee),  // 63
    (6, 0x22),  // 64
    (7, 0x65),  // 65
    (6, 0x31),  // 66
    (4, 0x02),  // 67
    (6, 0x26),  // 68
    (8, 0xed),  // 69
    (6, 0x25),  // 70
    (7, 0x6a),  // 71
    (9, 0x1fb), // 72
    (7, 0x72),  // 73
    (9, 0x1fe), // 74
    (7, 0x69),  // 75
    (6, 0x2e),  // 76
    (8, 0xf6),  // 77
    (9, 0x1ff), // 78
    (7, 0x6d),  // 79
    (9, 0x1f6), // 80
];

/// Encode a Codebook 2 codeword index (`0..=80`) to the wire Huffman
/// codeword from Table 4.A.3.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`] carrying the
/// codebook number `2`; the legal range is `0..=80` (the 81-entry
/// `3^4` enumeration of every legal signed 4-tuple with each
/// coefficient in `-1..=+1` — the same universe as Codebook 1).
///
/// The inverse of [`hcod2_decode`].
pub fn hcod2_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD2
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(2))?;
    Ok(*entry)
}

/// Decode one Codebook 2 Huffman codeword from `reader`, returning
/// the codeword index in `0..=80`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 81-entry table. The table is
/// small (max codeword length 9 bits, 81 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 9 bits (Kraft
/// equality `Σᵢ 2^(9 − Lᵢ) = 512 = 2⁹`), so any 9-bit prefix fully
/// read from `reader` is guaranteed to match exactly one entry — the
/// bottom of the loop is unreachable when `reader` produces 9 bits
/// without underflowing. A purely defensive `unreachable!()` guards
/// the loop fall-through; it is verified dead by the
/// `hcod2_is_complete` regression test that exhaustively walks all
/// `2⁹` 9-bit prefixes.
pub fn hcod2_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD2_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD2.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD2 is a complete 9-bit prefix code. The
    // `hcod2_is_complete` regression test verifies every 9-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD2 is a complete 9-bit prefix code; the 9-bit walk must match");
}

/// Write a Codebook 2 codeword to `writer` by index.
///
/// Convenience over `hcod2_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 80`.
pub fn hcod2_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod2_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.4 — Spectrum Huffman Codebook 3
// =============================================================================
//
// 81 entries, indices 0..=80. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the
// wire codeword at bit `length − 1`). Transcribed verbatim from
// ISO/IEC 14496-3:2009(E) §4.A.1 Table 4.A.4.
//
// The codebook is a complete prefix code: Σᵢ 2^(16 − Lᵢ) = 65536 = 2¹⁶.
// This is exhaustively verified by the `hcod3_is_complete` regression
// test (which walks every 16-bit prefix and asserts each maps to
// exactly one index).
//
// Codebook 3 is the first *unsigned* spectrum book: each tuple
// coefficient is a non-negative magnitude in `0..=LAV = 0..=2`, and
// the §4.6.3.3 sign-bit suffix carries the sign of each non-zero
// coefficient outside the Huffman codeword. The §4.6.3.3 index ↔
// 4-tuple translation lives in
// [`crate::spectral_codebook::decode_index_to_tuple`] /
// [`crate::spectral_codebook::encode_tuple_to_index`]; the sign-bit
// suffix lives in
// [`crate::spectral_codebook::apply_sign_bits`] /
// [`crate::spectral_codebook::derive_sign_bits`].

/// Number of entries in Table 4.A.4 (`81`, indices `0..=80`).
pub const HCOD3_NUM_ENTRIES: usize = 81;

/// Maximum codeword length emitted by Table 4.A.4 (16 bits).
pub const HCOD3_MAX_LEN: u32 = 16;

/// Table 4.A.4 — `(length_in_bits, codeword)` per index `0..=80`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD3: [(u8, u16); HCOD3_NUM_ENTRIES] = [
    (1, 0x0000),  // 0 — zero-tuple, single bit `0`
    (4, 0x0009),  // 1
    (8, 0x00ef),  // 2
    (4, 0x000b),  // 3
    (5, 0x0019),  // 4
    (8, 0x00f0),  // 5
    (9, 0x01eb),  // 6
    (9, 0x01e6),  // 7
    (10, 0x03f2), // 8
    (4, 0x000a),  // 9
    (6, 0x0035),  // 10
    (9, 0x01ef),  // 11
    (6, 0x0034),  // 12
    (6, 0x0037),  // 13
    (9, 0x01e9),  // 14
    (9, 0x01ed),  // 15
    (9, 0x01e7),  // 16
    (10, 0x03f3), // 17
    (9, 0x01ee),  // 18
    (10, 0x03ed), // 19
    (13, 0x1ffa), // 20
    (9, 0x01ec),  // 21
    (9, 0x01f2),  // 22
    (11, 0x07f9), // 23
    (11, 0x07f8), // 24
    (10, 0x03f8), // 25
    (12, 0x0ff8), // 26
    (4, 0x0008),  // 27
    (6, 0x0038),  // 28
    (10, 0x03f6), // 29
    (6, 0x0036),  // 30
    (7, 0x0075),  // 31
    (10, 0x03f1), // 32
    (10, 0x03eb), // 33
    (10, 0x03ec), // 34
    (12, 0x0ff4), // 35
    (5, 0x0018),  // 36
    (7, 0x0076),  // 37
    (11, 0x07f4), // 38
    (6, 0x0039),  // 39
    (7, 0x0074),  // 40
    (10, 0x03ef), // 41
    (9, 0x01f3),  // 42
    (9, 0x01f4),  // 43
    (11, 0x07f6), // 44
    (9, 0x01e8),  // 45
    (10, 0x03ea), // 46
    (13, 0x1ffc), // 47
    (8, 0x00f2),  // 48
    (9, 0x01f1),  // 49
    (12, 0x0ffb), // 50
    (10, 0x03f5), // 51
    (11, 0x07f3), // 52
    (12, 0x0ffc), // 53
    (8, 0x00ee),  // 54
    (10, 0x03f7), // 55
    (15, 0x7ffe), // 56
    (9, 0x01f0),  // 57
    (11, 0x07f5), // 58
    (15, 0x7ffd), // 59
    (13, 0x1ffb), // 60
    (14, 0x3ffa), // 61
    (16, 0xffff), // 62
    (8, 0x00f1),  // 63
    (10, 0x03f0), // 64
    (14, 0x3ffc), // 65
    (9, 0x01ea),  // 66
    (10, 0x03ee), // 67
    (14, 0x3ffb), // 68
    (12, 0x0ff6), // 69
    (12, 0x0ffa), // 70
    (15, 0x7ffc), // 71
    (11, 0x07f2), // 72
    (12, 0x0ff5), // 73
    (16, 0xfffe), // 74
    (10, 0x03f4), // 75
    (11, 0x07f7), // 76
    (15, 0x7ffb), // 77
    (12, 0x0ff7), // 78
    (12, 0x0ff9), // 79
    (15, 0x7ffa), // 80
];

/// Encode a Codebook 3 codeword index (`0..=80`) to the wire Huffman
/// codeword from Table 4.A.4.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`] carrying the
/// codebook number `3`; the legal range is `0..=80` (the 81-entry
/// `3^4` enumeration of every legal unsigned 4-tuple with each
/// coefficient in `0..=LAV = 0..=2`).
///
/// The inverse of [`hcod3_decode`]. The sign-bit suffix for each
/// non-zero coefficient is *not* part of the returned codeword — the
/// caller emits sign bits separately per
/// [`crate::spectral_codebook::derive_sign_bits`].
pub fn hcod3_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD3
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(3))?;
    Ok(*entry)
}

/// Decode one Codebook 3 Huffman codeword from `reader`, returning
/// the codeword index in `0..=80`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 81-entry table. The table is
/// small (max codeword length 16 bits, 81 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 16 bits (Kraft
/// equality `Σᵢ 2^(16 − Lᵢ) = 65536 = 2¹⁶`), so any 16-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 16 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod3_is_complete` regression test that exhaustively
/// walks all `2¹⁶` 16-bit prefixes.
///
/// The sign-bit suffix for non-zero coefficients is *not* consumed
/// here — the caller pairs the returned index with the §4.6.3.3
/// translation and then reads exactly one sign bit per non-zero
/// coefficient in low-frequency-first order.
pub fn hcod3_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD3_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD3.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD3 is a complete 16-bit prefix code. The
    // `hcod3_is_complete` regression test verifies every 16-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD3 is a complete 16-bit prefix code; the 16-bit walk must match");
}

/// Write a Codebook 3 codeword to `writer` by index.
///
/// Convenience over `hcod3_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 80`. The
/// caller is responsible for emitting the §4.6.3.3 sign bits for
/// every non-zero coefficient after this call.
pub fn hcod3_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod3_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.5 — Spectrum Huffman Codebook 4
// =============================================================================
//
// 81 entries, indices 0..=80. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the
// wire codeword at bit `length − 1`). Transcribed verbatim from
// ISO/IEC 14496-3:2001(E) §4.A.1 Table 4.A.5.
//
// The codebook is a complete prefix code: Σᵢ 2^(12 − Lᵢ) = 4096 = 2¹².
// This is exhaustively verified by the `hcod4_is_complete` regression
// test (which walks every 12-bit prefix and asserts each maps to
// exactly one index).
//
// Codebook 4 shares Codebook 3's unsigned dim-4 LAV-2 tuple universe
// (Table 4.95 row 4 = row 3 except for the source-table column);
// the §4.6.3.3 index ↔ 4-tuple translation in
// [`crate::spectral_codebook`] is reused as-is. The §4.6.3.3 sign-bit
// suffix lives in [`crate::spectral_codebook::apply_sign_bits`] /
// [`crate::spectral_codebook::derive_sign_bits`].

/// Number of entries in Table 4.A.5 (`81`, indices `0..=80`).
pub const HCOD4_NUM_ENTRIES: usize = 81;

/// Maximum codeword length emitted by Table 4.A.5 (12 bits).
pub const HCOD4_MAX_LEN: u32 = 12;

/// Table 4.A.5 — `(length_in_bits, codeword)` per index `0..=80`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD4: [(u8, u16); HCOD4_NUM_ENTRIES] = [
    (4, 0x007),  // 0
    (5, 0x016),  // 1
    (8, 0x0f6),  // 2
    (5, 0x018),  // 3
    (4, 0x008),  // 4
    (8, 0x0ef),  // 5
    (9, 0x1ef),  // 6
    (8, 0x0f3),  // 7
    (11, 0x7f8), // 8
    (5, 0x019),  // 9
    (5, 0x017),  // 10
    (8, 0x0ed),  // 11
    (5, 0x015),  // 12
    (4, 0x001),  // 13
    (8, 0x0e2),  // 14
    (8, 0x0f0),  // 15
    (7, 0x070),  // 16
    (10, 0x3f0), // 17
    (9, 0x1ee),  // 18
    (8, 0x0f1),  // 19
    (11, 0x7fa), // 20
    (8, 0x0ee),  // 21
    (8, 0x0e4),  // 22
    (10, 0x3f2), // 23
    (11, 0x7f6), // 24
    (10, 0x3ef), // 25
    (11, 0x7fd), // 26
    (4, 0x005),  // 27
    (5, 0x014),  // 28
    (8, 0x0f2),  // 29
    (4, 0x009),  // 30
    (4, 0x004),  // 31
    (8, 0x0e5),  // 32
    (8, 0x0f4),  // 33
    (8, 0x0e8),  // 34
    (10, 0x3f4), // 35
    (4, 0x006),  // 36
    (4, 0x002),  // 37
    (8, 0x0e7),  // 38
    (4, 0x003),  // 39
    (4, 0x000),  // 40 — shortest codeword in Codebook 4
    (7, 0x06b),  // 41
    (8, 0x0e3),  // 42
    (7, 0x069),  // 43
    (9, 0x1f3),  // 44
    (8, 0x0eb),  // 45
    (8, 0x0e6),  // 46
    (10, 0x3f6), // 47
    (7, 0x06e),  // 48
    (7, 0x06a),  // 49
    (9, 0x1f4),  // 50
    (10, 0x3ec), // 51
    (9, 0x1f0),  // 52
    (10, 0x3f9), // 53
    (8, 0x0f5),  // 54
    (8, 0x0ec),  // 55
    (11, 0x7fb), // 56
    (8, 0x0ea),  // 57
    (7, 0x06f),  // 58
    (10, 0x3f7), // 59
    (11, 0x7f9), // 60
    (10, 0x3f3), // 61
    (12, 0xfff), // 62
    (8, 0x0e9),  // 63
    (7, 0x06d),  // 64
    (10, 0x3f8), // 65
    (7, 0x06c),  // 66
    (7, 0x068),  // 67
    (9, 0x1f5),  // 68
    (10, 0x3ee), // 69
    (9, 0x1f2),  // 70
    (11, 0x7f4), // 71
    (11, 0x7f7), // 72
    (10, 0x3f1), // 73
    (12, 0xffe), // 74
    (10, 0x3ed), // 75
    (9, 0x1f1),  // 76
    (11, 0x7f5), // 77
    (11, 0x7fe), // 78
    (10, 0x3f5), // 79
    (11, 0x7fc), // 80
];

/// Encode a Codebook 4 codeword index (`0..=80`) to the wire Huffman
/// codeword from Table 4.A.5.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`] carrying the
/// codebook number `4`; the legal range is `0..=80` (the 81-entry
/// `3^4` enumeration of every legal unsigned 4-tuple with each
/// coefficient in `0..=LAV = 0..=2` — the same universe as Codebook
/// 3).
///
/// The inverse of [`hcod4_decode`]. The sign-bit suffix for each
/// non-zero coefficient is *not* part of the returned codeword — the
/// caller emits sign bits separately per
/// [`crate::spectral_codebook::derive_sign_bits`].
pub fn hcod4_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD4
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(4))?;
    Ok(*entry)
}

/// Decode one Codebook 4 Huffman codeword from `reader`, returning
/// the codeword index in `0..=80`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 81-entry table. The table is
/// small (max codeword length 12 bits, 81 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 12 bits (Kraft
/// equality `Σᵢ 2^(12 − Lᵢ) = 4096 = 2¹²`), so any 12-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 12 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod4_is_complete` regression test that exhaustively
/// walks all `2¹²` 12-bit prefixes.
///
/// The sign-bit suffix for non-zero coefficients is *not* consumed
/// here — the caller pairs the returned index with the §4.6.3.3
/// translation and then reads exactly one sign bit per non-zero
/// coefficient in low-frequency-first order.
pub fn hcod4_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD4_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD4.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD4 is a complete 12-bit prefix code. The
    // `hcod4_is_complete` regression test verifies every 12-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD4 is a complete 12-bit prefix code; the 12-bit walk must match");
}

/// Write a Codebook 4 codeword to `writer` by index.
///
/// Convenience over `hcod4_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 80`. The
/// caller is responsible for emitting the §4.6.3.3 sign bits for
/// every non-zero coefficient after this call.
pub fn hcod4_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod4_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.6 — Spectrum Huffman Codebook 5
// =============================================================================
//
// 81 entries, indices 0..=80. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the
// wire codeword at bit `length − 1`). Transcribed verbatim from
// ISO/IEC 14496-3:2001(E) §4.A.1 Table 4.A.6.
//
// The codebook is a complete prefix code: Σᵢ 2^(13 − Lᵢ) = 8192 = 2¹³.
// This is exhaustively verified by the `hcod5_is_complete` regression
// test (which walks every 13-bit prefix and asserts each maps to
// exactly one index).
//
// Codebook 5 is the first **pair** spectrum book (Table 4.95 row 5:
// `unsigned_cb = 0`, `dim = 2`, `LAV = 4`). Per §4.6.3.3 the
// index↔tuple translation evaluates `idx = (y + LAV) * 9 + (z + LAV)`
// so the signed pair lattice spans `(-4, -4) .. (+4, +4)` and the
// zero-tuple `(0, 0)` lands at the centre row index 40. The
// [`crate::spectral_codebook`] §4.6.3.3 dispatcher already handles
// the dim=2 path; this module owns only the codeword wire layer.
// Because Codebook 5 is signed, no sign-bit suffix follows the
// codeword on the wire — the index alone fully specifies the
// signed pair.

/// Number of entries in Table 4.A.6 (`81`, indices `0..=80`).
pub const HCOD5_NUM_ENTRIES: usize = 81;

/// Maximum codeword length emitted by Table 4.A.6 (13 bits).
pub const HCOD5_MAX_LEN: u32 = 13;

/// Table 4.A.6 — `(length_in_bits, codeword)` per index `0..=80`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD5: [(u8, u16); HCOD5_NUM_ENTRIES] = [
    (13, 0x1fff), // 0  — (y, z) = (-4, -4); one of the four 13-bit corners
    (12, 0xff7),  // 1
    (11, 0x7f4),  // 2
    (11, 0x7e8),  // 3
    (10, 0x3f1),  // 4
    (11, 0x7ee),  // 5
    (11, 0x7f9),  // 6
    (12, 0xff8),  // 7
    (13, 0x1ffd), // 8  — (y, z) = (-4, +4); 13-bit corner
    (12, 0xffd),  // 9
    (11, 0x7f1),  // 10
    (10, 0x3e8),  // 11
    (9, 0x1e8),   // 12
    (8, 0xf0),    // 13
    (9, 0x1ec),   // 14
    (10, 0x3ee),  // 15
    (11, 0x7f2),  // 16
    (12, 0xffa),  // 17
    (12, 0xff4),  // 18
    (10, 0x3ef),  // 19
    (9, 0x1f2),   // 20
    (8, 0xe8),    // 21
    (7, 0x70),    // 22
    (8, 0xec),    // 23
    (9, 0x1f0),   // 24
    (10, 0x3ea),  // 25
    (11, 0x7f3),  // 26
    (11, 0x7eb),  // 27
    (9, 0x1eb),   // 28
    (8, 0xea),    // 29
    (5, 0x1a),    // 30
    (4, 0x8),     // 31
    (5, 0x19),    // 32
    (8, 0xee),    // 33
    (9, 0x1ef),   // 34
    (11, 0x7ed),  // 35
    (10, 0x3f0),  // 36
    (8, 0xf2),    // 37
    (7, 0x73),    // 38
    (4, 0xb),     // 39
    (1, 0x0),     // 40 — (y, z) = (0, 0); single-bit zero codeword
    (4, 0xa),     // 41
    (7, 0x71),    // 42
    (8, 0xf3),    // 43
    (11, 0x7e9),  // 44
    (11, 0x7ef),  // 45
    (9, 0x1ee),   // 46
    (8, 0xef),    // 47
    (5, 0x18),    // 48
    (4, 0x9),     // 49
    (5, 0x1b),    // 50
    (8, 0xeb),    // 51
    (9, 0x1e9),   // 52
    (11, 0x7ec),  // 53
    (11, 0x7f6),  // 54
    (10, 0x3eb),  // 55
    (9, 0x1f3),   // 56
    (8, 0xed),    // 57
    (7, 0x72),    // 58
    (8, 0xe9),    // 59
    (9, 0x1f1),   // 60
    (10, 0x3ed),  // 61
    (11, 0x7f7),  // 62
    (12, 0xff6),  // 63
    (11, 0x7f0),  // 64
    (10, 0x3e9),  // 65
    (9, 0x1ed),   // 66
    (8, 0xf1),    // 67
    (9, 0x1ea),   // 68
    (10, 0x3ec),  // 69
    (11, 0x7f8),  // 70
    (12, 0xff9),  // 71
    (13, 0x1ffc), // 72 — (y, z) = (+4, -4); 13-bit corner
    (12, 0xffc),  // 73
    (12, 0xff5),  // 74
    (11, 0x7ea),  // 75
    (10, 0x3f3),  // 76
    (10, 0x3f2),  // 77
    (11, 0x7f5),  // 78
    (12, 0xffb),  // 79
    (13, 0x1ffe), // 80 — (y, z) = (+4, +4); 13-bit corner
];

/// Encode a Codebook 5 codeword index (`0..=80`) to the wire Huffman
/// codeword from Table 4.A.6.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`] carrying the
/// codebook number `5`; the legal range is `0..=80` (the 81-entry
/// `9^2` enumeration of every legal signed 2-tuple with each
/// coefficient in `-LAV..=+LAV = -4..=+4`).
///
/// The inverse of [`hcod5_decode`]. Because Codebook 5 is signed,
/// no sign-bit suffix follows the codeword on the wire — the
/// `offset = LAV = 4` shift inside the §4.6.3.3 translation already
/// encodes every coefficient's sign into the index.
pub fn hcod5_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD5
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(5))?;
    Ok(*entry)
}

/// Decode one Codebook 5 Huffman codeword from `reader`, returning
/// the codeword index in `0..=80`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 81-entry table. The table is
/// small (max codeword length 13 bits, 81 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 13 bits (Kraft
/// equality `Σᵢ 2^(13 − Lᵢ) = 8192 = 2¹³`), so any 13-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 13 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod5_is_complete` regression test that exhaustively
/// walks all `2¹³` 13-bit prefixes.
///
/// No sign-bit suffix is read here — Codebook 5 is signed, so every
/// coefficient's sign is already baked into the index via the
/// `offset = LAV = 4` §4.6.3.3 polynomial.
pub fn hcod5_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD5_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD5.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD5 is a complete 13-bit prefix code. The
    // `hcod5_is_complete` regression test verifies every 13-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD5 is a complete 13-bit prefix code; the 13-bit walk must match");
}

/// Write a Codebook 5 codeword to `writer` by index.
///
/// Convenience over `hcod5_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 80`. No
/// sign bits follow on the wire (Codebook 5 is signed).
pub fn hcod5_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod5_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.7 — Spectrum Huffman Codebook 6
// =============================================================================
//
// 81 entries, indices 0..=80. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the wire
// codeword at bit `length − 1`). Transcribed verbatim from ISO/IEC
// 14496-3:2001(E) §4.A.1 Table 4.A.7.
//
// The codebook is a complete prefix code: Σᵢ 2^(11 − Lᵢ) = 2048 = 2¹¹.
// This is exhaustively verified by the `hcod6_is_complete` regression
// test (which walks every 11-bit prefix and asserts each maps to
// exactly one index).
//
// Codebook 6 is the second signed pair spectrum book (Table 4.95 row 6:
// `unsigned_cb = 0`, `dim = 2`, `LAV = 4` → `9^2 = 81` entries, each
// coefficient in `-4..=+4`). The §4.6.3.3 polynomial places the
// zero-tuple `(0, 0)` at the centre of the index range (index 40);
// the four `(±4, ±4)` lattice corners sit at indices 0, 8, 72, 80.
// Because Codebook 6 is signed, no sign-bit suffix follows the
// codeword on the wire.

/// Number of entries in Table 4.A.7 (`81`, indices `0..=80`).
pub const HCOD6_NUM_ENTRIES: usize = 81;

/// Maximum codeword length emitted by Table 4.A.7 (11 bits).
pub const HCOD6_MAX_LEN: u32 = 11;

/// Table 4.A.7 — `(length_in_bits, codeword)` per index `0..=80`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD6: [(u8, u16); HCOD6_NUM_ENTRIES] = [
    (11, 0x7fe), // 0  — (y, z) = (-4, -4)
    (10, 0x3fd), // 1
    (9, 0x1f1),  // 2
    (9, 0x1eb),  // 3
    (9, 0x1f4),  // 4
    (9, 0x1ea),  // 5
    (9, 0x1f0),  // 6
    (10, 0x3fc), // 7
    (11, 0x7fd), // 8  — (y, z) = (-4, +4)
    (10, 0x3f6), // 9
    (9, 0x1e5),  // 10
    (8, 0xea),   // 11
    (7, 0x6c),   // 12
    (7, 0x71),   // 13
    (7, 0x68),   // 14
    (8, 0xf0),   // 15
    (9, 0x1e6),  // 16
    (10, 0x3f7), // 17
    (9, 0x1f3),  // 18
    (8, 0xef),   // 19
    (6, 0x32),   // 20
    (6, 0x27),   // 21
    (6, 0x28),   // 22
    (6, 0x26),   // 23
    (6, 0x31),   // 24
    (8, 0xeb),   // 25
    (9, 0x1f7),  // 26
    (9, 0x1e8),  // 27
    (7, 0x6f),   // 28
    (6, 0x2e),   // 29
    (4, 0x8),    // 30
    (4, 0x4),    // 31
    (4, 0x6),    // 32
    (6, 0x29),   // 33
    (7, 0x6b),   // 34
    (9, 0x1ee),  // 35
    (9, 0x1ef),  // 36
    (7, 0x72),   // 37
    (6, 0x2d),   // 38
    (4, 0x2),    // 39
    (4, 0x0),    // 40 — zero-tuple (y, z) = (0, 0), 4-bit `0b0000`
    (4, 0x3),    // 41
    (6, 0x2f),   // 42
    (7, 0x73),   // 43
    (9, 0x1fa),  // 44
    (9, 0x1e7),  // 45
    (7, 0x6e),   // 46
    (6, 0x2b),   // 47
    (4, 0x7),    // 48
    (4, 0x1),    // 49
    (4, 0x5),    // 50
    (6, 0x2c),   // 51
    (7, 0x6d),   // 52
    (9, 0x1ec),  // 53
    (9, 0x1f9),  // 54
    (8, 0xee),   // 55
    (6, 0x30),   // 56
    (6, 0x24),   // 57
    (6, 0x2a),   // 58
    (6, 0x25),   // 59
    (6, 0x33),   // 60
    (8, 0xec),   // 61
    (9, 0x1f2),  // 62
    (10, 0x3f8), // 63
    (9, 0x1e4),  // 64
    (8, 0xed),   // 65
    (7, 0x6a),   // 66
    (7, 0x70),   // 67
    (7, 0x69),   // 68
    (7, 0x74),   // 69
    (8, 0xf1),   // 70
    (10, 0x3fa), // 71
    (11, 0x7ff), // 72 — (y, z) = (+4, -4)
    (10, 0x3f9), // 73
    (9, 0x1f6),  // 74
    (9, 0x1ed),  // 75
    (9, 0x1f8),  // 76
    (9, 0x1e9),  // 77
    (9, 0x1f5),  // 78
    (10, 0x3fb), // 79
    (11, 0x7fc), // 80 — (y, z) = (+4, +4)
];

/// Encode a Codebook 6 codeword index (`0..=80`) to the wire Huffman
/// codeword from Table 4.A.7.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=80` (the 81-entry `9^2` enumeration of every legal
/// signed pair with each coefficient in `-4..=+4`).
///
/// The inverse of [`hcod6_decode`]. Because Codebook 6 is signed,
/// each tuple coefficient's sign is already encoded in the index via
/// the §4.6.3.3 `offset = LAV = 4` shift — no sign-bit suffix is
/// emitted after the codeword.
pub fn hcod6_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD6
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(6))?;
    Ok(*entry)
}

/// Decode one Codebook 6 Huffman codeword from `reader`, returning
/// the codeword index in `0..=80`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 81-entry table. The table is
/// small (max codeword length 11 bits, 81 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 11 bits (Kraft
/// equality `Σᵢ 2^(11 − Lᵢ) = 2048 = 2¹¹`), so any 11-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 11 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod6_is_complete` regression test that exhaustively
/// walks all `2¹¹` 11-bit prefixes.
///
/// No sign-bit suffix is read here — Codebook 6 is signed, so every
/// `(y, z)` pair carries its sign inside the §4.6.3.3 index via the
/// `offset = LAV = 4` shift.
pub fn hcod6_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD6_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD6.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD6 is a complete 11-bit prefix code. The
    // `hcod6_is_complete` regression test verifies every 11-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD6 is a complete 11-bit prefix code; the 11-bit walk must match");
}

/// Write a Codebook 6 codeword to `writer` by index.
///
/// Convenience over `hcod6_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 80`. No
/// sign bits follow on the wire (Codebook 6 is signed).
pub fn hcod6_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod6_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.8 — Spectrum Huffman Codebook 7
// =============================================================================
//
// 64 entries, indices 0..=63. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the
// wire codeword at bit `length − 1`). Transcribed verbatim from
// ISO/IEC 14496-3:2001(E) §4.A.1 Table 4.A.8.
//
// The codebook is a complete prefix code: Σᵢ 2^(12 − Lᵢ) = 4096 = 2¹².
// This is exhaustively verified by the `hcod7_is_complete` regression
// test (which walks every 12-bit prefix and asserts each maps to
// exactly one entry).
//
// Codebook 7 is the first unsigned pair spectrum book (Table 4.95 row 7:
// `unsigned_cb = 1`, `dim = 2`, `LAV = 7` → `8^2 = 64` entries, each
// coefficient in `0..=7`). The §4.6.3.3 polynomial
// `idx = y * (LAV + 1) + z = y * 8 + z` places the zero-tuple `(0, 0)`
// at index 0 (the origin of the unsigned dim-2 lattice) and the maximum
// tuple `(7, 7)` at index 63 (the far corner). Because Codebook 7 is
// unsigned, a sign-bit suffix follows the Huffman codeword for every
// non-zero coefficient per §4.6.3.3 — the suffix is delivered by
// `crate::spectral_codebook::apply_sign_bits` /
// `crate::spectral_codebook::derive_sign_bits`, separate from the
// Huffman codeword carried here.

/// Number of entries in Table 4.A.8 (`64`, indices `0..=63`).
pub const HCOD7_NUM_ENTRIES: usize = 64;

/// Maximum codeword length emitted by Table 4.A.8 (12 bits).
pub const HCOD7_MAX_LEN: u32 = 12;

/// Table 4.A.8 — `(length_in_bits, codeword)` per index `0..=63`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD7: [(u8, u16); HCOD7_NUM_ENTRIES] = [
    (1, 0x000),  // 0  — zero-tuple (y, z) = (0, 0), 1-bit `0`
    (3, 0x005),  // 1
    (6, 0x037),  // 2
    (7, 0x074),  // 3
    (8, 0x0f2),  // 4
    (9, 0x1eb),  // 5
    (10, 0x3ed), // 6
    (11, 0x7f7), // 7
    (3, 0x004),  // 8
    (4, 0x00c),  // 9
    (6, 0x035),  // 10
    (7, 0x071),  // 11
    (8, 0x0ec),  // 12
    (8, 0x0ee),  // 13
    (9, 0x1ee),  // 14
    (9, 0x1f5),  // 15
    (6, 0x036),  // 16
    (6, 0x034),  // 17
    (7, 0x072),  // 18
    (8, 0x0ea),  // 19
    (8, 0x0f1),  // 20
    (9, 0x1e9),  // 21
    (9, 0x1f3),  // 22
    (10, 0x3f5), // 23
    (7, 0x073),  // 24
    (7, 0x070),  // 25
    (8, 0x0eb),  // 26
    (8, 0x0f0),  // 27
    (9, 0x1f1),  // 28
    (9, 0x1f0),  // 29
    (10, 0x3ec), // 30
    (10, 0x3fa), // 31
    (8, 0x0f3),  // 32
    (8, 0x0ed),  // 33
    (9, 0x1e8),  // 34
    (9, 0x1ef),  // 35
    (10, 0x3ef), // 36
    (10, 0x3f1), // 37
    (10, 0x3f9), // 38
    (11, 0x7fb), // 39
    (9, 0x1ed),  // 40
    (8, 0x0ef),  // 41
    (9, 0x1ea),  // 42
    (9, 0x1f2),  // 43
    (10, 0x3f3), // 44
    (10, 0x3f8), // 45
    (11, 0x7f9), // 46
    (11, 0x7fc), // 47
    (10, 0x3ee), // 48
    (9, 0x1ec),  // 49
    (9, 0x1f4),  // 50
    (10, 0x3f4), // 51
    (10, 0x3f7), // 52
    (11, 0x7f8), // 53
    (12, 0xffd), // 54
    (12, 0xffe), // 55
    (11, 0x7f6), // 56
    (10, 0x3f0), // 57
    (10, 0x3f2), // 58
    (10, 0x3f6), // 59
    (11, 0x7fa), // 60
    (11, 0x7fd), // 61
    (12, 0xffc), // 62
    (12, 0xfff), // 63 — far corner (y, z) = (7, 7)
];

/// Encode a Codebook 7 codeword index (`0..=63`) to the wire Huffman
/// codeword from Table 4.A.8.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=63` (the 64-entry `8^2` enumeration of every legal
/// unsigned pair with each coefficient in `0..=7`).
///
/// The inverse of [`hcod7_decode`]. Because Codebook 7 is unsigned,
/// callers transmit one sign bit after the codeword for each non-zero
/// coefficient via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
/// [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
/// — the §4.6.3.3 suffix sits outside the Huffman codeword carried
/// here.
pub fn hcod7_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD7
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(7))?;
    Ok(*entry)
}

/// Decode one Codebook 7 Huffman codeword from `reader`, returning
/// the codeword index in `0..=63`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 64-entry table. The table is
/// small (max codeword length 12 bits, 64 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 12 bits (Kraft
/// equality `Σᵢ 2^(12 − Lᵢ) = 4096 = 2¹²`), so any 12-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 12 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod7_is_complete` regression test that exhaustively
/// walks all `2¹²` 12-bit prefixes.
///
/// The §4.6.3.3 sign-bit suffix lies outside this routine — for
/// unsigned Codebook 7 the caller consumes one sign bit per non-zero
/// coefficient after the Huffman codeword via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits).
pub fn hcod7_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD7_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD7.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD7 is a complete 12-bit prefix code. The
    // `hcod7_is_complete` regression test verifies every 12-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD7 is a complete 12-bit prefix code; the 12-bit walk must match");
}

/// Write a Codebook 7 codeword to `writer` by index.
///
/// Convenience over `hcod7_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 63`. The
/// §4.6.3.3 sign-bit suffix is the caller's responsibility (one
/// suffix bit per non-zero coefficient, low-frequency-first).
pub fn hcod7_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod7_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =============================================================================
// Table 4.A.9 — Spectrum Huffman Codebook 8
// =============================================================================
//
// 64 entries, indices 0..=63. Each row is `(length_in_bits,
// codeword)` with `codeword` right-aligned in a `u16` (MSB of the
// wire codeword at bit `length − 1`). Transcribed verbatim from
// ISO/IEC 14496-3:2001(E) §4.A.1 Table 4.A.9 (page 198).
//
// The codebook is a complete prefix code: Σᵢ 2^(10 − Lᵢ) = 1024 = 2¹⁰.
// This is exhaustively verified by the `hcod8_is_complete` regression
// test (which walks every 10-bit prefix and asserts each maps to
// exactly one entry).
//
// Codebook 8 is the second unsigned pair spectrum book — it shares
// Codebook 7's Table 4.95 row shape (row 8 column-for-column matches
// row 7 except for the `Codebook listed in Table` cell pointing at
// Table 4.A.9): `unsigned_cb = 1`, `dim = 2`, `LAV = 7` → `(7 + 1)^2
// = 8^2 = 64` entries, each coefficient in `0..=7`. The §4.6.3.3
// unsigned polynomial `idx = y * (LAV + 1) + z = y * 8 + z` places
// the zero-tuple `(0, 0)` at index 0, the interior `(1, 1)` at
// index 9, and the far corner `(7, 7)` at index 63 — the same head
// and far-corner placements Codebook 7 also uses for its unsigned
// dim-2 universe. The Huffman-length tuning differs: Codebook 8
// lifts the zero-tuple off the single-bit codeword (now 5 bits at
// index 0) and migrates the shortest codeword (3 bits `0b000`) to
// the interior tuple `(1, 1)` at index 9. The maximum codeword
// length is 10 bits; exactly four rows reach the ceiling
// (indices 7, 47, 56, 63).
//
// Because Codebook 8 is unsigned, a sign-bit suffix follows the
// Huffman codeword for every non-zero coefficient per §4.6.3.3 —
// the suffix is delivered by `crate::spectral_codebook::apply_sign_bits`
// / `crate::spectral_codebook::derive_sign_bits`, separate from the
// Huffman codeword carried here.

/// Number of entries in Table 4.A.9 (`64`, indices `0..=63`).
pub const HCOD8_NUM_ENTRIES: usize = 64;

/// Maximum codeword length emitted by Table 4.A.9 (10 bits).
pub const HCOD8_MAX_LEN: u32 = 10;

/// Table 4.A.9 — `(length_in_bits, codeword)` per index `0..=63`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD8: [(u8, u16); HCOD8_NUM_ENTRIES] = [
    (5, 0x00e),  // 0 — zero-tuple (y, z) = (0, 0)
    (4, 0x005),  // 1
    (5, 0x010),  // 2
    (6, 0x030),  // 3
    (7, 0x06f),  // 4
    (8, 0x0f1),  // 5
    (9, 0x1fa),  // 6
    (10, 0x3fe), // 7
    (4, 0x003),  // 8
    (3, 0x000),  // 9 — interior (y, z) = (1, 1), shortest 3-bit `0`
    (4, 0x004),  // 10
    (5, 0x012),  // 11
    (6, 0x02c),  // 12
    (7, 0x06a),  // 13
    (7, 0x075),  // 14
    (8, 0x0f8),  // 15
    (5, 0x00f),  // 16
    (4, 0x002),  // 17
    (4, 0x006),  // 18
    (5, 0x014),  // 19
    (6, 0x02e),  // 20
    (7, 0x069),  // 21
    (7, 0x072),  // 22
    (8, 0x0f5),  // 23
    (6, 0x02f),  // 24
    (5, 0x011),  // 25
    (5, 0x013),  // 26
    (6, 0x02a),  // 27
    (6, 0x032),  // 28
    (7, 0x06c),  // 29
    (8, 0x0ec),  // 30
    (8, 0x0fa),  // 31
    (7, 0x071),  // 32
    (6, 0x02b),  // 33
    (6, 0x02d),  // 34
    (6, 0x031),  // 35
    (7, 0x06d),  // 36
    (7, 0x070),  // 37
    (8, 0x0f2),  // 38
    (9, 0x1f9),  // 39
    (8, 0x0ef),  // 40
    (7, 0x068),  // 41
    (6, 0x033),  // 42
    (7, 0x06b),  // 43
    (7, 0x06e),  // 44
    (8, 0x0ee),  // 45
    (8, 0x0f9),  // 46
    (10, 0x3fc), // 47
    (9, 0x1f8),  // 48
    (7, 0x074),  // 49
    (7, 0x073),  // 50
    (8, 0x0ed),  // 51
    (8, 0x0f0),  // 52
    (8, 0x0f6),  // 53
    (9, 0x1f6),  // 54
    (9, 0x1fd),  // 55
    (10, 0x3fd), // 56
    (8, 0x0f3),  // 57
    (8, 0x0f4),  // 58
    (8, 0x0f7),  // 59
    (9, 0x1f7),  // 60
    (9, 0x1fb),  // 61
    (9, 0x1fc),  // 62
    (10, 0x3ff), // 63 — far corner (y, z) = (7, 7)
];

/// Encode a Codebook 8 codeword index (`0..=63`) to the wire Huffman
/// codeword from Table 4.A.9.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=63` (the 64-entry `8^2` enumeration of every legal
/// unsigned pair with each coefficient in `0..=7`).
///
/// The inverse of [`hcod8_decode`]. Because Codebook 8 is unsigned,
/// callers transmit one sign bit after the codeword for each non-zero
/// coefficient via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
/// [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
/// — the §4.6.3.3 suffix sits outside the Huffman codeword carried
/// here.
pub fn hcod8_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD8
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(8))?;
    Ok(*entry)
}

/// Decode one Codebook 8 Huffman codeword from `reader`, returning
/// the codeword index in `0..=63`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 64-entry table. The table is
/// small (max codeword length 10 bits, 64 entries) so a single
/// linear scan per bit-extend is cheaper than the storage and
/// build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 10 bits (Kraft
/// equality `Σᵢ 2^(10 − Lᵢ) = 1024 = 2¹⁰`), so any 10-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 10 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod8_is_complete` regression test that exhaustively
/// walks all `2¹⁰` 10-bit prefixes.
///
/// The §4.6.3.3 sign-bit suffix lies outside this routine — for
/// unsigned Codebook 8 the caller consumes one sign bit per non-zero
/// coefficient after the Huffman codeword via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits).
pub fn hcod8_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD8_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD8.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD8 is a complete 10-bit prefix code. The
    // `hcod8_is_complete` regression test verifies every 10-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD8 is a complete 10-bit prefix code; the 10-bit walk must match");
}

/// Write a Codebook 8 codeword to `writer` by index.
///
/// Convenience over `hcod8_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 63`. The
/// §4.6.3.3 sign-bit suffix is the caller's responsibility (one
/// suffix bit per non-zero coefficient, low-frequency-first).
pub fn hcod8_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod8_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =====================================================================
// Codebook 9 — Table 4.A.10
// =====================================================================
//
// Codebook 9 is the first expanded-LAV unsigned pair spectrum book —
// Table 4.95 row 9 declares `unsigned_cb = 1`, `dim = 2`, `LAV = 12`,
// so the §4.6.3.3 universe shifts to `(12 + 1)^2 = 13^2 = 169`
// entries indexed `0..=168` with each `(y, z)` coefficient in
// `0..=12`. That is a substantial step up from Codebooks 7 and 8's
// shared `8 × 8 = 64`-entry unsigned pair lattice — the `169 / 64 ≈
// 2.6×` universe expansion widens the distribution's tail and lifts
// the codeword ceiling from Codebook 8's 10 bits to **15 bits**, the
// widest non-ESC codeword in the entire Annex 4.A book set. The
// §4.6.3.3 unsigned polynomial `idx = y * (LAV + 1) + z = y * 13 + z`
// places the zero-tuple `(0, 0)` at index 0 and the maximum tuple
// `(12, 12)` at index 168 (`12 * 13 + 12 = 168`). The single-bit
// codeword `0` parks at index 0 — the same shortest-codeword head
// placement Codebook 7 uses for its zero-tuple. Exactly four rows
// reach the 15-bit ceiling (indices 142, 154, 155, 168) — the
// rarest pair magnitudes near the `LAV = 12` cap.
//
// Because Codebook 9 is unsigned, a sign-bit suffix follows the
// Huffman codeword for every non-zero coefficient per §4.6.3.3 —
// the suffix is delivered by `crate::spectral_codebook::apply_sign_bits`
// / `crate::spectral_codebook::derive_sign_bits`, separate from the
// Huffman codeword carried here.

/// Number of entries in Table 4.A.10 (`169`, indices `0..=168`).
pub const HCOD9_NUM_ENTRIES: usize = 169;

/// Maximum codeword length emitted by Table 4.A.10 (15 bits).
pub const HCOD9_MAX_LEN: u32 = 15;

/// Table 4.A.10 — `(length_in_bits, codeword)` per index `0..=168`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD9: [(u8, u16); HCOD9_NUM_ENTRIES] = [
    (1, 0x0000),  // 0 — zero-tuple (y, z) = (0, 0), shortest 1-bit `0`
    (3, 0x0005),  // 1
    (6, 0x0037),  // 2
    (8, 0x00e7),  // 3
    (9, 0x01de),  // 4
    (10, 0x03ce), // 5
    (10, 0x03d9), // 6
    (11, 0x07c8), // 7
    (11, 0x07cd), // 8
    (12, 0x0fc8), // 9
    (12, 0x0fdd), // 10
    (13, 0x1fe4), // 11
    (13, 0x1fec), // 12
    (3, 0x0004),  // 13
    (4, 0x000c),  // 14 — interior (y, z) = (1, 1)
    (6, 0x0035),  // 15
    (7, 0x0072),  // 16
    (8, 0x00ea),  // 17
    (8, 0x00ed),  // 18
    (9, 0x01e2),  // 19
    (10, 0x03d1), // 20
    (10, 0x03d3), // 21
    (10, 0x03e0), // 22
    (11, 0x07d8), // 23
    (12, 0x0fcf), // 24
    (12, 0x0fd5), // 25
    (6, 0x0036),  // 26
    (6, 0x0034),  // 27
    (7, 0x0071),  // 28
    (8, 0x00e8),  // 29
    (8, 0x00ec),  // 30
    (9, 0x01e1),  // 31
    (10, 0x03cf), // 32
    (10, 0x03dd), // 33
    (10, 0x03db), // 34
    (11, 0x07d0), // 35
    (12, 0x0fc7), // 36
    (12, 0x0fd4), // 37
    (12, 0x0fe4), // 38
    (8, 0x00e6),  // 39
    (7, 0x0070),  // 40
    (8, 0x00e9),  // 41
    (9, 0x01dd),  // 42
    (9, 0x01e3),  // 43
    (10, 0x03d2), // 44
    (10, 0x03dc), // 45
    (11, 0x07cc), // 46
    (11, 0x07ca), // 47
    (11, 0x07de), // 48
    (12, 0x0fd8), // 49
    (12, 0x0fea), // 50
    (13, 0x1fdb), // 51
    (9, 0x01df),  // 52
    (8, 0x00eb),  // 53
    (9, 0x01dc),  // 54
    (9, 0x01e6),  // 55
    (10, 0x03d5), // 56
    (10, 0x03de), // 57
    (11, 0x07cb), // 58
    (11, 0x07dd), // 59
    (11, 0x07dc), // 60
    (12, 0x0fcd), // 61
    (12, 0x0fe2), // 62
    (12, 0x0fe7), // 63
    (13, 0x1fe1), // 64
    (10, 0x03d0), // 65
    (9, 0x01e0),  // 66
    (9, 0x01e4),  // 67
    (10, 0x03d6), // 68
    (11, 0x07c5), // 69
    (11, 0x07d1), // 70
    (11, 0x07db), // 71
    (12, 0x0fd2), // 72
    (11, 0x07e0), // 73
    (12, 0x0fd9), // 74
    (12, 0x0feb), // 75
    (13, 0x1fe3), // 76
    (13, 0x1fe9), // 77
    (11, 0x07c4), // 78
    (9, 0x01e5),  // 79
    (10, 0x03d7), // 80
    (11, 0x07c6), // 81
    (11, 0x07cf), // 82
    (11, 0x07da), // 83
    (12, 0x0fcb), // 84
    (12, 0x0fda), // 85
    (12, 0x0fe3), // 86
    (12, 0x0fe9), // 87
    (13, 0x1fe6), // 88
    (13, 0x1ff3), // 89
    (13, 0x1ff7), // 90
    (11, 0x07d3), // 91
    (10, 0x03d8), // 92
    (10, 0x03e1), // 93
    (11, 0x07d4), // 94
    (11, 0x07d9), // 95
    (12, 0x0fd3), // 96
    (12, 0x0fde), // 97
    (13, 0x1fdd), // 98
    (13, 0x1fd9), // 99
    (13, 0x1fe2), // 100
    (13, 0x1fea), // 101
    (13, 0x1ff1), // 102
    (13, 0x1ff6), // 103
    (11, 0x07d2), // 104
    (10, 0x03d4), // 105
    (10, 0x03da), // 106
    (11, 0x07c7), // 107
    (11, 0x07d7), // 108
    (11, 0x07e2), // 109
    (12, 0x0fce), // 110
    (12, 0x0fdb), // 111
    (13, 0x1fd8), // 112
    (13, 0x1fee), // 113
    (14, 0x3ff0), // 114
    (13, 0x1ff4), // 115
    (14, 0x3ff2), // 116
    (11, 0x07e1), // 117
    (10, 0x03df), // 118
    (11, 0x07c9), // 119
    (11, 0x07d6), // 120
    (12, 0x0fca), // 121
    (12, 0x0fd0), // 122
    (12, 0x0fe5), // 123
    (12, 0x0fe6), // 124
    (13, 0x1feb), // 125
    (13, 0x1fef), // 126
    (14, 0x3ff3), // 127
    (14, 0x3ff4), // 128
    (14, 0x3ff5), // 129
    (12, 0x0fe0), // 130
    (11, 0x07ce), // 131
    (11, 0x07d5), // 132
    (12, 0x0fc6), // 133
    (12, 0x0fd1), // 134
    (12, 0x0fe1), // 135
    (13, 0x1fe0), // 136
    (13, 0x1fe8), // 137
    (13, 0x1ff0), // 138
    (14, 0x3ff1), // 139
    (14, 0x3ff8), // 140
    (14, 0x3ff6), // 141
    (15, 0x7ffc), // 142
    (12, 0x0fe8), // 143
    (11, 0x07df), // 144
    (12, 0x0fc9), // 145
    (12, 0x0fd7), // 146
    (12, 0x0fdc), // 147
    (13, 0x1fdc), // 148
    (13, 0x1fdf), // 149
    (13, 0x1fed), // 150
    (13, 0x1ff5), // 151
    (14, 0x3ff9), // 152
    (14, 0x3ffb), // 153
    (15, 0x7ffd), // 154
    (15, 0x7ffe), // 155
    (13, 0x1fe7), // 156
    (12, 0x0fcc), // 157
    (12, 0x0fd6), // 158
    (12, 0x0fdf), // 159
    (13, 0x1fde), // 160
    (13, 0x1fda), // 161
    (13, 0x1fe5), // 162
    (13, 0x1ff2), // 163
    (14, 0x3ffa), // 164
    (14, 0x3ff7), // 165
    (14, 0x3ffc), // 166
    (14, 0x3ffd), // 167
    (15, 0x7fff), // 168 — far corner (y, z) = (12, 12)
];

/// Encode a Codebook 9 codeword index (`0..=168`) to the wire Huffman
/// codeword from Table 4.A.10.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=168` (the 169-entry `13^2` enumeration of every
/// legal unsigned pair with each coefficient in `0..=12`).
///
/// The inverse of [`hcod9_decode`]. Because Codebook 9 is unsigned,
/// callers transmit one sign bit after the codeword for each non-zero
/// coefficient via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
/// [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
/// — the §4.6.3.3 suffix sits outside the Huffman codeword carried
/// here.
pub fn hcod9_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD9
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(9))?;
    Ok(*entry)
}

/// Decode one Codebook 9 Huffman codeword from `reader`, returning
/// the codeword index in `0..=168`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 169-entry table. The table is
/// small enough (max codeword length 15 bits, 169 entries) that a
/// single linear scan per bit-extend is cheaper than the storage
/// and build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 15 bits (Kraft
/// equality `Σᵢ 2^(15 − Lᵢ) = 32768 = 2¹⁵`), so any 15-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 15 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod9_is_complete` regression test that exhaustively
/// walks all `2¹⁵` 15-bit prefixes.
///
/// The §4.6.3.3 sign-bit suffix lies outside this routine — for
/// unsigned Codebook 9 the caller consumes one sign bit per non-zero
/// coefficient after the Huffman codeword via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits).
pub fn hcod9_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD9_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD9.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD9 is a complete 15-bit prefix code. The
    // `hcod9_is_complete` regression test verifies every 15-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD9 is a complete 15-bit prefix code; the 15-bit walk must match");
}

/// Write a Codebook 9 codeword to `writer` by index.
///
/// Convenience over `hcod9_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 168`. The
/// §4.6.3.3 sign-bit suffix is the caller's responsibility (one
/// suffix bit per non-zero coefficient, low-frequency-first).
pub fn hcod9_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod9_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =====================================================================
// Codebook 10 — Table 4.A.11
// =====================================================================
//
// Codebook 10 is the second expanded-LAV unsigned pair spectrum book —
// Table 4.95 row 10 mirrors Codebook 9's row 9 column-for-column
// (`unsigned_cb = 1`, `dim = 2`, `LAV = 12`) so the §4.6.3.3 universe
// is the same `13 × 13 = 169`-entry lattice indexed `0..=168` with
// each `(y, z)` coefficient in `0..=12`. The §4.6.3.3 unsigned
// polynomial `idx = y * (LAV + 1) + z = y * 13 + z` places the
// zero-tuple `(0, 0)` at index 0 and the maximum tuple `(12, 12)` at
// index 168 (`12 * 13 + 12 = 168`). Where Codebook 9 parks the
// single-bit codeword `0` on the zero-tuple at index 0, Codebook 10
// lifts the zero-tuple to a 6-bit `0b100010` (`0x22`) and migrates
// the shortest codeword (4 bits) onto the interior `(1, 1)` at
// index 14 with codeword `0b0000` — the same head-displacement
// pattern Codebook 8 uses to relocate its shortest slot off the
// zero-tuple. Exactly three rows reach the 4-bit floor (indices
// 14, 15, 27 with codewords `0x0`, `0x1`, `0x2`), reflecting an
// encoder target whose magnitude statistics are denser around
// `(±1, ±1) .. (±2, ±2)` than Codebook 9's zero-heavy distribution.
// The maximum codeword length is **12 bits** — a 3-bit pull-down
// from Codebook 9's 15-bit ceiling — and exactly eight rows reach
// that 12-bit ceiling (indices 12, 129, 142, 155, 165, 166, 167,
// 168 with codewords `0xffd, 0xffa, 0xff9, 0xffb, 0xff8, 0xffe,
// 0xffc, 0xfff`), the rarest pair magnitudes near the `LAV = 12`
// cap.
//
// Because Codebook 10 is unsigned, a sign-bit suffix follows the
// Huffman codeword for every non-zero coefficient per §4.6.3.3 —
// the suffix is delivered by `crate::spectral_codebook::apply_sign_bits`
// / `crate::spectral_codebook::derive_sign_bits`, separate from the
// Huffman codeword carried here.

/// Number of entries in Table 4.A.11 (`169`, indices `0..=168`).
pub const HCOD10_NUM_ENTRIES: usize = 169;

/// Maximum codeword length emitted by Table 4.A.11 (12 bits).
pub const HCOD10_MAX_LEN: u32 = 12;

/// Table 4.A.11 — `(length_in_bits, codeword)` per index `0..=168`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
const HCOD10: [(u8, u16); HCOD10_NUM_ENTRIES] = [
    (6, 0x0022),  // 0 — zero-tuple (y, z) = (0, 0)
    (5, 0x0008),  // 1
    (6, 0x001d),  // 2
    (6, 0x0026),  // 3
    (7, 0x005f),  // 4
    (8, 0x00d3),  // 5
    (9, 0x01cf),  // 6
    (10, 0x03d0), // 7
    (10, 0x03d7), // 8
    (10, 0x03ed), // 9
    (11, 0x07f0), // 10
    (11, 0x07f6), // 11
    (12, 0x0ffd), // 12
    (5, 0x0007),  // 13
    (4, 0x0000),  // 14 — interior (y, z) = (1, 1), shortest 4-bit `0b0000`
    (4, 0x0001),  // 15
    (5, 0x0009),  // 16
    (6, 0x0020),  // 17
    (7, 0x0054),  // 18
    (7, 0x0060),  // 19
    (8, 0x00d5),  // 20
    (8, 0x00dc),  // 21
    (9, 0x01d4),  // 22
    (10, 0x03cd), // 23
    (10, 0x03de), // 24
    (11, 0x07e7), // 25
    (6, 0x001c),  // 26
    (4, 0x0002),  // 27
    (5, 0x0006),  // 28
    (5, 0x000c),  // 29
    (6, 0x001e),  // 30
    (6, 0x0028),  // 31
    (7, 0x005b),  // 32
    (8, 0x00cd),  // 33
    (8, 0x00d9),  // 34
    (9, 0x01ce),  // 35
    (9, 0x01dc),  // 36
    (10, 0x03d9), // 37
    (10, 0x03f1), // 38
    (6, 0x0025),  // 39
    (5, 0x000b),  // 40
    (5, 0x000a),  // 41
    (5, 0x000d),  // 42
    (6, 0x0024),  // 43
    (7, 0x0057),  // 44
    (7, 0x0061),  // 45
    (8, 0x00cc),  // 46
    (8, 0x00dd),  // 47
    (9, 0x01cc),  // 48
    (9, 0x01de),  // 49
    (10, 0x03d3), // 50
    (10, 0x03e7), // 51
    (7, 0x005d),  // 52
    (6, 0x0021),  // 53
    (6, 0x001f),  // 54
    (6, 0x0023),  // 55
    (6, 0x0027),  // 56
    (7, 0x0059),  // 57
    (7, 0x0064),  // 58
    (8, 0x00d8),  // 59
    (8, 0x00df),  // 60
    (9, 0x01d2),  // 61
    (9, 0x01e2),  // 62
    (10, 0x03dd), // 63
    (10, 0x03ee), // 64
    (8, 0x00d1),  // 65
    (7, 0x0055),  // 66
    (6, 0x0029),  // 67
    (7, 0x0056),  // 68
    (7, 0x0058),  // 69
    (7, 0x0062),  // 70
    (8, 0x00ce),  // 71
    (8, 0x00e0),  // 72
    (8, 0x00e2),  // 73
    (9, 0x01da),  // 74
    (10, 0x03d4), // 75
    (10, 0x03e3), // 76
    (11, 0x07eb), // 77
    (9, 0x01c9),  // 78
    (7, 0x005e),  // 79
    (7, 0x005a),  // 80
    (7, 0x005c),  // 81
    (7, 0x0063),  // 82
    (8, 0x00ca),  // 83
    (8, 0x00da),  // 84
    (9, 0x01c7),  // 85
    (9, 0x01ca),  // 86
    (9, 0x01e0),  // 87
    (10, 0x03db), // 88
    (10, 0x03e8), // 89
    (11, 0x07ec), // 90
    (9, 0x01e3),  // 91
    (8, 0x00d2),  // 92
    (8, 0x00cb),  // 93
    (8, 0x00d0),  // 94
    (8, 0x00d7),  // 95
    (8, 0x00db),  // 96
    (9, 0x01c6),  // 97
    (9, 0x01d5),  // 98
    (9, 0x01d8),  // 99
    (10, 0x03ca), // 100
    (10, 0x03da), // 101
    (11, 0x07ea), // 102
    (11, 0x07f1), // 103
    (9, 0x01e1),  // 104
    (8, 0x00d4),  // 105
    (8, 0x00cf),  // 106
    (8, 0x00d6),  // 107
    (8, 0x00de),  // 108
    (8, 0x00e1),  // 109
    (9, 0x01d0),  // 110
    (9, 0x01d6),  // 111
    (10, 0x03d1), // 112
    (10, 0x03d5), // 113
    (10, 0x03f2), // 114
    (11, 0x07ee), // 115
    (11, 0x07fb), // 116
    (10, 0x03e9), // 117
    (9, 0x01cd),  // 118
    (9, 0x01c8),  // 119
    (9, 0x01cb),  // 120
    (9, 0x01d1),  // 121
    (9, 0x01d7),  // 122
    (9, 0x01df),  // 123
    (10, 0x03cf), // 124
    (10, 0x03e0), // 125
    (10, 0x03ef), // 126
    (11, 0x07e6), // 127
    (11, 0x07f8), // 128
    (12, 0x0ffa), // 129
    (10, 0x03eb), // 130
    (9, 0x01dd),  // 131
    (9, 0x01d3),  // 132
    (9, 0x01d9),  // 133
    (9, 0x01db),  // 134
    (10, 0x03d2), // 135
    (10, 0x03cc), // 136
    (10, 0x03dc), // 137
    (10, 0x03ea), // 138
    (11, 0x07ed), // 139
    (11, 0x07f3), // 140
    (11, 0x07f9), // 141
    (12, 0x0ff9), // 142
    (11, 0x07f2), // 143
    (10, 0x03ce), // 144
    (9, 0x01e4),  // 145
    (10, 0x03cb), // 146
    (10, 0x03d8), // 147
    (10, 0x03d6), // 148
    (10, 0x03e2), // 149
    (10, 0x03e5), // 150
    (11, 0x07e8), // 151
    (11, 0x07f4), // 152
    (11, 0x07f5), // 153
    (11, 0x07f7), // 154
    (12, 0x0ffb), // 155
    (11, 0x07fa), // 156
    (10, 0x03ec), // 157
    (10, 0x03df), // 158
    (10, 0x03e1), // 159
    (10, 0x03e4), // 160
    (10, 0x03e6), // 161
    (10, 0x03f0), // 162
    (11, 0x07e9), // 163
    (11, 0x07ef), // 164
    (12, 0x0ff8), // 165
    (12, 0x0ffe), // 166
    (12, 0x0ffc), // 167
    (12, 0x0fff), // 168 — far corner (y, z) = (12, 12)
];

/// Encode a Codebook 10 codeword index (`0..=168`) to the wire Huffman
/// codeword from Table 4.A.11.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=168` (the 169-entry `13^2` enumeration of every
/// legal unsigned pair with each coefficient in `0..=12`).
///
/// The inverse of [`hcod10_decode`]. Because Codebook 10 is unsigned,
/// callers transmit one sign bit after the codeword for each non-zero
/// coefficient via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
/// [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
/// — the §4.6.3.3 suffix sits outside the Huffman codeword carried
/// here.
pub fn hcod10_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD10
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(10))?;
    Ok(*entry)
}

/// Decode one Codebook 10 Huffman codeword from `reader`, returning
/// the codeword index in `0..=168`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 169-entry table. The table is
/// small enough (max codeword length 12 bits, 169 entries) that a
/// single linear scan per bit-extend is cheaper than the storage
/// and build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 12 bits (Kraft
/// equality `Σᵢ 2^(12 − Lᵢ) = 4096 = 2¹²`), so any 12-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 12 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod10_is_complete` regression test that
/// exhaustively walks all `2¹²` 12-bit prefixes.
///
/// The §4.6.3.3 sign-bit suffix lies outside this routine — for
/// unsigned Codebook 10 the caller consumes one sign bit per
/// non-zero coefficient after the Huffman codeword via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits).
pub fn hcod10_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD10_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD10.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD10 is a complete 12-bit prefix code. The
    // `hcod10_is_complete` regression test verifies every 12-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD10 is a complete 12-bit prefix code; the 12-bit walk must match");
}

/// Write a Codebook 10 codeword to `writer` by index.
///
/// Convenience over `hcod10_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 168`. The
/// §4.6.3.3 sign-bit suffix is the caller's responsibility (one
/// suffix bit per non-zero coefficient, low-frequency-first).
pub fn hcod10_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod10_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

// =====================================================================
// Codebook 11 — Table 4.A.12
// =====================================================================
//
// Codebook 11 is the only AAC spectrum book that carries an **escape
// (ESC) sequence**. Table 4.95 row 11 declares `unsigned_cb = 1`,
// `dim = 2`, `LAV = 16` and an ESC threshold of `8191` — the §4.6.1.3
// `x_quant` ceiling. The in-band Huffman universe is therefore the
// `(LAV + 1)^dim = 17^2 = 289`-entry lattice indexed `0..=288` with
// each `(y, z)` coefficient in `0..=16`. A coefficient value of `16`
// in either `y` or `z` is **not** a literal 16: per §4.6.3.3 it is the
// `escape_flag` that signals an `escape_sequence` follows the
// Huffman codeword (and any sign-bit suffix). The
// `escape_sequence` is a unary `escape_prefix` of N `1` bits, a
// `0` `escape_separator`, and an `(N + 4)`-bit unsigned `escape_word`,
// whose reconstructed magnitude is `2^(N + 4) + escape_word`. The
// ESC bridge sits in [`crate::spectral_codebook::decode_esc_value`]
// / [`crate::spectral_codebook::encode_esc_value`] and is **not**
// part of the Huffman codeword carried here; this module's
// `hcod11_encode` / `hcod11_decode` cover the codeword only.
//
// The §4.6.3.3 unsigned polynomial `idx = y * (LAV + 1) + z = y * 17
// + z` parks the zero-tuple `(0, 0)` at index 0 with the 4-bit
// codeword `0b0000` — the shortest slot. The interior pair `(1, 1)`
// lives at index `1 * 17 + 1 = 18` and shares the 4-bit floor with
// the zero-tuple (codeword `0b0001`). The far corner `(16, 16)` —
// both coefficients flagged as ESC — lives at index `16 * 17 + 16 =
// 288` with the 5-bit `0b00100` (`0x04`); the `(0, 16)` half-ESC
// tuple lives at index 16 with the 10-bit `0x38e`; the `(16, 0)`
// half-ESC tuple lives at index `16 * 17 = 272` with the 9-bit
// `0x1c2`. The maximum codeword length is **12 bits** — matching
// Codebook 10's ceiling — and exactly six rows reach that 12-bit
// ceiling (indices 12, 14, 15, 255, 269, 270 with codewords
// `0xffb`, `0xffa`, `0xffe`, `0xffd`, `0xffc`, `0xfff`). Exactly two
// rows reach the 4-bit floor: indices 0 and 18 (the zero-tuple and
// the interior `(1, 1)` pair). The codeword-length histogram is
// `{4: 2, 5: 6, 6: 7, 7: 16, 8: 59, 9: 55, 10: 95, 11: 43, 12: 6}`.
//
// Because Codebook 11 is unsigned, a sign-bit suffix follows the
// Huffman codeword for every non-zero coefficient per §4.6.3.3 —
// the suffix is delivered by `crate::spectral_codebook::apply_sign_bits`
// / `crate::spectral_codebook::derive_sign_bits`, separate from the
// Huffman codeword carried here. The §4.6.3.3 wire layout for an
// in-band coefficient pair is: `<Huffman codeword>` then `0..=2`
// sign bits (one per non-zero coefficient). When `y` or `z` is at
// the ESC threshold (`= 16`), the wire layout extends with the
// `escape_sequence` bridge per §4.6.3 (handled outside this
// module).

/// Number of entries in Table 4.A.12 (`289`, indices `0..=288`).
pub const HCOD11_NUM_ENTRIES: usize = 289;

/// Maximum codeword length emitted by Table 4.A.12 (12 bits).
pub const HCOD11_MAX_LEN: u32 = 12;

/// Table 4.A.12 — `(length_in_bits, codeword)` per index `0..=288`.
///
/// Codewords are right-aligned within the `u16`. To emit one
/// bit-for-bit, write `codeword` as `length` bits MSB-first.
///
/// A coefficient value of `16` in either slot of the decoded
/// `(y, z)` pair is the §4.6.3.3 `escape_flag` — the actual
/// magnitude is reconstructed by the
/// [`crate::spectral_codebook::decode_esc_value`] bridge from the
/// `escape_sequence` that follows the Huffman codeword (and any
/// sign-bit suffix) on the wire.
const HCOD11: [(u8, u16); HCOD11_NUM_ENTRIES] = [
    (4, 0x0000),  // 0 — zero-tuple (y, z) = (0, 0)
    (5, 0x0006),  // 1
    (6, 0x0019),  // 2
    (7, 0x003d),  // 3
    (8, 0x009c),  // 4
    (8, 0x00c6),  // 5
    (9, 0x01a7),  // 6
    (10, 0x0390), // 7
    (10, 0x03c2), // 8
    (10, 0x03df), // 9
    (11, 0x07e6), // 10
    (11, 0x07f3), // 11
    (12, 0x0ffb), // 12
    (11, 0x07ec), // 13
    (12, 0x0ffa), // 14
    (12, 0x0ffe), // 15
    (10, 0x038e), // 16 — (y, z) = (0, 16) — z at ESC threshold
    (5, 0x0005),  // 17
    (4, 0x0001),  // 18 — interior (y, z) = (1, 1), shortest 4-bit `0b0000`
    (5, 0x0008),  // 19
    (6, 0x0014),  // 20
    (7, 0x0037),  // 21
    (7, 0x0042),  // 22
    (8, 0x0092),  // 23
    (8, 0x00af),  // 24
    (9, 0x0191),  // 25
    (9, 0x01a5),  // 26
    (9, 0x01b5),  // 27
    (10, 0x039e), // 28
    (10, 0x03c0), // 29
    (10, 0x03a2), // 30
    (10, 0x03cd), // 31
    (11, 0x07d6), // 32
    (8, 0x00ae),  // 33
    (6, 0x0017),  // 34
    (5, 0x0007),  // 35
    (5, 0x0009),  // 36
    (6, 0x0018),  // 37
    (7, 0x0039),  // 38
    (7, 0x0040),  // 39
    (8, 0x008e),  // 40
    (8, 0x00a3),  // 41
    (8, 0x00b8),  // 42
    (9, 0x0199),  // 43
    (9, 0x01ac),  // 44
    (9, 0x01c1),  // 45
    (10, 0x03b1), // 46
    (10, 0x0396), // 47
    (10, 0x03be), // 48
    (10, 0x03ca), // 49
    (8, 0x009d),  // 50
    (7, 0x003c),  // 51
    (6, 0x0015),  // 52
    (6, 0x0016),  // 53
    (6, 0x001a),  // 54
    (7, 0x003b),  // 55
    (7, 0x0044),  // 56
    (8, 0x0091),  // 57
    (8, 0x00a5),  // 58
    (8, 0x00be),  // 59
    (9, 0x0196),  // 60
    (9, 0x01ae),  // 61
    (9, 0x01b9),  // 62
    (10, 0x03a1), // 63
    (10, 0x0391), // 64
    (10, 0x03a5), // 65
    (10, 0x03d5), // 66
    (8, 0x0094),  // 67
    (8, 0x009a),  // 68
    (7, 0x0036),  // 69
    (7, 0x0038),  // 70
    (7, 0x003a),  // 71
    (7, 0x0041),  // 72
    (8, 0x008c),  // 73
    (8, 0x009b),  // 74
    (8, 0x00b0),  // 75
    (8, 0x00c3),  // 76
    (9, 0x019e),  // 77
    (9, 0x01ab),  // 78
    (9, 0x01bc),  // 79
    (10, 0x039f), // 80
    (10, 0x038f), // 81
    (10, 0x03a9), // 82
    (10, 0x03cf), // 83
    (8, 0x0093),  // 84
    (8, 0x00bf),  // 85
    (7, 0x003e),  // 86
    (7, 0x003f),  // 87
    (7, 0x0043),  // 88
    (7, 0x0045),  // 89
    (8, 0x009e),  // 90
    (8, 0x00a7),  // 91
    (8, 0x00b9),  // 92
    (9, 0x0194),  // 93
    (9, 0x01a2),  // 94
    (9, 0x01ba),  // 95
    (9, 0x01c3),  // 96
    (10, 0x03a6), // 97
    (10, 0x03a7), // 98
    (10, 0x03bb), // 99
    (10, 0x03d4), // 100
    (8, 0x009f),  // 101
    (9, 0x01a0),  // 102
    (8, 0x008f),  // 103
    (8, 0x008d),  // 104
    (8, 0x0090),  // 105
    (8, 0x0098),  // 106
    (8, 0x00a6),  // 107
    (8, 0x00b6),  // 108
    (8, 0x00c4),  // 109
    (9, 0x019f),  // 110
    (9, 0x01af),  // 111
    (9, 0x01bf),  // 112
    (10, 0x0399), // 113
    (10, 0x03bf), // 114
    (10, 0x03b4), // 115
    (10, 0x03c9), // 116
    (10, 0x03e7), // 117
    (8, 0x00a8),  // 118
    (9, 0x01b6),  // 119
    (8, 0x00ab),  // 120
    (8, 0x00a4),  // 121
    (8, 0x00aa),  // 122
    (8, 0x00b2),  // 123
    (8, 0x00c2),  // 124
    (8, 0x00c5),  // 125
    (9, 0x0198),  // 126
    (9, 0x01a4),  // 127
    (9, 0x01b8),  // 128
    (10, 0x038c), // 129
    (10, 0x03a4), // 130
    (10, 0x03c4), // 131
    (10, 0x03c6), // 132
    (10, 0x03dd), // 133
    (10, 0x03e8), // 134
    (8, 0x00ad),  // 135
    (10, 0x03af), // 136
    (9, 0x0192),  // 137
    (8, 0x00bd),  // 138
    (8, 0x00bc),  // 139
    (9, 0x018e),  // 140
    (9, 0x0197),  // 141
    (9, 0x019a),  // 142
    (9, 0x01a3),  // 143
    (9, 0x01b1),  // 144
    (10, 0x038d), // 145
    (10, 0x0398), // 146
    (10, 0x03b7), // 147
    (10, 0x03d3), // 148
    (10, 0x03d1), // 149
    (10, 0x03db), // 150
    (11, 0x07dd), // 151
    (8, 0x00b4),  // 152
    (10, 0x03de), // 153
    (9, 0x01a9),  // 154
    (9, 0x019b),  // 155
    (9, 0x019c),  // 156
    (9, 0x01a1),  // 157
    (9, 0x01aa),  // 158
    (9, 0x01ad),  // 159
    (9, 0x01b3),  // 160
    (10, 0x038b), // 161
    (10, 0x03b2), // 162
    (10, 0x03b8), // 163
    (10, 0x03ce), // 164
    (10, 0x03e1), // 165
    (10, 0x03e0), // 166
    (11, 0x07d2), // 167
    (11, 0x07e5), // 168
    (8, 0x00b7),  // 169
    (11, 0x07e3), // 170
    (9, 0x01bb),  // 171
    (9, 0x01a8),  // 172
    (9, 0x01a6),  // 173
    (9, 0x01b0),  // 174
    (9, 0x01b2),  // 175
    (9, 0x01b7),  // 176
    (10, 0x039b), // 177
    (10, 0x039a), // 178
    (10, 0x03ba), // 179
    (10, 0x03b5), // 180
    (10, 0x03d6), // 181
    (11, 0x07d7), // 182
    (10, 0x03e4), // 183
    (11, 0x07d8), // 184
    (11, 0x07ea), // 185
    (8, 0x00ba),  // 186
    (11, 0x07e8), // 187
    (10, 0x03a0), // 188
    (9, 0x01bd),  // 189
    (9, 0x01b4),  // 190
    (10, 0x038a), // 191
    (9, 0x01c4),  // 192
    (10, 0x0392), // 193
    (10, 0x03aa), // 194
    (10, 0x03b0), // 195
    (10, 0x03bc), // 196
    (10, 0x03d7), // 197
    (11, 0x07d4), // 198
    (11, 0x07dc), // 199
    (11, 0x07db), // 200
    (11, 0x07d5), // 201
    (11, 0x07f0), // 202
    (8, 0x00c1),  // 203
    (11, 0x07fb), // 204
    (10, 0x03c8), // 205
    (10, 0x03a3), // 206
    (10, 0x0395), // 207
    (10, 0x039d), // 208
    (10, 0x03ac), // 209
    (10, 0x03ae), // 210
    (10, 0x03c5), // 211
    (10, 0x03d8), // 212
    (10, 0x03e2), // 213
    (10, 0x03e6), // 214
    (11, 0x07e4), // 215
    (11, 0x07e7), // 216
    (11, 0x07e0), // 217
    (11, 0x07e9), // 218
    (11, 0x07f7), // 219
    (9, 0x0190),  // 220
    (11, 0x07f2), // 221
    (10, 0x0393), // 222
    (9, 0x01be),  // 223
    (9, 0x01c0),  // 224
    (10, 0x0394), // 225
    (10, 0x0397), // 226
    (10, 0x03ad), // 227
    (10, 0x03c3), // 228
    (10, 0x03c1), // 229
    (10, 0x03d2), // 230
    (11, 0x07da), // 231
    (11, 0x07d9), // 232
    (11, 0x07df), // 233
    (11, 0x07eb), // 234
    (11, 0x07f4), // 235
    (11, 0x07fa), // 236
    (9, 0x0195),  // 237
    (11, 0x07f8), // 238
    (10, 0x03bd), // 239
    (10, 0x039c), // 240
    (10, 0x03ab), // 241
    (10, 0x03a8), // 242
    (10, 0x03b3), // 243
    (10, 0x03b9), // 244
    (10, 0x03d0), // 245
    (10, 0x03e3), // 246
    (10, 0x03e5), // 247
    (11, 0x07e2), // 248
    (11, 0x07de), // 249
    (11, 0x07ed), // 250
    (11, 0x07f1), // 251
    (11, 0x07f9), // 252
    (11, 0x07fc), // 253
    (9, 0x0193),  // 254
    (12, 0x0ffd), // 255
    (10, 0x03dc), // 256
    (10, 0x03b6), // 257
    (10, 0x03c7), // 258
    (10, 0x03cc), // 259
    (10, 0x03cb), // 260
    (10, 0x03d9), // 261
    (10, 0x03da), // 262
    (11, 0x07d3), // 263
    (11, 0x07e1), // 264
    (11, 0x07ee), // 265
    (11, 0x07ef), // 266
    (11, 0x07f5), // 267
    (11, 0x07f6), // 268
    (12, 0x0ffc), // 269
    (12, 0x0fff), // 270
    (9, 0x019d),  // 271
    (9, 0x01c2),  // 272 — (y, z) = (16, 0) — y at ESC threshold
    (8, 0x00b5),  // 273
    (8, 0x00a1),  // 274
    (8, 0x0096),  // 275
    (8, 0x0097),  // 276
    (8, 0x0095),  // 277
    (8, 0x0099),  // 278
    (8, 0x00a0),  // 279
    (8, 0x00a2),  // 280
    (8, 0x00ac),  // 281
    (8, 0x00a9),  // 282
    (8, 0x00b1),  // 283
    (8, 0x00b3),  // 284
    (8, 0x00bb),  // 285
    (8, 0x00c0),  // 286
    (9, 0x018f),  // 287
    (5, 0x0004),  // 288 — far corner (y, z) = (16, 16) — both at ESC threshold
];

/// Encode a Codebook 11 codeword index (`0..=288`) to the wire Huffman
/// codeword from Table 4.A.12.
///
/// Returns `(length_in_bits, codeword)` with `codeword` right-aligned
/// in the `u16` (MSB at bit `length − 1`). Out-of-range `idx`
/// produces [`Error::SpectralCodebookIndexOutOfRange`]; the legal
/// range is `0..=288` (the 289-entry `17^2` enumeration of every
/// legal unsigned pair with each coefficient in `0..=16` where `16`
/// is the §4.6.3.3 escape flag).
///
/// The inverse of [`hcod11_decode`]. Because Codebook 11 is unsigned,
/// callers transmit one sign bit after the codeword for each non-zero
/// coefficient via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits) /
/// [`derive_sign_bits`](crate::spectral_codebook::derive_sign_bits)
/// — the §4.6.3.3 suffix sits outside the Huffman codeword carried
/// here. When either coefficient is `16`, the ESC sequence from
/// [`encode_esc_value`](crate::spectral_codebook::encode_esc_value)
/// follows the sign-bit suffix; the ESC bridge is also outside this
/// module.
pub fn hcod11_encode(idx: u32) -> Result<(u8, u16)> {
    let entry = HCOD11
        .get(idx as usize)
        .ok_or(Error::SpectralCodebookIndexOutOfRange(11))?;
    Ok(*entry)
}

/// Decode one Codebook 11 Huffman codeword from `reader`, returning
/// the codeword index in `0..=288`.
///
/// The decoder is a straight prefix-match: read one bit at a time
/// (MSB-first), look it up in a flat 289-entry table. The table is
/// small enough (max codeword length 12 bits, 289 entries) that a
/// single linear scan per bit-extend is cheaper than the storage
/// and build-time cost of a multi-level lookup acceleration table.
/// Returns [`Error::UnexpectedEnd`] on reader underflow.
///
/// The codebook is a **complete** prefix code over 12 bits (Kraft
/// equality `Σᵢ 2^(12 − Lᵢ) = 4096 = 2¹²`), so any 12-bit prefix
/// fully read from `reader` is guaranteed to match exactly one
/// entry — the bottom of the loop is unreachable when `reader`
/// produces 12 bits without underflowing. A purely defensive
/// `unreachable!()` guards the loop fall-through; it is verified
/// dead by the `hcod11_is_complete` regression test that
/// exhaustively walks all `2¹²` 12-bit prefixes.
///
/// The §4.6.3.3 sign-bit suffix and the ESC sequence (when either
/// coefficient is `16`) lie outside this routine — for unsigned
/// Codebook 11 the caller consumes one sign bit per non-zero
/// coefficient after the Huffman codeword via
/// [`apply_sign_bits`](crate::spectral_codebook::apply_sign_bits)
/// and dispatches onto the
/// [`decode_esc_value`](crate::spectral_codebook::decode_esc_value)
/// bridge when the §4.6.3.3 index translation surfaces a `16` in
/// either slot.
pub fn hcod11_decode(reader: &mut BitReader<'_>) -> Result<u32> {
    let mut acc: u32 = 0;
    for len in 1..=HCOD11_MAX_LEN {
        let bit = reader.read_u32(1).map_err(|_| Error::UnexpectedEnd)?;
        acc = (acc << 1) | bit;
        for (idx, &(entry_len, entry_cw)) in HCOD11.iter().enumerate() {
            if u32::from(entry_len) == len && u32::from(entry_cw) == acc {
                return Ok(idx as u32);
            }
        }
    }
    // Unreachable: HCOD11 is a complete 12-bit prefix code. The
    // `hcod11_is_complete` regression test verifies every 12-bit
    // prefix maps to exactly one entry.
    unreachable!("HCOD11 is a complete 12-bit prefix code; the 12-bit walk must match");
}

/// Write a Codebook 11 codeword to `writer` by index.
///
/// Convenience over `hcod11_encode` + manual `write_u32`. Returns
/// [`Error::SpectralCodebookIndexOutOfRange`] for `idx > 288`. The
/// §4.6.3.3 sign-bit suffix and the ESC sequence are the caller's
/// responsibility — the suffix is one bit per non-zero coefficient
/// emitted low-frequency-first, and the ESC sequence is appended
/// after the sign bits for each coefficient whose value reaches the
/// `16` flag.
pub fn hcod11_write(writer: &mut BitWriter, idx: u32) -> Result<()> {
    let (len, cw) = hcod11_encode(idx)?;
    writer.write_u32(u32::from(cw), u32::from(len));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------
    // Table-shape invariants
    // -------------------------------------------------------------------

    #[test]
    fn hcod1_has_exactly_81_entries() {
        // 3^4 = 81 (signed LAV=1 → mod = 2*1+1 = 3, dim = 4).
        assert_eq!(HCOD1.len(), HCOD1_NUM_ENTRIES);
        assert_eq!(HCOD1_NUM_ENTRIES, 81);
    }

    #[test]
    fn hcod1_max_length_is_11_bits() {
        let max = HCOD1.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD1_MAX_LEN);
        assert_eq!(HCOD1_MAX_LEN, 11);
    }

    #[test]
    fn hcod1_min_length_is_one_bit_at_index_40() {
        // The zero-tuple (w, x, y, z) = (0, 0, 0, 0) at index 40
        // gets the single bit `0`. Every other index has length >= 5.
        for (idx, &(len, cw)) in HCOD1.iter().enumerate() {
            if idx == 40 {
                assert_eq!(len, 1, "index 40 must be 1-bit");
                assert_eq!(cw, 0, "index 40 codeword must be `0`");
            } else {
                assert!(
                    len >= 5,
                    "every non-zero-tuple index must have length >= 5; idx={} len={}",
                    idx,
                    len
                );
            }
        }
    }

    #[test]
    fn hcod1_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD1.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    // -------------------------------------------------------------------
    // Kraft equality / completeness
    // -------------------------------------------------------------------

    #[test]
    fn hcod1_kraft_sum_is_two_to_the_eleven() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD1_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD1 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 2048);
    }

    #[test]
    fn hcod1_is_complete() {
        // Walk every 11-bit prefix, decode it via the same path the
        // production decoder uses, and confirm every prefix yields
        // exactly one entry. Bonus: confirm the decoded index round-
        // trips back to the same codeword via `hcod1_encode`.
        for prefix in 0u32..(1u32 << HCOD1_MAX_LEN) {
            let bytes = [(prefix >> 3) as u8, ((prefix & 0x7) << 5) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod1_decode(&mut br).expect("11-bit prefix must decode");
            let (len, cw) = hcod1_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD1_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    // -------------------------------------------------------------------
    // Encoder API
    // -------------------------------------------------------------------

    #[test]
    fn encode_zero_tuple_is_single_zero_bit() {
        // Index 40 = the zero 4-tuple → 1-bit `0` codeword.
        let (len, cw) = hcod1_encode(40).unwrap();
        assert_eq!(len, 1);
        assert_eq!(cw, 0);
    }

    #[test]
    fn encode_first_entry_matches_table() {
        // Spec PDF Table 4.A.2 row 0: length 11, codeword 0x7f8.
        let (len, cw) = hcod1_encode(0).unwrap();
        assert_eq!(len, 11);
        assert_eq!(cw, 0x7f8);
    }

    #[test]
    fn encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.2 row 80: length 11, codeword 0x7f4.
        let (len, cw) = hcod1_encode(80).unwrap();
        assert_eq!(len, 11);
        assert_eq!(cw, 0x7f4);
    }

    #[test]
    fn encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod1_encode(81),
            Err(Error::SpectralCodebookIndexOutOfRange(1))
        ));
        assert!(matches!(
            hcod1_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(1))
        ));
    }

    // -------------------------------------------------------------------
    // Decoder API
    // -------------------------------------------------------------------

    #[test]
    fn decode_single_zero_bit_yields_index_40() {
        // One byte starting with `0` followed by anything → idx 40.
        let bytes = [0b0111_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod1_decode(&mut br).unwrap();
        assert_eq!(idx, 40);
        // Only one bit consumed; the remaining 7 are untouched.
        assert_eq!(br.bit_position(), 1);
    }

    #[test]
    fn decode_first_entry_round_trip() {
        // Index 0 → length 11, codeword 0x7f8 = 0b111_1111_1000.
        // Pack into 2 bytes left-aligned: 0xff, 0x00.
        let bytes = [0xff, 0x00];
        let mut br = BitReader::new(&bytes);
        let idx = hcod1_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(br.bit_position(), 11);
    }

    #[test]
    fn decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod1_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    // -------------------------------------------------------------------
    // Writer API
    // -------------------------------------------------------------------

    #[test]
    fn write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD1_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod1_write(&mut w, idx).unwrap();
            // Pad to byte boundary if needed so BitReader can consume.
            let (len, _) = hcod1_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod1_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod1_write(&mut w, 81),
            Err(Error::SpectralCodebookIndexOutOfRange(1))
        ));
    }

    // -------------------------------------------------------------------
    // Codebook 2 — Table 4.A.3
    // -------------------------------------------------------------------

    #[test]
    fn hcod2_has_exactly_81_entries() {
        // 3^4 = 81 (signed LAV=1 → mod = 2*1+1 = 3, dim = 4) — same
        // tuple universe as Codebook 1.
        assert_eq!(HCOD2.len(), HCOD2_NUM_ENTRIES);
        assert_eq!(HCOD2_NUM_ENTRIES, 81);
    }

    #[test]
    fn hcod2_max_length_is_9_bits() {
        let max = HCOD2.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD2_MAX_LEN);
        assert_eq!(HCOD2_MAX_LEN, 9);
    }

    #[test]
    fn hcod2_min_length_is_three_bits_at_index_40() {
        // The zero-tuple (w, x, y, z) = (0, 0, 0, 0) at index 40
        // gets a 3-bit codeword `0b000` (vs the 1-bit `0` of
        // Codebook 1). Every other index has length >= 4.
        for (idx, &(len, cw)) in HCOD2.iter().enumerate() {
            if idx == 40 {
                assert_eq!(len, 3, "index 40 must be 3-bit");
                assert_eq!(cw, 0, "index 40 codeword must be `0`");
            } else {
                assert!(
                    len >= 4,
                    "every non-zero-tuple index must have length >= 4; idx={} len={}",
                    idx,
                    len
                );
            }
        }
    }

    #[test]
    fn hcod2_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD2.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod2_kraft_sum_is_two_to_the_nine() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD2_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD2 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 512);
    }

    #[test]
    fn hcod2_is_complete() {
        // Walk every 9-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod2_encode`.
        for prefix in 0u32..(1u32 << HCOD2_MAX_LEN) {
            // Pack `prefix` (9 bits) left-aligned into two bytes:
            // [bits 8..1] [bit 0 << 7 | rest].
            let bytes = [(prefix >> 1) as u8, ((prefix & 0x1) << 7) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod2_decode(&mut br).expect("9-bit prefix must decode");
            let (len, cw) = hcod2_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD2_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn encode_zero_tuple_is_three_zero_bits_in_codebook_2() {
        // Index 40 = the zero 4-tuple → 3-bit `000` codeword.
        let (len, cw) = hcod2_encode(40).unwrap();
        assert_eq!(len, 3);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod2_encode_first_entry_matches_table() {
        // Spec PDF Table 4.A.3 row 0: length 9, codeword 0x1f3.
        let (len, cw) = hcod2_encode(0).unwrap();
        assert_eq!(len, 9);
        assert_eq!(cw, 0x1f3);
    }

    #[test]
    fn hcod2_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.3 row 80: length 9, codeword 0x1f6.
        let (len, cw) = hcod2_encode(80).unwrap();
        assert_eq!(len, 9);
        assert_eq!(cw, 0x1f6);
    }

    #[test]
    fn hcod2_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod2_encode(81),
            Err(Error::SpectralCodebookIndexOutOfRange(2))
        ));
        assert!(matches!(
            hcod2_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(2))
        ));
    }

    #[test]
    fn hcod2_decode_three_zero_bits_yields_index_40() {
        // Three leading `0` bits → idx 40. Remaining 5 bits untouched.
        let bytes = [0b0001_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod2_decode(&mut br).unwrap();
        assert_eq!(idx, 40);
        assert_eq!(br.bit_position(), 3);
    }

    #[test]
    fn hcod2_decode_first_entry_round_trip() {
        // Index 0 → length 9, codeword 0x1f3 = 0b1_1111_0011.
        // Pack into 2 bytes left-aligned: 0xf9, 0x80.
        // 0x1f3 << 7 = 0xf980 (16-bit big-endian).
        let bytes = [0xf9, 0x80];
        let mut br = BitReader::new(&bytes);
        let idx = hcod2_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(br.bit_position(), 9);
    }

    #[test]
    fn hcod2_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod2_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod2_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD2_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod2_write(&mut w, idx).unwrap();
            let (len, _) = hcod2_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod2_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod2_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod2_write(&mut w, 81),
            Err(Error::SpectralCodebookIndexOutOfRange(2))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebooks 1 and 2 share the same tuple universe but
    // never share a codeword for the same index (different lengths
    // and codewords for index 40 — 1 bit `0` vs 3 bits `0b000`).
    // -------------------------------------------------------------------

    #[test]
    fn codebook_1_and_2_disagree_on_zero_tuple_codeword_length() {
        let (l1, _) = hcod1_encode(40).unwrap();
        let (l2, _) = hcod2_encode(40).unwrap();
        // Both books carry the zero-tuple at index 40 but use
        // different codeword lengths: 1 bit for Codebook 1, 3 bits
        // for Codebook 2.
        assert_eq!(l1, 1);
        assert_eq!(l2, 3);
        assert_ne!(l1, l2);
    }

    // -------------------------------------------------------------------
    // Codebook 3 — Table 4.A.4
    // -------------------------------------------------------------------

    #[test]
    fn hcod3_has_exactly_81_entries() {
        // 3^4 = 81 (unsigned LAV=2 → mod = lav+1 = 3, dim = 4).
        assert_eq!(HCOD3.len(), HCOD3_NUM_ENTRIES);
        assert_eq!(HCOD3_NUM_ENTRIES, 81);
    }

    #[test]
    fn hcod3_max_length_is_16_bits() {
        let max = HCOD3.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD3_MAX_LEN);
        assert_eq!(HCOD3_MAX_LEN, 16);
    }

    #[test]
    fn hcod3_min_length_is_one_bit_at_index_0() {
        // Unsigned books put the all-zero magnitude n-tuple at
        // index 0 (vs index 40 for the signed books); it carries the
        // single bit `0`. Every other index has length >= 4.
        for (idx, &(len, cw)) in HCOD3.iter().enumerate() {
            if idx == 0 {
                assert_eq!(len, 1, "index 0 must be 1-bit");
                assert_eq!(cw, 0, "index 0 codeword must be `0`");
            } else {
                assert!(
                    len >= 4,
                    "every non-zero-tuple index must have length >= 4; idx={} len={}",
                    idx,
                    len
                );
            }
        }
    }

    #[test]
    fn hcod3_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD3.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod3_kraft_sum_is_two_to_the_sixteen() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD3_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD3 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 65536);
    }

    #[test]
    fn hcod3_is_complete() {
        // Walk every 16-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod3_encode`.
        for prefix in 0u32..(1u32 << HCOD3_MAX_LEN) {
            // `prefix` already fits in 16 bits: pack left-aligned
            // into two bytes (high byte first).
            let bytes = [(prefix >> 8) as u8, (prefix & 0xff) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod3_decode(&mut br).expect("16-bit prefix must decode");
            let (len, cw) = hcod3_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD3_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#06x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod3_encode_zero_tuple_is_single_zero_bit() {
        // Index 0 = the zero 4-tuple `(0, 0, 0, 0)` in the unsigned
        // book → 1-bit `0` codeword.
        let (len, cw) = hcod3_encode(0).unwrap();
        assert_eq!(len, 1);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod3_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.4 row 80: length 15, codeword 0x7ffa.
        let (len, cw) = hcod3_encode(80).unwrap();
        assert_eq!(len, 15);
        assert_eq!(cw, 0x7ffa);
    }

    #[test]
    fn hcod3_encode_index_62_is_the_only_full_16_bit_codeword_0xffff() {
        // Spec PDF Table 4.A.4 row 62: length 16, codeword 0xffff
        // (the all-ones 16-bit pattern). Verify by spot-check that
        // this is the unique row with codeword 0xffff.
        let (len, cw) = hcod3_encode(62).unwrap();
        assert_eq!(len, 16);
        assert_eq!(cw, 0xffff);
        let count_matching = HCOD3.iter().filter(|&&(_, c)| c == 0xffff).count();
        assert_eq!(count_matching, 1);
    }

    #[test]
    fn hcod3_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod3_encode(81),
            Err(Error::SpectralCodebookIndexOutOfRange(3))
        ));
        assert!(matches!(
            hcod3_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(3))
        ));
    }

    #[test]
    fn hcod3_decode_single_zero_bit_yields_index_0() {
        // Leading `0` bit → idx 0 (the unsigned book's zero-tuple).
        let bytes = [0b0111_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod3_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        // Only one bit consumed; the remaining 7 are untouched.
        assert_eq!(br.bit_position(), 1);
    }

    #[test]
    fn hcod3_decode_full_16_bit_codeword_round_trips() {
        // Index 62 → length 16, codeword 0xffff. Pack as two bytes.
        let bytes = [0xff, 0xff];
        let mut br = BitReader::new(&bytes);
        let idx = hcod3_decode(&mut br).unwrap();
        assert_eq!(idx, 62);
        assert_eq!(br.bit_position(), 16);
    }

    #[test]
    fn hcod3_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod3_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod3_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD3_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod3_write(&mut w, idx).unwrap();
            let (len, _) = hcod3_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod3_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod3_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod3_write(&mut w, 81),
            Err(Error::SpectralCodebookIndexOutOfRange(3))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebook 3 zero-tuple sits at a different index
    // than Codebooks 1 / 2 because unsigned books use a different
    // index origin from signed books.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_3_zero_tuple_lives_at_index_zero_not_forty() {
        // The zero magnitude 4-tuple `(0, 0, 0, 0)`:
        //   - signed book (mod = 3, offset = LAV = 1): polynomial
        //     evaluates to (0+1)*27 + (0+1)*9 + (0+1)*3 + (0+1) = 40.
        //   - unsigned book (mod = 3, offset = 0): polynomial
        //     evaluates to (0)*27 + (0)*9 + (0)*3 + (0) = 0.
        // So the zero-tuple lives at index 40 in HCOD1 / HCOD2 and
        // at index 0 in HCOD3. Both still carry a 1-bit codeword in
        // their respective books (Codebook 1 + 3); Codebook 2 trades
        // the 1-bit zero-tuple for a 3-bit one to free up the short
        // codes for the non-zero tuples its target statistics prefer.
        let (l1, cw1) = hcod1_encode(40).unwrap();
        let (l3, cw3) = hcod3_encode(0).unwrap();
        assert_eq!(l1, 1);
        assert_eq!(cw1, 0);
        assert_eq!(l3, 1);
        assert_eq!(cw3, 0);
    }

    // -------------------------------------------------------------------
    // Codebook 4 — Table 4.A.5
    // -------------------------------------------------------------------

    #[test]
    fn hcod4_has_exactly_81_entries() {
        // 3^4 = 81 (unsigned LAV=2 → mod = lav+1 = 3, dim = 4) — same
        // tuple universe as Codebook 3.
        assert_eq!(HCOD4.len(), HCOD4_NUM_ENTRIES);
        assert_eq!(HCOD4_NUM_ENTRIES, 81);
    }

    #[test]
    fn hcod4_max_length_is_12_bits() {
        let max = HCOD4.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD4_MAX_LEN);
        assert_eq!(HCOD4_MAX_LEN, 12);
    }

    #[test]
    fn hcod4_min_length_is_four_bits_at_index_40() {
        // The shortest codeword in Codebook 4 is 4 bits, parked at
        // index 40 with the all-zero pattern `0b0000`. Every other
        // index has length >= 4 (Codebook 4's distribution has a
        // dense 4-bit head: indices 0, 4, 13, 27, 30, 31, 36, 37, 39,
        // 40 all share length 4).
        let (len_40, cw_40) = (HCOD4[40].0, HCOD4[40].1);
        assert_eq!(len_40, 4, "index 40 must be 4-bit");
        assert_eq!(cw_40, 0, "index 40 codeword must be `0b0000`");
        for (idx, &(len, _)) in HCOD4.iter().enumerate() {
            assert!(
                len >= 4,
                "every index must have length >= 4; idx={} len={}",
                idx,
                len
            );
        }
    }

    #[test]
    fn hcod4_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD4.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod4_kraft_sum_is_two_to_the_twelve() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD4_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD4 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 4096);
    }

    #[test]
    fn hcod4_is_complete() {
        // Walk every 12-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod4_encode`.
        for prefix in 0u32..(1u32 << HCOD4_MAX_LEN) {
            // Pack `prefix` (12 bits) left-aligned into two bytes:
            // high byte = bits 11..4, low byte = (bits 3..0) << 4.
            let bytes = [(prefix >> 4) as u8, ((prefix & 0xf) << 4) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod4_decode(&mut br).expect("12-bit prefix must decode");
            let (len, cw) = hcod4_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD4_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod4_encode_index_40_is_4_bit_zero_codeword() {
        // Spec PDF Table 4.A.5 row 40: length 4, codeword 0 (the
        // shortest codeword in the table).
        let (len, cw) = hcod4_encode(40).unwrap();
        assert_eq!(len, 4);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod4_encode_first_entry_matches_table() {
        // Spec PDF Table 4.A.5 row 0: length 4, codeword 0x7.
        let (len, cw) = hcod4_encode(0).unwrap();
        assert_eq!(len, 4);
        assert_eq!(cw, 0x7);
    }

    #[test]
    fn hcod4_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.5 row 80: length 11, codeword 0x7fc.
        let (len, cw) = hcod4_encode(80).unwrap();
        assert_eq!(len, 11);
        assert_eq!(cw, 0x7fc);
    }

    #[test]
    fn hcod4_encode_indices_62_and_74_are_the_full_12_bit_codewords() {
        // Spec PDF Table 4.A.5 row 62: length 12, codeword 0xfff.
        // Spec PDF Table 4.A.5 row 74: length 12, codeword 0xffe.
        // These are the only two 12-bit rows in Codebook 4.
        let (len_62, cw_62) = hcod4_encode(62).unwrap();
        assert_eq!((len_62, cw_62), (12, 0xfff));
        let (len_74, cw_74) = hcod4_encode(74).unwrap();
        assert_eq!((len_74, cw_74), (12, 0xffe));
        let count_12_bit = HCOD4.iter().filter(|&&(l, _)| l == 12).count();
        assert_eq!(count_12_bit, 2);
    }

    #[test]
    fn hcod4_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod4_encode(81),
            Err(Error::SpectralCodebookIndexOutOfRange(4))
        ));
        assert!(matches!(
            hcod4_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(4))
        ));
    }

    #[test]
    fn hcod4_decode_four_zero_bits_yields_index_40() {
        // Leading `0b0000` → idx 40 (Codebook 4's shortest codeword).
        // Remaining 4 bits of the byte untouched.
        let bytes = [0b0000_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod4_decode(&mut br).unwrap();
        assert_eq!(idx, 40);
        assert_eq!(br.bit_position(), 4);
    }

    #[test]
    fn hcod4_decode_full_12_bit_codeword_round_trips_index_62() {
        // Index 62 → length 12, codeword 0xfff = 0b1111_1111_1111.
        // Pack left-aligned into 2 bytes: 0xff, 0xf0.
        let bytes = [0xff, 0xf0];
        let mut br = BitReader::new(&bytes);
        let idx = hcod4_decode(&mut br).unwrap();
        assert_eq!(idx, 62);
        assert_eq!(br.bit_position(), 12);
    }

    #[test]
    fn hcod4_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod4_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod4_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD4_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod4_write(&mut w, idx).unwrap();
            let (len, _) = hcod4_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod4_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod4_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod4_write(&mut w, 81),
            Err(Error::SpectralCodebookIndexOutOfRange(4))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebook 3 and Codebook 4 share the unsigned dim-4
    // LAV-2 tuple universe (same Table 4.95 row shape) but assign
    // different codewords for the same tuple — Codebook 3 gives the
    // zero-tuple the single-bit codeword `0`; Codebook 4 lifts it to
    // a 4-bit `0b0111` and parks the 4-bit `0b0000` shortest at
    // index 40 instead.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_3_and_4_disagree_on_zero_tuple_codeword() {
        let (l3, cw3) = hcod3_encode(0).unwrap();
        let (l4, cw4) = hcod4_encode(0).unwrap();
        assert_eq!((l3, cw3), (1, 0));
        assert_eq!((l4, cw4), (4, 0x7));
        // Codebook 4's shortest codeword sits at a different index
        // (40) with a different value (`0b0000`).
        let (l40, cw40) = hcod4_encode(40).unwrap();
        assert_eq!((l40, cw40), (4, 0));
    }

    // -------------------------------------------------------------------
    // Codebook 5 (Table 4.A.6) — signed dim-2 LAV-4 pair book
    // -------------------------------------------------------------------

    #[test]
    fn hcod5_has_exactly_81_entries() {
        // 9^2 = 81 (signed LAV=4 → mod = 2*4+1 = 9, dim = 2).
        assert_eq!(HCOD5.len(), HCOD5_NUM_ENTRIES);
        assert_eq!(HCOD5_NUM_ENTRIES, 81);
    }

    #[test]
    fn hcod5_max_length_is_13_bits() {
        let max = HCOD5.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD5_MAX_LEN);
        assert_eq!(HCOD5_MAX_LEN, 13);
    }

    #[test]
    fn hcod5_min_length_is_one_bit_at_index_40() {
        // The shortest codeword in Codebook 5 is the single bit `0`
        // at index 40 — the §4.6.3.3 zero-tuple `(0, 0)` for a
        // signed pair book with LAV = 4 lands at the centre of the
        // index range, not at the edges.
        let (len_40, cw_40) = (HCOD5[40].0, HCOD5[40].1);
        assert_eq!(len_40, 1, "index 40 must be 1-bit");
        assert_eq!(cw_40, 0, "index 40 codeword must be `0`");
        let count_1_bit = HCOD5.iter().filter(|&&(l, _)| l == 1).count();
        assert_eq!(count_1_bit, 1, "exactly one 1-bit codeword");
    }

    #[test]
    fn hcod5_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD5.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod5_kraft_sum_is_two_to_the_thirteen() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD5_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD5 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 8192);
    }

    #[test]
    fn hcod5_is_complete() {
        // Walk every 13-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod5_encode`.
        for prefix in 0u32..(1u32 << HCOD5_MAX_LEN) {
            // Pack `prefix` (13 bits) left-aligned into two bytes:
            // high byte = bits 12..5, low byte = (bits 4..0) << 3.
            let bytes = [(prefix >> 5) as u8, ((prefix & 0x1f) << 3) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod5_decode(&mut br).expect("13-bit prefix must decode");
            let (len, cw) = hcod5_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD5_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#06x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod5_encode_index_40_is_single_zero_bit() {
        // Spec PDF Table 4.A.6 row 40: length 1, codeword 0 — the
        // §4.6.3.3 zero-tuple `(0, 0)`.
        let (len, cw) = hcod5_encode(40).unwrap();
        assert_eq!(len, 1);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod5_encode_first_entry_matches_table() {
        // Spec PDF Table 4.A.6 row 0: length 13, codeword 0x1fff —
        // the lower-left corner `(-4, -4)` of the signed pair lattice.
        let (len, cw) = hcod5_encode(0).unwrap();
        assert_eq!(len, 13);
        assert_eq!(cw, 0x1fff);
    }

    #[test]
    fn hcod5_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.6 row 80: length 13, codeword 0x1ffe —
        // the upper-right corner `(+4, +4)` of the signed pair lattice.
        let (len, cw) = hcod5_encode(80).unwrap();
        assert_eq!(len, 13);
        assert_eq!(cw, 0x1ffe);
    }

    #[test]
    fn hcod5_encode_four_13_bit_rows_are_the_lattice_corners() {
        // The four 13-bit codewords sit at indices 0, 8, 72, 80 — the
        // four `(±4, ±4)` corners of the signed `9 × 9` pair lattice.
        let expected = [
            (0u32, 0x1fffu16),  // (-4, -4)
            (8u32, 0x1ffdu16),  // (-4, +4)
            (72u32, 0x1ffcu16), // (+4, -4)
            (80u32, 0x1ffeu16), // (+4, +4)
        ];
        let observed: Vec<_> = HCOD5
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, cw))| if l == 13 { Some((i as u32, cw)) } else { None })
            .collect();
        assert_eq!(observed.len(), 4);
        for (e, o) in expected.iter().zip(observed.iter()) {
            assert_eq!(*e, *o, "expected {:?} got {:?}", e, o);
        }
    }

    #[test]
    fn hcod5_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod5_encode(81),
            Err(Error::SpectralCodebookIndexOutOfRange(5))
        ));
        assert!(matches!(
            hcod5_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(5))
        ));
    }

    #[test]
    fn hcod5_decode_single_zero_bit_yields_index_40() {
        // Leading bit `0` → idx 40 (the zero-tuple `(0, 0)`).
        // Remaining 7 bits of the byte untouched.
        let bytes = [0b0111_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod5_decode(&mut br).unwrap();
        assert_eq!(idx, 40);
        assert_eq!(br.bit_position(), 1);
    }

    #[test]
    fn hcod5_decode_full_13_bit_codeword_round_trips_index_0() {
        // Index 0 → length 13, codeword 0x1fff = 0b1_1111_1111_1111.
        // Pack left-aligned into 2 bytes: 0xff, 0xf8.
        let bytes = [0xff, 0xf8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod5_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(br.bit_position(), 13);
    }

    #[test]
    fn hcod5_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod5_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod5_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD5_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod5_write(&mut w, idx).unwrap();
            let (len, _) = hcod5_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod5_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod5_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod5_write(&mut w, 81),
            Err(Error::SpectralCodebookIndexOutOfRange(5))
        ));
    }

    // -------------------------------------------------------------------
    // Codebook 6 (Table 4.A.7) — signed dim-2 LAV-4 pair book
    // -------------------------------------------------------------------

    #[test]
    fn hcod6_has_exactly_81_entries() {
        // 9^2 = 81 (signed LAV=4 → mod = 2*4+1 = 9, dim = 2) — same
        // tuple universe as Codebook 5.
        assert_eq!(HCOD6.len(), HCOD6_NUM_ENTRIES);
        assert_eq!(HCOD6_NUM_ENTRIES, 81);
    }

    #[test]
    fn hcod6_max_length_is_11_bits() {
        let max = HCOD6.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD6_MAX_LEN);
        assert_eq!(HCOD6_MAX_LEN, 11);
    }

    #[test]
    fn hcod6_min_length_is_four_bits_at_index_40() {
        // The shortest codeword in Codebook 6 is 4 bits, parked at
        // index 40 (the §4.6.3.3 zero-tuple `(0, 0)` for a signed
        // pair book with LAV=4) with the all-zero pattern `0b0000`.
        // Every other index has length >= 4 (Codebook 6's
        // distribution has a dense 4-bit head: indices 30, 31, 32,
        // 39, 40, 41, 48, 49, 50 all share length 4).
        let (len_40, cw_40) = (HCOD6[40].0, HCOD6[40].1);
        assert_eq!(len_40, 4, "index 40 must be 4-bit");
        assert_eq!(cw_40, 0, "index 40 codeword must be `0b0000`");
        for (idx, &(len, _)) in HCOD6.iter().enumerate() {
            assert!(
                len >= 4,
                "every index must have length >= 4; idx={} len={}",
                idx,
                len
            );
        }
    }

    #[test]
    fn hcod6_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD6.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod6_kraft_sum_is_two_to_the_eleven() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD6_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD6 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 2048);
    }

    #[test]
    fn hcod6_is_complete() {
        // Walk every 11-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod6_encode`.
        for prefix in 0u32..(1u32 << HCOD6_MAX_LEN) {
            // Pack `prefix` (11 bits) left-aligned into two bytes:
            // high byte = bits 10..3, low byte = (bits 2..0) << 5.
            let bytes = [(prefix >> 3) as u8, ((prefix & 0x7) << 5) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod6_decode(&mut br).expect("11-bit prefix must decode");
            let (len, cw) = hcod6_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD6_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod6_encode_index_40_is_4_bit_zero_codeword() {
        // Spec PDF Table 4.A.7 row 40: length 4, codeword 0 — the
        // §4.6.3.3 zero-tuple `(0, 0)`.
        let (len, cw) = hcod6_encode(40).unwrap();
        assert_eq!(len, 4);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod6_encode_first_entry_matches_table() {
        // Spec PDF Table 4.A.7 row 0: length 11, codeword 0x7fe —
        // the lower-left corner `(-4, -4)` of the signed pair lattice.
        let (len, cw) = hcod6_encode(0).unwrap();
        assert_eq!(len, 11);
        assert_eq!(cw, 0x7fe);
    }

    #[test]
    fn hcod6_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.7 row 80: length 11, codeword 0x7fc —
        // the upper-right corner `(+4, +4)` of the signed pair lattice.
        let (len, cw) = hcod6_encode(80).unwrap();
        assert_eq!(len, 11);
        assert_eq!(cw, 0x7fc);
    }

    #[test]
    fn hcod6_encode_four_11_bit_rows_are_the_lattice_corners() {
        // The four 11-bit codewords sit at indices 0, 8, 72, 80 — the
        // four `(±4, ±4)` corners of the signed `9 × 9` pair lattice.
        let expected = [
            (0u32, 0x7feu16),  // (-4, -4)
            (8u32, 0x7fdu16),  // (-4, +4)
            (72u32, 0x7ffu16), // (+4, -4)
            (80u32, 0x7fcu16), // (+4, +4)
        ];
        let observed: Vec<_> = HCOD6
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, cw))| if l == 11 { Some((i as u32, cw)) } else { None })
            .collect();
        assert_eq!(observed.len(), 4);
        for (e, o) in expected.iter().zip(observed.iter()) {
            assert_eq!(*e, *o, "expected {:?} got {:?}", e, o);
        }
    }

    #[test]
    fn hcod6_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod6_encode(81),
            Err(Error::SpectralCodebookIndexOutOfRange(6))
        ));
        assert!(matches!(
            hcod6_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(6))
        ));
    }

    #[test]
    fn hcod6_decode_four_zero_bits_yields_index_40() {
        // Leading `0b0000` → idx 40 (the zero-tuple `(0, 0)`).
        // Remaining 4 bits of the byte untouched.
        let bytes = [0b0000_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod6_decode(&mut br).unwrap();
        assert_eq!(idx, 40);
        assert_eq!(br.bit_position(), 4);
    }

    #[test]
    fn hcod6_decode_full_11_bit_codeword_round_trips_index_72() {
        // Index 72 → length 11, codeword 0x7ff = 0b111_1111_1111.
        // Pack left-aligned into 2 bytes: 0xff, 0xe0.
        let bytes = [0xff, 0xe0];
        let mut br = BitReader::new(&bytes);
        let idx = hcod6_decode(&mut br).unwrap();
        assert_eq!(idx, 72);
        assert_eq!(br.bit_position(), 11);
    }

    #[test]
    fn hcod6_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod6_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod6_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD6_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod6_write(&mut w, idx).unwrap();
            let (len, _) = hcod6_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod6_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod6_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod6_write(&mut w, 81),
            Err(Error::SpectralCodebookIndexOutOfRange(6))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebooks 5 and 6 share the signed pair tuple
    // universe (Table 4.95 rows 5 and 6 are identical except for the
    // `Codebook listed in Table` column) but assign different codewords
    // for the same tuple — Codebook 5 gives the zero-tuple the single-
    // bit codeword `0`; Codebook 6 lifts it to a 4-bit `0b0000` and
    // pulls the ceiling back from 13 down to 11 bits.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_5_and_6_disagree_on_zero_tuple_codeword() {
        let (l5, cw5) = hcod5_encode(40).unwrap();
        let (l6, cw6) = hcod6_encode(40).unwrap();
        assert_eq!((l5, cw5), (1, 0));
        assert_eq!((l6, cw6), (4, 0));
    }

    #[test]
    fn codebook_5_and_6_agree_on_lattice_corner_indices() {
        // Both books pin the four (±4, ±4) lattice corners to their
        // respective maximum-length codewords — Codebook 5 at 13 bits,
        // Codebook 6 at 11 bits — but at the same four index positions.
        let corners: Vec<usize> = [0, 8, 72, 80].to_vec();
        let cb5_max_idx: Vec<usize> = HCOD5
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, _))| {
                if u32::from(l) == HCOD5_MAX_LEN {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        let cb6_max_idx: Vec<usize> = HCOD6
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, _))| {
                if u32::from(l) == HCOD6_MAX_LEN {
                    Some(i)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(cb5_max_idx, corners);
        assert_eq!(cb6_max_idx, corners);
    }

    // -------------------------------------------------------------------
    // Codebook 7 (Table 4.A.8): unsigned pair, dim=2, LAV=7,
    // 64 entries indexed 0..=63 (8^2 lattice). Zero-tuple `(0, 0)` at
    // index 0 carries the single-bit codeword `0`. Maximum codeword
    // length 12 bits. Complete prefix code: Kraft sum = 4096 = 2^12.
    // -------------------------------------------------------------------

    #[test]
    fn hcod7_has_exactly_64_entries() {
        // 8^2 = 64 (unsigned LAV=7 → mod = 7+1 = 8, dim = 2).
        assert_eq!(HCOD7.len(), HCOD7_NUM_ENTRIES);
        assert_eq!(HCOD7_NUM_ENTRIES, 64);
    }

    #[test]
    fn hcod7_max_length_is_12_bits() {
        let max = HCOD7.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD7_MAX_LEN);
        assert_eq!(HCOD7_MAX_LEN, 12);
    }

    #[test]
    fn hcod7_min_length_is_one_bit_at_index_0() {
        // The shortest codeword in Codebook 7 is 1 bit, parked at
        // index 0 (the §4.6.3.3 zero-tuple `(0, 0)` for an unsigned
        // pair book with LAV=7) with the codeword `0`. Index 0 is the
        // only 1-bit entry; every other index has length >= 3.
        let (len_0, cw_0) = (HCOD7[0].0, HCOD7[0].1);
        assert_eq!(len_0, 1, "index 0 must be 1-bit");
        assert_eq!(cw_0, 0, "index 0 codeword must be `0`");
        let single_bit_entries: usize = HCOD7.iter().filter(|&&(len, _)| len == 1).count();
        assert_eq!(single_bit_entries, 1, "exactly one 1-bit codeword");
        for (idx, &(len, _)) in HCOD7.iter().enumerate().skip(1) {
            assert!(
                len >= 3,
                "every index > 0 must have length >= 3; idx={} len={}",
                idx,
                len
            );
        }
    }

    #[test]
    fn hcod7_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD7.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod7_kraft_sum_is_two_to_the_twelve() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD7_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD7 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 4096);
    }

    #[test]
    fn hcod7_is_complete() {
        // Walk every 12-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod7_encode`.
        for prefix in 0u32..(1u32 << HCOD7_MAX_LEN) {
            // Pack `prefix` (12 bits) left-aligned into two bytes:
            // high byte = bits 11..4, low byte = (bits 3..0) << 4.
            let bytes = [(prefix >> 4) as u8, ((prefix & 0xf) << 4) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod7_decode(&mut br).expect("12-bit prefix must decode");
            let (len, cw) = hcod7_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD7_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod7_encode_index_0_is_1_bit_zero_codeword() {
        // Spec PDF Table 4.A.8 row 0: length 1, codeword 0 — the
        // §4.6.3.3 zero-tuple `(0, 0)`.
        let (len, cw) = hcod7_encode(0).unwrap();
        assert_eq!(len, 1);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod7_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.8 row 63: length 12, codeword 0xfff —
        // the far corner `(7, 7)` of the unsigned `8 × 8` pair lattice.
        let (len, cw) = hcod7_encode(63).unwrap();
        assert_eq!(len, 12);
        assert_eq!(cw, 0xfff);
    }

    #[test]
    fn hcod7_encode_four_12_bit_rows_match_table() {
        // Exactly four rows reach the 12-bit ceiling in Table 4.A.8:
        // indices 54, 55, 62, 63 with codewords ffd, ffe, ffc, fff.
        let expected = [
            (54u32, 0xffdu16),
            (55u32, 0xffeu16),
            (62u32, 0xffcu16),
            (63u32, 0xfffu16),
        ];
        let observed: Vec<_> = HCOD7
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, cw))| if l == 12 { Some((i as u32, cw)) } else { None })
            .collect();
        assert_eq!(observed.len(), 4);
        for (e, o) in expected.iter().zip(observed.iter()) {
            assert_eq!(*e, *o, "expected {:?} got {:?}", e, o);
        }
    }

    #[test]
    fn hcod7_encode_index_8_is_first_y1_row() {
        // Index 8 = (y, z) = (1, 0) via `y * 8 + z`. Table 4.A.8 row
        // 8: length 3, codeword 4.
        let (len, cw) = hcod7_encode(8).unwrap();
        assert_eq!(len, 3);
        assert_eq!(cw, 4);
    }

    #[test]
    fn hcod7_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod7_encode(64),
            Err(Error::SpectralCodebookIndexOutOfRange(7))
        ));
        assert!(matches!(
            hcod7_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(7))
        ));
    }

    #[test]
    fn hcod7_decode_single_zero_bit_yields_index_0() {
        // Leading `0` → idx 0 (the zero-tuple `(0, 0)`). Remaining 7
        // bits of the byte untouched.
        let bytes = [0b0111_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod7_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(br.bit_position(), 1);
    }

    #[test]
    fn hcod7_decode_full_12_bit_codeword_round_trips_index_63() {
        // Index 63 → length 12, codeword 0xfff = 0b1111_1111_1111.
        // Pack left-aligned into 2 bytes: 0xff, 0xf0.
        let bytes = [0xff, 0xf0];
        let mut br = BitReader::new(&bytes);
        let idx = hcod7_decode(&mut br).unwrap();
        assert_eq!(idx, 63);
        assert_eq!(br.bit_position(), 12);
    }

    #[test]
    fn hcod7_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod7_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod7_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD7_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod7_write(&mut w, idx).unwrap();
            let (len, _) = hcod7_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod7_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod7_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod7_write(&mut w, 64),
            Err(Error::SpectralCodebookIndexOutOfRange(7))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebook 7 is the first unsigned **pair** book.
    // It shares Codebook 3's "zero-tuple at index 0 with the shortest
    // codeword" placement (both are unsigned books with the §4.6.3.3
    // polynomial origin at index 0) but at dim=2 vs dim=4, and with
    // a 12-bit ceiling vs Codebook 3's 16-bit ceiling.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_3_and_7_both_park_zero_tuple_at_index_0() {
        // Both unsigned books map the §4.6.3.3 origin to index 0 and
        // hand it the shortest available codeword (length 1, value 0).
        let (l3, cw3) = hcod3_encode(0).unwrap();
        let (l7, cw7) = hcod7_encode(0).unwrap();
        assert_eq!((l3, cw3), (1, 0));
        assert_eq!((l7, cw7), (1, 0));
    }

    #[test]
    fn codebook_7_entry_count_is_64_vs_81_for_dim4_books() {
        // Dim-4 unsigned (HCB3/HCB4 with LAV=2): (2+1)^4 = 81.
        // Dim-2 unsigned (HCB7 with LAV=7): (7+1)^2 = 64. The
        // dim-2 → dim-4 split affects the §4.6.3.3 universe size.
        assert_eq!(HCOD3.len(), 81);
        assert_eq!(HCOD4.len(), 81);
        assert_eq!(HCOD7.len(), 64);
    }

    // -------------------------------------------------------------------
    // Codebook 8 (Table 4.A.9): unsigned pair, dim=2, LAV=7,
    // 64 entries indexed 0..=63 (8^2 lattice). The §4.6.3.3 zero-tuple
    // `(0, 0)` at index 0 carries a 5-bit `0b01110` (not the shortest);
    // the shortest 3-bit codeword `0` parks at index 9 (= (1, 1)).
    // Maximum codeword length 10 bits. Complete prefix code: Kraft
    // sum = 1024 = 2^10.
    // -------------------------------------------------------------------

    #[test]
    fn hcod8_has_exactly_64_entries() {
        // 8^2 = 64 (unsigned LAV=7 → mod = 7+1 = 8, dim = 2). Shares
        // the universe size with Codebook 7.
        assert_eq!(HCOD8.len(), HCOD8_NUM_ENTRIES);
        assert_eq!(HCOD8_NUM_ENTRIES, 64);
    }

    #[test]
    fn hcod8_max_length_is_10_bits() {
        let max = HCOD8.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD8_MAX_LEN);
        assert_eq!(HCOD8_MAX_LEN, 10);
    }

    #[test]
    fn hcod8_min_length_is_three_bits_at_index_9() {
        // The shortest codeword in Codebook 8 is 3 bits, parked at
        // index 9 (the §4.6.3.3 interior tuple `(y, z) = (1, 1)` for
        // an unsigned pair book with LAV=7) with the codeword `0`.
        // Index 9 is the only 3-bit entry; every other index has
        // length >= 4.
        let (len_9, cw_9) = (HCOD8[9].0, HCOD8[9].1);
        assert_eq!(len_9, 3, "index 9 must be 3-bit");
        assert_eq!(cw_9, 0, "index 9 codeword must be `0`");
        let three_bit_entries: usize = HCOD8.iter().filter(|&&(len, _)| len == 3).count();
        assert_eq!(three_bit_entries, 1, "exactly one 3-bit codeword");
        for (idx, &(len, _)) in HCOD8.iter().enumerate() {
            if idx == 9 {
                continue;
            }
            assert!(
                len >= 4,
                "every index != 9 must have length >= 4; idx={} len={}",
                idx,
                len
            );
        }
    }

    #[test]
    fn hcod8_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD8.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod8_kraft_sum_is_two_to_the_ten() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD8_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD8 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 1024);
    }

    #[test]
    fn hcod8_is_complete() {
        // Walk every 10-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod8_encode`.
        for prefix in 0u32..(1u32 << HCOD8_MAX_LEN) {
            // Pack `prefix` (10 bits) left-aligned into two bytes:
            // high byte = bits 9..2, low byte = (bits 1..0) << 6.
            let bytes = [(prefix >> 2) as u8, ((prefix & 0x3) << 6) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod8_decode(&mut br).expect("10-bit prefix must decode");
            let (len, cw) = hcod8_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len` bits
            // of `prefix`.
            let lead = prefix >> (HCOD8_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod8_encode_index_0_is_5_bit_zero_tuple_codeword() {
        // Spec PDF Table 4.A.9 row 0: length 5, codeword 0xe — the
        // §4.6.3.3 zero-tuple `(0, 0)` lifted off the shortest slot.
        let (len, cw) = hcod8_encode(0).unwrap();
        assert_eq!(len, 5);
        assert_eq!(cw, 0xe);
    }

    #[test]
    fn hcod8_encode_index_9_is_3_bit_zero_codeword() {
        // Spec PDF Table 4.A.9 row 9: length 3, codeword 0 — the
        // shortest codeword, parked on `(1, 1)` (= y * 8 + z = 9).
        let (len, cw) = hcod8_encode(9).unwrap();
        assert_eq!(len, 3);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod8_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.9 row 63: length 10, codeword 0x3ff —
        // the far corner `(7, 7)` of the unsigned `8 × 8` pair lattice.
        let (len, cw) = hcod8_encode(63).unwrap();
        assert_eq!(len, 10);
        assert_eq!(cw, 0x3ff);
    }

    #[test]
    fn hcod8_encode_four_10_bit_rows_match_table() {
        // Exactly four rows reach the 10-bit ceiling in Table 4.A.9:
        // indices 7, 47, 56, 63 with codewords 3fe, 3fc, 3fd, 3ff.
        let expected = [
            (7u32, 0x3feu16),
            (47u32, 0x3fcu16),
            (56u32, 0x3fdu16),
            (63u32, 0x3ffu16),
        ];
        let observed: Vec<_> = HCOD8
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, cw))| if l == 10 { Some((i as u32, cw)) } else { None })
            .collect();
        assert_eq!(observed.len(), 4);
        for (e, o) in expected.iter().zip(observed.iter()) {
            assert_eq!(*e, *o, "expected {:?} got {:?}", e, o);
        }
    }

    #[test]
    fn hcod8_encode_index_8_is_first_y1_row() {
        // Index 8 = (y, z) = (1, 0) via `y * 8 + z`. Table 4.A.9 row
        // 8: length 4, codeword 0x3.
        let (len, cw) = hcod8_encode(8).unwrap();
        assert_eq!(len, 4);
        assert_eq!(cw, 0x3);
    }

    #[test]
    fn hcod8_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod8_encode(64),
            Err(Error::SpectralCodebookIndexOutOfRange(8))
        ));
        assert!(matches!(
            hcod8_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(8))
        ));
    }

    #[test]
    fn hcod8_decode_three_zero_bits_yields_index_9() {
        // Leading `0b000` → idx 9 (the interior tuple `(1, 1)`).
        // Remaining 5 bits of the byte untouched.
        let bytes = [0b0001_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod8_decode(&mut br).unwrap();
        assert_eq!(idx, 9);
        assert_eq!(br.bit_position(), 3);
    }

    #[test]
    fn hcod8_decode_full_10_bit_codeword_round_trips_index_63() {
        // Index 63 → length 10, codeword 0x3ff = 0b1111_1111_11.
        // Pack left-aligned into 2 bytes: 0xff, 0xc0.
        let bytes = [0xff, 0xc0];
        let mut br = BitReader::new(&bytes);
        let idx = hcod8_decode(&mut br).unwrap();
        assert_eq!(idx, 63);
        assert_eq!(br.bit_position(), 10);
    }

    #[test]
    fn hcod8_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod8_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod8_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD8_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod8_write(&mut w, idx).unwrap();
            let (len, _) = hcod8_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod8_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod8_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod8_write(&mut w, 64),
            Err(Error::SpectralCodebookIndexOutOfRange(8))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebooks 7 and 8 share the unsigned pair tuple
    // universe (Table 4.95 rows 7 and 8 are identical except for the
    // `Codebook listed in Table` column) but assign different codewords
    // for the same `(y, z)` tuple. Where Codebook 7 pins the zero-tuple
    // to the 1-bit slot, Codebook 8 lifts it to a 5-bit codeword and
    // hands the 3-bit shortest-codeword slot to the `(1, 1)` interior.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_7_and_8_share_universe_size_but_disagree_on_shortest_slot() {
        assert_eq!(HCOD7_NUM_ENTRIES, HCOD8_NUM_ENTRIES);
        assert_eq!(HCOD7_NUM_ENTRIES, 64);
        // Codebook 7: zero-tuple at index 0 takes the 1-bit slot.
        let (l7_0, _) = hcod7_encode(0).unwrap();
        assert_eq!(l7_0, 1);
        // Codebook 8: zero-tuple at index 0 takes 5 bits; the 3-bit
        // shortest slot lives on the (1, 1) interior at index 9.
        let (l8_0, _) = hcod8_encode(0).unwrap();
        let (l8_9, cw8_9) = hcod8_encode(9).unwrap();
        assert_eq!(l8_0, 5);
        assert_eq!((l8_9, cw8_9), (3, 0));
    }

    #[test]
    fn codebook_8_far_corner_matches_codebook_7_far_corner_index() {
        // Both unsigned dim-2 LAV-7 books park `(7, 7)` at index 63
        // (the §4.6.3.3 unsigned polynomial puts the far corner at
        // the highest index). Only the codeword length / value
        // differs: Codebook 7 → 12-bit 0xfff; Codebook 8 → 10-bit 0x3ff.
        let (l7, cw7) = hcod7_encode(63).unwrap();
        let (l8, cw8) = hcod8_encode(63).unwrap();
        assert_eq!((l7, cw7), (12, 0xfff));
        assert_eq!((l8, cw8), (10, 0x3ff));
    }

    // -------------------------------------------------------------------
    // Codebook 9 — Table 4.A.10
    // -------------------------------------------------------------------

    #[test]
    fn hcod9_has_exactly_169_entries() {
        // 13^2 = 169 (unsigned LAV=12 → mod = lav+1 = 13, dim = 2).
        assert_eq!(HCOD9.len(), HCOD9_NUM_ENTRIES);
        assert_eq!(HCOD9_NUM_ENTRIES, 169);
    }

    #[test]
    fn hcod9_max_length_is_15_bits() {
        let max = HCOD9.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD9_MAX_LEN);
        assert_eq!(HCOD9_MAX_LEN, 15);
    }

    #[test]
    fn hcod9_min_length_is_one_bit_at_index_0() {
        // Unsigned books put the all-zero magnitude pair tuple at
        // index 0; Codebook 9 carries it as the single bit `0`.
        // Every other index has length >= 3.
        for (idx, &(len, cw)) in HCOD9.iter().enumerate() {
            if idx == 0 {
                assert_eq!(len, 1, "index 0 must be 1-bit");
                assert_eq!(cw, 0, "index 0 codeword must be `0`");
            } else {
                assert!(
                    len >= 3,
                    "every non-zero index must have length >= 3; idx={} len={}",
                    idx,
                    len
                );
            }
        }
    }

    #[test]
    fn hcod9_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD9.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod9_kraft_sum_is_two_to_the_fifteen() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD9_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD9 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 32768);
    }

    #[test]
    fn hcod9_is_complete() {
        // Walk every 15-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod9_encode`.
        for prefix in 0u32..(1u32 << HCOD9_MAX_LEN) {
            // Pack `prefix` (15 bits) left-aligned into two bytes:
            // high byte = bits 14..7, low byte = (bits 6..0) << 1.
            let bytes = [(prefix >> 7) as u8, ((prefix & 0x7f) << 1) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod9_decode(&mut br).expect("15-bit prefix must decode");
            let (len, cw) = hcod9_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len`
            // bits of `prefix`.
            let lead = prefix >> (HCOD9_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#06x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod9_encode_index_0_is_one_bit_zero_codeword() {
        // Spec PDF Table 4.A.10 row 0: length 1, codeword 0 — the
        // §4.6.3.3 zero-tuple `(0, 0)` carries the shortest possible
        // codeword.
        let (len, cw) = hcod9_encode(0).unwrap();
        assert_eq!(len, 1);
        assert_eq!(cw, 0);
    }

    #[test]
    fn hcod9_encode_first_few_rows_match_spec() {
        // Spec PDF Table 4.A.10 spot checks: indices 1, 13, 14.
        // Row 1: length 3, codeword 0x5; row 13: length 3, codeword
        // 0x4 (the only other 3-bit row); row 14: length 4,
        // codeword 0xc (interior `(y, z) = (1, 1)` since `idx =
        // 1 * 13 + 1 = 14`).
        assert_eq!(hcod9_encode(1).unwrap(), (3, 0x5));
        assert_eq!(hcod9_encode(13).unwrap(), (3, 0x4));
        assert_eq!(hcod9_encode(14).unwrap(), (4, 0xc));
    }

    #[test]
    fn hcod9_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.10 row 168: length 15, codeword 0x7fff
        // — the far corner `(12, 12)` of the unsigned `13 × 13`
        // pair lattice.
        let (len, cw) = hcod9_encode(168).unwrap();
        assert_eq!(len, 15);
        assert_eq!(cw, 0x7fff);
    }

    #[test]
    fn hcod9_encode_four_15_bit_rows_match_table() {
        // Exactly four rows reach the 15-bit ceiling in Table 4.A.10:
        // indices 142, 154, 155, 168 with codewords 7ffc, 7ffd,
        // 7ffe, 7fff.
        let expected = [
            (142u32, 0x7ffcu16),
            (154u32, 0x7ffdu16),
            (155u32, 0x7ffeu16),
            (168u32, 0x7fffu16),
        ];
        let observed: Vec<_> = HCOD9
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, cw))| if l == 15 { Some((i as u32, cw)) } else { None })
            .collect();
        assert_eq!(observed.len(), 4);
        for (e, o) in expected.iter().zip(observed.iter()) {
            assert_eq!(*e, *o, "expected {:?} got {:?}", e, o);
        }
    }

    #[test]
    fn hcod9_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod9_encode(169),
            Err(Error::SpectralCodebookIndexOutOfRange(9))
        ));
        assert!(matches!(
            hcod9_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(9))
        ));
    }

    #[test]
    fn hcod9_decode_single_zero_bit_yields_index_0() {
        // Leading `0` → idx 0 (the zero-tuple).
        // Remaining 7 bits of the byte untouched.
        let bytes = [0b0111_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod9_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(br.bit_position(), 1);
    }

    #[test]
    fn hcod9_decode_full_15_bit_codeword_round_trips_index_168() {
        // Index 168 → length 15, codeword 0x7fff = 0b111_1111_1111_1111.
        // Pack left-aligned into 2 bytes: 0xff, 0xfe.
        let bytes = [0xff, 0xfe];
        let mut br = BitReader::new(&bytes);
        let idx = hcod9_decode(&mut br).unwrap();
        assert_eq!(idx, 168);
        assert_eq!(br.bit_position(), 15);
    }

    #[test]
    fn hcod9_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod9_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod9_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD9_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod9_write(&mut w, idx).unwrap();
            let (len, _) = hcod9_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod9_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod9_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod9_write(&mut w, 169),
            Err(Error::SpectralCodebookIndexOutOfRange(9))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebook 9 expands the unsigned pair universe.
    // Codebooks 7 and 8 share the `8 × 8 = 64`-entry `LAV = 7`
    // lattice; Codebook 9 widens the per-coefficient ceiling to
    // `LAV = 12`, producing the `13 × 13 = 169`-entry lattice and
    // lifting the codeword ceiling from 10 (HCOD8) to 15 bits.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_9_universe_size_grows_to_169_from_codebook_8_64() {
        assert_eq!(HCOD7_NUM_ENTRIES, 64);
        assert_eq!(HCOD8_NUM_ENTRIES, 64);
        assert_eq!(HCOD9_NUM_ENTRIES, 169);
        // 169 / 64 ≈ 2.64 — the §4.6.3.3 universe more than doubles.
        const { assert!(HCOD9_NUM_ENTRIES > 2 * HCOD8_NUM_ENTRIES) };
    }

    #[test]
    fn codebook_9_zero_tuple_shares_codebook_7_head_placement() {
        // Both Codebook 7 and Codebook 9 are unsigned pair books
        // that park the §4.6.3.3 zero-tuple `(0, 0)` at index 0 with
        // the single-bit `0` codeword — the shortest-possible slot.
        // Codebook 8 lifts the zero-tuple off the 1-bit slot (it
        // becomes 5 bits at index 0 and the 3-bit shortest moves to
        // the (1, 1) interior at index 9).
        let (l7_0, cw7_0) = hcod7_encode(0).unwrap();
        let (l9_0, cw9_0) = hcod9_encode(0).unwrap();
        assert_eq!((l7_0, cw7_0), (1, 0));
        assert_eq!((l9_0, cw9_0), (1, 0));
    }

    #[test]
    fn codebook_9_far_corner_index_matches_lav_12_polynomial() {
        // The §4.6.3.3 unsigned polynomial puts the max pair tuple
        // `(LAV, LAV)` at index `LAV * (LAV + 1) + LAV`. For
        // Codebook 9 with `LAV = 12` that's `12 * 13 + 12 = 168`,
        // which carries the 15-bit codeword `0x7fff` — the widest
        // codeword in any non-ESC spectrum book.
        let (l9, cw9) = hcod9_encode(168).unwrap();
        assert_eq!((l9, cw9), (15, 0x7fff));
        // Compare to Codebook 8's far corner (LAV = 7) at index
        // 63: that's only 10 bits wide.
        let (l8, cw8) = hcod8_encode(63).unwrap();
        assert_eq!((l8, cw8), (10, 0x3ff));
        // Codebook 9's ceiling is 5 bits wider than Codebook 8's.
        assert_eq!(HCOD9_MAX_LEN - HCOD8_MAX_LEN, 5);
    }

    // -------------------------------------------------------------------
    // Codebook 10 — Table 4.A.11
    // -------------------------------------------------------------------

    #[test]
    fn hcod10_has_exactly_169_entries() {
        // 13^2 = 169 (unsigned LAV=12 → mod = lav+1 = 13, dim = 2).
        assert_eq!(HCOD10.len(), HCOD10_NUM_ENTRIES);
        assert_eq!(HCOD10_NUM_ENTRIES, 169);
    }

    #[test]
    fn hcod10_max_length_is_12_bits() {
        let max = HCOD10.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD10_MAX_LEN);
        assert_eq!(HCOD10_MAX_LEN, 12);
    }

    #[test]
    fn hcod10_min_length_is_four_bits_at_interior_tuple_index_14() {
        // Codebook 10 lifts the zero-tuple off the shortest slot (it
        // sits at 6 bits at index 0) and parks the 4-bit shortest
        // codeword on the interior `(1, 1)` tuple at index 14, the
        // same head-displacement pattern Codebook 8 uses.
        // Exactly three rows reach 4 bits: indices 14, 15, 27.
        let mut four_bit_indices = Vec::new();
        for (idx, &(len, _)) in HCOD10.iter().enumerate() {
            if len == 4 {
                four_bit_indices.push(idx);
            }
            assert!(
                len >= 4,
                "every row must have length >= 4; idx={} len={}",
                idx,
                len
            );
        }
        assert_eq!(four_bit_indices, vec![14, 15, 27]);
    }

    #[test]
    fn hcod10_zero_tuple_lives_at_index_0_with_six_bit_codeword() {
        // Codebook 10 places the §4.6.3.3 zero-tuple `(0, 0)` at
        // index 0 via the unsigned polynomial idx = 0 * 13 + 0 = 0,
        // but the Huffman row carries a 6-bit `0b100010` (`0x22`)
        // codeword — not the 1-bit `0` that Codebook 9 uses.
        let (len, cw) = HCOD10[0];
        assert_eq!(len, 6);
        assert_eq!(cw, 0x22);
    }

    #[test]
    fn hcod10_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD10.iter().enumerate() {
            let max = if len == 0 { 0 } else { (1u32 << len) - 1 };
            assert!(
                u32::from(cw) <= max,
                "idx={}: codeword {:#x} does not fit {} bits",
                idx,
                cw,
                len
            );
        }
    }

    #[test]
    fn hcod10_kraft_sum_is_two_to_the_twelve() {
        // Σᵢ 2^(L_max − Lᵢ) must equal 2^L_max for a complete code.
        let lmax = HCOD10_MAX_LEN;
        let mut sum: u64 = 0;
        for &(len, _) in &HCOD10 {
            sum += 1u64 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u64 << lmax, "Kraft equality failed");
        assert_eq!(sum, 4096);
    }

    #[test]
    fn hcod10_is_complete() {
        // Walk every 12-bit prefix, decode it via the production
        // decoder, and confirm every prefix yields exactly one entry.
        // Bonus: confirm the decoded index round-trips back to the
        // same codeword via `hcod10_encode`.
        for prefix in 0u32..(1u32 << HCOD10_MAX_LEN) {
            // Pack `prefix` (12 bits) left-aligned into two bytes:
            // high byte = bits 11..4, low byte = (bits 3..0) << 4.
            let bytes = [(prefix >> 4) as u8, ((prefix & 0xf) << 4) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod10_decode(&mut br).expect("12-bit prefix must decode");
            let (len, cw) = hcod10_encode(idx).expect("decoded index must round-trip");
            // The decoded codeword should match the leading `len`
            // bits of `prefix`.
            let lead = prefix >> (HCOD10_MAX_LEN - u32::from(len));
            assert_eq!(
                u32::from(cw),
                lead,
                "round-trip prefix={:#05x} idx={} len={} cw={:#x}",
                prefix,
                idx,
                len,
                cw
            );
        }
    }

    #[test]
    fn hcod10_encode_index_0_is_six_bit_codeword_0x22() {
        // Spec PDF Table 4.A.11 row 0: length 6, codeword 0x22 — the
        // §4.6.3.3 zero-tuple `(0, 0)` does NOT carry the shortest
        // possible codeword in Codebook 10.
        let (len, cw) = hcod10_encode(0).unwrap();
        assert_eq!(len, 6);
        assert_eq!(cw, 0x22);
    }

    #[test]
    fn hcod10_encode_shortest_codewords_match_spec() {
        // Spec PDF Table 4.A.11 spot checks: the three 4-bit rows are
        // indices 14, 15, 27 with codewords 0, 1, 2.
        assert_eq!(hcod10_encode(14).unwrap(), (4, 0x0));
        assert_eq!(hcod10_encode(15).unwrap(), (4, 0x1));
        assert_eq!(hcod10_encode(27).unwrap(), (4, 0x2));
    }

    #[test]
    fn hcod10_encode_last_entry_matches_table() {
        // Spec PDF Table 4.A.11 row 168: length 12, codeword 0xfff —
        // the far corner `(12, 12)` of the unsigned `13 × 13` pair
        // lattice.
        let (len, cw) = hcod10_encode(168).unwrap();
        assert_eq!(len, 12);
        assert_eq!(cw, 0xfff);
    }

    #[test]
    fn hcod10_encode_eight_12_bit_rows_match_table() {
        // Exactly eight rows reach the 12-bit ceiling in
        // Table 4.A.11. Their indices and codewords are pinned here.
        let expected = [
            (12u32, 0x0ffdu16),
            (129u32, 0x0ffau16),
            (142u32, 0x0ff9u16),
            (155u32, 0x0ffbu16),
            (165u32, 0x0ff8u16),
            (166u32, 0x0ffeu16),
            (167u32, 0x0ffcu16),
            (168u32, 0x0fffu16),
        ];
        let observed: Vec<_> = HCOD10
            .iter()
            .enumerate()
            .filter_map(|(i, &(l, cw))| if l == 12 { Some((i as u32, cw)) } else { None })
            .collect();
        assert_eq!(observed.len(), 8);
        for (e, o) in expected.iter().zip(observed.iter()) {
            assert_eq!(*e, *o, "expected {:?} got {:?}", e, o);
        }
    }

    #[test]
    fn hcod10_encode_rejects_out_of_range_index() {
        assert!(matches!(
            hcod10_encode(169),
            Err(Error::SpectralCodebookIndexOutOfRange(10))
        ));
        assert!(matches!(
            hcod10_encode(0xffff_ffff),
            Err(Error::SpectralCodebookIndexOutOfRange(10))
        ));
    }

    #[test]
    fn hcod10_decode_four_bit_zero_codeword_yields_index_14() {
        // Index 14 → length 4, codeword 0 = 0b0000. Pack
        // left-aligned in a single byte: top 4 bits = 0, bottom 4
        // bits arbitrary.
        let bytes = [0b0000_1111u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod10_decode(&mut br).unwrap();
        assert_eq!(idx, 14);
        assert_eq!(br.bit_position(), 4);
    }

    #[test]
    fn hcod10_decode_full_12_bit_codeword_round_trips_index_168() {
        // Index 168 → length 12, codeword 0xfff = 0b1111_1111_1111.
        // Pack left-aligned: high byte = 0xff (bits 11..4), low byte
        // = (0xf << 4) = 0xf0 (bits 3..0 in the top of the low byte).
        let bytes = [0xff, 0xf0];
        let mut br = BitReader::new(&bytes);
        let idx = hcod10_decode(&mut br).unwrap();
        assert_eq!(idx, 168);
        assert_eq!(br.bit_position(), 12);
    }

    #[test]
    fn hcod10_decode_propagates_unexpected_end() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        assert_eq!(hcod10_decode(&mut br), Err(Error::UnexpectedEnd));
    }

    #[test]
    fn hcod10_write_then_decode_round_trips_every_index() {
        for idx in 0..HCOD10_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod10_write(&mut w, idx).unwrap();
            let (len, _) = hcod10_encode(idx).unwrap();
            let mut w2 = w;
            let pad = (8 - (u32::from(len) % 8)) % 8;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let decoded = hcod10_decode(&mut br).unwrap();
            assert_eq!(
                decoded, idx,
                "round-trip mismatch at idx={} (encoded as {} bits)",
                idx, len
            );
        }
    }

    #[test]
    fn hcod10_write_rejects_out_of_range_index() {
        let mut w = BitWriter::new();
        assert!(matches!(
            hcod10_write(&mut w, 169),
            Err(Error::SpectralCodebookIndexOutOfRange(10))
        ));
    }

    // -------------------------------------------------------------------
    // Cross-check: Codebooks 9 and 10 share the unsigned dim-2 LAV-12
    // universe (169 entries each) but differ in codeword distribution.
    // Codebook 9's ceiling is 15 bits with the zero-tuple at the
    // 1-bit head; Codebook 10's ceiling pulls down to 12 bits and
    // lifts the zero-tuple off the head — the shortest 4-bit slot
    // sits on the interior `(1, 1)` tuple at index 14.
    // -------------------------------------------------------------------

    #[test]
    fn codebook_10_matches_codebook_9_universe_size() {
        assert_eq!(HCOD9_NUM_ENTRIES, 169);
        assert_eq!(HCOD10_NUM_ENTRIES, 169);
    }

    #[test]
    fn codebook_10_ceiling_is_3_bits_below_codebook_9() {
        // Codebook 9's ceiling is 15 bits; Codebook 10's ceiling
        // is 12 bits — a 3-bit pull-down reflecting the flatter
        // codeword distribution targeted by Codebook 10's
        // encoder-statistics tuning.
        assert_eq!(HCOD9_MAX_LEN, 15);
        assert_eq!(HCOD10_MAX_LEN, 12);
        assert_eq!(HCOD9_MAX_LEN - HCOD10_MAX_LEN, 3);
    }

    #[test]
    fn codebook_10_lifts_zero_tuple_off_codebook_9_head_placement() {
        // Codebook 9 parks the §4.6.3.3 zero-tuple at index 0 with
        // the 1-bit `0` codeword (shortest possible slot). Codebook
        // 10 keeps the zero-tuple at index 0 (the §4.6.3.3 polynomial
        // index is fixed by the tuple, not the codebook) but the
        // codeword swells to 6 bits — the shortest 4-bit slot
        // migrates onto the interior `(1, 1)` tuple at index 14.
        let (l9_0, cw9_0) = hcod9_encode(0).unwrap();
        let (l10_0, cw10_0) = hcod10_encode(0).unwrap();
        let (l10_14, cw10_14) = hcod10_encode(14).unwrap();
        assert_eq!((l9_0, cw9_0), (1, 0));
        assert_eq!((l10_0, cw10_0), (6, 0x22));
        assert_eq!((l10_14, cw10_14), (4, 0));
    }

    #[test]
    fn codebook_10_far_corner_matches_codebook_9_far_corner_index() {
        // Both codebooks share LAV = 12, so the §4.6.3.3 unsigned
        // polynomial parks `(12, 12)` at index 12 * 13 + 12 = 168.
        // Codeword shapes differ: Codebook 9 → 15-bit 0x7fff;
        // Codebook 10 → 12-bit 0xfff.
        let (l9, cw9) = hcod9_encode(168).unwrap();
        let (l10, cw10) = hcod10_encode(168).unwrap();
        assert_eq!((l9, cw9), (15, 0x7fff));
        assert_eq!((l10, cw10), (12, 0xfff));
    }

    // -------------------------------------------------------------------
    // Codebook 11 invariants (Table 4.A.12)
    // -------------------------------------------------------------------

    #[test]
    fn hcod11_has_exactly_289_entries() {
        // 17^2 = 289 (unsigned LAV=16 → mod = 17, dim = 2).
        assert_eq!(HCOD11.len(), HCOD11_NUM_ENTRIES);
        assert_eq!(HCOD11_NUM_ENTRIES, 289);
    }

    #[test]
    fn hcod11_max_length_is_12_bits() {
        let max = HCOD11.iter().map(|&(len, _)| len).max().unwrap();
        assert_eq!(u32::from(max), HCOD11_MAX_LEN);
        assert_eq!(HCOD11_MAX_LEN, 12);
    }

    #[test]
    fn hcod11_min_length_is_four_bits_at_zero_tuple_and_interior_pair() {
        // The 4-bit floor is shared by exactly two rows: index 0
        // (the zero-tuple (0, 0)) and index 18 (the interior (1, 1)
        // pair, since 1 * 17 + 1 = 18). The zero-tuple carries
        // 0b0000 and (1, 1) carries 0b0001.
        let mut min: u32 = u32::MAX;
        let mut min_indices: Vec<usize> = Vec::new();
        for (idx, &(len, _)) in HCOD11.iter().enumerate() {
            let l = u32::from(len);
            if l < min {
                min = l;
                min_indices.clear();
                min_indices.push(idx);
            } else if l == min {
                min_indices.push(idx);
            }
        }
        assert_eq!(min, 4);
        assert_eq!(min_indices, vec![0, 18]);
    }

    #[test]
    fn hcod11_zero_tuple_lives_at_index_0_with_four_bit_codeword() {
        // The §4.6.3.3 unsigned polynomial idx = y * 17 + z places
        // the zero-tuple (0, 0) at index 0; Codebook 11 hands it
        // the shortest 4-bit codeword 0b0000.
        let (len, cw) = HCOD11[0];
        assert_eq!(len, 4);
        assert_eq!(cw, 0x0000);
    }

    #[test]
    fn hcod11_interior_one_one_tuple_lives_at_index_18_with_four_bit_codeword() {
        // 1 * 17 + 1 = 18 → the second 4-bit slot, codeword 0b0001.
        let (len, cw) = HCOD11[18];
        assert_eq!(len, 4);
        assert_eq!(cw, 0x0001);
    }

    #[test]
    fn hcod11_far_corner_lives_at_index_288_with_five_bit_codeword() {
        // (16, 16) at 16 * 17 + 16 = 288 — both coefficients flagged
        // as ESC. Codebook 11 spends only 5 bits on this far corner
        // (codeword 0b00100), keeping the in-band codeword short
        // because the wire layout extends with two escape sequences
        // and (where the magnitudes are non-zero) two sign bits.
        let (len, cw) = HCOD11[288];
        assert_eq!(len, 5);
        assert_eq!(cw, 0x0004);
    }

    #[test]
    fn hcod11_codewords_fit_their_declared_length() {
        for (idx, &(len, cw)) in HCOD11.iter().enumerate() {
            assert!(
                u32::from(cw) < (1u32 << u32::from(len)),
                "row {idx}: codeword 0x{cw:x} >= 2^{len}",
            );
        }
    }

    #[test]
    fn hcod11_kraft_sum_is_two_to_the_twelve() {
        // Σ 2^(L_max - L) = 2^L_max ⇔ complete prefix code.
        let lmax = HCOD11_MAX_LEN;
        let mut sum: u32 = 0;
        for &(len, _) in &HCOD11 {
            sum += 1u32 << (lmax - u32::from(len));
        }
        assert_eq!(sum, 1u32 << lmax);
        assert_eq!(sum, 4096);
    }

    #[test]
    fn hcod11_is_complete() {
        // Exhaustively walk every 12-bit prefix and verify each
        // matches exactly one entry. This is the strongest
        // possible check that the table is a complete prefix code
        // and that `hcod11_decode`'s `unreachable!()` is dead.
        for prefix in 0u32..(1u32 << HCOD11_MAX_LEN) {
            let bytes = [((prefix >> 4) & 0xff) as u8, ((prefix & 0xf) << 4) as u8];
            let mut br = BitReader::new(&bytes);
            let idx = hcod11_decode(&mut br).expect("12-bit prefix must decode");
            let (len, cw) = hcod11_encode(idx).expect("decoded index must round-trip");
            // The decoded prefix must match the leading `len` bits
            // of our 12-bit walk.
            let lead = prefix >> (HCOD11_MAX_LEN - u32::from(len));
            assert_eq!(
                lead,
                u32::from(cw),
                "prefix 0b{prefix:012b} decoded idx={idx} → codeword ({len}, 0x{cw:x})",
            );
        }
    }

    #[test]
    fn hcod11_twelve_bit_ceiling_hits_exactly_six_indices() {
        // Indices 12, 14, 15, 255, 269, 270 are the only rows whose
        // codeword length reaches the 12-bit ceiling.
        let ceiling: Vec<usize> = HCOD11
            .iter()
            .enumerate()
            .filter_map(|(i, &(len, _))| if len == 12 { Some(i) } else { None })
            .collect();
        assert_eq!(ceiling, vec![12, 14, 15, 255, 269, 270]);
        assert_eq!(HCOD11[12], (12, 0x0ffb));
        assert_eq!(HCOD11[14], (12, 0x0ffa));
        assert_eq!(HCOD11[15], (12, 0x0ffe));
        assert_eq!(HCOD11[255], (12, 0x0ffd));
        assert_eq!(HCOD11[269], (12, 0x0ffc));
        assert_eq!(HCOD11[270], (12, 0x0fff));
    }

    #[test]
    fn hcod11_half_esc_rows_match_spec() {
        // Index 16 corresponds to (y, z) = (0, 16), index 272 to
        // (16, 0). Both are half-ESC tuples — exactly one
        // coefficient at the §4.6.3.3 escape flag.
        assert_eq!(HCOD11[16], (10, 0x038e));
        assert_eq!(HCOD11[272], (9, 0x01c2));
    }

    #[test]
    fn hcod11_encode_rejects_out_of_range_indices() {
        for bad in [289u32, 290, 300, 1000, u32::MAX] {
            assert!(matches!(
                hcod11_encode(bad),
                Err(Error::SpectralCodebookIndexOutOfRange(11))
            ));
        }
    }

    #[test]
    fn hcod11_write_rejects_out_of_range_indices() {
        let mut w = BitWriter::new();
        for bad in [289u32, 1000, u32::MAX] {
            assert!(matches!(
                hcod11_write(&mut w, bad),
                Err(Error::SpectralCodebookIndexOutOfRange(11))
            ));
        }
    }

    #[test]
    fn hcod11_decode_index_0_zero_bits() {
        // Index 0 → 4-bit `0`. Padding to a byte boundary with zeros
        // keeps the wire byte at 0x00.
        let bytes = [0x00u8];
        let mut br = BitReader::new(&bytes);
        let idx = hcod11_decode(&mut br).unwrap();
        assert_eq!(idx, 0);
        assert_eq!(br.bit_position(), 4u64);
    }

    #[test]
    fn hcod11_decode_index_270_full_12_bit_far_codeword() {
        // Index 270 → 12-bit 0xfff packed left-aligned: high byte =
        // 0xff (bits 11..4), low byte = (0xf << 4) = 0xf0 (bits
        // 3..0 in the high nibble of the low byte).
        let bytes = [0xffu8, 0xf0];
        let mut br = BitReader::new(&bytes);
        let idx = hcod11_decode(&mut br).unwrap();
        assert_eq!(idx, 270);
        assert_eq!(br.bit_position(), 12u64);
    }

    #[test]
    fn hcod11_writer_round_trip_pins_every_index() {
        // Writer → reader round-trip for every legal index. Each
        // index must produce the exact bit-stream the encode
        // function claims, and decode must recover the original
        // index using exactly `len` bits.
        for idx in 0..HCOD11_NUM_ENTRIES as u32 {
            let mut w = BitWriter::new();
            hcod11_write(&mut w, idx).unwrap();
            let (len, _) = hcod11_encode(idx).unwrap();
            let pad = (8 - (u32::from(len) % 8)) % 8;
            let mut w2 = w;
            if pad > 0 {
                w2.write_u32(0, pad);
            }
            let bytes = w2.into_bytes();
            let mut br = BitReader::new(&bytes);
            let got = hcod11_decode(&mut br).unwrap();
            assert_eq!(got, idx, "round-trip mismatch at idx={idx}");
            assert_eq!(
                br.bit_position(),
                u64::from(len),
                "bit consumption mismatch at idx={idx}",
            );
        }
    }

    #[test]
    fn hcod11_decoder_returns_unexpected_end_on_truncation() {
        let bytes: [u8; 0] = [];
        let mut br = BitReader::new(&bytes);
        let err = hcod11_decode(&mut br).unwrap_err();
        assert_eq!(err, Error::UnexpectedEnd);
    }

    #[test]
    fn hcod11_max_len_constant_matches_table_data() {
        let mut observed_max = 0u32;
        for idx in 0..HCOD11_NUM_ENTRIES as u32 {
            let (len, _) = hcod11_encode(idx).unwrap();
            observed_max = observed_max.max(u32::from(len));
        }
        assert_eq!(observed_max, HCOD11_MAX_LEN);
    }

    #[test]
    fn hcod11_ceiling_matches_codebook_10_ceiling() {
        // Codebook 10 caps at 12 bits; Codebook 11 also caps at 12
        // bits — the universe widens (169 → 289 entries) but the
        // codeword ceiling stays the same because the ESC sequence
        // soaks up the tail-distribution rather than spending
        // longer Huffman codewords on it.
        assert_eq!(HCOD10_MAX_LEN, HCOD11_MAX_LEN);
        assert_eq!(HCOD11_MAX_LEN, 12);
    }

    #[test]
    fn hcod11_universe_is_69_entries_wider_than_codebook_10() {
        // 289 - 169 = 120 extra rows = (17 + 17 - 1) extra entries
        // along the ESC border `y == 16 || z == 16`.
        assert_eq!(HCOD11_NUM_ENTRIES - HCOD10_NUM_ENTRIES, 120);
        assert_eq!(HCOD11_NUM_ENTRIES, 289);
        assert_eq!(HCOD10_NUM_ENTRIES, 169);
    }
}
