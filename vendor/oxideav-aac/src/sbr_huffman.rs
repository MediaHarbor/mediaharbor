//! SBR Huffman codebooks + `sbr_huff_dec()` — ISO/IEC 14496-3
//! Annex 4.A.6.1 (Tables 4.A.78–4.A.88).
//!
//! Spectral Band Replication codes its envelope scalefactors and
//! noise-floor values as DPCM deltas entropy-coded with one of ten
//! canonical Huffman codebooks. The codebook is selected per the
//! §4.6.18.3 `sbr_envelope()` / `sbr_noise()` switch on the coupling
//! flag, the channel index, the amplitude resolution (`bs_amp_res`),
//! and the time/frequency direction (`bs_df_*`):
//!
//! | direction | amp_res | coupling | which | table |
//! |-----------|---------|----------|-------|-------|
//! | time      | 0 (1.5 dB) | level   | env   | [`T_HUFFMAN_ENV_1_5DB`] |
//! | freq      | 0 (1.5 dB) | level   | env   | [`F_HUFFMAN_ENV_1_5DB`] |
//! | time      | 0 (1.5 dB) | balance | env   | [`T_HUFFMAN_ENV_BAL_1_5DB`] |
//! | freq      | 0 (1.5 dB) | balance | env   | [`F_HUFFMAN_ENV_BAL_1_5DB`] |
//! | time      | 1 (3.0 dB) | level   | env   | [`T_HUFFMAN_ENV_3_0DB`] |
//! | freq      | 1 (3.0 dB) | level   | env   | [`F_HUFFMAN_ENV_3_0DB`] |
//! | time      | 1 (3.0 dB) | balance | env   | [`T_HUFFMAN_ENV_BAL_3_0DB`] |
//! | freq      | 1 (3.0 dB) | balance | env   | [`F_HUFFMAN_ENV_BAL_3_0DB`] |
//! | time      | dc          | level   | noise | [`T_HUFFMAN_NOISE_3_0DB`] |
//! | time      | dc          | balance | noise | [`T_HUFFMAN_NOISE_BAL_3_0DB`] |
//!
//! Per Table 4.A.78 Note 2, the *frequency*-direction noise codebooks
//! `f_huffman_noise_3_0dB` / `f_huffman_noise_bal_3_0dB` are identical
//! to the 3.0 dB envelope freq codebooks `f_huffman_env_3_0dB` /
//! `f_huffman_env_bal_3_0dB`, so they are not duplicated here — the
//! [`noise_tables`] selector aliases them.
//!
//! ## Codeword representation
//!
//! Each table is `[(u8, u32); N]` indexed by the Huffman table index,
//! where the tuple is `(code_length_bits, codeword)`. Codewords are
//! MSB-first prefix codes (the most-significant of the `length` low
//! bits is read first). [`sbr_huff_dec`] reads one bit at a time,
//! accumulating MSB-first, and returns the first table index whose
//! `(length, codeword)` matches, with the table's largest-absolute-
//! value (LAV) subtracted so the result is the signed DPCM delta.
//!
//! ## Provenance
//!
//! All ten tables are transcribed directly from the normative
//! codeword grids in ISO/IEC 14496-3:2009 Annex 4.A (Tables 4.A.79
//! through 4.A.88). Each table was validated for completeness (every
//! index 0..=2·LAV present), self-consistency (every codeword fits in
//! its declared bit length), and the prefix-free property (no codeword
//! is a prefix of another) at extraction time.

use crate::{Error, Result};

/// `t_huffman_env_1_5dB` — ISO/IEC 14496-3 Table 4.A.79 (LAV = 60).
///
/// 121 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 60`.
pub const T_HUFFMAN_ENV_1_5DB: [(u8, u32); 121] = [
    (18, 0x0003FFD6),
    (18, 0x0003FFD7),
    (18, 0x0003FFD8),
    (18, 0x0003FFD9),
    (18, 0x0003FFDA),
    (18, 0x0003FFDB),
    (19, 0x0007FFB8),
    (19, 0x0007FFB9),
    (19, 0x0007FFBA),
    (19, 0x0007FFBB),
    (19, 0x0007FFBC),
    (19, 0x0007FFBD),
    (19, 0x0007FFBE),
    (19, 0x0007FFBF),
    (19, 0x0007FFC0),
    (19, 0x0007FFC1),
    (19, 0x0007FFC2),
    (19, 0x0007FFC3),
    (19, 0x0007FFC4),
    (19, 0x0007FFC5),
    (19, 0x0007FFC6),
    (19, 0x0007FFC7),
    (19, 0x0007FFC8),
    (19, 0x0007FFC9),
    (19, 0x0007FFCA),
    (19, 0x0007FFCB),
    (19, 0x0007FFCC),
    (19, 0x0007FFCD),
    (19, 0x0007FFCE),
    (19, 0x0007FFCF),
    (19, 0x0007FFD0),
    (19, 0x0007FFD1),
    (19, 0x0007FFD2),
    (19, 0x0007FFD3),
    (17, 0x0001FFE6),
    (18, 0x0003FFD4),
    (16, 0x0000FFF0),
    (17, 0x0001FFE9),
    (18, 0x0003FFD5),
    (17, 0x0001FFE7),
    (16, 0x0000FFF1),
    (16, 0x0000FFEC),
    (16, 0x0000FFED),
    (16, 0x0000FFEE),
    (15, 0x00007FF4),
    (14, 0x00003FF9),
    (14, 0x00003FF7),
    (13, 0x00001FFA),
    (13, 0x00001FF9),
    (12, 0x00000FFB),
    (11, 0x000007FC),
    (10, 0x000003FC),
    (9, 0x000001FD),
    (8, 0x000000FD),
    (7, 0x0000007D),
    (6, 0x0000003D),
    (5, 0x0000001D),
    (4, 0x0000000D),
    (3, 0x00000005),
    (2, 0x00000001),
    (2, 0x00000000),
    (3, 0x00000004),
    (4, 0x0000000C),
    (5, 0x0000001C),
    (6, 0x0000003C),
    (7, 0x0000007C),
    (8, 0x000000FC),
    (9, 0x000001FC),
    (10, 0x000003FD),
    (12, 0x00000FFA),
    (13, 0x00001FF8),
    (14, 0x00003FF6),
    (14, 0x00003FF8),
    (15, 0x00007FF5),
    (16, 0x0000FFEF),
    (17, 0x0001FFE8),
    (16, 0x0000FFF2),
    (19, 0x0007FFD4),
    (19, 0x0007FFD5),
    (19, 0x0007FFD6),
    (19, 0x0007FFD7),
    (19, 0x0007FFD8),
    (19, 0x0007FFD9),
    (19, 0x0007FFDA),
    (19, 0x0007FFDB),
    (19, 0x0007FFDC),
    (19, 0x0007FFDD),
    (19, 0x0007FFDE),
    (19, 0x0007FFDF),
    (19, 0x0007FFE0),
    (19, 0x0007FFE1),
    (19, 0x0007FFE2),
    (19, 0x0007FFE3),
    (19, 0x0007FFE4),
    (19, 0x0007FFE5),
    (19, 0x0007FFE6),
    (19, 0x0007FFE7),
    (19, 0x0007FFE8),
    (19, 0x0007FFE9),
    (19, 0x0007FFEA),
    (19, 0x0007FFEB),
    (19, 0x0007FFEC),
    (19, 0x0007FFED),
    (19, 0x0007FFEE),
    (19, 0x0007FFEF),
    (19, 0x0007FFF0),
    (19, 0x0007FFF1),
    (19, 0x0007FFF2),
    (19, 0x0007FFF3),
    (19, 0x0007FFF4),
    (19, 0x0007FFF5),
    (19, 0x0007FFF6),
    (19, 0x0007FFF7),
    (19, 0x0007FFF8),
    (19, 0x0007FFF9),
    (19, 0x0007FFFA),
    (19, 0x0007FFFB),
    (19, 0x0007FFFC),
    (19, 0x0007FFFD),
    (19, 0x0007FFFE),
    (19, 0x0007FFFF),
];

/// `f_huffman_env_1_5dB` — ISO/IEC 14496-3 Table 4.A.80 (LAV = 60).
///
/// 121 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 60`.
pub const F_HUFFMAN_ENV_1_5DB: [(u8, u32); 121] = [
    (19, 0x0007FFE7),
    (19, 0x0007FFE8),
    (20, 0x000FFFD2),
    (20, 0x000FFFD3),
    (20, 0x000FFFD4),
    (20, 0x000FFFD5),
    (20, 0x000FFFD6),
    (20, 0x000FFFD7),
    (20, 0x000FFFD8),
    (19, 0x0007FFDA),
    (20, 0x000FFFD9),
    (20, 0x000FFFDA),
    (20, 0x000FFFDB),
    (20, 0x000FFFDC),
    (19, 0x0007FFDB),
    (20, 0x000FFFDD),
    (19, 0x0007FFDC),
    (19, 0x0007FFDD),
    (20, 0x000FFFDE),
    (18, 0x0003FFE4),
    (20, 0x000FFFDF),
    (20, 0x000FFFE0),
    (20, 0x000FFFE1),
    (19, 0x0007FFDE),
    (20, 0x000FFFE2),
    (20, 0x000FFFE3),
    (20, 0x000FFFE4),
    (19, 0x0007FFDF),
    (20, 0x000FFFE5),
    (19, 0x0007FFE0),
    (18, 0x0003FFE8),
    (19, 0x0007FFE1),
    (18, 0x0003FFE0),
    (18, 0x0003FFE9),
    (17, 0x0001FFEF),
    (18, 0x0003FFE5),
    (17, 0x0001FFEC),
    (17, 0x0001FFED),
    (17, 0x0001FFEE),
    (16, 0x0000FFF4),
    (16, 0x0000FFF3),
    (16, 0x0000FFF0),
    (15, 0x00007FF7),
    (15, 0x00007FF6),
    (14, 0x00003FFA),
    (13, 0x00001FFA),
    (13, 0x00001FF9),
    (12, 0x00000FFA),
    (12, 0x00000FF8),
    (11, 0x000007F9),
    (10, 0x000003FB),
    (9, 0x000001FC),
    (9, 0x000001FA),
    (8, 0x000000FB),
    (7, 0x0000007C),
    (6, 0x0000003C),
    (5, 0x0000001C),
    (4, 0x0000000C),
    (3, 0x00000005),
    (2, 0x00000001),
    (2, 0x00000000),
    (3, 0x00000004),
    (4, 0x0000000D),
    (5, 0x0000001D),
    (6, 0x0000003D),
    (8, 0x000000FA),
    (8, 0x000000FC),
    (9, 0x000001FB),
    (10, 0x000003FA),
    (11, 0x000007F8),
    (11, 0x000007FA),
    (11, 0x000007FB),
    (12, 0x00000FF9),
    (12, 0x00000FFB),
    (13, 0x00001FF8),
    (13, 0x00001FFB),
    (14, 0x00003FF8),
    (14, 0x00003FF9),
    (16, 0x0000FFF1),
    (16, 0x0000FFF2),
    (17, 0x0001FFEA),
    (17, 0x0001FFEB),
    (18, 0x0003FFE1),
    (18, 0x0003FFE2),
    (18, 0x0003FFEA),
    (18, 0x0003FFE3),
    (18, 0x0003FFE6),
    (18, 0x0003FFE7),
    (18, 0x0003FFEB),
    (20, 0x000FFFE6),
    (19, 0x0007FFE2),
    (20, 0x000FFFE7),
    (20, 0x000FFFE8),
    (20, 0x000FFFE9),
    (20, 0x000FFFEA),
    (20, 0x000FFFEB),
    (20, 0x000FFFEC),
    (19, 0x0007FFE3),
    (20, 0x000FFFED),
    (20, 0x000FFFEE),
    (20, 0x000FFFEF),
    (20, 0x000FFFF0),
    (19, 0x0007FFE4),
    (20, 0x000FFFF1),
    (18, 0x0003FFEC),
    (20, 0x000FFFF2),
    (20, 0x000FFFF3),
    (19, 0x0007FFE5),
    (19, 0x0007FFE6),
    (20, 0x000FFFF4),
    (20, 0x000FFFF5),
    (20, 0x000FFFF6),
    (20, 0x000FFFF7),
    (20, 0x000FFFF8),
    (20, 0x000FFFF9),
    (20, 0x000FFFFA),
    (20, 0x000FFFFB),
    (20, 0x000FFFFC),
    (20, 0x000FFFFD),
    (20, 0x000FFFFE),
    (20, 0x000FFFFF),
];

/// `t_huffman_env_bal_1_5dB` — ISO/IEC 14496-3 Table 4.A.81 (LAV = 24).
///
/// 49 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 24`.
pub const T_HUFFMAN_ENV_BAL_1_5DB: [(u8, u32); 49] = [
    (16, 0x0000FFE4),
    (16, 0x0000FFE5),
    (16, 0x0000FFE6),
    (16, 0x0000FFE7),
    (16, 0x0000FFE8),
    (16, 0x0000FFE9),
    (16, 0x0000FFEA),
    (16, 0x0000FFEB),
    (16, 0x0000FFEC),
    (16, 0x0000FFED),
    (16, 0x0000FFEE),
    (16, 0x0000FFEF),
    (16, 0x0000FFF0),
    (16, 0x0000FFF1),
    (16, 0x0000FFF2),
    (16, 0x0000FFF3),
    (16, 0x0000FFF4),
    (16, 0x0000FFE2),
    (12, 0x00000FFC),
    (11, 0x000007FC),
    (9, 0x000001FE),
    (7, 0x0000007E),
    (5, 0x0000001E),
    (3, 0x00000006),
    (1, 0x00000000),
    (2, 0x00000002),
    (4, 0x0000000E),
    (6, 0x0000003E),
    (8, 0x000000FE),
    (11, 0x000007FD),
    (12, 0x00000FFD),
    (15, 0x00007FF0),
    (16, 0x0000FFE3),
    (16, 0x0000FFF5),
    (16, 0x0000FFF6),
    (16, 0x0000FFF7),
    (16, 0x0000FFF8),
    (16, 0x0000FFF9),
    (16, 0x0000FFFA),
    (17, 0x0001FFF6),
    (17, 0x0001FFF7),
    (17, 0x0001FFF8),
    (17, 0x0001FFF9),
    (17, 0x0001FFFA),
    (17, 0x0001FFFB),
    (17, 0x0001FFFC),
    (17, 0x0001FFFD),
    (17, 0x0001FFFE),
    (17, 0x0001FFFF),
];

/// `f_huffman_env_bal_1_5dB` — ISO/IEC 14496-3 Table 4.A.82 (LAV = 24).
///
/// 49 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 24`.
pub const F_HUFFMAN_ENV_BAL_1_5DB: [(u8, u32); 49] = [
    (18, 0x0003FFE2),
    (18, 0x0003FFE3),
    (18, 0x0003FFE4),
    (18, 0x0003FFE5),
    (18, 0x0003FFE6),
    (18, 0x0003FFE7),
    (18, 0x0003FFE8),
    (18, 0x0003FFE9),
    (18, 0x0003FFEA),
    (18, 0x0003FFEB),
    (18, 0x0003FFEC),
    (18, 0x0003FFED),
    (18, 0x0003FFEE),
    (18, 0x0003FFEF),
    (18, 0x0003FFF0),
    (16, 0x0000FFF7),
    (17, 0x0001FFF0),
    (14, 0x00003FFC),
    (11, 0x000007FE),
    (11, 0x000007FC),
    (8, 0x000000FE),
    (7, 0x0000007E),
    (4, 0x0000000E),
    (2, 0x00000002),
    (1, 0x00000000),
    (3, 0x00000006),
    (5, 0x0000001E),
    (6, 0x0000003E),
    (9, 0x000001FE),
    (11, 0x000007FD),
    (12, 0x00000FFE),
    (15, 0x00007FFA),
    (16, 0x0000FFF6),
    (18, 0x0003FFF1),
    (18, 0x0003FFF2),
    (18, 0x0003FFF3),
    (18, 0x0003FFF4),
    (18, 0x0003FFF5),
    (18, 0x0003FFF6),
    (18, 0x0003FFF7),
    (18, 0x0003FFF8),
    (18, 0x0003FFF9),
    (18, 0x0003FFFA),
    (18, 0x0003FFFB),
    (18, 0x0003FFFC),
    (18, 0x0003FFFD),
    (18, 0x0003FFFE),
    (19, 0x0007FFFE),
    (19, 0x0007FFFF),
];

/// `t_huffman_env_3_0dB` — ISO/IEC 14496-3 Table 4.A.83 (LAV = 31).
///
/// 63 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 31`.
pub const T_HUFFMAN_ENV_3_0DB: [(u8, u32); 63] = [
    (18, 0x0003FFED),
    (18, 0x0003FFEE),
    (19, 0x0007FFDE),
    (19, 0x0007FFDF),
    (19, 0x0007FFE0),
    (19, 0x0007FFE1),
    (19, 0x0007FFE2),
    (19, 0x0007FFE3),
    (19, 0x0007FFE4),
    (19, 0x0007FFE5),
    (19, 0x0007FFE6),
    (19, 0x0007FFE7),
    (19, 0x0007FFE8),
    (19, 0x0007FFE9),
    (19, 0x0007FFEA),
    (19, 0x0007FFEB),
    (19, 0x0007FFEC),
    (17, 0x0001FFF4),
    (16, 0x0000FFF7),
    (16, 0x0000FFF9),
    (16, 0x0000FFF8),
    (14, 0x00003FFB),
    (14, 0x00003FFA),
    (14, 0x00003FF8),
    (13, 0x00001FFA),
    (12, 0x00000FFC),
    (11, 0x000007FC),
    (8, 0x000000FE),
    (6, 0x0000003E),
    (4, 0x0000000E),
    (2, 0x00000002),
    (1, 0x00000000),
    (3, 0x00000006),
    (5, 0x0000001E),
    (7, 0x0000007E),
    (9, 0x000001FE),
    (11, 0x000007FD),
    (13, 0x00001FFB),
    (14, 0x00003FF9),
    (14, 0x00003FFC),
    (15, 0x00007FFA),
    (16, 0x0000FFF6),
    (17, 0x0001FFF5),
    (18, 0x0003FFEC),
    (19, 0x0007FFED),
    (19, 0x0007FFEE),
    (19, 0x0007FFEF),
    (19, 0x0007FFF0),
    (19, 0x0007FFF1),
    (19, 0x0007FFF2),
    (19, 0x0007FFF3),
    (19, 0x0007FFF4),
    (19, 0x0007FFF5),
    (19, 0x0007FFF6),
    (19, 0x0007FFF7),
    (19, 0x0007FFF8),
    (19, 0x0007FFF9),
    (19, 0x0007FFFA),
    (19, 0x0007FFFB),
    (19, 0x0007FFFC),
    (19, 0x0007FFFD),
    (19, 0x0007FFFE),
    (19, 0x0007FFFF),
];

/// `f_huffman_env_3_0dB` — ISO/IEC 14496-3 Table 4.A.84 (LAV = 31).
///
/// 63 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 31`.
pub const F_HUFFMAN_ENV_3_0DB: [(u8, u32); 63] = [
    (20, 0x000FFFF0),
    (20, 0x000FFFF1),
    (20, 0x000FFFF2),
    (20, 0x000FFFF3),
    (20, 0x000FFFF4),
    (20, 0x000FFFF5),
    (20, 0x000FFFF6),
    (18, 0x0003FFF3),
    (19, 0x0007FFF5),
    (19, 0x0007FFEE),
    (19, 0x0007FFEF),
    (19, 0x0007FFF6),
    (18, 0x0003FFF4),
    (18, 0x0003FFF2),
    (20, 0x000FFFF7),
    (19, 0x0007FFF0),
    (17, 0x0001FFF5),
    (18, 0x0003FFF0),
    (17, 0x0001FFF4),
    (16, 0x0000FFF7),
    (16, 0x0000FFF6),
    (15, 0x00007FF8),
    (14, 0x00003FFB),
    (12, 0x00000FFD),
    (11, 0x000007FD),
    (10, 0x000003FD),
    (9, 0x000001FD),
    (8, 0x000000FD),
    (6, 0x0000003E),
    (4, 0x0000000E),
    (2, 0x00000002),
    (1, 0x00000000),
    (3, 0x00000006),
    (5, 0x0000001E),
    (8, 0x000000FC),
    (9, 0x000001FC),
    (10, 0x000003FC),
    (11, 0x000007FC),
    (12, 0x00000FFC),
    (13, 0x00001FFC),
    (14, 0x00003FFA),
    (15, 0x00007FF9),
    (15, 0x00007FFA),
    (16, 0x0000FFF8),
    (16, 0x0000FFF9),
    (17, 0x0001FFF6),
    (17, 0x0001FFF7),
    (18, 0x0003FFF5),
    (18, 0x0003FFF6),
    (18, 0x0003FFF1),
    (20, 0x000FFFF8),
    (19, 0x0007FFF1),
    (19, 0x0007FFF2),
    (19, 0x0007FFF3),
    (20, 0x000FFFF9),
    (19, 0x0007FFF7),
    (19, 0x0007FFF4),
    (20, 0x000FFFFA),
    (20, 0x000FFFFB),
    (20, 0x000FFFFC),
    (20, 0x000FFFFD),
    (20, 0x000FFFFE),
    (20, 0x000FFFFF),
];

/// `t_huffman_env_bal_3_0dB` — ISO/IEC 14496-3 Table 4.A.85 (LAV = 12).
///
/// 25 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 12`.
pub const T_HUFFMAN_ENV_BAL_3_0DB: [(u8, u32); 25] = [
    (13, 0x00001FF2),
    (13, 0x00001FF3),
    (13, 0x00001FF4),
    (13, 0x00001FF5),
    (13, 0x00001FF6),
    (13, 0x00001FF7),
    (13, 0x00001FF8),
    (12, 0x00000FF8),
    (8, 0x000000FE),
    (7, 0x0000007E),
    (4, 0x0000000E),
    (3, 0x00000006),
    (1, 0x00000000),
    (2, 0x00000002),
    (5, 0x0000001E),
    (6, 0x0000003E),
    (9, 0x000001FE),
    (13, 0x00001FF9),
    (13, 0x00001FFA),
    (13, 0x00001FFB),
    (13, 0x00001FFC),
    (13, 0x00001FFD),
    (13, 0x00001FFE),
    (14, 0x00003FFE),
    (14, 0x00003FFF),
];

/// `f_huffman_env_bal_3_0dB` — ISO/IEC 14496-3 Table 4.A.86 (LAV = 12).
///
/// 25 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 12`.
pub const F_HUFFMAN_ENV_BAL_3_0DB: [(u8, u32); 25] = [
    (13, 0x00001FF7),
    (13, 0x00001FF8),
    (13, 0x00001FF9),
    (13, 0x00001FFA),
    (13, 0x00001FFB),
    (14, 0x00003FF8),
    (14, 0x00003FF9),
    (11, 0x000007FC),
    (8, 0x000000FE),
    (7, 0x0000007E),
    (4, 0x0000000E),
    (2, 0x00000002),
    (1, 0x00000000),
    (3, 0x00000006),
    (5, 0x0000001E),
    (6, 0x0000003E),
    (9, 0x000001FE),
    (12, 0x00000FFA),
    (13, 0x00001FF6),
    (14, 0x00003FFA),
    (14, 0x00003FFB),
    (14, 0x00003FFC),
    (14, 0x00003FFD),
    (14, 0x00003FFE),
    (14, 0x00003FFF),
];

/// `t_huffman_noise_3_0dB` — ISO/IEC 14496-3 Table 4.A.87 (LAV = 31).
///
/// 63 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 31`.
pub const T_HUFFMAN_NOISE_3_0DB: [(u8, u32); 63] = [
    (13, 0x00001FCE),
    (13, 0x00001FCF),
    (13, 0x00001FD0),
    (13, 0x00001FD1),
    (13, 0x00001FD2),
    (13, 0x00001FD3),
    (13, 0x00001FD4),
    (13, 0x00001FD5),
    (13, 0x00001FD6),
    (13, 0x00001FD7),
    (13, 0x00001FD8),
    (13, 0x00001FD9),
    (13, 0x00001FDA),
    (13, 0x00001FDB),
    (13, 0x00001FDC),
    (13, 0x00001FDD),
    (13, 0x00001FDE),
    (13, 0x00001FDF),
    (13, 0x00001FE0),
    (13, 0x00001FE1),
    (13, 0x00001FE2),
    (13, 0x00001FE3),
    (13, 0x00001FE4),
    (13, 0x00001FE5),
    (13, 0x00001FE6),
    (13, 0x00001FE7),
    (11, 0x000007F2),
    (8, 0x000000FD),
    (6, 0x0000003E),
    (4, 0x0000000E),
    (3, 0x00000006),
    (1, 0x00000000),
    (2, 0x00000002),
    (5, 0x0000001E),
    (8, 0x000000FC),
    (10, 0x000003F8),
    (13, 0x00001FCC),
    (13, 0x00001FE8),
    (13, 0x00001FE9),
    (13, 0x00001FEA),
    (13, 0x00001FEB),
    (13, 0x00001FEC),
    (13, 0x00001FCD),
    (13, 0x00001FED),
    (13, 0x00001FEE),
    (13, 0x00001FEF),
    (13, 0x00001FF0),
    (13, 0x00001FF1),
    (13, 0x00001FF2),
    (13, 0x00001FF3),
    (13, 0x00001FF4),
    (13, 0x00001FF5),
    (13, 0x00001FF6),
    (13, 0x00001FF7),
    (13, 0x00001FF8),
    (13, 0x00001FF9),
    (13, 0x00001FFA),
    (13, 0x00001FFB),
    (13, 0x00001FFC),
    (13, 0x00001FFD),
    (13, 0x00001FFE),
    (14, 0x00003FFE),
    (14, 0x00003FFF),
];

/// `t_huffman_noise_bal_3_0dB` — ISO/IEC 14496-3 Table 4.A.88 (LAV = 12).
///
/// 25 entries `(code_length_bits, codeword)` indexed by the Huffman
/// table index; the decoded value is `index - 12`.
pub const T_HUFFMAN_NOISE_BAL_3_0DB: [(u8, u32); 25] = [
    (8, 0x000000EC),
    (8, 0x000000ED),
    (8, 0x000000EE),
    (8, 0x000000EF),
    (8, 0x000000F0),
    (8, 0x000000F1),
    (8, 0x000000F2),
    (8, 0x000000F3),
    (8, 0x000000F4),
    (8, 0x000000F5),
    (5, 0x0000001C),
    (2, 0x00000002),
    (1, 0x00000000),
    (3, 0x00000006),
    (6, 0x0000003A),
    (8, 0x000000F6),
    (8, 0x000000F7),
    (8, 0x000000F8),
    (8, 0x000000F9),
    (8, 0x000000FA),
    (8, 0x000000FB),
    (8, 0x000000FC),
    (8, 0x000000FD),
    (8, 0x000000FE),
    (8, 0x000000FF),
];

/// Resolution / coupling context that picks an envelope or noise
/// codebook pair, per the §4.6.18.3 `sbr_envelope()` / `sbr_noise()`
/// table-selection pseudo-code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SbrHuffContext {
    /// `bs_coupling` — the channel pair is coupled (balance coding).
    pub coupling: bool,
    /// Channel index within the element (`0` or `1`); only relevant
    /// when `coupling` is set (the second coupled channel carries the
    /// balance values).
    pub ch: bool,
    /// `bs_amp_res` — `false` = 1.5 dB resolution, `true` = 3.0 dB.
    pub amp_res: bool,
}

/// One SBR Huffman codebook ready for [`sbr_huff_dec`]: the table
/// slice and its largest-absolute-value (`lav`) offset.
pub type SbrHuffCodebook = (&'static [(u8, u32)], i32);

/// Returns the `(t_huff, f_huff)` envelope codebook pair for a given
/// `sbr_envelope()` context, per the §4.6.18.3 selection pseudo-code
/// (Table 4.72 surrounding text). `t_huff` is the time-direction
/// table, `f_huff` the frequency-direction table; each is returned as
/// `(slice, lav)`.
pub fn env_tables(ctx: SbrHuffContext) -> (SbrHuffCodebook, SbrHuffCodebook) {
    // The balance tables are only ever selected for the *second*
    // channel of a coupled pair; otherwise the level tables apply.
    if ctx.coupling && ctx.ch {
        if ctx.amp_res {
            (
                (&T_HUFFMAN_ENV_BAL_3_0DB, 12),
                (&F_HUFFMAN_ENV_BAL_3_0DB, 12),
            )
        } else {
            (
                (&T_HUFFMAN_ENV_BAL_1_5DB, 24),
                (&F_HUFFMAN_ENV_BAL_1_5DB, 24),
            )
        }
    } else if ctx.amp_res {
        ((&T_HUFFMAN_ENV_3_0DB, 31), (&F_HUFFMAN_ENV_3_0DB, 31))
    } else {
        ((&T_HUFFMAN_ENV_1_5DB, 60), (&F_HUFFMAN_ENV_1_5DB, 60))
    }
}

/// Returns the `(t_huff, f_huff)` noise codebook pair for a given
/// `sbr_noise()` context, per the §4.6.18.3 selection pseudo-code
/// (Table 4.73 surrounding text). Noise floors are always coded at the
/// 3.0 dB resolution (`bs_amp_res` is "don't care" for noise). Per
/// Table 4.A.78 Note 2 the frequency-direction noise codebooks reuse
/// the 3.0 dB *envelope* frequency codebooks.
pub fn noise_tables(ctx: SbrHuffContext) -> (SbrHuffCodebook, SbrHuffCodebook) {
    if ctx.coupling && ctx.ch {
        (
            (&T_HUFFMAN_NOISE_BAL_3_0DB, 12),
            // f_huffman_noise_bal_3_0dB == f_huffman_env_bal_3_0dB.
            (&F_HUFFMAN_ENV_BAL_3_0DB, 12),
        )
    } else {
        (
            (&T_HUFFMAN_NOISE_3_0DB, 31),
            // f_huffman_noise_3_0dB == f_huffman_env_3_0dB.
            (&F_HUFFMAN_ENV_3_0DB, 31),
        )
    }
}

/// The longest codeword across every SBR Huffman table is 20 bits
/// (`f_huffman_env_1_5dB` / `f_huffman_env_3_0dB`). `sbr_huff_dec`
/// refuses to read past this many bits without a match (a malformed
/// bitstream would otherwise loop until the reader runs dry).
pub const SBR_HUFF_MAX_CODE_LEN: u32 = 20;

/// `sbr_huff_dec()` — ISO/IEC 14496-3 Annex 4.A.6.1.
///
/// Reads bits MSB-first from `reader`, accumulating a codeword, until
/// it matches an entry `(length, codeword)` of `table`. Returns the
/// matching table index minus `lav`, i.e. the signed DPCM delta the
/// envelope / noise reconstruction adds to the running value.
///
/// Returns [`Error::SbrHuffInvalid`] if no codeword of length up to
/// [`SBR_HUFF_MAX_CODE_LEN`] matches (a corrupt or truncated payload).
pub fn sbr_huff_dec(
    reader: &mut oxideav_core::bits::BitReader<'_>,
    table: &[(u8, u32)],
    lav: i32,
) -> Result<i32> {
    let mut codeword: u32 = 0;
    let mut len: u32 = 0;
    loop {
        codeword = (codeword << 1) | reader.read_u32(1).map_err(|_| Error::SbrHuffInvalid)?;
        len += 1;
        for (idx, &(clen, ccode)) in table.iter().enumerate() {
            if u32::from(clen) == len && ccode == codeword {
                return Ok(idx as i32 - lav);
            }
        }
        if len >= SBR_HUFF_MAX_CODE_LEN {
            return Err(Error::SbrHuffInvalid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxideav_core::bits::BitReader;

    /// Every table is complete, codewords fit their declared length,
    /// and the table is prefix-free — the canonical-Huffman invariants
    /// the spec grids must satisfy.
    fn check_table(table: &[(u8, u32)]) {
        for &(len, code) in table {
            assert!(len >= 1 && len <= SBR_HUFF_MAX_CODE_LEN as u8);
            // codeword fits in its declared bit length.
            assert!(
                code < (1u32 << len),
                "codeword 0x{code:08X} overflows its {len}-bit length"
            );
        }
        // Prefix-free: no codeword is a prefix of another. With
        // `lb >= la` (the shorter or equal code is `ca`), truncating
        // the longer code `cb` to `la` bits must not equal `ca` —
        // covering both the equal-length collision and the strict
        // prefix case in one comparison.
        for (a, &(la, ca)) in table.iter().enumerate() {
            for (b, &(lb, cb)) in table.iter().enumerate() {
                if a == b || lb < la {
                    continue;
                }
                let shifted = cb >> (lb - la);
                assert!(shifted != ca, "prefix conflict between index {a} and {b}");
            }
        }
    }

    #[test]
    fn all_tables_valid() {
        check_table(&T_HUFFMAN_ENV_1_5DB);
        check_table(&F_HUFFMAN_ENV_1_5DB);
        check_table(&T_HUFFMAN_ENV_BAL_1_5DB);
        check_table(&F_HUFFMAN_ENV_BAL_1_5DB);
        check_table(&T_HUFFMAN_ENV_3_0DB);
        check_table(&F_HUFFMAN_ENV_3_0DB);
        check_table(&T_HUFFMAN_ENV_BAL_3_0DB);
        check_table(&F_HUFFMAN_ENV_BAL_3_0DB);
        check_table(&T_HUFFMAN_NOISE_3_0DB);
        check_table(&T_HUFFMAN_NOISE_BAL_3_0DB);
    }

    #[test]
    fn table_sizes_match_lav() {
        assert_eq!(T_HUFFMAN_ENV_1_5DB.len(), 121);
        assert_eq!(F_HUFFMAN_ENV_1_5DB.len(), 121);
        assert_eq!(T_HUFFMAN_ENV_BAL_1_5DB.len(), 49);
        assert_eq!(F_HUFFMAN_ENV_BAL_1_5DB.len(), 49);
        assert_eq!(T_HUFFMAN_ENV_3_0DB.len(), 63);
        assert_eq!(F_HUFFMAN_ENV_3_0DB.len(), 63);
        assert_eq!(T_HUFFMAN_ENV_BAL_3_0DB.len(), 25);
        assert_eq!(F_HUFFMAN_ENV_BAL_3_0DB.len(), 25);
        assert_eq!(T_HUFFMAN_NOISE_3_0DB.len(), 63);
        assert_eq!(T_HUFFMAN_NOISE_BAL_3_0DB.len(), 25);
    }

    /// Encode each codeword MSB-first into a byte buffer and confirm
    /// `sbr_huff_dec` decodes back to `index - lav`.
    fn roundtrip(table: &[(u8, u32)], lav: i32) {
        for (idx, &(len, code)) in table.iter().enumerate() {
            // Pack the codeword MSB-first, then pad to a byte so the
            // reader has whole bytes to consume.
            let mut bits: Vec<u8> = Vec::new();
            for b in (0..len).rev() {
                bits.push(((code >> b) & 1) as u8);
            }
            let mut bytes = vec![0u8; len.div_ceil(8) as usize];
            for (i, &bit) in bits.iter().enumerate() {
                if bit != 0 {
                    bytes[i / 8] |= 1 << (7 - (i % 8));
                }
            }
            let mut reader = BitReader::new(&bytes);
            let got = sbr_huff_dec(&mut reader, table, lav).unwrap();
            assert_eq!(got, idx as i32 - lav, "table index {idx}");
        }
    }

    #[test]
    fn roundtrip_all() {
        roundtrip(&T_HUFFMAN_ENV_1_5DB, 60);
        roundtrip(&F_HUFFMAN_ENV_1_5DB, 60);
        roundtrip(&T_HUFFMAN_ENV_BAL_1_5DB, 24);
        roundtrip(&F_HUFFMAN_ENV_BAL_1_5DB, 24);
        roundtrip(&T_HUFFMAN_ENV_3_0DB, 31);
        roundtrip(&F_HUFFMAN_ENV_3_0DB, 31);
        roundtrip(&T_HUFFMAN_ENV_BAL_3_0DB, 12);
        roundtrip(&F_HUFFMAN_ENV_BAL_3_0DB, 12);
        roundtrip(&T_HUFFMAN_NOISE_3_0DB, 31);
        roundtrip(&T_HUFFMAN_NOISE_BAL_3_0DB, 12);
    }

    #[test]
    fn context_selectors() {
        // Mono / level path picks the level tables.
        let ((tt, tl), (ft, fl)) = env_tables(SbrHuffContext {
            coupling: false,
            ch: false,
            amp_res: false,
        });
        assert_eq!(tl, 60);
        assert_eq!(fl, 60);
        assert_eq!(tt.len(), 121);
        assert_eq!(ft.len(), 121);

        // Coupled second channel at 3.0 dB picks the balance tables.
        let ((tt, tl), (_ft, _fl)) = env_tables(SbrHuffContext {
            coupling: true,
            ch: true,
            amp_res: true,
        });
        assert_eq!(tl, 12);
        assert_eq!(tt.len(), 25);

        // Noise freq-direction reuses the 3.0 dB envelope freq table
        // (Table 4.A.78 Note 2): same contents, same LAV.
        let ((_nt, _nl), (nf, nfl)) = noise_tables(SbrHuffContext {
            coupling: false,
            ch: false,
            amp_res: false,
        });
        assert_eq!(nf, &F_HUFFMAN_ENV_3_0DB[..]);
        assert_eq!(nfl, 31);
        // Coupled noise balance freq-direction reuses the 3.0 dB
        // envelope balance freq table.
        let ((_nt, _nl), (nf, nfl)) = noise_tables(SbrHuffContext {
            coupling: true,
            ch: true,
            amp_res: false,
        });
        assert_eq!(nf, &F_HUFFMAN_ENV_BAL_3_0DB[..]);
        assert_eq!(nfl, 12);
    }

    #[test]
    fn truncated_payload_errors() {
        // An empty buffer can never complete a codeword — the first
        // bit read fails and maps to SbrHuffInvalid rather than the raw
        // bitreader error.
        let bytes: [u8; 0] = [];
        let mut reader = BitReader::new(&bytes);
        assert!(matches!(
            sbr_huff_dec(&mut reader, &T_HUFFMAN_ENV_1_5DB, 60),
            Err(Error::SbrHuffInvalid)
        ));
    }

    /// The noise balance table's longest codeword followed by the
    /// shortest exercises the bit-at-a-time accumulation past a byte
    /// boundary.
    #[test]
    fn decode_across_byte_boundary() {
        // f_huffman_env_1_5dB index 0 is an 18-bit codeword; decode it
        // then immediately decode index 60's 2-bit codeword from the
        // same stream.
        let (l0, c0) = F_HUFFMAN_ENV_1_5DB[0];
        let (l1, c1) = F_HUFFMAN_ENV_1_5DB[60];
        let total = l0 as u32 + l1 as u32;
        let combined = (u64::from(c0) << l1) | u64::from(c1);
        let nbytes = total.div_ceil(8) as usize;
        let mut bytes = vec![0u8; nbytes];
        for b in 0..total {
            let bit = (combined >> (total - 1 - b)) & 1;
            if bit != 0 {
                bytes[(b / 8) as usize] |= 1 << (7 - (b % 8));
            }
        }
        let mut reader = BitReader::new(&bytes);
        assert_eq!(
            sbr_huff_dec(&mut reader, &F_HUFFMAN_ENV_1_5DB, 60).unwrap(),
            -60
        );
        assert_eq!(
            sbr_huff_dec(&mut reader, &F_HUFFMAN_ENV_1_5DB, 60).unwrap(),
            0
        );
    }
}
