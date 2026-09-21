//! `tns_data()` parser + encoder primitive — ISO/IEC 14496-3
//! §4.4.6 / Table 4.54 (syntax) and §4.6.9 / Table 4.155 (field-size
//! switching).
//!
//! Temporal Noise Shaping is an in-MDCT prediction tool that shapes
//! the temporal envelope of quantisation noise inside each transform
//! window. The encoder emits one or more all-pole filters per
//! window, each covering a contiguous range of scalefactor bands.
//! The decoder reverses the filtering after Huffman decoding but
//! before IMDCT. `tns_data()` is the wire record of those filters.
//! It rides inside an `individual_channel_stream()` between
//! `pulse_data()` and `gain_control_data()` / `spectral_data()`,
//! gated by the dispatching `tns_data_present` flag (Tables 4.44 /
//! 4.50).
//!
//! ## Wire layout (Table 4.54)
//!
//! ```text
//! tns_data() {
//!     for (w = 0; w < num_windows; w++) {
//!         n_filt[w];                                1..2 bits   (Table 4.155)
//!         if (n_filt[w])
//!             coef_res[w];                          1 bit
//!         for (filt = 0; filt < n_filt[w]; filt++) {
//!             length[w][filt];                      4 or 6 bits (Table 4.155)
//!             order[w][filt];                       3 or 5 bits (Table 4.155)
//!             if (order[w][filt]) {
//!                 direction[w][filt];               1 bit
//!                 coef_compress[w][filt];           1 bit
//!                 for (i = 0; i < order[w][filt]; i++)
//!                     coef[w][filt][i];             2..4 bits   (see below)
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! Two `window_sequence`-dependent field-width pairs control the
//! per-window dispatch (§4.6.9.2 Table 4.155):
//!
//! | name      | EIGHT_SHORT (128-line) | other window sizes |
//! |-----------|-------------------------|--------------------|
//! | `n_filt`  | 1 bit                   | 2 bits             |
//! | `length`  | 4 bits                  | 6 bits             |
//! | `order`   | 3 bits                  | 5 bits             |
//!
//! Per-filter `coef[i]` width is determined by `coef_res[w]` and
//! `coef_compress[w][filt]` per §4.6.9.3 `tns_decode_coef`:
//!
//! ```text
//! coef_res_bits  = coef_res[w] ? 4 : 3
//! coef_bits      = coef_res_bits - coef_compress[w][filt]
//!                 ∈ {2, 3, 4}
//! ```
//!
//! `coef_res` is **only** present on the wire when at least one
//! filter is emitted for the window (`n_filt[w] > 0`); zero-filter
//! windows simply skip the bit.
//!
//! `num_windows` is supplied by the surrounding `ics_info()`: `8` for
//! `EIGHT_SHORT_SEQUENCE`, `1` for every other window sequence
//! (§4.5.2.3.4).
//!
//! ## What this module covers
//!
//! * [`TnsData::parse`] — read a Table 4.54 block from a
//!   [`BitReader`], surfacing every wire field literally. Per-filter
//!   `coef[]` widths are computed from the freshly-read `coef_res`
//!   and `coef_compress` flags exactly as §4.6.9.3 prescribes.
//! * [`TnsData::write`] — the inverse: serialise a [`TnsData`] onto
//!   a [`BitWriter`] in bit-exact Table 4.54 form. Surfaces caller-
//!   side structural bugs (field overflow, length mismatch between
//!   `order` and the `coef` slice, out-of-range `coef` value) as
//!   [`Error::TnsDataEncodeInvalid`].
//!
//! ## What this module does *not* cover
//!
//! * The §4.6.9.3 `tns_decode_coef` LPC reconstruction (signed-magnitude
//!   conversion, `iqfac` arcsine inverse-quantisation, Levinson-style
//!   conversion to LPC coefficients) is **not** performed here — it
//!   needs a floating-point or fixed-point spectral context that
//!   arrives with the per-AOT IMDCT back-end.
//! * The §4.6.9.3 `tns_ar_filter` all-pole filtering pass over the
//!   spectrum is similarly deferred.
//! * The §4.6.9.4 `TNS_MAX_ORDER` and `TNS_MAX_BANDS` clamp tables
//!   (Tables 4.156 / 4.157) are not consulted by the wire encoder
//!   or parser. The parser surfaces the literal wire `order` /
//!   `length` regardless of whether they exceed the AOT-and-sample-
//!   rate-dependent caps; the decoder's reconstruction loop is the
//!   layer that applies `min(order, TNS_MAX_ORDER)` and
//!   `min(bands, TNS_MAX_BANDS, max_sfb)`.
//! * The normative constraint that `tns_data_present == 0` for the
//!   ER AAC LD `gain_control_data` path (Table 4.50) is the
//!   responsibility of the dispatching `individual_channel_stream()`
//!   (which has not landed yet).

use oxideav_core::bits::{BitReader, BitWriter};

use crate::ics_info::WindowSequence;
use crate::{Error, Result};

/// One TNS noise-shaping filter inside a single transform window.
///
/// Fields are the literal Table 4.54 wire values. The `coef` slot
/// holds the unsigned magnitudes as transmitted (each entry occupies
/// `coef_bits` per §4.6.9.3 — `coef_res_bits − coef_compress`);
/// signed-magnitude conversion is performed by `tns_decode_coef()`
/// at decode time and is *not* applied here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TnsFilter {
    /// `length[w][filt]` — number of scalefactor bands covered by this
    /// filter (4 bits on `EIGHT_SHORT_SEQUENCE`, 6 bits otherwise).
    pub length: u8,
    /// `order[w][filt]` — all-pole filter order (3 bits on
    /// `EIGHT_SHORT_SEQUENCE`, 5 bits otherwise). When `order == 0`
    /// no `direction` / `coef_compress` / `coef[]` are emitted.
    pub order: u8,
    /// `direction[w][filt]` — slide direction across the spectrum:
    /// `false` = upward, `true` = downward. Absent on the wire when
    /// `order == 0`; the [`TnsData::parse`] caller-side default is
    /// `false` in that case.
    pub direction: bool,
    /// `coef_compress[w][filt]` — when `true` the MSB of every
    /// transmitted coefficient is omitted, shrinking each `coef[i]`
    /// from `coef_res_bits` to `coef_res_bits − 1`. Absent on the
    /// wire when `order == 0`.
    pub coef_compress: bool,
    /// `coef[w][filt][i]` for `i in 0..order` — unsigned magnitudes
    /// as transmitted. Length **must** equal `order`. Each entry
    /// is in `0..(1 << coef_bits)` where
    /// `coef_bits = (3 + coef_res as u32) − coef_compress as u32`.
    pub coef: Vec<u8>,
}

/// Per-window TNS payload. Always carries `coef_res` even when
/// `filters.is_empty()`; the [`TnsData::write`] code path omits the
/// wire bit in that case but the field is meaningful for callers
/// that round-trip a structurally identical block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TnsWindow {
    /// `coef_res[w]` — `false` selects a 3-bit `coef_res_bits`,
    /// `true` selects a 4-bit `coef_res_bits` per §4.6.9.3. When
    /// `filters.is_empty()` the bit is **not** transmitted; both the
    /// parser and the writer treat the stored value as a don't-care
    /// in that case.
    pub coef_res: bool,
    /// The filters for this window, in wire order. `n_filt[w]` on
    /// the wire is `filters.len()` and is capped by the field width
    /// (1 bit on `EIGHT_SHORT_SEQUENCE`, 2 bits otherwise → 0..=1
    /// vs 0..=3).
    pub filters: Vec<TnsFilter>,
}

/// Parsed `tns_data()` block (Table 4.54).
///
/// `windows` always carries exactly `num_windows` entries (8 for
/// `EIGHT_SHORT_SEQUENCE`, 1 otherwise). The [`TnsData::write`] code
/// path validates this against the surrounding [`WindowSequence`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TnsData {
    /// One entry per transform window. Length must match the
    /// surrounding [`WindowSequence`]: 8 for `EightShort`, 1
    /// otherwise.
    pub windows: Vec<TnsWindow>,
}

/// `n_filt` field width for `EIGHT_SHORT_SEQUENCE` per Table 4.155.
pub const N_FILT_BITS_SHORT: u32 = 1;
/// `n_filt` field width for any non-`EIGHT_SHORT_SEQUENCE` per Table
/// 4.155.
pub const N_FILT_BITS_LONG: u32 = 2;
/// `length` field width for `EIGHT_SHORT_SEQUENCE` per Table 4.155.
pub const LENGTH_BITS_SHORT: u32 = 4;
/// `length` field width for any non-`EIGHT_SHORT_SEQUENCE` per Table
/// 4.155.
pub const LENGTH_BITS_LONG: u32 = 6;
/// `order` field width for `EIGHT_SHORT_SEQUENCE` per Table 4.155.
pub const ORDER_BITS_SHORT: u32 = 3;
/// `order` field width for any non-`EIGHT_SHORT_SEQUENCE` per Table
/// 4.155.
pub const ORDER_BITS_LONG: u32 = 5;
/// `coef_res` field width per Table 4.54.
pub const COEF_RES_BITS: u32 = 1;
/// `direction` field width per Table 4.54.
pub const DIRECTION_BITS: u32 = 1;
/// `coef_compress` field width per Table 4.54.
pub const COEF_COMPRESS_BITS: u32 = 1;

/// `(n_filt_bits, length_bits, order_bits)` triple for the given
/// `window_sequence`. The selection rule is §4.6.9.2 Table 4.155 —
/// the 128-line `EIGHT_SHORT_SEQUENCE` shrinks every field by one
/// or two bits versus the other window sizes.
pub fn field_widths(seq: WindowSequence) -> (u32, u32, u32) {
    if seq.is_eight_short() {
        (N_FILT_BITS_SHORT, LENGTH_BITS_SHORT, ORDER_BITS_SHORT)
    } else {
        (N_FILT_BITS_LONG, LENGTH_BITS_LONG, ORDER_BITS_LONG)
    }
}

/// `num_windows` for the given `window_sequence` per §4.5.2.3.4:
/// `8` for `EIGHT_SHORT_SEQUENCE`, `1` otherwise.
pub fn num_windows(seq: WindowSequence) -> usize {
    if seq.is_eight_short() {
        8
    } else {
        1
    }
}

/// Per-filter `coef_bits` width per §4.6.9.3:
///
/// ```text
/// coef_res_bits = 3 + (coef_res ? 1 : 0)
/// coef_bits     = coef_res_bits - (coef_compress ? 1 : 0)
/// ```
///
/// Result is in `{2, 3, 4}`.
pub fn coef_bits(coef_res: bool, coef_compress: bool) -> u32 {
    let coef_res_bits = 3 + u32::from(coef_res);
    coef_res_bits - u32::from(coef_compress)
}

impl TnsData {
    /// Parse a `tns_data()` from `reader`, given the surrounding
    /// `window_sequence` (which selects the per-window field widths
    /// and `num_windows`).
    ///
    /// Returns [`Error::UnexpectedEnd`] on bit-reader underflow.
    /// Returns [`Error::TnsDataEncodeInvalid`] when a `coef[i]` is
    /// large enough to indicate a parser/spec mismatch (which cannot
    /// happen for a conforming stream — every `coef[i]` is bounded
    /// by its field width — but the check guards round-trip
    /// invariants for hostile inputs).
    pub fn parse(reader: &mut BitReader<'_>, window_sequence: WindowSequence) -> Result<Self> {
        let (n_filt_bits, length_bits, order_bits) = field_widths(window_sequence);
        let nw = num_windows(window_sequence);
        let mut windows = Vec::with_capacity(nw);
        for _ in 0..nw {
            let n_filt = read_u8(reader, n_filt_bits)?;
            let coef_res = if n_filt > 0 {
                reader.read_bit().map_err(|_| Error::UnexpectedEnd)?
            } else {
                false
            };
            let mut filters = Vec::with_capacity(n_filt as usize);
            for _ in 0..n_filt {
                let length = read_u8(reader, length_bits)?;
                let order = read_u8(reader, order_bits)?;
                let (direction, coef_compress, coef) = if order > 0 {
                    let direction = reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
                    let coef_compress = reader.read_bit().map_err(|_| Error::UnexpectedEnd)?;
                    let bits = coef_bits(coef_res, coef_compress);
                    let mut coef = Vec::with_capacity(order as usize);
                    for _ in 0..order {
                        coef.push(read_u8(reader, bits)?);
                    }
                    (direction, coef_compress, coef)
                } else {
                    (false, false, Vec::new())
                };
                filters.push(TnsFilter {
                    length,
                    order,
                    direction,
                    coef_compress,
                    coef,
                });
            }
            windows.push(TnsWindow { coef_res, filters });
        }
        Ok(TnsData { windows })
    }

    /// Encode `tns_data()` onto `writer`, the inverse of
    /// [`TnsData::parse`].
    ///
    /// Returns [`Error::TnsDataEncodeInvalid`] if:
    ///
    /// * `windows.len()` differs from [`num_windows`] for
    ///   `window_sequence` (1 for long sequences, 8 for
    ///   `EIGHT_SHORT_SEQUENCE`).
    /// * `filters.len()` exceeds the `n_filt` field cap
    ///   (`(1 << n_filt_bits) - 1`) — 1 on `EIGHT_SHORT_SEQUENCE`,
    ///   3 otherwise.
    /// * Any `length` exceeds the `length` field cap
    ///   (`(1 << length_bits) - 1`) — 15 on `EIGHT_SHORT_SEQUENCE`,
    ///   63 otherwise.
    /// * Any `order` exceeds the `order` field cap — 7 on
    ///   `EIGHT_SHORT_SEQUENCE`, 31 otherwise.
    /// * A filter's `coef.len()` differs from its `order`.
    /// * A filter's `coef[i]` exceeds the `(1 << coef_bits) - 1`
    ///   field cap (where `coef_bits = (3 + coef_res) - coef_compress`).
    /// * A filter has populated `direction` / `coef_compress` /
    ///   `coef` slots while `order == 0` (those fields are not
    ///   transmitted on the wire and a non-default value would not
    ///   round-trip).
    pub fn write(&self, writer: &mut BitWriter, window_sequence: WindowSequence) -> Result<()> {
        let (n_filt_bits, length_bits, order_bits) = field_widths(window_sequence);
        let nw = num_windows(window_sequence);
        if self.windows.len() != nw {
            return Err(Error::TnsDataEncodeInvalid);
        }
        let n_filt_max = (1u32 << n_filt_bits) - 1;
        let length_max = (1u32 << length_bits) - 1;
        let order_max = (1u32 << order_bits) - 1;
        for w in &self.windows {
            if (w.filters.len() as u32) > n_filt_max {
                return Err(Error::TnsDataEncodeInvalid);
            }
            for f in &w.filters {
                if u32::from(f.length) > length_max || u32::from(f.order) > order_max {
                    return Err(Error::TnsDataEncodeInvalid);
                }
                if f.order as usize != f.coef.len() {
                    return Err(Error::TnsDataEncodeInvalid);
                }
                if f.order == 0 && (f.direction || f.coef_compress) {
                    // Non-default direction/compress would silently be
                    // dropped on the wire (the spec emits neither field
                    // when order == 0); reject to keep round-trip
                    // identity.
                    return Err(Error::TnsDataEncodeInvalid);
                }
                if f.order > 0 {
                    let bits = coef_bits(w.coef_res, f.coef_compress);
                    let coef_max = (1u32 << bits) - 1;
                    for c in &f.coef {
                        if u32::from(*c) > coef_max {
                            return Err(Error::TnsDataEncodeInvalid);
                        }
                    }
                }
            }
        }

        for w in &self.windows {
            writer.write_u32(w.filters.len() as u32, n_filt_bits);
            if !w.filters.is_empty() {
                writer.write_bit(w.coef_res);
            }
            for f in &w.filters {
                writer.write_u32(u32::from(f.length), length_bits);
                writer.write_u32(u32::from(f.order), order_bits);
                if f.order > 0 {
                    writer.write_bit(f.direction);
                    writer.write_bit(f.coef_compress);
                    let bits = coef_bits(w.coef_res, f.coef_compress);
                    for c in &f.coef {
                        writer.write_u32(u32::from(*c), bits);
                    }
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
