//! Shared bit-level I/O — MSB-first and LSB-first readers and writers.
//!
//! Every `oxideav-*` codec crate used to ship its own bitwriter /
//! bitreader implementation. Those implementations converged on the
//! same two layouts (MSB-first for ~everything, LSB-first for Vorbis)
//! with only cosmetic method-name differences, so the core copy lives
//! here and each codec crate pulls it in via `oxideav_core::bits`.
//!
//! # Bit orders
//!
//! * [`BitReader`] / [`BitWriter`] — **MSB-first**. Within each byte
//!   the high bit is read/written first. This matches AAC, MP1/2/3,
//!   FLAC, Speex, H.263/4, MPEG-1/2/4 video, and just about every
//!   modern codec bitstream.
//! * [`BitReaderLsb`] / [`BitWriterLsb`] — **LSB-first**. Within each
//!   byte the low bit is read/written first. Vorbis I §2.1.4 packs
//!   its bitstream this way.
//!
//! Both variants carry a 64-bit accumulator so callers can request up
//! to 32 bits per call without straddling refill logic. 64-bit values
//! are handled in two halves by `read_u64` / `write_u64`.
//!
//! # Method naming
//!
//! Every writer exposes both of these names for the same operation —
//! historical drift across the codec crates made both common, and
//! keeping both as aliases lets migration stay mechanical:
//!
//! * `write_u32(value, n)` ≡ `write_bits(value, n)`  — append low `n` bits
//! * `finish(self)`        ≡ `into_bytes(self)`      — pad + consume
//!
//! Similarly `skip(n)` and `consume(n)` are aliases on the reader.

use crate::{Error, Result};

// ==================== MSB-first ====================

/// MSB-first bit reader over a borrowed byte slice.
///
/// Copy+Clone: the reader owns no heap state, only a borrowed slice
/// and a handful of counters, so `let saved = br;` followed by a
/// later `br = saved;` is a valid checkpoint/restore — some codec
/// parsers (e.g. MPEG-4 Part 2's VOP resync) use exactly that pattern.
#[derive(Clone, Copy)]
pub struct BitReader<'a> {
    data: &'a [u8],
    /// Index of the next byte to load into the accumulator.
    byte_pos: usize,
    /// Bits buffered from `data`, left-aligned (high bit = next to consume).
    acc: u64,
    /// Valid bits currently in `acc`, in the range `0..=64`.
    bits_in_acc: u32,
}

impl<'a> BitReader<'a> {
    /// Start reading at the beginning of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// Start reading at a specific byte offset (useful for parsers that
    /// need to re-anchor into the middle of a buffer without copying).
    pub fn with_position(data: &'a [u8], byte_pos: usize) -> Self {
        let byte_pos = byte_pos.min(data.len());
        Self {
            data,
            byte_pos,
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// Bits already consumed from the logical stream.
    pub fn bit_position(&self) -> u64 {
        self.byte_pos as u64 * 8 - self.bits_in_acc as u64
    }

    /// Byte offset of the reader (floor of `bit_position / 8`).
    pub fn byte_position(&self) -> usize {
        (self.bit_position() / 8) as usize
    }

    /// Total remaining bits (buffered + unread from the slice).
    pub fn bits_remaining(&self) -> u64 {
        self.bits_in_acc as u64 + ((self.data.len() - self.byte_pos) as u64) * 8
    }

    /// True if the reader is positioned on a byte boundary.
    pub fn is_byte_aligned(&self) -> bool {
        self.bits_in_acc % 8 == 0
    }

    /// Skip remaining bits in the current byte, leaving the reader byte-aligned.
    pub fn align_to_byte(&mut self) {
        let drop = self.bits_in_acc % 8;
        self.acc <<= drop;
        self.bits_in_acc -= drop;
    }

    fn refill(&mut self) {
        while self.bits_in_acc <= 56 && self.byte_pos < self.data.len() {
            self.acc |= (self.data[self.byte_pos] as u64) << (56 - self.bits_in_acc);
            self.bits_in_acc += 8;
            self.byte_pos += 1;
        }
    }

    /// Read `n` bits (0..=32) as an unsigned integer.
    pub fn read_u32(&mut self, n: u32) -> Result<u32> {
        debug_assert!(n <= 32, "BitReader::read_u32 supports up to 32 bits");
        if n == 0 {
            return Ok(0);
        }
        if self.bits_in_acc < n {
            self.refill();
            if self.bits_in_acc < n {
                return Err(Error::invalid("bitreader: out of bits"));
            }
        }
        let v = (self.acc >> (64 - n)) as u32;
        self.acc <<= n;
        self.bits_in_acc -= n;
        Ok(v)
    }

    /// Read `n` bits (0..=64) as an unsigned integer.
    pub fn read_u64(&mut self, n: u32) -> Result<u64> {
        debug_assert!(n <= 64);
        if n <= 32 {
            return self.read_u32(n).map(|v| v as u64);
        }
        let hi = self.read_u32(n - 32)? as u64;
        let lo = self.read_u32(32)? as u64;
        Ok((hi << 32) | lo)
    }

    /// Read `n` bits as a signed integer, sign-extended from the high bit.
    pub fn read_i32(&mut self, n: u32) -> Result<i32> {
        if n == 0 {
            return Ok(0);
        }
        let raw = self.read_u32(n)? as i32;
        let shift = 32 - n;
        Ok((raw << shift) >> shift)
    }

    /// Read a single bit as a bool.
    pub fn read_bit(&mut self) -> Result<bool> {
        Ok(self.read_u32(1)? != 0)
    }

    /// Read a single bit as `0` or `1` (some codec specs phrase flags this way).
    pub fn read_u1(&mut self) -> Result<u32> {
        self.read_u32(1)
    }

    /// Peek `n` bits (0..=32) without consuming them.
    pub fn peek_u32(&mut self, n: u32) -> Result<u32> {
        debug_assert!(n <= 32);
        if n == 0 {
            return Ok(0);
        }
        if self.bits_in_acc < n {
            self.refill();
            if self.bits_in_acc < n {
                return Err(Error::invalid("bitreader: out of bits for peek"));
            }
        }
        Ok((self.acc >> (64 - n)) as u32)
    }

    /// Discard `n` bits.
    pub fn skip(&mut self, n: u32) -> Result<()> {
        let mut left = n;
        while left > 32 {
            self.read_u32(32)?;
            left -= 32;
        }
        self.read_u32(left)?;
        Ok(())
    }

    /// Alias for [`Self::skip`] — some spec wordings prefer "consume".
    pub fn consume(&mut self, n: u32) -> Result<()> {
        self.skip(n)
    }

    /// Read a unary-coded value: the count of leading zero bits, terminated
    /// by a single `1`. Used by FLAC Rice residuals and any other codec
    /// that needs variable-length counts. Uses `leading_zeros()` on the
    /// 64-bit accumulator for the fast path.
    pub fn read_unary(&mut self) -> Result<u32> {
        let mut count = 0u32;
        loop {
            if self.bits_in_acc == 0 {
                self.refill();
                if self.bits_in_acc == 0 {
                    return Err(Error::invalid("bitreader: out of bits in unary code"));
                }
            }
            let lz_total = self.acc.leading_zeros();
            let lz_avail = lz_total.min(self.bits_in_acc);
            count = count
                .checked_add(lz_avail)
                .ok_or_else(|| Error::invalid("bitreader: unary count overflow"))?;
            // Shifting a u64 by 64 is UB in Rust — guard that case. It only
            // arises for very long zero runs.
            if lz_avail >= 64 {
                self.acc = 0;
            } else {
                self.acc <<= lz_avail;
            }
            self.bits_in_acc -= lz_avail;
            if lz_avail < lz_total || self.bits_in_acc == 0 {
                continue;
            }
            // Consume the terminating 1 bit.
            self.acc <<= 1;
            self.bits_in_acc -= 1;
            return Ok(count);
        }
    }

    /// Read `n` bytes. Requires the reader to be byte-aligned.
    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        if !self.is_byte_aligned() {
            return Err(Error::invalid(
                "bitreader: read_bytes requires byte alignment",
            ));
        }
        self.align_to_byte();
        let start = self.byte_pos - (self.bits_in_acc as usize / 8);
        // `bits_in_acc` is a multiple of 8 here (because we're aligned); each
        // full byte in the accumulator is one unconsumed input byte whose
        // `byte_pos` has already been advanced — so the actual logical cursor
        // is `byte_pos - bits_in_acc / 8`.
        if start + n > self.data.len() {
            return Err(Error::invalid("bitreader: read_bytes past end"));
        }
        let out = self.data[start..start + n].to_vec();
        // Advance: empty the accumulator and re-anchor `byte_pos` past the
        // copied region.
        self.acc = 0;
        self.bits_in_acc = 0;
        self.byte_pos = start + n;
        Ok(out)
    }
}

/// MSB-first bit writer over an internal byte buffer.
pub struct BitWriter {
    data: Vec<u8>,
    /// Bits buffered at the *high* end of `acc` (next-to-emit at top).
    acc: u64,
    /// Valid bits currently in `acc` (0..=64).
    bits_in_acc: u32,
}

impl BitWriter {
    /// An empty writer.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// An empty writer whose output buffer is pre-allocated for `cap` bytes.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            data: Vec::with_capacity(cap),
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// Total bits written so far (including any in the unflushed accumulator).
    pub fn bit_position(&self) -> u64 {
        self.data.len() as u64 * 8 + self.bits_in_acc as u64
    }

    /// Bytes of output produced so far (excluding any unflushed partial byte).
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }

    /// True if the writer is currently on a byte boundary.
    pub fn is_byte_aligned(&self) -> bool {
        self.bits_in_acc % 8 == 0
    }

    /// Append `n` bits (0..=32) from the low `n` bits of `value`, MSB first.
    pub fn write_u32(&mut self, value: u32, n: u32) {
        debug_assert!(n <= 32, "BitWriter::write_u32 supports up to 32 bits");
        if n == 0 {
            return;
        }
        let mask: u32 = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        let v = (value & mask) as u64;
        let shift = 64 - self.bits_in_acc - n;
        self.acc |= v << shift;
        self.bits_in_acc += n;
        while self.bits_in_acc >= 8 {
            let byte = (self.acc >> 56) as u8;
            self.data.push(byte);
            self.acc <<= 8;
            self.bits_in_acc -= 8;
        }
    }

    /// Alias of [`Self::write_u32`] — some codec crates historically
    /// spell this `write_bits`.
    pub fn write_bits(&mut self, value: u32, n: u32) {
        self.write_u32(value, n)
    }

    /// Append up to 64 bits.
    pub fn write_u64(&mut self, value: u64, n: u32) {
        debug_assert!(n <= 64);
        if n <= 32 {
            self.write_u32(value as u32, n);
        } else {
            self.write_u32((value >> 32) as u32, n - 32);
            self.write_u32(value as u32, 32);
        }
    }

    /// Append `n` bits interpreted as a signed integer. Only the low
    /// `n` bits of the 2's-complement representation are written.
    pub fn write_i32(&mut self, value: i32, n: u32) {
        self.write_u32(value as u32, n);
    }

    /// Append a single bit.
    pub fn write_bit(&mut self, bit: bool) {
        self.write_u32(bit as u32, 1);
    }

    /// Emit a unary-coded value: `n` zero bits followed by a single `1`.
    /// Inverse of [`BitReader::read_unary`].
    pub fn write_unary(&mut self, n: u32) {
        let mut remaining = n;
        while remaining >= 32 {
            self.write_u32(0, 32);
            remaining -= 32;
        }
        if remaining > 0 {
            self.write_u32(0, remaining);
        }
        self.write_bit(true);
    }

    /// Append one whole byte (8 bits).
    pub fn write_byte(&mut self, b: u8) {
        self.write_u32(b as u32, 8);
    }

    /// Append a slice of bytes. Fast path when byte-aligned.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        if self.is_byte_aligned() {
            // Flush the accumulator (which is already at 0 bits_in_acc).
            self.data.extend_from_slice(bytes);
        } else {
            for &b in bytes {
                self.write_u32(b as u32, 8);
            }
        }
    }

    /// Pad to the next byte boundary with zero bits.
    pub fn align_to_byte(&mut self) {
        let pad = (8 - self.bits_in_acc % 8) % 8;
        if pad > 0 {
            self.write_u32(0, pad);
        }
    }

    /// Alias of [`Self::align_to_byte`] — MPEG-4 video spells it
    /// `align_to_byte_zero` since the spec also defines alternate
    /// align-with-ones tails in other contexts.
    pub fn align_to_byte_zero(&mut self) {
        self.align_to_byte()
    }

    /// Borrow the bytes accumulated so far (excluding any unflushed partial byte).
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Alias of [`Self::bytes`] — some codec crates spell this `buffer`.
    pub fn buffer(&self) -> &[u8] {
        &self.data
    }

    /// Pad with zero bits to the next byte boundary, then return the bytes.
    pub fn finish(mut self) -> Vec<u8> {
        if self.bits_in_acc > 0 {
            let byte = (self.acc >> 56) as u8;
            self.data.push(byte);
            self.acc = 0;
            self.bits_in_acc = 0;
        }
        self.data
    }

    /// Alias of [`Self::finish`] — some codec crates spell this `into_bytes`.
    pub fn into_bytes(self) -> Vec<u8> {
        self.finish()
    }
}

impl Default for BitWriter {
    fn default() -> Self {
        Self::new()
    }
}

// ==================== LSB-first (Vorbis) ====================

/// LSB-first bit reader. See [module docs](self) for the LSB convention.
///
/// Copy+Clone for the same reason as [`BitReader`] — no heap state.
#[derive(Clone, Copy)]
pub struct BitReaderLsb<'a> {
    data: &'a [u8],
    byte_pos: usize,
    /// Buffered bits, low-aligned (next bit to emit is bit 0 of `acc`).
    acc: u64,
    bits_in_acc: u32,
}

impl<'a> BitReaderLsb<'a> {
    /// Start reading at the beginning of `data`.
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// Start reading at a specific byte offset (useful for parsers that
    /// need to re-anchor into the middle of a buffer without copying).
    /// Mirrors [`BitReader::with_position`].
    pub fn with_position(data: &'a [u8], byte_pos: usize) -> Self {
        let byte_pos = byte_pos.min(data.len());
        Self {
            data,
            byte_pos,
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// Bits already consumed from the logical stream.
    pub fn bit_position(&self) -> u64 {
        self.byte_pos as u64 * 8 - self.bits_in_acc as u64
    }

    /// Byte offset of the reader (floor of `bit_position / 8`).
    pub fn byte_position(&self) -> usize {
        (self.bit_position() / 8) as usize
    }

    /// Total remaining bits (buffered + unread from the slice).
    pub fn bits_remaining(&self) -> u64 {
        self.bits_in_acc as u64 + ((self.data.len() - self.byte_pos) as u64) * 8
    }

    /// True if the reader is positioned on a byte boundary.
    pub fn is_byte_aligned(&self) -> bool {
        self.bits_in_acc % 8 == 0
    }

    /// Skip remaining bits in the current byte, leaving the reader
    /// byte-aligned. In the LSB layout the *low* bits of the partial
    /// byte are the already-consumed ones, so this drops the buffered
    /// low bits.
    pub fn align_to_byte(&mut self) {
        let drop = self.bits_in_acc % 8;
        self.acc >>= drop;
        self.bits_in_acc -= drop;
    }

    fn refill(&mut self) {
        while self.bits_in_acc <= 56 && self.byte_pos < self.data.len() {
            self.acc |= (self.data[self.byte_pos] as u64) << self.bits_in_acc;
            self.bits_in_acc += 8;
            self.byte_pos += 1;
        }
    }

    /// Read `n` bits (0..=32) as an unsigned integer.
    pub fn read_u32(&mut self, n: u32) -> Result<u32> {
        debug_assert!(n <= 32, "BitReaderLsb::read_u32 supports up to 32 bits");
        if n == 0 {
            return Ok(0);
        }
        if self.bits_in_acc < n {
            self.refill();
            if self.bits_in_acc < n {
                return Err(Error::Eof);
            }
        }
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        let v = (self.acc as u32) & mask;
        self.acc >>= n;
        self.bits_in_acc -= n;
        Ok(v)
    }

    /// Read `n` bits (0..=64) as an unsigned integer (low half first).
    pub fn read_u64(&mut self, n: u32) -> Result<u64> {
        debug_assert!(n <= 64);
        if n == 0 {
            return Ok(0);
        }
        if n <= 32 {
            return Ok(self.read_u32(n)? as u64);
        }
        let lo = self.read_u32(32)? as u64;
        let hi = self.read_u32(n - 32)? as u64;
        Ok(lo | (hi << 32))
    }

    /// Read `n` bits as a signed integer, sign-extended from the high bit.
    pub fn read_i32(&mut self, n: u32) -> Result<i32> {
        if n == 0 {
            return Ok(0);
        }
        let raw = self.read_u32(n)? as i32;
        let shift = 32 - n;
        Ok((raw << shift) >> shift)
    }

    /// Read a single bit as a bool.
    pub fn read_bit(&mut self) -> Result<bool> {
        Ok(self.read_u32(1)? != 0)
    }

    /// Read a single bit as `0` or `1` (some codec specs phrase flags
    /// this way). Mirrors [`BitReader::read_u1`].
    pub fn read_u1(&mut self) -> Result<u32> {
        self.read_u32(1)
    }

    /// Peek `n` bits (0..=32) without consuming them. Mirrors
    /// [`BitReader::peek_u32`] — the workhorse of table-driven Huffman
    /// decoders (peek a fixed window, look up, then consume the code
    /// length).
    pub fn peek_u32(&mut self, n: u32) -> Result<u32> {
        debug_assert!(n <= 32);
        if n == 0 {
            return Ok(0);
        }
        if self.bits_in_acc < n {
            self.refill();
            if self.bits_in_acc < n {
                return Err(Error::Eof);
            }
        }
        let mask = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        Ok((self.acc as u32) & mask)
    }

    /// Discard `n` bits.
    pub fn skip(&mut self, n: u32) -> Result<()> {
        let mut left = n;
        while left > 32 {
            self.read_u32(32)?;
            left -= 32;
        }
        self.read_u32(left)?;
        Ok(())
    }

    /// Alias for [`Self::skip`] — some spec wordings prefer "consume".
    pub fn consume(&mut self, n: u32) -> Result<()> {
        self.skip(n)
    }

    /// Read `n` bytes. Requires the reader to be byte-aligned. Mirrors
    /// [`BitReader::read_bytes`].
    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>> {
        if !self.is_byte_aligned() {
            return Err(Error::invalid(
                "bitreader: read_bytes requires byte alignment",
            ));
        }
        // Aligned, so `bits_in_acc` is a multiple of 8; each full byte
        // in the accumulator is one unconsumed input byte whose
        // `byte_pos` has already been advanced.
        let start = self.byte_pos - (self.bits_in_acc as usize / 8);
        if start + n > self.data.len() {
            return Err(Error::Eof);
        }
        let out = self.data[start..start + n].to_vec();
        self.acc = 0;
        self.bits_in_acc = 0;
        self.byte_pos = start + n;
        Ok(out)
    }
}

/// LSB-first bit writer — inverse of [`BitReaderLsb`].
pub struct BitWriterLsb {
    data: Vec<u8>,
    /// Bits held over from the last partial byte, low-aligned.
    acc: u64,
    bits_in_acc: u32,
}

impl BitWriterLsb {
    /// An empty writer.
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// An empty writer whose output buffer is pre-allocated for `cap` bytes.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            data: Vec::with_capacity(cap),
            acc: 0,
            bits_in_acc: 0,
        }
    }

    /// Total bits written so far (including any in the unflushed accumulator).
    pub fn bit_position(&self) -> u64 {
        self.data.len() as u64 * 8 + self.bits_in_acc as u64
    }

    /// Bytes of output produced so far (excluding any unflushed partial
    /// byte).
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }

    /// True if the writer is currently on a byte boundary.
    pub fn is_byte_aligned(&self) -> bool {
        self.bits_in_acc % 8 == 0
    }

    /// Append `n` bits (0..=32) from the low `n` bits of `value`, LSB first.
    pub fn write_u32(&mut self, value: u32, n: u32) {
        debug_assert!(n <= 32, "BitWriterLsb::write_u32 supports up to 32 bits");
        if n == 0 {
            return;
        }
        let mask: u32 = if n == 32 { u32::MAX } else { (1u32 << n) - 1 };
        let v = value & mask;
        self.acc |= (v as u64) << self.bits_in_acc;
        self.bits_in_acc += n;
        while self.bits_in_acc >= 8 {
            self.data.push((self.acc & 0xFF) as u8);
            self.acc >>= 8;
            self.bits_in_acc -= 8;
        }
    }

    /// Append up to 64 bits (low half first).
    pub fn write_u64(&mut self, value: u64, n: u32) {
        debug_assert!(n <= 64);
        if n <= 32 {
            self.write_u32(value as u32, n);
        } else {
            self.write_u32(value as u32, 32);
            self.write_u32((value >> 32) as u32, n - 32);
        }
    }

    /// Alias of [`Self::write_u32`] — mirrors [`BitWriter::write_bits`].
    pub fn write_bits(&mut self, value: u32, n: u32) {
        self.write_u32(value, n)
    }

    /// Append `n` bits interpreted as a signed integer. Only the low
    /// `n` bits of the 2's-complement representation are written.
    pub fn write_i32(&mut self, value: i32, n: u32) {
        self.write_u32(value as u32, n);
    }

    /// Append a single bit.
    pub fn write_bit(&mut self, bit: bool) {
        self.write_u32(bit as u32, 1);
    }

    /// Append one whole byte (8 bits).
    pub fn write_byte(&mut self, b: u8) {
        self.write_u32(b as u32, 8);
    }

    /// Append a slice of bytes. Fast path when byte-aligned.
    pub fn write_bytes(&mut self, bytes: &[u8]) {
        if self.is_byte_aligned() {
            self.data.extend_from_slice(bytes);
        } else {
            for &b in bytes {
                self.write_u32(b as u32, 8);
            }
        }
    }

    /// Pad to the next byte boundary with zero bits.
    pub fn align_to_byte(&mut self) {
        let pad = (8 - self.bits_in_acc % 8) % 8;
        self.write_u32(0, pad);
    }

    /// Borrow the bytes accumulated so far (excluding any unflushed
    /// partial byte).
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }

    /// Alias of [`Self::bytes`] — mirrors [`BitWriter::buffer`].
    pub fn buffer(&self) -> &[u8] {
        &self.data
    }

    /// Pad with zero bits to the next byte boundary, then return the bytes.
    pub fn finish(mut self) -> Vec<u8> {
        if self.bits_in_acc > 0 {
            self.data.push((self.acc & 0xFF) as u8);
            self.acc = 0;
            self.bits_in_acc = 0;
        }
        self.data
    }

    /// Alias of [`Self::finish`] — mirrors [`BitWriter::into_bytes`].
    pub fn into_bytes(self) -> Vec<u8> {
        self.finish()
    }
}

impl Default for BitWriterLsb {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- MSB ----

    #[test]
    fn msb_roundtrip_byte() {
        let mut w = BitWriter::new();
        for &b in &[1u32, 0, 1, 0, 0, 1, 0, 1] {
            w.write_u32(b, 1);
        }
        assert_eq!(w.finish(), vec![0xA5]);
    }

    #[test]
    fn msb_roundtrip_varied_widths() {
        let mut bw = BitWriter::new();
        let writes: Vec<(u32, u32)> = vec![
            (0b1, 1),
            (0b10101, 5),
            (0b111100001111, 12),
            (0xDEADBEEF, 32),
            (0b001, 3),
            (0xC, 4),
            (0xABCD, 16),
            (0x12345, 20),
            (0, 8),
            (0xFFFFFFFF, 32),
        ];
        for &(v, n) in &writes {
            bw.write_u32(v, n);
        }
        let bytes = bw.finish();
        let mut br = BitReader::new(&bytes);
        for &(v, n) in &writes {
            let got = br.read_u32(n).unwrap();
            let mask = if n == 32 { u32::MAX } else { (1 << n) - 1 };
            assert_eq!(got, v & mask, "mismatch for ({v:#x}, {n})");
        }
    }

    #[test]
    fn msb_signed_extension() {
        let mut br = BitReader::new(&[0xFF]);
        assert_eq!(br.read_i32(4).unwrap(), -1);
        assert_eq!(br.read_i32(4).unwrap(), -1);
    }

    #[test]
    fn msb_peek_skip() {
        let mut br = BitReader::new(&[0xFF, 0x00]);
        assert_eq!(br.peek_u32(12).unwrap(), 0xFF0);
        br.skip(4).unwrap();
        assert_eq!(br.read_u32(8).unwrap(), 0xF0);
    }

    #[test]
    fn msb_alignment() {
        let mut br = BitReader::new(&[0xFF, 0x55]);
        br.read_u32(3).unwrap();
        assert!(!br.is_byte_aligned());
        br.align_to_byte();
        assert!(br.is_byte_aligned());
        assert_eq!(br.read_u32(8).unwrap(), 0x55);
    }

    #[test]
    fn msb_write_bytes_fast_path() {
        let mut w = BitWriter::new();
        w.write_bytes(&[0x11, 0x22, 0x33]);
        assert_eq!(w.finish(), vec![0x11, 0x22, 0x33]);
    }

    #[test]
    fn msb_write_bytes_unaligned() {
        let mut w = BitWriter::new();
        w.write_u32(0b101, 3);
        w.write_bytes(&[0xFF, 0x00]);
        // 3 bits + 2*8 = 19 bits → 3 bytes after zero-pad.
        let out = w.finish();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn msb_write_bits_alias() {
        let mut w = BitWriter::new();
        w.write_bits(0xA, 4);
        w.write_u32(0x5, 4);
        assert_eq!(w.finish(), vec![0xA5]);
    }

    #[test]
    fn msb_into_bytes_alias() {
        let mut w = BitWriter::new();
        w.write_u32(0xA5, 8);
        assert_eq!(w.into_bytes(), vec![0xA5]);
    }

    #[test]
    fn msb_read_u64_high_bits() {
        // Write 0x1234567890ABCDEF as 64 bits MSB-first, read back.
        let mut w = BitWriter::new();
        w.write_u64(0x1234567890ABCDEF, 64);
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        assert_eq!(r.read_u64(64).unwrap(), 0x1234567890ABCDEF);
    }

    #[test]
    fn msb_unary_roundtrip() {
        let mut w = BitWriter::new();
        let counts: Vec<u32> = vec![0, 1, 7, 31, 32, 33, 64, 65, 100];
        for &c in &counts {
            w.write_unary(c);
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        for &c in &counts {
            assert_eq!(r.read_unary().unwrap(), c, "unary roundtrip for {c}");
        }
    }

    #[test]
    fn msb_read_bytes_aligned() {
        let mut br = BitReader::new(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let _ = br.read_u32(8).unwrap();
        let got = br.read_bytes(2).unwrap();
        assert_eq!(got, vec![0xBB, 0xCC]);
        assert_eq!(br.read_u32(8).unwrap(), 0xDD);
    }

    // ---- LSB ----

    #[test]
    fn lsb_roundtrip_byte() {
        let mut w = BitWriterLsb::new();
        for &b in &[1u32, 0, 1, 0, 0, 1, 0, 1] {
            w.write_u32(b, 1);
        }
        assert_eq!(w.finish(), vec![0xA5]);
    }

    #[test]
    fn lsb_multi_byte() {
        let mut w = BitWriterLsb::new();
        w.write_u32(0x3412, 16);
        let bytes = w.finish();
        assert_eq!(bytes, vec![0x12, 0x34]);
        let mut r = BitReaderLsb::new(&bytes);
        assert_eq!(r.read_u32(16).unwrap(), 0x3412);
    }

    #[test]
    fn lsb_roundtrip_varied_widths() {
        let mut bw = BitWriterLsb::new();
        let writes: Vec<(u32, u32)> = vec![(5, 3), (0xABCD, 16), (0x1234567, 27), (1, 1)];
        for &(v, n) in &writes {
            bw.write_u32(v, n);
        }
        let bytes = bw.finish();
        let mut r = BitReaderLsb::new(&bytes);
        for &(v, n) in &writes {
            assert_eq!(r.read_u32(n).unwrap(), v);
        }
    }

    #[test]
    fn lsb_peek_skip_consume() {
        let mut r = BitReaderLsb::new(&[0xA5, 0x5A]);
        // Low nibble of 0xA5 first in LSB order.
        assert_eq!(r.peek_u32(4).unwrap(), 0x5);
        // Peek does not consume.
        assert_eq!(r.peek_u32(8).unwrap(), 0xA5);
        r.skip(4).unwrap();
        assert_eq!(r.read_u32(4).unwrap(), 0xA);
        r.consume(4).unwrap();
        assert_eq!(r.read_u32(4).unwrap(), 0x5);
        // Past-end peek reports Eof.
        assert!(r.peek_u32(8).is_err());
    }

    #[test]
    fn lsb_alignment_and_positions() {
        let mut r = BitReaderLsb::new(&[0xFF, 0x55, 0x33]);
        assert_eq!(r.bits_remaining(), 24);
        r.read_u32(3).unwrap();
        assert!(!r.is_byte_aligned());
        assert_eq!(r.bit_position(), 3);
        assert_eq!(r.byte_position(), 0);
        r.align_to_byte();
        assert!(r.is_byte_aligned());
        assert_eq!(r.bit_position(), 8);
        assert_eq!(r.byte_position(), 1);
        assert_eq!(r.bits_remaining(), 16);
        assert_eq!(r.read_u32(8).unwrap(), 0x55);
        // with_position anchors mid-buffer (and clamps past-end).
        let mut r2 = BitReaderLsb::with_position(&[0xFF, 0x55, 0x33], 2);
        assert_eq!(r2.read_u32(8).unwrap(), 0x33);
        let r3 = BitReaderLsb::with_position(&[0xFF], 9);
        assert_eq!(r3.bits_remaining(), 0);
    }

    #[test]
    fn lsb_read_bytes_aligned() {
        let mut r = BitReaderLsb::new(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let _ = r.read_u32(8).unwrap();
        let got = r.read_bytes(2).unwrap();
        assert_eq!(got, vec![0xBB, 0xCC]);
        assert_eq!(r.read_u32(8).unwrap(), 0xDD);
        // Unaligned read_bytes is a usage error.
        let mut r = BitReaderLsb::new(&[0xAA, 0xBB]);
        r.read_u32(3).unwrap();
        assert!(r.read_bytes(1).is_err());
        // Past-end read_bytes reports Eof.
        let mut r = BitReaderLsb::new(&[0xAA]);
        assert!(r.read_bytes(2).is_err());
    }

    #[test]
    fn lsb_read_u1() {
        // 0xA5 LSB-first = 1,0,1,0,0,1,0,1.
        let mut r = BitReaderLsb::new(&[0xA5]);
        let bits: Vec<u32> = (0..8).map(|_| r.read_u1().unwrap()).collect();
        assert_eq!(bits, vec![1, 0, 1, 0, 0, 1, 0, 1]);
    }

    #[test]
    fn lsb_writer_bytes_and_aliases() {
        let mut w = BitWriterLsb::new();
        assert!(w.is_byte_aligned());
        w.write_bits(0x5, 4);
        assert!(!w.is_byte_aligned());
        assert_eq!(w.byte_len(), 0);
        w.write_u32(0xA, 4);
        assert_eq!(w.byte_len(), 1);
        assert_eq!(w.bytes(), &[0xA5]);
        assert_eq!(w.buffer(), &[0xA5]);
        w.write_byte(0x7E);
        w.write_bytes(&[0x11, 0x22]);
        assert_eq!(w.into_bytes(), vec![0xA5, 0x7E, 0x11, 0x22]);
    }

    #[test]
    fn lsb_writer_unaligned_write_bytes() {
        let mut w = BitWriterLsb::new();
        w.write_u32(0b101, 3);
        w.write_bytes(&[0xFF, 0x00]);
        let out = w.finish();
        assert_eq!(out.len(), 3);
        // Read back: 3 bits then the two bytes.
        let mut r = BitReaderLsb::new(&out);
        assert_eq!(r.read_u32(3).unwrap(), 0b101);
        assert_eq!(r.read_u32(8).unwrap(), 0xFF);
        assert_eq!(r.read_u32(8).unwrap(), 0x00);
    }

    #[test]
    fn lsb_writer_signed() {
        let mut w = BitWriterLsb::new();
        w.write_i32(-1, 4);
        w.write_i32(3, 4);
        let bytes = w.finish();
        let mut r = BitReaderLsb::new(&bytes);
        assert_eq!(r.read_i32(4).unwrap(), -1);
        assert_eq!(r.read_i32(4).unwrap(), 3);
    }
}
