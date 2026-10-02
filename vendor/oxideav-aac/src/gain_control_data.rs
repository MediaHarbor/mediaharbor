//! `gain_control_data()` parser + encoder primitive — ISO/IEC 14496-3
//! §4.4.6.5 / Table 4.12.
//!
//! `gain_control_data()` is the wire record of the SSR (Scalable
//! Sample Rate, AOT 3) gain-control tool. SSR splits each AAC frame
//! through a 4-band polyphase quadrature filterbank (PQF) **before**
//! the MDCT, and applies a per-band, per-window gain-adjustment
//! ladder to attenuate pre-echo artefacts. The decoder reads the
//! ladder out of `gain_control_data()` and reverses it after the
//! per-band IMDCTs. The block rides inside an
//! `individual_channel_stream()` between `tns_data()` and
//! `spectral_data()`, gated by the dispatching
//! `gain_control_data_present` flag (Tables 4.44 / 4.50).
//!
//! ## Wire layout (Table 4.12)
//!
//! ```text
//! gain_control_data() {
//!     max_band;                                      2 bits
//!     for (bd = 1; bd <= max_band; bd++) {
//!         for (wd = 0; wd < N(window_sequence); wd++) {
//!             adjust_num[bd][wd];                    3 bits
//!             for (ad = 0; ad < adjust_num[bd][wd]; ad++) {
//!                 alevcode[bd][wd][ad];              4 bits
//!                 aloccode[bd][wd][ad];              W(seq, wd) bits
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! Per Table 4.12 the per-window count `N(window_sequence)` and the
//! per-`(window_sequence, wd)` `aloccode` width `W(seq, wd)` are:
//!
//! | `window_sequence`        | N | `W(seq, wd=0)` | `W(seq, wd≥1)` |
//! |--------------------------|---|----------------|----------------|
//! | `ONLY_LONG_SEQUENCE`     | 1 | 5              | n/a            |
//! | `LONG_START_SEQUENCE`    | 2 | 4              | 2              |
//! | `EIGHT_SHORT_SEQUENCE`   | 8 | 2              | 2              |
//! | `LONG_STOP_SEQUENCE`     | 2 | 4              | 5              |
//!
//! `alevcode` is always 4 bits; `adjust_num` is always 3 bits (so
//! per `(bd, wd)` slot the ladder length is `0..=7`).
//!
//! The outer band loop iterates `1..=max_band` (note the **`bd =
//! 1`** start — band 0 carries no gain ladder by spec).
//! `max_band ∈ 0..=3` (2-bit field); when `max_band == 0` the body
//! collapses to just the 2-bit field. Per the §4.6.12 SSR backend
//! the legal `max_band` for a decoder targeting 4-band PQF output
//! is `0..=3`; the wire-format itself does not constrain values
//! further than the field width.
//!
//! ## What this module covers
//!
//! * [`GainControlData::parse`] — read a Table 4.12 block from a
//!   [`BitReader`], surfacing the raw wire fields without applying
//!   the §4.6.12 SSR gain-reconstruction (the actual ladder
//!   application needs the SSR PQF backend, which is not part of
//!   Phase 2).
//! * [`GainControlData::write`] — the inverse: serialise a
//!   [`GainControlData`] onto a [`BitWriter`] in bit-exact
//!   Table 4.12 form. Caller-side field overflow surfaces as
//!   [`Error::GainControlDataEncodeInvalid`].
//!
//! ## What this module does *not* cover
//!
//! * The §4.6.12 ladder-application loop (per-window gain envelope
//!   reconstruction from `(alevcode, aloccode)` pairs into
//!   sample-domain attenuation factors) is deferred until the SSR
//!   PQF / IMDCT back-end lands.
//! * The normative §4.6.12 constraint that the SSR profile's
//!   `gain_control_data_present` flag is **0** for AOTs other than
//!   3 (SSR) is the responsibility of the dispatching
//!   `individual_channel_stream()` (not yet wired up); the parser
//!   and writer here surface the literal Table 4.12 bytes
//!   regardless of the surrounding AOT so future round work has
//!   access to the raw decoded record.

use oxideav_core::bits::{BitReader, BitWriter};

use crate::ics_info::WindowSequence;
use crate::{Error, Result};

/// Width in bits of the `max_band` field. Table 4.12.
pub const MAX_BAND_BITS: u32 = 2;

/// Width in bits of the `adjust_num` field. Table 4.12.
pub const ADJUST_NUM_BITS: u32 = 3;

/// Width in bits of the `alevcode` field. Table 4.12.
pub const ALEVCODE_BITS: u32 = 4;

/// Maximum value of the `max_band` field (2-bit width cap).
pub const MAX_BAND_CAP: u8 = 0x03;

/// Maximum value of the `adjust_num` field (3-bit width cap). Each
/// per-`(bd, wd)` slot can carry between 0 and 7 ladder entries.
pub const MAX_ADJUST_NUM: u8 = 0x07;

/// Maximum value of the `alevcode` field (4-bit width cap).
pub const MAX_ALEVCODE: u8 = 0x0f;

/// Per-window count `N(window_sequence)` from Table 4.12.
///
/// * `ONLY_LONG_SEQUENCE` → 1
/// * `LONG_START_SEQUENCE` → 2
/// * `EIGHT_SHORT_SEQUENCE` → 8
/// * `LONG_STOP_SEQUENCE` → 2
pub fn num_windows(window_sequence: WindowSequence) -> usize {
    match window_sequence {
        WindowSequence::OnlyLong => 1,
        WindowSequence::LongStart => 2,
        WindowSequence::EightShort => 8,
        WindowSequence::LongStop => 2,
    }
}

/// Width in bits of the `aloccode` field at the given
/// `(window_sequence, wd)` position per Table 4.12.
///
/// * `ONLY_LONG_SEQUENCE`   — always 5 (only `wd == 0` is reached).
/// * `LONG_START_SEQUENCE`  — 4 if `wd == 0`, else 2.
/// * `EIGHT_SHORT_SEQUENCE` — always 2.
/// * `LONG_STOP_SEQUENCE`   — 4 if `wd == 0`, else 5.
///
/// Returns `0` for `wd` indices outside the per-sequence range — the
/// caller is responsible for honouring [`num_windows`] when stepping
/// the inner loop.
pub fn aloccode_bits(window_sequence: WindowSequence, wd: usize) -> u32 {
    match window_sequence {
        WindowSequence::OnlyLong => {
            if wd == 0 {
                5
            } else {
                0
            }
        }
        WindowSequence::LongStart => match wd {
            0 => 4,
            1 => 2,
            _ => 0,
        },
        WindowSequence::EightShort => {
            if wd < 8 {
                2
            } else {
                0
            }
        }
        WindowSequence::LongStop => match wd {
            0 => 4,
            1 => 5,
            _ => 0,
        },
    }
}

/// Single `(alevcode, aloccode)` ladder entry within one
/// `(bd, wd)` slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GainAdjust {
    /// `alevcode[bd][wd][ad]` — 4-bit unsigned level code.
    pub alevcode: u8,
    /// `aloccode[bd][wd][ad]` — unsigned location code; field width
    /// is selected by [`aloccode_bits`] from the surrounding
    /// `window_sequence` and `wd` index.
    pub aloccode: u8,
}

/// Per-window ladder for a single `(bd, wd)` slot. The vector length
/// is the wire `adjust_num[bd][wd]` value (`0..=7`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GainWindow {
    /// Ladder entries in wire order. `len()` equals
    /// `adjust_num[bd][wd]`.
    pub adjustments: Vec<GainAdjust>,
}

/// Per-band collection of per-window ladders for one `bd` value.
///
/// `windows.len()` must equal [`num_windows`] for the surrounding
/// `window_sequence`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GainBand {
    /// Per-`wd` ladder entries. `windows[wd]` is the
    /// `(bd, wd)` slot.
    pub windows: Vec<GainWindow>,
}

/// Parsed `gain_control_data()` block (Table 4.12).
///
/// `bands.len()` equals `max_band` (the **wire** field value); the
/// per-spec `bd = 1..=max_band` outer loop maps onto `bands[bd - 1]`.
/// `bands` is empty when `max_band == 0` (the body collapses to a
/// bare 2-bit zero).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GainControlData {
    /// `max_band` per Table 4.12 — 2-bit field, `0..=3`. The length
    /// of `bands` is `max_band`.
    pub max_band: u8,
    /// Per-band ladders, indexed `bands[bd - 1]` for spec band
    /// `bd ∈ 1..=max_band`. `bands.len() == max_band as usize`.
    pub bands: Vec<GainBand>,
}

impl GainControlData {
    /// Parse a `gain_control_data()` block from `reader`, using
    /// `window_sequence` to choose the per-window count and the
    /// `aloccode` field widths.
    ///
    /// Returns [`Error::UnexpectedEnd`] on bit-reader underflow.
    /// Never returns an encode-side variant — every field of
    /// Table 4.12 is fixed-width and unconditionally well-formed up
    /// to bit-position arithmetic.
    pub fn parse(reader: &mut BitReader<'_>, window_sequence: WindowSequence) -> Result<Self> {
        let max_band = read_u8(reader, MAX_BAND_BITS)?;
        let n_win = num_windows(window_sequence);
        let mut bands = Vec::with_capacity(max_band as usize);
        for _bd in 1..=max_band as usize {
            let mut windows = Vec::with_capacity(n_win);
            for wd in 0..n_win {
                let adjust_num = read_u8(reader, ADJUST_NUM_BITS)?;
                let aloc_bits = aloccode_bits(window_sequence, wd);
                let mut adjustments = Vec::with_capacity(adjust_num as usize);
                for _ad in 0..adjust_num as usize {
                    let alevcode = read_u8(reader, ALEVCODE_BITS)?;
                    let aloccode = read_u8(reader, aloc_bits)?;
                    adjustments.push(GainAdjust { alevcode, aloccode });
                }
                windows.push(GainWindow { adjustments });
            }
            bands.push(GainBand { windows });
        }
        Ok(GainControlData { max_band, bands })
    }

    /// Encode `gain_control_data()` onto `writer`, the bit-exact
    /// inverse of [`GainControlData::parse`].
    ///
    /// Returns [`Error::GainControlDataEncodeInvalid`] if any of the
    /// following caller-side invariants are violated:
    ///
    /// * `max_band > MAX_BAND_CAP` (2-bit `max_band` overflow).
    /// * `bands.len() != max_band as usize` (the outer band-loop
    ///   count must match the dispatched wire value).
    /// * Any `band.windows.len() != num_windows(window_sequence)`
    ///   (the per-band window count must match the wire dispatch).
    /// * Any `window.adjustments.len() > MAX_ADJUST_NUM as usize`
    ///   (3-bit `adjust_num` overflow).
    /// * Any `GainAdjust::alevcode > MAX_ALEVCODE` (4-bit overflow).
    /// * Any `GainAdjust::aloccode` exceeds the
    ///   `(1 << aloccode_bits(seq, wd)) - 1` cap for its slot.
    pub fn write(&self, writer: &mut BitWriter, window_sequence: WindowSequence) -> Result<()> {
        if self.max_band > MAX_BAND_CAP {
            return Err(Error::GainControlDataEncodeInvalid);
        }
        if self.bands.len() != self.max_band as usize {
            return Err(Error::GainControlDataEncodeInvalid);
        }
        let n_win = num_windows(window_sequence);
        for band in &self.bands {
            if band.windows.len() != n_win {
                return Err(Error::GainControlDataEncodeInvalid);
            }
            for (wd, window) in band.windows.iter().enumerate() {
                if window.adjustments.len() > MAX_ADJUST_NUM as usize {
                    return Err(Error::GainControlDataEncodeInvalid);
                }
                let aloc_bits = aloccode_bits(window_sequence, wd);
                let aloc_cap: u32 = if aloc_bits == 0 {
                    0
                } else {
                    (1u32 << aloc_bits) - 1
                };
                for adj in &window.adjustments {
                    if adj.alevcode > MAX_ALEVCODE {
                        return Err(Error::GainControlDataEncodeInvalid);
                    }
                    if adj.aloccode as u32 > aloc_cap {
                        return Err(Error::GainControlDataEncodeInvalid);
                    }
                }
            }
        }

        writer.write_u32(self.max_band as u32, MAX_BAND_BITS);
        for band in &self.bands {
            for (wd, window) in band.windows.iter().enumerate() {
                let adjust_num = window.adjustments.len() as u32;
                writer.write_u32(adjust_num, ADJUST_NUM_BITS);
                let aloc_bits = aloccode_bits(window_sequence, wd);
                for adj in &window.adjustments {
                    writer.write_u32(adj.alevcode as u32, ALEVCODE_BITS);
                    writer.write_u32(adj.aloccode as u32, aloc_bits);
                }
            }
        }
        Ok(())
    }
}

fn read_u8(reader: &mut BitReader<'_>, n: u32) -> Result<u8> {
    debug_assert!(n <= 8);
    Ok(reader.read_u32(n).map_err(|_| Error::UnexpectedEnd)? as u8)
}
