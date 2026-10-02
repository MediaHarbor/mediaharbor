//! `pulse_data()` parser + encoder primitive — ISO/IEC 14496-3
//! §4.4.6.3 / Table 4.7.
//!
//! `pulse_data()` is the optional "pulse escape" tool inside
//! `individual_channel_stream()`: when the encoder finds it cheaper
//! to replace a small number (1..=4) of quantised spectral
//! coefficients with smaller ones plus a fix-up record than to spend
//! the bits on the literal escape codeword, it writes a `pulse_data()`
//! block that the decoder uses to restore the original amplitudes
//! after Huffman decoding. The pulse escape is dispatched by the
//! one-bit `pulse_data_present` flag immediately after
//! `scale_factor_data()` (Table 4.44 / Table 4.50).
//!
//! ## Wire layout (Table 4.7)
//!
//! ```text
//! pulse_data() {
//!     number_pulse;                                   2 bits
//!     pulse_start_sfb;                                6 bits
//!     for (i = 0; i < number_pulse + 1; i++) {
//!         pulse_offset[i];                            5 bits
//!         pulse_amp[i];                               4 bits
//!     }
//! }
//! ```
//!
//! Every field is fixed-width. The actual pulse count on the wire
//! is `number_pulse + 1` (so 1..=4 pulses, never zero), encoded in
//! 2 bits as `0..=3`.
//!
//! ## What this module covers
//!
//! * [`PulseData::parse`] — read a Table 4.7 block from a
//!   [`BitReader`], surfacing the raw wire fields without applying
//!   the §4.6.13 reconstruction (the spectral fix-up itself needs
//!   `swb_offset_long_window[]` + the post-Huffman `x_quant` array,
//!   neither of which exists in Phase 2 yet).
//! * [`PulseData::write`] — the inverse: serialise a [`PulseData`]
//!   onto a [`BitWriter`] in bit-exact Table 4.7 form. Surfaces
//!   field-overflow as [`Error::PulseDataEncodeInvalid`].
//!
//! ## What this module does *not* cover
//!
//! * The §4.6.13 reconstruction loop (`k +=
//!   swb_offset[pulse_start_sfb]; k += pulse_offset[j]; x_quant[…] ±=
//!   pulse_amp[j]`) is deferred until `swb_offset` tables land with
//!   `spectral_data()`.
//! * The normative constraint that `pulse_data_present` *must* be 0
//!   when `window_sequence == EIGHT_SHORT_SEQUENCE` (§4.4.6.3 last
//!   paragraph) is the responsibility of the dispatching
//!   `individual_channel_stream()` (which has not landed yet); the
//!   parser and writer here intentionally surface the literal Table 4.7
//!   bytes regardless of the surrounding window sequence so that
//!   future round work has access to the raw decoded record.
//! * No validation against `swb_offset_long_window[fs_index]` — the
//!   parser cannot tell whether `pulse_start_sfb` is in-range for a
//!   given sample rate without the offset table; the encoder cannot
//!   tell whether a pulse position lands inside the represented
//!   coefficient grid. These are §4.6.13 reconstruction concerns,
//!   not Table 4.7 wire-format concerns.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::{Error, Result};

/// Per-pulse `(offset, amp)` record. Both fields are unsigned.
///
/// * `offset` — 5 bits. `pulse_offset[i]` per Table 4.7. Read by
///   the decoder as a delta added to the running coefficient index
///   `k` (initialised to `swb_offset[pulse_start_sfb]` before the
///   loop).
/// * `amp` — 4 bits. `pulse_amp[i]` per Table 4.7. Unsigned
///   magnitude added to (or subtracted from, depending on the sign
///   of the existing `x_quant` coefficient) the reconstructed
///   spectral coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pulse {
    /// `pulse_offset[i]` — 5-bit unsigned delta.
    pub offset: u8,
    /// `pulse_amp[i]` — 4-bit unsigned magnitude.
    pub amp: u8,
}

/// Width in bits of the wire `pulse_offset` field. ISO/IEC 14496-3
/// Table 4.7.
pub const PULSE_OFFSET_BITS: u32 = 5;

/// Width in bits of the wire `pulse_amp` field. ISO/IEC 14496-3
/// Table 4.7.
pub const PULSE_AMP_BITS: u32 = 4;

/// Maximum pulse count expressible in the 2-bit `number_pulse`
/// field. The wire value runs `0..=3`; the actual pulse count is
/// `number_pulse + 1`, so `MAX_PULSES == 4`.
pub const MAX_PULSES: usize = 4;

/// Parsed `pulse_data()` block (Table 4.7).
///
/// `pulses` always carries 1..=4 entries (since `number_pulse + 1
/// >= 1`); this is enforced by the writer and produced by the
/// parser. An empty `pulses` vector is rejected by [`PulseData::write`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PulseData {
    /// `pulse_start_sfb` — 6-bit index of the lowest scalefactor
    /// band that carries a pulse fix-up.
    pub pulse_start_sfb: u8,
    /// `pulses[i]` — the `(offset, amp)` records, in wire order.
    /// Length is in `1..=MAX_PULSES`; the wire `number_pulse` field
    /// is `pulses.len() - 1`.
    pub pulses: Vec<Pulse>,
}

impl PulseData {
    /// Parse a `pulse_data()` from `reader`.
    ///
    /// Returns [`Error::UnexpectedEnd`] on bit-reader underflow.
    /// Never returns a structural-error variant because every field
    /// of Table 4.7 is fixed-width and unconditionally well-formed
    /// up to bit-position arithmetic.
    pub fn parse(reader: &mut BitReader<'_>) -> Result<Self> {
        let number_pulse = read_u8(reader, 2)?;
        let pulse_start_sfb = read_u8(reader, 6)?;
        let count = number_pulse as usize + 1;
        let mut pulses = Vec::with_capacity(count);
        for _ in 0..count {
            let offset = read_u8(reader, PULSE_OFFSET_BITS)?;
            let amp = read_u8(reader, PULSE_AMP_BITS)?;
            pulses.push(Pulse { offset, amp });
        }
        Ok(PulseData {
            pulse_start_sfb,
            pulses,
        })
    }

    /// Wire `number_pulse` value (always `pulses.len() - 1`).
    /// Returns `0` for an empty `pulses` vector, which is itself
    /// rejected by [`PulseData::write`] — the accessor exists so the
    /// writer doesn't have to inline the subtraction with a saturating
    /// path of its own.
    pub fn number_pulse(&self) -> u8 {
        self.pulses.len().saturating_sub(1) as u8
    }

    /// Encode `pulse_data()` onto `writer`, the inverse of
    /// [`PulseData::parse`].
    ///
    /// The writer mirrors Table 4.7 verbatim — 2-bit `number_pulse`
    /// (where the wire value is `pulses.len() - 1`), 6-bit
    /// `pulse_start_sfb`, then `(5-bit pulse_offset + 4-bit
    /// pulse_amp)` per entry.
    ///
    /// Returns [`Error::PulseDataEncodeInvalid`] if:
    ///
    /// * `pulses.is_empty()` — Table 4.7's loop bound is
    ///   `number_pulse + 1`, so the smallest legal pulse count is 1.
    ///   A zero-pulse block has no wire representation that round-
    ///   trips through [`PulseData::parse`].
    /// * `pulses.len() > MAX_PULSES` (the 2-bit field maxes at 4).
    /// * `pulse_start_sfb > 0x3f` (6-bit field overflow).
    /// * Any `Pulse::offset > 0x1f` (5-bit field overflow).
    /// * Any `Pulse::amp > 0x0f` (4-bit field overflow).
    pub fn write(&self, writer: &mut BitWriter) -> Result<()> {
        if self.pulses.is_empty() || self.pulses.len() > MAX_PULSES {
            return Err(Error::PulseDataEncodeInvalid);
        }
        if self.pulse_start_sfb > 0x3f {
            return Err(Error::PulseDataEncodeInvalid);
        }
        for p in &self.pulses {
            if p.offset > 0x1f || p.amp > 0x0f {
                return Err(Error::PulseDataEncodeInvalid);
            }
        }

        let number_pulse = (self.pulses.len() - 1) as u32;
        writer.write_u32(number_pulse, 2);
        writer.write_u32(self.pulse_start_sfb as u32, 6);
        for p in &self.pulses {
            writer.write_u32(p.offset as u32, PULSE_OFFSET_BITS);
            writer.write_u32(p.amp as u32, PULSE_AMP_BITS);
        }
        Ok(())
    }
}

fn read_u8(reader: &mut BitReader<'_>, n: u32) -> Result<u8> {
    debug_assert!(n <= 8);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u8)
}
