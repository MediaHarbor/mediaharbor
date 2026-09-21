//! Error-protection CRC generator — ISO/IEC 14496-3 §1.8.4.5.
//!
//! §1.8.4.5 defines a family of cyclic-redundancy-check codes used by
//! the MPEG-4 Audio error-protection (EP) tool and by the LATM
//! `StreamMuxConfig()` `crcCheckSum` field (§1.7.3.1, Table 1.42:
//! "This CRC uses the generation polynomial CRC8, as defined in
//! subclause 1.8.4.5 and covers the entire StreamMuxConfig() up to but
//! excluding the crcCheckPresent bit").
//!
//! ## Generation polynomials (§1.8.4.5)
//!
//! Each `k`-bit CRC has a generator polynomial `G(x)` of degree `k`:
//!
//! | `k`  | `G(x)`                                                          |
//! |------|-----------------------------------------------------------------|
//! | 4    | x⁴ + x³ + x² + 1                                                |
//! | 5    | x⁵ + x⁴ + x² + 1                                                |
//! | 6    | x⁶ + x⁵ + x⁴ + x² + x + 1                                       |
//! | 7    | x⁷ + x³ + x + 1                                                 |
//! | 8    | x⁸ + x⁴ + x³ + x² + 1                                           |
//! | 9    | x⁹ + x⁴ + x³ + x² + x + 1                                       |
//! | 10   | x¹⁰ + x⁹ + x⁵ + x⁴ + x + 1                                      |
//! | 11   | x¹¹ + x¹⁰ + x⁹ + x⁵ + x + 1                                     |
//! | 12   | x¹² + x¹¹ + x³ + x² + x + 1                                     |
//! | 13   | x¹³ + x¹² + x¹¹ + x⁸ + x⁷ + x⁴ + x² + 1                        |
//! | 14   | x¹⁴ + x¹³ + x¹⁰ + x⁵ + x³ + x + 1                              |
//! | 15   | x¹⁵ + x¹⁴ + x¹³ + x¹⁰ + x⁸ + x⁵ + x² + x + 1                   |
//! | 16   | x¹⁶ + x¹⁵ + x² + 1                                              |
//! | 24   | x²⁴ + x²³ + x⁶ + x⁵ + x + 1                                     |
//! | 32   | x³² + x²⁶ + x²³ + x²² + x¹⁶ + x¹² + x¹¹ + x¹⁰ + x⁸ + x⁷ + x⁵ + x⁴ + x² + x + 1 |
//!
//! ## Encoding procedure (§1.8.4.5)
//!
//! With these polynomials the CRC encoding proceeds as follows. Let
//! `M(x)` be the information bits (highest order = first bit
//! transmitted) and `k` the number of CRC bits. Compute the remainder
//! `R(x)` satisfying
//!
//! ```text
//! M(x)·xᵏ = Q(x)·G(x) + R(x)
//! ```
//!
//! i.e. `R(x)` is the degree-`(k−1)` remainder of the message shifted
//! left by `k` bits (`M(x)·xᵏ`) divided by `G(x)`, with a zero initial
//! register and no input reflection (MSB-first). The transmitted CRC
//! word is then
//!
//! ```text
//! W(x) = M(x)·xᵏ + R(x)
//! ```
//!
//! with the normative final step: "The CRC bits are written in a
//! reversed manner, i. e. each bit is inverted." So the `k` remainder
//! bits are **bit-inverted** (one's complement) before transmission.
//! [`crc_bits`] returns the post-inversion value — the exact bits a
//! conforming bitstream carries in `crcCheckSum` — so a decoder
//! validates simply by recomputing over the protected region and
//! comparing for equality with the field it read off the wire.
//!
//! ## Scope
//!
//! This module is the §1.8.4.5 generator only. It does **not** apply
//! the §1.8.4.6 SRCPC convolutional FEC stage, nor does it implement
//! the ADTS (`adts_error_check()`) region selection, whose CRC is
//! cited by ISO/IEC 13818-7 to a different normative reference
//! (ISO/IEC 11172-3 §2.4.3.1) and is therefore not covered here.

/// A CRC generation polynomial from ISO/IEC 14496-3 §1.8.4.5.
///
/// Each variant fixes both the bit width `k` and the generator
/// polynomial `G(x)`. The polynomial is stored as the low `k` bits of
/// the generator (the implicit `xᵏ` leading term is dropped, as is
/// conventional for a shift-register CRC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrcPoly {
    /// 4-bit CRC: x⁴ + x³ + x² + 1.
    Crc4,
    /// 5-bit CRC: x⁵ + x⁴ + x² + 1.
    Crc5,
    /// 6-bit CRC: x⁶ + x⁵ + x⁴ + x² + x + 1.
    Crc6,
    /// 7-bit CRC: x⁷ + x³ + x + 1.
    Crc7,
    /// 8-bit CRC: x⁸ + x⁴ + x³ + x² + 1. Used by LATM
    /// `StreamMuxConfig()` `crcCheckSum`.
    Crc8,
    /// 9-bit CRC: x⁹ + x⁴ + x³ + x² + x + 1.
    Crc9,
    /// 10-bit CRC: x¹⁰ + x⁹ + x⁵ + x⁴ + x + 1.
    Crc10,
    /// 11-bit CRC: x¹¹ + x¹⁰ + x⁹ + x⁵ + x + 1.
    Crc11,
    /// 12-bit CRC: x¹² + x¹¹ + x³ + x² + x + 1.
    Crc12,
    /// 13-bit CRC: x¹³ + x¹² + x¹¹ + x⁸ + x⁷ + x⁴ + x² + 1.
    Crc13,
    /// 14-bit CRC: x¹⁴ + x¹³ + x¹⁰ + x⁵ + x³ + x + 1.
    Crc14,
    /// 15-bit CRC: x¹⁵ + x¹⁴ + x¹³ + x¹⁰ + x⁸ + x⁵ + x² + x + 1.
    Crc15,
    /// 16-bit CRC: x¹⁶ + x¹⁵ + x² + 1.
    Crc16,
    /// 24-bit CRC: x²⁴ + x²³ + x⁶ + x⁵ + x + 1.
    Crc24,
    /// 32-bit CRC: x³² + x²⁶ + x²³ + x²² + x¹⁶ + x¹² + x¹¹ + x¹⁰ +
    /// x⁸ + x⁷ + x⁵ + x⁴ + x² + x + 1.
    Crc32,
}

impl CrcPoly {
    /// The CRC width `k` in bits.
    pub const fn width(self) -> u32 {
        match self {
            CrcPoly::Crc4 => 4,
            CrcPoly::Crc5 => 5,
            CrcPoly::Crc6 => 6,
            CrcPoly::Crc7 => 7,
            CrcPoly::Crc8 => 8,
            CrcPoly::Crc9 => 9,
            CrcPoly::Crc10 => 10,
            CrcPoly::Crc11 => 11,
            CrcPoly::Crc12 => 12,
            CrcPoly::Crc13 => 13,
            CrcPoly::Crc14 => 14,
            CrcPoly::Crc15 => 15,
            CrcPoly::Crc16 => 16,
            CrcPoly::Crc24 => 24,
            CrcPoly::Crc32 => 32,
        }
    }

    /// The generator polynomial `G(x)` as the low `k` bits (the
    /// implicit leading `xᵏ` term is not stored). Bit `i` is set iff
    /// the term `xⁱ` is present in `G(x)`.
    ///
    /// Derived directly from the §1.8.4.5 polynomial listing — e.g.
    /// `Crc8` (x⁸ + x⁴ + x³ + x² + 1) drops the `x⁸` and keeps
    /// `x⁴ + x³ + x² + x⁰`, i.e. bits 4, 3, 2, 0 ⇒ `0b0001_1101`.
    pub const fn generator(self) -> u64 {
        match self {
            // x⁴+x³+x²+1            → bits 3,2,0
            CrcPoly::Crc4 => bits(&[3, 2, 0]),
            // x⁵+x⁴+x²+1            → bits 4,2,0
            CrcPoly::Crc5 => bits(&[4, 2, 0]),
            // x⁶+x⁵+x⁴+x²+x+1       → bits 5,4,2,1,0
            CrcPoly::Crc6 => bits(&[5, 4, 2, 1, 0]),
            // x⁷+x³+x+1             → bits 3,1,0
            CrcPoly::Crc7 => bits(&[3, 1, 0]),
            // x⁸+x⁴+x³+x²+1         → bits 4,3,2,0
            CrcPoly::Crc8 => bits(&[4, 3, 2, 0]),
            // x⁹+x⁴+x³+x²+x+1       → bits 4,3,2,1,0
            CrcPoly::Crc9 => bits(&[4, 3, 2, 1, 0]),
            // x¹⁰+x⁹+x⁵+x⁴+x+1      → bits 9,5,4,1,0
            CrcPoly::Crc10 => bits(&[9, 5, 4, 1, 0]),
            // x¹¹+x¹⁰+x⁹+x⁵+x+1     → bits 10,9,5,1,0
            CrcPoly::Crc11 => bits(&[10, 9, 5, 1, 0]),
            // x¹²+x¹¹+x³+x²+x+1     → bits 11,3,2,1,0
            CrcPoly::Crc12 => bits(&[11, 3, 2, 1, 0]),
            // x¹³+x¹²+x¹¹+x⁸+x⁷+x⁴+x²+1 → bits 12,11,8,7,4,2,0
            CrcPoly::Crc13 => bits(&[12, 11, 8, 7, 4, 2, 0]),
            // x¹⁴+x¹³+x¹⁰+x⁵+x³+x+1 → bits 13,10,5,3,1,0
            CrcPoly::Crc14 => bits(&[13, 10, 5, 3, 1, 0]),
            // x¹⁵+x¹⁴+x¹³+x¹⁰+x⁸+x⁵+x²+x+1 → bits 14,13,10,8,5,2,1,0
            CrcPoly::Crc15 => bits(&[14, 13, 10, 8, 5, 2, 1, 0]),
            // x¹⁶+x¹⁵+x²+1          → bits 15,2,0
            CrcPoly::Crc16 => bits(&[15, 2, 0]),
            // x²⁴+x²³+x⁶+x⁵+x+1     → bits 23,6,5,1,0
            CrcPoly::Crc24 => bits(&[23, 6, 5, 1, 0]),
            // x³²+x²⁶+x²³+x²²+x¹⁶+x¹²+x¹¹+x¹⁰+x⁸+x⁷+x⁵+x⁴+x²+x+1
            // → bits 26,23,22,16,12,11,10,8,7,5,4,2,1,0
            CrcPoly::Crc32 => bits(&[26, 23, 22, 16, 12, 11, 10, 8, 7, 5, 4, 2, 1, 0]),
        }
    }

    /// Mask of the low `k` bits: `(1 << k) - 1`.
    const fn mask(self) -> u64 {
        let k = self.width();
        if k >= 64 {
            u64::MAX
        } else {
            (1u64 << k) - 1
        }
    }
}

/// Build a generator bitmask from a list of present term exponents
/// (each `< k`). Used by [`CrcPoly::generator`].
const fn bits(exponents: &[u32]) -> u64 {
    let mut acc = 0u64;
    let mut i = 0;
    while i < exponents.len() {
        acc |= 1u64 << exponents[i];
        i += 1;
    }
    acc
}

/// Compute the §1.8.4.5 CRC over `message_bits`, MSB-first.
///
/// `message_bits` is the protected bit sequence `M(x)` in transmission
/// order: `message_bits[0]` is the highest-order coefficient (the
/// first bit transmitted). The returned value is the `k`-bit
/// `crcCheckSum` exactly as it appears on the wire — the degree-`(k−1)`
/// remainder of `M(x)·xᵏ ÷ G(x)` with the normative final one's
/// complement applied ("written in a reversed manner, i. e. each bit
/// is inverted"). Only the low `poly.width()` bits are significant.
pub fn crc_bits(poly: CrcPoly, message_bits: &[bool]) -> u64 {
    let k = poly.width();
    let gen = poly.generator();
    let mask = poly.mask();
    let top = 1u64 << (k - 1);

    // Standard MSB-first shift register: zero init, no input
    // reflection. Feeding the message bits and then `k` implicit zero
    // bits (the `·xᵏ` shift) leaves the remainder R(x) in `reg`.
    let mut reg: u64 = 0;
    for &bit in message_bits {
        let high = (reg & top) != 0;
        reg = (reg << 1) & mask;
        if high {
            reg ^= gen;
        }
        if bit {
            reg ^= 1; // fold the incoming message bit into x⁰
        }
    }
    // Flush k zero bits so the register holds M(x)·xᵏ mod G(x).
    for _ in 0..k {
        let high = (reg & top) != 0;
        reg = (reg << 1) & mask;
        if high {
            reg ^= gen;
        }
    }

    // §1.8.4.5: "The CRC bits are written in a reversed manner, i. e.
    // each bit is inverted." One's-complement the k remainder bits.
    (!reg) & mask
}

/// Convenience wrapper: compute the §1.8.4.5 CRC over a whole-byte
/// `message`, MSB-first within each byte.
///
/// Equivalent to [`crc_bits`] fed `message.len() * 8` bits in
/// big-endian bit order.
pub fn crc_bytes(poly: CrcPoly, message: &[u8]) -> u64 {
    let k = poly.width();
    let gen = poly.generator();
    let mask = poly.mask();
    let top = 1u64 << (k - 1);

    let mut reg: u64 = 0;
    for &byte in message {
        for i in (0..8).rev() {
            let bit = (byte >> i) & 1 != 0;
            let high = (reg & top) != 0;
            reg = (reg << 1) & mask;
            if high {
                reg ^= gen;
            }
            if bit {
                reg ^= 1;
            }
        }
    }
    for _ in 0..k {
        let high = (reg & top) != 0;
        reg = (reg << 1) & mask;
        if high {
            reg ^= gen;
        }
    }
    (!reg) & mask
}

/// Compute the LATM `StreamMuxConfig()` `crcCheckSum` (§1.7.3.1,
/// Table 1.42) over the protected bit region.
///
/// Per Table 1.42 the CRC "uses the generation polynomial CRC8, as
/// defined in subclause 1.8.4.5 and covers the entire
/// StreamMuxConfig() up to but excluding the crcCheckPresent bit".
/// `config_bits` must therefore be exactly that prefix of the
/// `StreamMuxConfig()` bitstream (from `audioMuxVersion` through the
/// last bit before `crcCheckPresent`), in transmission (MSB-first)
/// order. The returned 8-bit value is the on-wire `crcCheckSum`; a
/// decoder validates by comparing it for equality against the field
/// it read.
pub fn stream_mux_config_crc(config_bits: &[bool]) -> u8 {
    crc_bits(CrcPoly::Crc8, config_bits) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reference long-division CRC: compute the remainder of
    /// `M(x)·xᵏ ÷ G(x)` over GF(2) directly, with the full `(k+1)`-bit
    /// generator (leading `xᵏ` term included). Independent of the
    /// shift-register implementation in `crc_bits`, so it cross-checks
    /// the register arithmetic against the textbook polynomial-division
    /// definition from §1.8.4.5. Returns the pre-inversion remainder.
    fn reference_remainder(poly: CrcPoly, message_bits: &[bool]) -> u64 {
        let k = poly.width();
        let full_gen = poly.generator() | (1u64 << k); // include xᵏ
                                                       // Build the dividend M(x)·xᵏ as a big sequence of bits.
        let mut dividend: Vec<bool> = message_bits.to_vec();
        dividend.extend(std::iter::repeat(false).take(k as usize));

        // Long division over GF(2), MSB-first, tracking a window of the
        // most recent (k+1) bits implicitly via a running register.
        let mut reg: u64 = 0;
        let topbit = 1u64 << k;
        for &bit in &dividend {
            reg = (reg << 1) | (bit as u64);
            if reg & topbit != 0 {
                reg ^= full_gen;
            }
        }
        reg & ((1u64 << k) - 1)
    }

    fn to_bits(bytes: &[u8]) -> Vec<bool> {
        let mut v = Vec::with_capacity(bytes.len() * 8);
        for &b in bytes {
            for i in (0..8).rev() {
                v.push((b >> i) & 1 != 0);
            }
        }
        v
    }

    #[test]
    fn generator_masks_match_spec_exponents() {
        // Spot-check the headline polynomials against §1.8.4.5.
        assert_eq!(CrcPoly::Crc8.generator(), 0b0001_1101); // x⁴+x³+x²+1
        assert_eq!(CrcPoly::Crc16.generator(), (1 << 15) | (1 << 2) | 1);
        assert_eq!(CrcPoly::Crc4.generator(), 0b1101); // x³+x²+1
                                                       // Every generator must fit within its width and carry the x⁰
                                                       // term (all listed polynomials have a constant 1).
        for p in [
            CrcPoly::Crc4,
            CrcPoly::Crc5,
            CrcPoly::Crc6,
            CrcPoly::Crc7,
            CrcPoly::Crc8,
            CrcPoly::Crc9,
            CrcPoly::Crc10,
            CrcPoly::Crc11,
            CrcPoly::Crc12,
            CrcPoly::Crc13,
            CrcPoly::Crc14,
            CrcPoly::Crc15,
            CrcPoly::Crc16,
            CrcPoly::Crc24,
            CrcPoly::Crc32,
        ] {
            assert!(p.generator() & 1 == 1, "{p:?} missing x⁰ term");
            assert!(p.generator() <= p.mask(), "{p:?} generator exceeds width");
        }
    }

    #[test]
    fn crc_bits_matches_reference_long_division() {
        let messages: [&[u8]; 5] = [
            &[],
            &[0x00],
            &[0xFF],
            &[0x12, 0x34, 0x56, 0x78],
            &[0xDE, 0xAD, 0xBE, 0xEF, 0x01, 0x02, 0x03],
        ];
        for p in [
            CrcPoly::Crc4,
            CrcPoly::Crc8,
            CrcPoly::Crc12,
            CrcPoly::Crc16,
            CrcPoly::Crc24,
            CrcPoly::Crc32,
        ] {
            let mask = p.mask();
            for m in messages {
                let bits = to_bits(m);
                let got = crc_bits(p, &bits);
                let expect = (!reference_remainder(p, &bits)) & mask;
                assert_eq!(got, expect, "poly {p:?} message {m:x?}");
            }
        }
    }

    #[test]
    fn crc_bytes_agrees_with_crc_bits() {
        let m: &[u8] = &[0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0xFF];
        for p in [CrcPoly::Crc8, CrcPoly::Crc16, CrcPoly::Crc32] {
            assert_eq!(crc_bytes(p, m), crc_bits(p, &to_bits(m)));
        }
    }

    #[test]
    fn inversion_is_present() {
        // The spec mandates the output bits be inverted. For an empty
        // message the pre-inversion remainder is 0, so the on-wire CRC
        // must be all-ones within the width.
        for p in [CrcPoly::Crc4, CrcPoly::Crc8, CrcPoly::Crc16] {
            assert_eq!(crc_bits(p, &[]), p.mask());
        }
    }

    #[test]
    fn appending_crc_makes_codeword_divisible_modulo_inversion() {
        // A defining property: M(x)·xᵏ + R(x) is divisible by G(x).
        // We store the *inverted* R(x), so re-derive R(x) and verify
        // the codeword M·xᵏ + R divides cleanly.
        let m: &[u8] = &[0x53, 0x91, 0x2C];
        for p in [CrcPoly::Crc8, CrcPoly::Crc16] {
            let mut bits = to_bits(m);
            let on_wire = crc_bits(p, &bits);
            let r = (!on_wire) & p.mask(); // undo inversion → true R(x)
                                           // Append the k remainder bits (MSB-first) to the message.
            for i in (0..p.width()).rev() {
                bits.push((r >> i) & 1 != 0);
            }
            // The remainder of the full codeword ÷ G(x) must be zero.
            assert_eq!(reference_remainder(p, &bits), 0, "poly {p:?}");
        }
    }

    #[test]
    fn stream_mux_config_crc_is_crc8() {
        let bits = to_bits(&[0x00, 0x10, 0x07, 0x00]);
        assert_eq!(
            stream_mux_config_crc(&bits) as u64,
            crc_bits(CrcPoly::Crc8, &bits)
        );
    }
}
