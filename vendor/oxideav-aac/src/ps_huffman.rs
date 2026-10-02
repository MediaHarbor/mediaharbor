//! Parametric Stereo Huffman codebooks + `ps_huff_dec()` — ISO/IEC
//! 14496-3:2009 Annex 8.B (Tables 8.B.17–8.B.21).
//!
//! The `ps_data()` element (§8.4.2 Table 8.9) entropy-codes its IID /
//! ICC / IPD / OPD parameters as DPCM deltas with ten canonical
//! Huffman codebooks, selected by parameter kind, quantization grid
//! (`iid_quant`, Table 8.24) and coding direction (time vs frequency
//! differential, the `*_dt[e]` flags):
//!
//! | parameter | grid   | direction | table |
//! |-----------|--------|-----------|-------|
//! | IID       | coarse | freq      | [`HUFF_IID_DF`] (8.B.18) |
//! | IID       | coarse | time      | [`HUFF_IID_DT`] (8.B.18) |
//! | IID       | fine   | freq      | [`HUFF_IID_FINE_DF`] (8.B.17) |
//! | IID       | fine   | time      | [`HUFF_IID_FINE_DT`] (8.B.17) |
//! | ICC       | —      | freq      | [`HUFF_ICC_DF`] (8.B.19) |
//! | ICC       | —      | time      | [`HUFF_ICC_DT`] (8.B.19) |
//! | IPD       | —      | freq      | [`HUFF_IPD_DF`] (8.B.20) |
//! | IPD       | —      | time      | [`HUFF_IPD_DT`] (8.B.20) |
//! | OPD       | —      | freq      | [`HUFF_OPD_DF`] (8.B.21) |
//! | OPD       | —      | time      | [`HUFF_OPD_DT`] (8.B.21) |
//!
//! ## Codeword representation
//!
//! Same shape as [`crate::sbr_huffman`]: each table is `[(u8, u32); N]`
//! `(code_length_bits, codeword)` pairs indexed by the Huffman table
//! index, MSB-first prefix codes. [`ps_huff_dec`] accumulates bits and
//! returns `index - lav` (the signed delta). The IID/ICC tables carry
//! their LAV in the index layout (`LAV = (N-1)/2`); IPD/OPD deltas are
//! phase-index differences taken modulo 8 by the caller, so their
//! tables decode with `lav = 0`.
//!
//! ## Provenance
//!
//! All ten tables are transcribed from the normative codeword grids in
//! ISO/IEC 14496-3:2009 Annex 8.B staged under `docs/audio/aac/`. All
//! six IID/ICC tables were additionally cross-checked leaf-for-leaf
//! against the staged `docs/audio/aac/sbr-tables/ps-huffbook-*.csv`
//! decode-tree data at transcription time; every table satisfies the
//! complete-prefix-code invariant (Kraft sum exactly 1).

use crate::{Error, Result};

/// Longest PS codeword across all Annex 8.B tables (`huff_iid_dt[0]`
/// reaches 20 bits).
pub const PS_HUFF_MAX_CODE_LEN: u32 = 20;

/// `huff_iid_df[1]` — Table 8.B.17 (fine grid, frequency direction).
/// Index `i` decodes the delta `i - 30`.
pub const HUFF_IID_FINE_DF: [(u8, u32); 61] = [
    (18, 0b011111111010110100), // -30
    (18, 0b011111111010110101), // -29
    (18, 0b011111110101110110), // -28
    (18, 0b011111110101110111), // -27
    (18, 0b011111110101110100), // -26
    (18, 0b011111110101110101), // -25
    (18, 0b011111111010001010), // -24
    (18, 0b011111111010001011), // -23
    (18, 0b011111111010001000), // -22
    (17, 0b01111111010000000),  // -21
    (18, 0b011111111010110110), // -20
    (17, 0b01111111010000010),  // -19
    (17, 0b01111111010111000),  // -18
    (16, 0b0111111101000010),   // -17
    (16, 0b0111111110101110),   // -16
    (15, 0b011111110101111),    // -15
    (14, 0b01111111010001),     // -14
    (14, 0b01111111101001),     // -13
    (13, 0b0111111101001),      // -12
    (12, 0b011111101010),       // -11
    (12, 0b011111111011),       // -10
    (11, 0b01111111011),        // -9
    (10, 0b0111111011),         // -8
    (10, 0b0111111111),         // -7
    (8, 0b01111100),            // -6
    (7, 0b0111100),             // -5
    (6, 0b011100),              // -4
    (5, 0b01100),               // -3
    (4, 0b0000),                // -2
    (3, 0b001),                 // -1
    (1, 0b1),                   // +0
    (3, 0b010),                 // +1
    (4, 0b0001),                // +2
    (5, 0b01101),               // +3
    (6, 0b011101),              // +4
    (7, 0b0111101),             // +5
    (8, 0b01111101),            // +6
    (9, 0b011111100),           // +7
    (10, 0b0111111100),         // +8
    (11, 0b01111111100),        // +9
    (11, 0b01111110100),        // +10
    (12, 0b011111101011),       // +11
    (13, 0b0111111101010),      // +12
    (14, 0b01111111101010),     // +13
    (14, 0b01111111010110),     // +14
    (15, 0b011111111010000),    // +15
    (16, 0b0111111110101111),   // +16
    (16, 0b0111111101000011),   // +17
    (17, 0b01111111010111001),  // +18
    (17, 0b01111111010000011),  // +19
    (18, 0b011111111010110111), // +20
    (17, 0b01111111010000001),  // +21
    (18, 0b011111111010001001), // +22
    (18, 0b011111111010001110), // +23
    (18, 0b011111111010001111), // +24
    (18, 0b011111111010001100), // +25
    (18, 0b011111111010001101), // +26
    (18, 0b011111111010110010), // +27
    (18, 0b011111111010110011), // +28
    (18, 0b011111111010110000), // +29
    (18, 0b011111111010110001), // +30
];

/// `huff_iid_dt[1]` — Table 8.B.17 (fine grid, time direction).
/// Index `i` decodes the delta `i - 30`.
pub const HUFF_IID_FINE_DT: [(u8, u32); 61] = [
    (16, 0b0100111011010100), // -30
    (16, 0b0100111011010101), // -29
    (16, 0b0100111011001110), // -28
    (16, 0b0100111011001111), // -27
    (16, 0b0100111011001100), // -26
    (16, 0b0100111011010110), // -25
    (16, 0b0100111011011000), // -24
    (16, 0b0100111101000110), // -23
    (16, 0b0100111101100000), // -22
    (15, 0b010011100011000),  // -21
    (15, 0b010011100011001),  // -20
    (15, 0b010011101100100),  // -19
    (15, 0b010011101100101),  // -18
    (15, 0b010011101101101),  // -17
    (15, 0b010011110110001),  // -16
    (14, 0b01001110110111),   // -15
    (14, 0b01001111010110),   // -14
    (13, 0b0100111000111),    // -13
    (13, 0b0100111101001),    // -12
    (13, 0b0100111101101),    // -11
    (12, 0b010011101110),     // -10
    (12, 0b010011110111),     // -9
    (11, 0b01001111000),      // -8
    (10, 0b0100111001),       // -7
    (9, 0b010011010),         // -6
    (9, 0b010011111),         // -5
    (7, 0b0100000),           // -4
    (6, 0b010001),            // -3
    (5, 0b01010),             // -2
    (3, 0b011),               // -1
    (1, 0b1),                 // +0
    (2, 0b00),                // +1
    (5, 0b01011),             // +2
    (6, 0b010010),            // +3
    (7, 0b0100001),           // +4
    (8, 0b01001100),          // +5
    (9, 0b010011011),         // +6
    (10, 0b0100111010),       // +7
    (11, 0b01001111001),      // +8
    (11, 0b01001110000),      // +9
    (12, 0b010011101111),     // +10
    (12, 0b010011100010),     // +11
    (13, 0b0100111101010),    // +12
    (13, 0b0100111011000),    // +13
    (14, 0b01001111010111),   // +14
    (14, 0b01001111010000),   // +15
    (15, 0b010011110110010),  // +16
    (15, 0b010011110100010),  // +17
    (15, 0b010011100011010),  // +18
    (15, 0b010011100011011),  // +19
    (16, 0b0100111101100110), // +20
    (16, 0b0100111101100111), // +21
    (16, 0b0100111101100001), // +22
    (16, 0b0100111101000111), // +23
    (16, 0b0100111011011001), // +24
    (16, 0b0100111011010111), // +25
    (16, 0b0100111011001101), // +26
    (16, 0b0100111011010010), // +27
    (16, 0b0100111011010011), // +28
    (16, 0b0100111011010000), // +29
    (16, 0b0100111011010001), // +30
];

/// `huff_iid_df[0]` — Table 8.B.18 (coarse grid, frequency direction).
/// Index `i` decodes the delta `i - 14`.
pub const HUFF_IID_DF: [(u8, u32); 29] = [
    (17, 0b11111111111111011),  // -14
    (17, 0b11111111111111100),  // -13
    (17, 0b11111111111111101),  // -12
    (17, 0b11111111111111010),  // -11
    (16, 0b1111111111111100),   // -10
    (15, 0b111111111111100),    // -9
    (13, 0b1111111111101),      // -8
    (10, 0b1111111110),         // -7
    (9, 0b111111110),           // -6
    (7, 0b1111110),             // -5
    (6, 0b111100),              // -4
    (5, 0b11101),               // -3
    (4, 0b1101),                // -2
    (3, 0b101),                 // -1
    (1, 0b0),                   // +0
    (3, 0b100),                 // +1
    (4, 0b1100),                // +2
    (5, 0b11100),               // +3
    (6, 0b111101),              // +4
    (6, 0b111110),              // +5
    (8, 0b11111110),            // +6
    (11, 0b11111111110),        // +7
    (13, 0b1111111111100),      // +8
    (14, 0b11111111111100),     // +9
    (14, 0b11111111111101),     // +10
    (15, 0b111111111111101),    // +11
    (17, 0b11111111111111110),  // +12
    (18, 0b111111111111111110), // +13
    (18, 0b111111111111111111), // +14
];

/// `huff_iid_dt[0]` — Table 8.B.18 (coarse grid, time direction).
/// Index `i` decodes the delta `i - 14`.
pub const HUFF_IID_DT: [(u8, u32); 29] = [
    (19, 0b1111111111111111001),  // -14
    (19, 0b1111111111111111010),  // -13
    (19, 0b1111111111111111011),  // -12
    (20, 0b11111111111111111000), // -11
    (20, 0b11111111111111111001), // -10
    (20, 0b11111111111111111010), // -9
    (17, 0b11111111111111101),    // -8
    (15, 0b111111111111110),      // -7
    (12, 0b111111111110),         // -6
    (10, 0b1111111110),           // -5
    (8, 0b11111110),              // -4
    (6, 0b111110),                // -3
    (4, 0b1110),                  // -2
    (2, 0b10),                    // -1
    (1, 0b0),                     // +0
    (3, 0b110),                   // +1
    (5, 0b11110),                 // +2
    (7, 0b1111110),               // +3
    (9, 0b111111110),             // +4
    (11, 0b11111111110),          // +5
    (13, 0b1111111111110),        // +6
    (14, 0b11111111111110),       // +7
    (17, 0b11111111111111100),    // +8
    (19, 0b1111111111111111000),  // +9
    (20, 0b11111111111111111011), // +10
    (20, 0b11111111111111111100), // +11
    (20, 0b11111111111111111101), // +12
    (20, 0b11111111111111111110), // +13
    (20, 0b11111111111111111111), // +14
];

/// `huff_icc_df` — Table 8.B.19 (frequency direction).
/// Index `i` decodes the delta `i - 7`.
pub const HUFF_ICC_DF: [(u8, u32); 15] = [
    (14, 0b11111111111111), // -7
    (14, 0b11111111111110), // -6
    (12, 0b111111111110),   // -5
    (10, 0b1111111110),     // -4
    (7, 0b1111110),         // -3
    (5, 0b11110),           // -2
    (3, 0b110),             // -1
    (1, 0b0),               // +0
    (2, 0b10),              // +1
    (4, 0b1110),            // +2
    (6, 0b111110),          // +3
    (8, 0b11111110),        // +4
    (9, 0b111111110),       // +5
    (11, 0b11111111110),    // +6
    (13, 0b1111111111110),  // +7
];

/// `huff_icc_dt` — Table 8.B.19 (time direction).
/// Index `i` decodes the delta `i - 7`.
pub const HUFF_ICC_DT: [(u8, u32); 15] = [
    (14, 0b11111111111110), // -7
    (13, 0b1111111111110),  // -6
    (11, 0b11111111110),    // -5
    (9, 0b111111110),       // -4
    (7, 0b1111110),         // -3
    (5, 0b11110),           // -2
    (3, 0b110),             // -1
    (1, 0b0),               // +0
    (2, 0b10),              // +1
    (4, 0b1110),            // +2
    (6, 0b111110),          // +3
    (8, 0b11111110),        // +4
    (10, 0b1111111110),     // +5
    (12, 0b111111111110),   // +6
    (14, 0b11111111111111), // +7
];

/// `huff_ipd_df` — Table 8.B.20 (frequency direction). Decodes the
/// raw phase-index delta `0..8` (`lav = 0`).
pub const HUFF_IPD_DF: [(u8, u32); 8] = [
    (1, 0b1),    // 0
    (3, 0b000),  // 1
    (4, 0b0110), // 2
    (4, 0b0100), // 3
    (4, 0b0010), // 4
    (4, 0b0011), // 5
    (4, 0b0101), // 6
    (4, 0b0111), // 7
];

/// `huff_ipd_dt` — Table 8.B.20 (time direction). Decodes the raw
/// phase-index delta `0..8` (`lav = 0`).
pub const HUFF_IPD_DT: [(u8, u32); 8] = [
    (1, 0b1),     // 0
    (3, 0b010),   // 1
    (4, 0b0010),  // 2
    (5, 0b00011), // 3
    (5, 0b00010), // 4
    (4, 0b0000),  // 5
    (4, 0b0011),  // 6
    (3, 0b011),   // 7
];

/// `huff_opd_df` — Table 8.B.21 (frequency direction). Decodes the
/// raw phase-index delta `0..8` (`lav = 0`).
pub const HUFF_OPD_DF: [(u8, u32); 8] = [
    (1, 0b1),     // 0
    (3, 0b001),   // 1
    (4, 0b0110),  // 2
    (4, 0b0100),  // 3
    (5, 0b01111), // 4
    (5, 0b01110), // 5
    (4, 0b0101),  // 6
    (3, 0b000),   // 7
];

/// `huff_opd_dt` — Table 8.B.21 (time direction). Decodes the raw
/// phase-index delta `0..8` (`lav = 0`).
pub const HUFF_OPD_DT: [(u8, u32); 8] = [
    (1, 0b1),     // 0
    (3, 0b010),   // 1
    (4, 0b0001),  // 2
    (5, 0b00111), // 3
    (5, 0b00110), // 4
    (4, 0b0000),  // 5
    (4, 0b0010),  // 6
    (3, 0b011),   // 7
];

/// Decode one PS Huffman codeword from `reader` against `table`,
/// returning `index - lav` (the signed DPCM delta).
///
/// Reads bits MSB-first, accumulating a codeword until it matches an
/// entry `(length, codeword)`. Returns [`Error::PsDataInvalid`] if no
/// codeword of length up to [`PS_HUFF_MAX_CODE_LEN`] matches (a
/// corrupt or truncated `ps_data()` payload).
pub fn ps_huff_dec(
    reader: &mut oxideav_core::bits::BitReader<'_>,
    table: &[(u8, u32)],
    lav: i32,
) -> Result<i32> {
    let mut codeword: u32 = 0;
    let mut len: u32 = 0;
    loop {
        codeword = (codeword << 1) | reader.read_u32(1).map_err(|_| Error::PsDataInvalid)?;
        len += 1;
        for (idx, &(clen, ccode)) in table.iter().enumerate() {
            if u32::from(clen) == len && ccode == codeword {
                return Ok(idx as i32 - lav);
            }
        }
        if len >= PS_HUFF_MAX_CODE_LEN {
            return Err(Error::PsDataInvalid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::bits::{BitReader, BitWriter};

    /// Every table: codewords fit their declared length, the code is
    /// prefix-free, and it is *complete* (Kraft sum exactly 1) — the
    /// invariants the Annex 8.B grids must satisfy.
    fn check_table(table: &[(u8, u32)]) {
        let mut kraft_num: u64 = 0; // sum of 2^(max_len - len)
        for &(len, code) in table {
            assert!(len >= 1 && u32::from(len) <= PS_HUFF_MAX_CODE_LEN);
            assert!(
                u64::from(code) < (1u64 << len),
                "codeword 0x{code:08X} overflows its {len}-bit length"
            );
            kraft_num += 1u64 << (PS_HUFF_MAX_CODE_LEN - u32::from(len));
        }
        assert_eq!(
            kraft_num,
            1u64 << PS_HUFF_MAX_CODE_LEN,
            "code is not complete"
        );
        for (a, &(la, ca)) in table.iter().enumerate() {
            for (b, &(lb, cb)) in table.iter().enumerate() {
                if a == b || lb < la {
                    continue;
                }
                assert!(cb >> (lb - la) != ca, "prefix conflict {a} vs {b}");
            }
        }
    }

    #[test]
    fn all_tables_are_complete_prefix_codes() {
        check_table(&HUFF_IID_FINE_DF);
        check_table(&HUFF_IID_FINE_DT);
        check_table(&HUFF_IID_DF);
        check_table(&HUFF_IID_DT);
        check_table(&HUFF_ICC_DF);
        check_table(&HUFF_ICC_DT);
        check_table(&HUFF_IPD_DF);
        check_table(&HUFF_IPD_DT);
        check_table(&HUFF_OPD_DF);
        check_table(&HUFF_OPD_DT);
    }

    /// Round-trip every index of every table through ps_huff_dec.
    #[test]
    fn every_codeword_decodes_to_its_index() {
        let cases: [(&[(u8, u32)], i32); 10] = [
            (&HUFF_IID_FINE_DF, 30),
            (&HUFF_IID_FINE_DT, 30),
            (&HUFF_IID_DF, 14),
            (&HUFF_IID_DT, 14),
            (&HUFF_ICC_DF, 7),
            (&HUFF_ICC_DT, 7),
            (&HUFF_IPD_DF, 0),
            (&HUFF_IPD_DT, 0),
            (&HUFF_OPD_DF, 0),
            (&HUFF_OPD_DT, 0),
        ];
        for (table, lav) in cases {
            for (idx, &(len, code)) in table.iter().enumerate() {
                let mut w = BitWriter::new();
                w.write_u32(code, u32::from(len));
                let bytes = w.finish();
                let mut r = BitReader::new(&bytes);
                let got = ps_huff_dec(&mut r, table, lav).unwrap();
                assert_eq!(got, idx as i32 - lav);
                assert_eq!(r.bit_position(), u64::from(len));
            }
        }
    }

    /// The zero delta is always the 1-bit codeword `1` for IID/ICC
    /// (Table 8.B.17–8.B.19 anchor `0 → 1`, except the coarse tables'
    /// `0 → 0`) — pin the two anchors that differ.
    #[test]
    fn zero_delta_anchors() {
        // Fine IID: delta 0 = codeword 1 (1 bit).
        assert_eq!(HUFF_IID_FINE_DF[30], (1, 0b1));
        // Coarse IID: delta 0 = codeword 0 (1 bit).
        assert_eq!(HUFF_IID_DF[14], (1, 0b0));
        // ICC: delta 0 = codeword 0 (1 bit).
        assert_eq!(HUFF_ICC_DF[7], (1, 0b0));
        // IPD/OPD: delta 0 = codeword 1 (1 bit).
        assert_eq!(HUFF_IPD_DF[0], (1, 0b1));
        assert_eq!(HUFF_OPD_DT[0], (1, 0b1));
    }

    /// A truncated payload (the reader running dry mid-codeword)
    /// surfaces the parse error rather than spinning.
    #[test]
    fn unmatched_bits_error() {
        // In HUFF_IID_DT the shortest all-ones codeword is 20 bits, so
        // 8 one-bits cannot complete a codeword; the reader runs dry.
        let bytes = [0xFFu8; 1];
        let mut r = BitReader::new(&bytes);
        assert!(matches!(
            ps_huff_dec(&mut r, &HUFF_IID_DT, 14),
            Err(Error::PsDataInvalid)
        ));
    }
}
