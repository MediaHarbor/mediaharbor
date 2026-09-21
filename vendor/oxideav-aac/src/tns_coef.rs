//! TNS coefficient inverse-quantisation and LPC step-up — ISO/IEC
//! 14496-3 §4.6.9.3 (`tns_decode_coef` pseudo-code) plus the
//! ISO/IEC 14496-3:2001 §C.6 encoder-side quantisation companion.
//!
//! Temporal Noise Shaping carries one all-pole filter per scalefactor
//! region. The wire `coef[i]` slots produced by [`crate::tns_data`]
//! hold the per-coefficient *quantised reflection (PARCOR)* index in
//! `coef_bits` (2..=4) of unsigned magnitude with the high bit acting
//! as a sign flag (signed-magnitude / two's-complement padding). The
//! decoder reconstructs the floating-point reflection coefficient
//! `rq[i]` by:
//!
//! 1. Sign-extending the truncated wire value to a normal signed int.
//! 2. Inverse-quantising with `sin(index / iqfac)` where `iqfac`
//!    depends on the sign of `index` (`iqfac` for non-negative,
//!    `iqfac_m` for negative — the half-bit offset matches the
//!    encoder's rounded quantisation).
//! 3. Running the §4.6.9.3 *conversion-to-LPC* step-up loop that
//!    converts the order-`order` PARCOR array into an order-`order`
//!    direct-form LPC `a[]` vector with `a[0] = 1`.
//!
//! The encoder side (§C.6) inverts steps (1) and (2): given the
//! floating-point reflection coefficients computed by Levinson-Durbin,
//! quantise via `NINT(arcsin(r) * iqfac)`, where `iqfac` again branches
//! on the sign of `r`. The step-up loop is unchanged — both encoder and
//! decoder run it to derive the same `a[]` array that drives the
//! `tns_ar_filter()` / inverse FIR pass.
//!
//! ## §4.6.9.3 pseudocode (transcribed for cross-check)
//!
//! ```text
//! tns_decode_coef( order, coef_res_bits, coef_compress, coef[], a[] )
//! {
//!     sgn_mask[] = { 0x2, 0x4, 0x8 };
//!     neg_mask[] = { ~0x3, ~0x7, ~0xf };
//!
//!     coef_res2 = coef_res_bits - coef_compress;
//!     s_mask = sgn_mask[ coef_res2 - 2 ];
//!     n_mask = neg_mask[ coef_res2 - 2 ];
//!
//!     for (i = 0; i < order; i++)
//!         tmp[i] = (coef[i] & s_mask) ? (coef[i] | n_mask) : coef[i];
//!
//!     iqfac   = ((1 << (coef_res_bits-1)) - 0.5) / (π/2.0);
//!     iqfac_m = ((1 << (coef_res_bits-1)) + 0.5) / (π/2.0);
//!     for (i = 0; i < order; i++) {
//!         tmp2[i] = sin( tmp[i] / ((tmp[i] >= 0) ? iqfac : iqfac_m) );
//!     }
//!
//!     a[0] = 1;
//!     for (m = 1; m <= order; m++) {
//!         for (i = 1; i < m; i++)
//!             b[i] = a[i] + tmp2[m-1] * a[m-i];
//!         for (i = 1; i < m; i++)
//!             a[i] = b[i];
//!         a[m] = tmp2[m-1];
//!     }
//! }
//! ```
//!
//! The `sgn_mask` / `neg_mask` pair encode the two's-complement
//! sign-extension of a `coef_res2`-bit field (`coef_res2 ∈ {2, 3, 4}`).
//! `s_mask` is `1 << (coef_res2 - 1)` (the MSB of the truncated field)
//! and `n_mask` is `~((1 << coef_res2) - 1)` (the bits that need to be
//! filled with 1 to extend a negative value into a normal signed int).
//!
//! ## What this module covers
//!
//! * [`iqfac`] / [`iqfac_m`] — the §4.6.9.3 quantiser scale factors
//!   `((1 << (n-1)) ± 0.5) / (π/2)`. Exposed as standalone helpers so
//!   the §C.6 encoder path can re-use the same constants.
//! * [`sign_extend_coef`] — inverse of `(coef & s_mask) ? coef |
//!   n_mask : coef`. Takes a wire `coef` (held in the low `coef_res2`
//!   bits as transmitted) and a `coef_res2 ∈ {2, 3, 4}` field width;
//!   returns the matching signed integer.
//! * [`tns_decode_coef`] — the full §4.6.9.3 path: wire `coef[]` →
//!   floating-point `tmp2[]` (the inverse-quantised PARCOR
//!   coefficients).
//! * [`tns_encode_coef`] — the §C.6 inverse: floating-point reflection
//!   coefficients → wire `coef[]` values ready for [`crate::tns_data`].
//! * [`lpc_step_up`] — the §4.6.9.3 *conversion-to-LPC* loop. Takes a
//!   slice of inverse-quantised PARCOR `tmp2[]` values and returns the
//!   `order + 1` direct-form LPC `a[]` vector with `a[0] = 1.0`.
//! * [`tns_decode_coef_to_lpc`] — convenience wrapper that runs
//!   [`tns_decode_coef`] followed by [`lpc_step_up`]; the
//!   reconstruction loop will call this once per `(window, filter)`
//!   pair.
//! * [`tns_ar_filter`] — the §4.6.9.3 `tns_ar_filter()` all-pole IIR
//!   pass. Operates in place over a strided region of the dequantised
//!   spectrum (`start` / `size` / `inc`) driven by the `lpc[]` array
//!   from [`lpc_step_up`]. Filter state is zero-seeded per
//!   invocation, exactly as the spec mandates.
//!
//! ## What this module does *not* cover
//!
//! * The §4.6.9 `tns_decode_frame()` orchestration that dispatches
//!   `tns_decode_coef_to_lpc` / `tns_ar_filter` per filter per window.
//!   That orchestration is the responsibility of the eventual
//!   `individual_channel_stream()` reconstruction driver.
//! * The §4.6.17.3.4 ER AAC LD `int_tns_decode_coef()` integer
//!   variant. The LD path uses a fixed-point arithmetic surface that
//!   we do not need until the AAC LD reconstruction path is wired.
//! * The §C.6 Levinson-Durbin / autocorrelation reflection-coefficient
//!   derivation. The encoder gets floating-point PARCOR coefficients
//!   from some upstream LPC estimator (a standard speech-coding
//!   procedure); this module accepts the already-derived `r[]` array
//!   and quantises it.
//!
//! ## Numerical contract
//!
//! Both `iqfac` and `iqfac_m` are exactly representable as `f64` for
//! every legal `coef_res_bits ∈ {3, 4}`:
//!
//! | coef_res_bits | iqfac (≈)               | iqfac_m (≈)             |
//! |---------------|-------------------------|-------------------------|
//! | 3             | `3.5 / (π/2) ≈ 2.228...`| `4.5 / (π/2) ≈ 2.864...`|
//! | 4             | `7.5 / (π/2) ≈ 4.774...`| `8.5 / (π/2) ≈ 5.411...`|
//!
//! The encoder's `NINT(arcsin(r) * iqfac)` rounding is implemented via
//! `f64::round` (round-half-away-from-zero, matching the spec's `NINT`
//! convention). A reflection coefficient `r = 0.0` quantises to
//! `index = 0` (the `iqfac` branch is taken because `r >= 0`); the
//! decoder then reconstructs `sin(0 / iqfac) = 0.0`. Likewise the
//! sentinel `r = 1.0` rounds to the field maximum (`6` for
//! `coef_res2 = 3`, `7` for `coef_res2 = 4` after `coef_compress = 0`)
//! and `r = -1.0` rounds to the field minimum (`-7` / `-8`); both
//! recover via `sin(±π/2) = ±1.0` to within IEEE-754 round-off
//! (`|round-trip error| < 1e-15`). All in-range PARCOR values quantise
//! cleanly without saturation; an out-of-range `r` (`|r| > 1.0`) is
//! rejected by [`tns_encode_coef`] with
//! [`Error::TnsCoefOutOfRange`] because `arcsin` is undefined there.
//!
//! The signed-magnitude wire fold preserves round-trip: every legal
//! sign-extended index `i` in `[-(1 << (coef_res2-1)),
//! (1 << (coef_res2-1)) - 1]` maps back to a unique `coef_res2`-bit
//! pattern. The step-up loop is exact (no quantisation), so the same
//! quantised PARCOR array always yields bit-identical LPC coefficients.

use core::f64::consts::PI;

use crate::{Error, Result};

/// Half-π. Cached so the [`iqfac`] / [`iqfac_m`] arithmetic matches
/// the spec's literal `π/2.0` division.
const HALF_PI: f64 = PI / 2.0;

/// `iqfac` per §4.6.9.3. Branches on a *non-negative* index / PARCOR
/// value. Defined as `((1 << (coef_res_bits-1)) - 0.5) / (π/2)`.
///
/// Returns [`Error::TnsCoefOutOfRange`] when `coef_res_bits` lies
/// outside `3..=4` (the legal `coef_res[w] + 3` values per
/// §4.6.9.3 and §C.6, where `coef_res[w] ∈ {0, 1}` is the wire flag).
pub fn iqfac(coef_res_bits: u32) -> Result<f64> {
    if !(3..=4).contains(&coef_res_bits) {
        return Err(Error::TnsCoefOutOfRange);
    }
    let scale = (1u32 << (coef_res_bits - 1)) as f64 - 0.5;
    Ok(scale / HALF_PI)
}

/// `iqfac_m` per §4.6.9.3. Branches on a *negative* index / PARCOR
/// value. Defined as `((1 << (coef_res_bits-1)) + 0.5) / (π/2)`.
///
/// Errors as [`iqfac`].
pub fn iqfac_m(coef_res_bits: u32) -> Result<f64> {
    if !(3..=4).contains(&coef_res_bits) {
        return Err(Error::TnsCoefOutOfRange);
    }
    let scale = (1u32 << (coef_res_bits - 1)) as f64 + 0.5;
    Ok(scale / HALF_PI)
}

/// Sign-extend a wire `coef` value held in the low `coef_res2` bits
/// into a normal signed integer.
///
/// `coef_res2 = coef_res_bits - coef_compress` per §4.6.9.3 and is
/// always in `{2, 3, 4}`. The spec's `sgn_mask = 1 <<
/// (coef_res2 - 1)` selects the MSB of the truncated field; if that
/// bit is set, the spec ORs in `neg_mask = ~((1 << coef_res2) - 1)`
/// to fill the upper bits with 1 (two's-complement sign extension).
///
/// Returns [`Error::TnsCoefOutOfRange`] when `coef_res2` lies outside
/// `2..=4` or when `coef` does not fit in `coef_res2` bits.
pub fn sign_extend_coef(coef: u32, coef_res2: u32) -> Result<i32> {
    if !(2..=4).contains(&coef_res2) {
        return Err(Error::TnsCoefOutOfRange);
    }
    let field_mask = (1u32 << coef_res2) - 1;
    if coef & !field_mask != 0 {
        return Err(Error::TnsCoefOutOfRange);
    }
    let sgn_mask = 1u32 << (coef_res2 - 1);
    if coef & sgn_mask != 0 {
        // Negative — OR with the bits above the field.
        let neg_mask = !field_mask;
        Ok((coef | neg_mask) as i32)
    } else {
        Ok(coef as i32)
    }
}

/// Inverse of [`sign_extend_coef`]: pack a signed integer back into a
/// `coef_res2`-bit wire field. Used by the encoder to emit the wire
/// `coef[i]` slot.
///
/// Returns [`Error::TnsCoefOutOfRange`] when `coef_res2` is outside
/// `2..=4`, or when `value` is outside the field-representable range
/// `-(1 << (coef_res2-1))..=(1 << (coef_res2-1)) - 1`.
pub fn pack_coef(value: i32, coef_res2: u32) -> Result<u32> {
    if !(2..=4).contains(&coef_res2) {
        return Err(Error::TnsCoefOutOfRange);
    }
    let half = 1i32 << (coef_res2 - 1);
    if !(-half..half).contains(&value) {
        return Err(Error::TnsCoefOutOfRange);
    }
    let field_mask = (1u32 << coef_res2) - 1;
    Ok((value as u32) & field_mask)
}

/// Run §4.6.9.3 `tns_decode_coef`: sign-extend the wire `coef[]`,
/// then inverse-quantise via `sin(tmp[i] / iqfac_branch)` to recover
/// the floating-point PARCOR (reflection-coefficient) array.
///
/// * `coef_res_bits` is the spec's `coef_res[w] + 3`, i.e. either
///   `3` (`coef_res = 0`) or `4` (`coef_res = 1`).
/// * `coef_compress` is the per-filter flag (0 or 1). The §4.6.9.3
///   field width on the wire is `coef_res2 = coef_res_bits -
///   coef_compress` bits per coefficient.
/// * `coef` is the per-coefficient wire slice produced by
///   [`crate::tns_data::TnsData::parse`], length `order`. Every entry
///   must fit in `coef_res2` bits (the parser already enforces this,
///   so a runtime overflow here means the caller fabricated an
///   in-memory [`crate::tns_data::TnsFilter`]).
///
/// The returned `Vec<f64>` has the same length as `coef` and contains
/// the §4.6.9.3 `tmp2[]` array (PARCOR coefficients in `[-1, 1]`).
///
/// Returns [`Error::TnsCoefOutOfRange`] on invalid `coef_res_bits`
/// (`!= 3 && != 4`), `coef_compress > 1`, or a `coef[i]` that does
/// not fit `coef_res2` bits.
pub fn tns_decode_coef(coef_res_bits: u32, coef_compress: u32, coef: &[u32]) -> Result<Vec<f64>> {
    if coef_compress > 1 {
        return Err(Error::TnsCoefOutOfRange);
    }
    let coef_res2 = coef_res_bits
        .checked_sub(coef_compress)
        .ok_or(Error::TnsCoefOutOfRange)?;
    let iq = iqfac(coef_res_bits)?;
    let iq_m = iqfac_m(coef_res_bits)?;

    let mut out = Vec::with_capacity(coef.len());
    for &c in coef {
        let signed = sign_extend_coef(c, coef_res2)?;
        let divisor = if signed >= 0 { iq } else { iq_m };
        out.push((signed as f64 / divisor).sin());
    }
    Ok(out)
}

/// Encoder-side inverse of [`tns_decode_coef`] per §C.6: quantise a
/// PARCOR reflection-coefficient array `r[]` into the wire `coef[]`
/// slots [`crate::tns_data::TnsFilter::coef`] consumes.
///
/// The quantisation rule is `index = NINT(arcsin(r) * iqfac_branch)`,
/// where the `iqfac_branch` selector is keyed on the *sign of `r`*
/// (not the index): non-negative `r` uses [`iqfac`], strictly
/// negative `r` uses [`iqfac_m`]. After rounding, the encoder clamps
/// `index` to the `coef_res2`-bit signed-magnitude range
/// `-(1 << (coef_res2-1))..=(1 << (coef_res2-1)) - 1` and folds it
/// through [`pack_coef`].
///
/// Returns [`Error::TnsCoefOutOfRange`] on invalid `coef_res_bits` /
/// `coef_compress`, or on a `|r| > 1.0` value (`arcsin` is undefined
/// outside `[-1, 1]`).
pub fn tns_encode_coef(coef_res_bits: u32, coef_compress: u32, r: &[f64]) -> Result<Vec<u32>> {
    if coef_compress > 1 {
        return Err(Error::TnsCoefOutOfRange);
    }
    let coef_res2 = coef_res_bits
        .checked_sub(coef_compress)
        .ok_or(Error::TnsCoefOutOfRange)?;
    let iq = iqfac(coef_res_bits)?;
    let iq_m = iqfac_m(coef_res_bits)?;
    let half = 1i32 << (coef_res2 - 1);
    let max_idx = half - 1;
    let min_idx = -half;

    let mut out = Vec::with_capacity(r.len());
    for &value in r {
        if !(-1.0..=1.0).contains(&value) {
            return Err(Error::TnsCoefOutOfRange);
        }
        let scale = if value >= 0.0 { iq } else { iq_m };
        // NINT = round-half-away-from-zero; f64::round matches this.
        let raw = (value.asin() * scale).round() as i32;
        let clamped = raw.clamp(min_idx, max_idx);
        out.push(pack_coef(clamped, coef_res2)?);
    }
    Ok(out)
}

/// §4.6.9.3 *conversion to LPC coefficients* — the "step-up procedure"
/// that converts an order-`N` PARCOR array `tmp2[]` (output of
/// [`tns_decode_coef`]) into the order-`N` direct-form LPC vector
/// `a[]` of length `N + 1` with `a[0] = 1.0`.
///
/// The loop is:
///
/// ```text
/// a[0] = 1
/// for (m = 1; m <= order; m++) {
///     for (i = 1; i < m; i++)
///         b[i] = a[i] + tmp2[m-1] * a[m-i];
///     for (i = 1; i < m; i++)
///         a[i] = b[i];
///     a[m] = tmp2[m-1];
/// }
/// ```
///
/// `parcor.len()` is the filter order; the returned vector has
/// `parcor.len() + 1` entries. An empty `parcor` slice produces the
/// degenerate `[1.0]` (no filtering — every filter with `order == 0`
/// is skipped by the §4.6.9.3 outer loop).
pub fn lpc_step_up(parcor: &[f64]) -> Vec<f64> {
    let order = parcor.len();
    // `a` is the running LPC coefficient array. Length is `order + 1`
    // throughout; only the first `m + 1` slots are meaningful at the
    // start of iteration `m` (the remainder is zeroed and overwritten
    // by later iterations).
    let mut a = vec![0.0_f64; order + 1];
    a[0] = 1.0;
    // Scratch `b[]` matches the spec's pseudocode literally. Allocated
    // once and reused across iterations; only the low `m` slots are
    // consulted per `m`.
    let mut b = vec![0.0_f64; order + 1];
    for m in 1..=order {
        let k = parcor[m - 1];
        for i in 1..m {
            b[i] = a[i] + k * a[m - i];
        }
        // Copy the m-1 newly-derived `b[1..m]` slots back into a;
        // clippy prefers `copy_from_slice` here over a manual loop.
        a[1..m].copy_from_slice(&b[1..m]);
        a[m] = k;
    }
    a
}

/// Convenience wrapper that runs [`tns_decode_coef`] then
/// [`lpc_step_up`] in one call.
///
/// Returns the `order + 1` LPC `a[]` vector (`a[0] = 1.0`) the
/// §4.6.9.3 `tns_ar_filter()` loop consumes.
pub fn tns_decode_coef_to_lpc(
    coef_res_bits: u32,
    coef_compress: u32,
    coef: &[u32],
) -> Result<Vec<f64>> {
    let parcor = tns_decode_coef(coef_res_bits, coef_compress, coef)?;
    Ok(lpc_step_up(&parcor))
}

/// §4.6.9.3 `tns_ar_filter()` — the simple all-pole (auto-regressive)
/// IIR filter that TNS slides across a strided region of the
/// dequantised MDCT spectrum, in place.
///
/// The §4.6.9.3 pseudocode defines the filter by the recurrence
///
/// ```text
/// y(n) = x(n) - lpc[1]*y(n-1) - ... - lpc[order]*y(n-order)
/// ```
///
/// with these spec-mandated properties:
///
/// * the filter state (`y(n-1) .. y(n-order)`) is **initialised to
///   zero** at every invocation;
/// * the output overwrites the input (**in-place operation**);
/// * `size` samples are processed, stepping to the next sample by the
///   index increment `inc` (`+1` upward, `−1` downward).
///
/// `lpc` is the direct-form `a[]` array produced by [`lpc_step_up`] /
/// [`tns_decode_coef_to_lpc`]: `lpc[0] == 1.0` and `lpc[1..=order]`
/// are the predictor taps. The filter order is `lpc.len() - 1`; a
/// `lpc` of length 1 (order 0) leaves the spectrum untouched.
///
/// `spectrum` is the full per-window coefficient buffer. `start` is
/// the index of the first sample to process — for an upward filter
/// (`inc = 1`) this is the §4.6.9.3 `start = swb_offset[bottom]`; for
/// a downward filter (`inc = -1`) the §4.6.9.3 `tns_decode_frame`
/// outer loop has already set `start = end - 1`, so the same `start`
/// argument is the top of the region and the walk proceeds toward
/// lower indices.
///
/// The recurrence is evaluated literally: because the output is
/// written over the input and the filter reads back its own previous
/// *outputs* (`y`), the per-tap history is a small ring of the last
/// `order` produced samples, seeded with zeros.
///
/// Returns [`Error::TnsCoefOutOfRange`] when:
///
/// * `lpc` is empty (no `a[0]`),
/// * `inc` is neither `+1` nor `-1`,
/// * the strided walk of `size` samples starting at `start` with step
///   `inc` would leave the bounds of `spectrum` (an out-of-range
///   `start`/`size`/`inc` triple the caller fabricated; the
///   §4.6.9.3 `size = end - start <= 0` guard and the `swb_offset`
///   clamping in `tns_decode_frame` keep legitimate callers in range).
pub fn tns_ar_filter(
    spectrum: &mut [f64],
    start: usize,
    size: usize,
    inc: i32,
    lpc: &[f64],
) -> Result<()> {
    if lpc.is_empty() {
        return Err(Error::TnsCoefOutOfRange);
    }
    if inc != 1 && inc != -1 {
        return Err(Error::TnsCoefOutOfRange);
    }
    let order = lpc.len() - 1;
    if size == 0 || order == 0 {
        // Nothing to shape: an order-0 filter (`lpc == [1.0]`) is the
        // identity, and a zero-length region is a no-op. Still
        // bounds-check the (degenerate) walk so a bad `start` is
        // rejected consistently.
        if size > 0 {
            walk_bounds_check(spectrum.len(), start, size, inc)?;
        }
        return Ok(());
    }

    walk_bounds_check(spectrum.len(), start, size, inc)?;

    // Filter-state ring: the last `order` *output* samples y(n-1) ..
    // y(n-order). Index `0` is the most recent output; the ring is
    // shifted by one each iteration. Seeded with zeros per §4.6.9.3.
    let mut history = vec![0.0_f64; order];

    let mut idx = start as isize;
    for _ in 0..size {
        let x = spectrum[idx as usize];
        // y(n) = x(n) - Σ_{k=1..order} lpc[k] * y(n-k)
        let mut y = x;
        for k in 1..=order {
            y -= lpc[k] * history[k - 1];
        }
        spectrum[idx as usize] = y;
        // Shift the history ring: y becomes the new y(n-1).
        for k in (1..order).rev() {
            history[k] = history[k - 1];
        }
        history[0] = y;
        idx += inc as isize;
    }
    Ok(())
}

/// §4.6.7.4.1 TNS **analysis** filter — the all-zero (moving-average,
/// FIR) inverse of the §4.6.9.3 [`tns_ar_filter`] all-pole synthesis
/// filter, applied in place over a strided region.
///
/// Figure 4.30 puts an additional TNS analysis filter in the LTP loop:
/// because TNS is applied to a *reconstructed* spectrum, the
/// LTP-predicted spectrum `X_est` has to be pushed through the same
/// noise-shaping the residual carries before it can be added to the
/// transmitted residual `Y_rec` (which sits in the pre-synthesis,
/// noise-shaped domain). That forward filter is the exact inverse of
/// the synthesis recurrence: where [`tns_ar_filter`] computes
///
/// ```text
/// y(n) = x(n) - Σ_{k=1..order} lpc[k] * y(n-k)      (all-pole)
/// ```
///
/// the analysis filter computes
///
/// ```text
/// y(n) = x(n) + Σ_{k=1..order} lpc[k] * x(n-k)      (all-zero)
/// ```
///
/// reading back its own *inputs* (`x`) rather than its outputs. Running
/// the analysis filter and then the synthesis filter over the same
/// region with the same `lpc` is the identity, which is the §4.6.7.4.1
/// requirement: the analysis step in the LTP loop is undone by the
/// §4.6.9 TNS synthesis step that follows the `X_est + Y_rec` add.
///
/// Argument and error semantics mirror [`tns_ar_filter`] exactly: the
/// filter state is seeded with zeros at every invocation, the output
/// overwrites the input in place, and `size` samples are processed
/// stepping by `inc ∈ {-1, +1}`. `lpc[0]` is the implicit `1.0`;
/// `lpc[1..=order]` are the predictor taps. An order-0 filter
/// (`lpc == [1.0]`) is the identity.
pub fn tns_ma_filter(
    spectrum: &mut [f64],
    start: usize,
    size: usize,
    inc: i32,
    lpc: &[f64],
) -> Result<()> {
    if lpc.is_empty() {
        return Err(Error::TnsCoefOutOfRange);
    }
    if inc != 1 && inc != -1 {
        return Err(Error::TnsCoefOutOfRange);
    }
    let order = lpc.len() - 1;
    if size == 0 || order == 0 {
        if size > 0 {
            walk_bounds_check(spectrum.len(), start, size, inc)?;
        }
        return Ok(());
    }

    walk_bounds_check(spectrum.len(), start, size, inc)?;

    // Filter-state ring: the last `order` *input* samples x(n-1) ..
    // x(n-order). Index `0` is the most recent input. Seeded with zeros,
    // matching the all-pole filter's zero-initialised state so the two
    // are mutual inverses over the region.
    let mut history = vec![0.0_f64; order];

    let mut idx = start as isize;
    for _ in 0..size {
        let x = spectrum[idx as usize];
        // y(n) = x(n) + Σ_{k=1..order} lpc[k] * x(n-k)
        let mut y = x;
        for k in 1..=order {
            y += lpc[k] * history[k - 1];
        }
        spectrum[idx as usize] = y;
        // Shift the history ring: x becomes the new x(n-1).
        for k in (1..order).rev() {
            history[k] = history[k - 1];
        }
        history[0] = x;
        idx += inc as isize;
    }
    Ok(())
}

/// Bounds-check the §4.6.9.3 strided walk: `size` samples starting at
/// `start`, stepping by `inc ∈ {-1, +1}`, must all land inside a
/// buffer of `len` elements. Returns [`Error::TnsCoefOutOfRange`]
/// otherwise.
fn walk_bounds_check(len: usize, start: usize, size: usize, inc: i32) -> Result<()> {
    if start >= len {
        return Err(Error::TnsCoefOutOfRange);
    }
    // Last visited index = start + (size-1)*inc. Validate it stays in
    // `0..len` without overflowing.
    let span = (size - 1) as isize;
    let last = start as isize + span * inc as isize;
    if last < 0 || last >= len as isize {
        return Err(Error::TnsCoefOutOfRange);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------- iqfac / iqfac_m ----------

    #[test]
    fn iqfac_matches_spec_formula_for_legal_widths() {
        // coef_res_bits = 3 (coef_res = 0): scale = 4 - 0.5 = 3.5
        let want3 = 3.5_f64 / HALF_PI;
        let want4 = 7.5_f64 / HALF_PI;
        assert!((iqfac(3).unwrap() - want3).abs() < 1e-15);
        assert!((iqfac(4).unwrap() - want4).abs() < 1e-15);
    }

    #[test]
    fn iqfac_m_matches_spec_formula_for_legal_widths() {
        let want3 = 4.5_f64 / HALF_PI;
        let want4 = 8.5_f64 / HALF_PI;
        assert!((iqfac_m(3).unwrap() - want3).abs() < 1e-15);
        assert!((iqfac_m(4).unwrap() - want4).abs() < 1e-15);
    }

    #[test]
    fn iqfac_rejects_widths_outside_3_to_4() {
        assert!(matches!(iqfac(0), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(iqfac(2), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(iqfac(5), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(iqfac_m(0), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(iqfac_m(2), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(iqfac_m(5), Err(Error::TnsCoefOutOfRange)));
    }

    #[test]
    fn iqfac_m_is_always_greater_than_iqfac() {
        // The +0.5 vs -0.5 offset guarantees `iqfac_m > iqfac` for
        // every coef_res_bits — this is what biases the round-to-zero
        // of negative reflection coefficients toward the next-larger
        // magnitude (so they don't underflow toward zero).
        for n in [3, 4] {
            assert!(iqfac_m(n).unwrap() > iqfac(n).unwrap());
        }
    }

    // ---------- sign extension ----------

    #[test]
    fn sign_extend_4bit_covers_signed_range() {
        // coef_res2 = 4 ⇒ signed range -8..=7. Walk every wire pattern.
        let expected: [i32; 16] = [0, 1, 2, 3, 4, 5, 6, 7, -8, -7, -6, -5, -4, -3, -2, -1];
        for wire in 0_u32..16 {
            assert_eq!(
                sign_extend_coef(wire, 4).unwrap(),
                expected[wire as usize],
                "wire {wire:04b}",
            );
        }
    }

    #[test]
    fn sign_extend_3bit_covers_signed_range() {
        // coef_res2 = 3 ⇒ signed range -4..=3.
        let expected: [i32; 8] = [0, 1, 2, 3, -4, -3, -2, -1];
        for wire in 0_u32..8 {
            assert_eq!(sign_extend_coef(wire, 3).unwrap(), expected[wire as usize]);
        }
    }

    #[test]
    fn sign_extend_2bit_covers_signed_range() {
        // coef_res2 = 2 ⇒ signed range -2..=1.
        let expected: [i32; 4] = [0, 1, -2, -1];
        for wire in 0_u32..4 {
            assert_eq!(sign_extend_coef(wire, 2).unwrap(), expected[wire as usize]);
        }
    }

    #[test]
    fn sign_extend_rejects_out_of_range_field_width() {
        assert!(matches!(
            sign_extend_coef(0, 1),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            sign_extend_coef(0, 5),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn sign_extend_rejects_wire_value_that_overflows_field() {
        // 4-bit field: a wire value of 16 (0b10000) doesn't fit.
        assert!(matches!(
            sign_extend_coef(16, 4),
            Err(Error::TnsCoefOutOfRange)
        ));
        // 2-bit field: 4 doesn't fit.
        assert!(matches!(
            sign_extend_coef(4, 2),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    // ---------- pack_coef (encoder-side) ----------

    #[test]
    fn pack_coef_round_trips_through_sign_extend_4bit() {
        for value in -8_i32..=7 {
            let packed = pack_coef(value, 4).unwrap();
            assert_eq!(sign_extend_coef(packed, 4).unwrap(), value);
        }
    }

    #[test]
    fn pack_coef_round_trips_through_sign_extend_3bit() {
        for value in -4_i32..=3 {
            let packed = pack_coef(value, 3).unwrap();
            assert_eq!(sign_extend_coef(packed, 3).unwrap(), value);
        }
    }

    #[test]
    fn pack_coef_round_trips_through_sign_extend_2bit() {
        for value in -2_i32..=1 {
            let packed = pack_coef(value, 2).unwrap();
            assert_eq!(sign_extend_coef(packed, 2).unwrap(), value);
        }
    }

    #[test]
    fn pack_coef_rejects_out_of_field_value() {
        // 4-bit signed range is -8..=7; 8 and -9 reject.
        assert!(matches!(pack_coef(8, 4), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(pack_coef(-9, 4), Err(Error::TnsCoefOutOfRange)));
        // 2-bit signed range is -2..=1; 2 and -3 reject.
        assert!(matches!(pack_coef(2, 2), Err(Error::TnsCoefOutOfRange)));
        assert!(matches!(pack_coef(-3, 2), Err(Error::TnsCoefOutOfRange)));
    }

    // ---------- tns_decode_coef ----------

    #[test]
    fn decode_zero_wire_yields_zero_parcor() {
        let parcor = tns_decode_coef(4, 0, &[0, 0, 0]).unwrap();
        assert_eq!(parcor.len(), 3);
        for v in parcor {
            assert!(v.abs() < 1e-15);
        }
    }

    #[test]
    fn decode_field_extrema_yield_near_unity_magnitudes() {
        // coef_res_bits=4, coef_compress=0 ⇒ coef_res2=4 ⇒ signed range
        // -8..=7. The extreme positive index is 7; it should decode to
        // sin(7 / iqfac) = sin(7 / (7.5 / (π/2))) ≈ sin(0.4666... · π/2).
        // The extreme negative index is -8 ⇒ sin(-8 / iqfac_m).
        let pos = tns_decode_coef(4, 0, &[7]).unwrap()[0];
        let neg = tns_decode_coef(4, 0, &[8]).unwrap()[0]; // wire 8 = -8 after sign-extend
        let want_pos = (7.0_f64 / (7.5 / HALF_PI)).sin();
        let want_neg = (-8.0_f64 / (8.5 / HALF_PI)).sin();
        assert!((pos - want_pos).abs() < 1e-15);
        assert!((neg - want_neg).abs() < 1e-15);
        // Both magnitudes are in [-1, 1] — PARCOR coefficient validity.
        assert!(pos.abs() <= 1.0);
        assert!(neg.abs() <= 1.0);
    }

    #[test]
    fn decode_negative_branch_uses_iqfac_m() {
        // Wire value 0xF in a 4-bit field sign-extends to -1.
        // Decoded value must use iqfac_m (not iqfac): sin(-1 / iqfac_m).
        let got = tns_decode_coef(4, 0, &[0xF]).unwrap()[0];
        let want = (-1.0_f64 / iqfac_m(4).unwrap()).sin();
        assert!((got - want).abs() < 1e-15);
    }

    #[test]
    fn decode_3bit_branch_uses_coef_res_bits_3() {
        // coef_res_bits = 3 always — coef_compress doesn't change the
        // iqfac arithmetic (only coef_res2 changes for sign extension).
        let got_long = tns_decode_coef(3, 0, &[1]).unwrap()[0];
        let want_long = (1.0_f64 / iqfac(3).unwrap()).sin();
        assert!((got_long - want_long).abs() < 1e-15);
        // coef_compress = 1 ⇒ coef_res2 = 2; signed range -2..=1.
        // Wire 1 ⇒ +1 after sign-extend, then sin(1 / iqfac(3)).
        let got_short = tns_decode_coef(3, 1, &[1]).unwrap()[0];
        assert!((got_short - want_long).abs() < 1e-15);
    }

    #[test]
    fn decode_rejects_oversized_wire_value_for_compress_path() {
        // coef_res_bits=4, coef_compress=1 ⇒ coef_res2=3 (signed -4..=3);
        // wire 8 (0b1000) does not fit a 3-bit field.
        assert!(matches!(
            tns_decode_coef(4, 1, &[8]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn decode_rejects_invalid_coef_res_bits() {
        assert!(matches!(
            tns_decode_coef(5, 0, &[0]),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            tns_decode_coef(2, 0, &[0]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn decode_rejects_invalid_coef_compress() {
        assert!(matches!(
            tns_decode_coef(4, 2, &[0]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn decode_empty_input_yields_empty_output() {
        let parcor = tns_decode_coef(4, 0, &[]).unwrap();
        assert!(parcor.is_empty());
    }

    // ---------- tns_encode_coef ----------

    #[test]
    fn encode_zero_parcor_yields_zero_wire() {
        let wire = tns_encode_coef(4, 0, &[0.0, 0.0, 0.0]).unwrap();
        assert_eq!(wire, vec![0, 0, 0]);
    }

    #[test]
    fn encode_unity_parcor_saturates_to_field_max() {
        // r = 1.0 ⇒ arcsin = π/2; index = round(π/2 * iqfac(4)) =
        // round(π/2 * (7.5 / (π/2))) = round(7.5) = 8, clamped to 7
        // (the 4-bit signed field maximum). Sign-extends back to +7.
        let wire = tns_encode_coef(4, 0, &[1.0]).unwrap();
        assert_eq!(wire, vec![7]);
        // r = -1.0 ⇒ arcsin = -π/2; index = round(-π/2 * iqfac_m(4)) =
        // round(-π/2 * (8.5 / (π/2))) = round(-8.5) = -9, clamped to
        // -8 (the 4-bit signed field minimum). Wire pattern is
        // 0b1000 = 8.
        let wire_neg = tns_encode_coef(4, 0, &[-1.0]).unwrap();
        assert_eq!(wire_neg, vec![8]);
    }

    #[test]
    fn encode_rejects_parcor_outside_minus_one_to_plus_one() {
        assert!(matches!(
            tns_encode_coef(4, 0, &[1.0001]),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            tns_encode_coef(4, 0, &[-1.0001]),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            tns_encode_coef(4, 0, &[f64::NAN]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn encode_rejects_invalid_coef_res_bits() {
        assert!(matches!(
            tns_encode_coef(5, 0, &[0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    // ---------- round-trip ----------

    #[test]
    fn round_trip_every_4bit_wire_value_through_decode_then_encode() {
        // For every 4-bit wire input, decode to PARCOR then re-encode
        // and confirm we land on the same wire pattern. This is the
        // fundamental invariant that §C.6 NINT(arcsin(sin(x))) returns
        // the input integer when the magnitude is in-range.
        for wire in 0_u32..16 {
            let parcor = tns_decode_coef(4, 0, &[wire]).unwrap();
            let back = tns_encode_coef(4, 0, &parcor).unwrap();
            assert_eq!(back, vec![wire], "wire {wire:04b} round-trip");
        }
    }

    #[test]
    fn round_trip_every_3bit_wire_value_through_decode_then_encode() {
        for wire in 0_u32..8 {
            let parcor = tns_decode_coef(3, 0, &[wire]).unwrap();
            let back = tns_encode_coef(3, 0, &parcor).unwrap();
            assert_eq!(back, vec![wire], "wire {wire:03b} round-trip");
        }
    }

    #[test]
    fn round_trip_with_coef_compress_for_both_res_settings() {
        // coef_res_bits = 4, coef_compress = 1 ⇒ coef_res2 = 3.
        // Sign-extension uses the 3-bit field but iqfac/iqfac_m use
        // coef_res_bits = 4. Confirm round-trip lands on the same
        // 3-bit wire pattern.
        for wire in 0_u32..8 {
            let parcor = tns_decode_coef(4, 1, &[wire]).unwrap();
            let back = tns_encode_coef(4, 1, &parcor).unwrap();
            assert_eq!(back, vec![wire], "coef_res=1 compress=1 wire {wire:03b}");
        }
        // coef_res_bits = 3, coef_compress = 1 ⇒ coef_res2 = 2.
        for wire in 0_u32..4 {
            let parcor = tns_decode_coef(3, 1, &[wire]).unwrap();
            let back = tns_encode_coef(3, 1, &parcor).unwrap();
            assert_eq!(back, vec![wire], "coef_res=0 compress=1 wire {wire:02b}");
        }
    }

    // ---------- lpc_step_up ----------

    #[test]
    fn step_up_zero_order_returns_unit_a() {
        let a = lpc_step_up(&[]);
        assert_eq!(a, vec![1.0]);
    }

    #[test]
    fn step_up_first_order_matches_hand_arithmetic() {
        // order = 1: a[0] = 1, a[1] = k. No inner-loop iterations.
        let a = lpc_step_up(&[0.5]);
        assert_eq!(a, vec![1.0, 0.5]);
    }

    #[test]
    fn step_up_second_order_matches_hand_arithmetic() {
        // order = 2 with parcor [k1, k2]:
        //   m=1: a = [1, k1]
        //   m=2: b[1] = a[1] + k2 * a[1] = k1 * (1 + k2)
        //        a[1] = b[1]; a[2] = k2
        // ⇒ a = [1, k1*(1 + k2), k2]
        let (k1, k2) = (0.3, 0.4);
        let a = lpc_step_up(&[k1, k2]);
        assert_eq!(a.len(), 3);
        assert!((a[0] - 1.0).abs() < 1e-15);
        assert!((a[1] - k1 * (1.0 + k2)).abs() < 1e-15);
        assert!((a[2] - k2).abs() < 1e-15);
    }

    #[test]
    fn step_up_third_order_matches_hand_arithmetic() {
        // order = 3:
        //   m=1: a = [1, k1, 0, 0]
        //   m=2: a = [1, k1*(1+k2), k2, 0]
        //   m=3: b[1] = a[1] + k3 * a[2] = k1*(1+k2) + k3*k2
        //        b[2] = a[2] + k3 * a[1] = k2 + k3*k1*(1+k2)
        //        a[3] = k3
        let (k1, k2, k3) = (0.2, 0.3, -0.4);
        let a = lpc_step_up(&[k1, k2, k3]);
        let want = [
            1.0,
            k1 * (1.0 + k2) + k3 * k2,
            k2 + k3 * k1 * (1.0 + k2),
            k3,
        ];
        for i in 0..4 {
            assert!(
                (a[i] - want[i]).abs() < 1e-15,
                "i={i} got {} want {}",
                a[i],
                want[i],
            );
        }
    }

    #[test]
    fn step_up_a0_always_one() {
        // a[0] = 1 for every PARCOR sequence — invariant of the
        // step-up loop init.
        for parcor in [
            vec![0.5],
            vec![-0.5],
            vec![0.1, -0.2],
            vec![0.3, -0.4, 0.5, -0.6, 0.7, -0.8, 0.9, -0.95],
        ] {
            let a = lpc_step_up(&parcor);
            assert_eq!(a.len(), parcor.len() + 1);
            assert!((a[0] - 1.0).abs() < 1e-15);
        }
    }

    #[test]
    fn step_up_last_coefficient_is_last_parcor() {
        // The §4.6.9.3 loop's final iteration sets a[m] = tmp2[m-1] at
        // m = order. So a[order] must equal parcor[order-1] for every
        // order. (The intermediate a[i] entries pick up the cross
        // terms.)
        for parcor in [vec![0.3], vec![0.3, -0.5], vec![0.1, 0.2, 0.3, 0.4]] {
            let a = lpc_step_up(&parcor);
            let last_idx = parcor.len();
            assert_eq!(a[last_idx], *parcor.last().unwrap());
        }
    }

    // ---------- combined wrapper ----------

    #[test]
    fn decode_to_lpc_combines_decode_and_step_up() {
        let wire = [3, 5, 0xF]; // mixed positive / negative
        let parcor = tns_decode_coef(4, 0, &wire).unwrap();
        let want = lpc_step_up(&parcor);
        let got = tns_decode_coef_to_lpc(4, 0, &wire).unwrap();
        assert_eq!(got, want);
    }

    #[test]
    fn decode_to_lpc_propagates_decode_errors() {
        assert!(matches!(
            tns_decode_coef_to_lpc(5, 0, &[0]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    // ---------- tns_ar_filter ----------

    /// Reference implementation of the §4.6.9.3 recurrence written the
    /// straightforward (non-ring-buffer) way, for cross-checking the
    /// production `tns_ar_filter`. Operates on a contiguous copy.
    fn ref_ar_filter(x: &[f64], lpc: &[f64]) -> Vec<f64> {
        let order = lpc.len() - 1;
        let mut y = vec![0.0_f64; x.len()];
        for n in 0..x.len() {
            let mut acc = x[n];
            for k in 1..=order {
                if n >= k {
                    acc -= lpc[k] * y[n - k];
                }
            }
            y[n] = acc;
        }
        y
    }

    #[test]
    fn ar_filter_order0_is_identity() {
        let mut spec = [1.0, 2.0, 3.0, 4.0];
        let before = spec;
        // lpc = [1.0] ⇒ order 0.
        tns_ar_filter(&mut spec, 0, 4, 1, &[1.0]).unwrap();
        assert_eq!(spec, before);
    }

    #[test]
    fn ar_filter_order1_matches_recurrence_upward() {
        // y(n) = x(n) - lpc[1]*y(n-1).
        let lpc = [1.0, 0.5];
        let x = [1.0, 0.0, 0.0, 0.0, 0.0];
        let want = ref_ar_filter(&x, &lpc);
        let mut spec = x;
        tns_ar_filter(&mut spec, 0, 5, 1, &lpc).unwrap();
        for (g, w) in spec.iter().zip(want.iter()) {
            assert!((g - w).abs() < 1e-12, "got {g} want {w}");
        }
        // Hand-check: unit impulse through y(n)+0.5 y(n-1) = x gives
        // y = 1, -0.5, 0.25, -0.125, 0.0625.
        let hand = [1.0, -0.5, 0.25, -0.125, 0.0625];
        for (g, h) in spec.iter().zip(hand.iter()) {
            assert!((g - h).abs() < 1e-12);
        }
    }

    #[test]
    fn ar_filter_order3_matches_reference() {
        let lpc = [1.0, -0.4, 0.2, 0.1];
        let x = [0.7, -1.3, 2.1, 0.0, -0.5, 1.1, 0.9, -0.2];
        let want = ref_ar_filter(&x, &lpc);
        let mut spec = x;
        tns_ar_filter(&mut spec, 0, x.len(), 1, &lpc).unwrap();
        for (g, w) in spec.iter().zip(want.iter()) {
            assert!((g - w).abs() < 1e-12, "got {g} want {w}");
        }
    }

    #[test]
    fn ar_filter_downward_walks_high_to_low() {
        // direction = 1 ⇒ inc = -1, start = end - 1. The §4.6.9.3
        // filter then processes the region top-to-bottom. Cross-check
        // by reversing the region, filtering forward, and reversing
        // back.
        let lpc = [1.0, 0.3, -0.15];
        let region = [0.5, -0.2, 0.9, 1.4, -0.7];
        // Place region inside a larger buffer with sentinel padding to
        // confirm only the targeted span is touched.
        let mut spec = vec![100.0, 0.5, -0.2, 0.9, 1.4, -0.7, 200.0];
        let start = 5; // end-1, where end = 6 (one past last region idx)
        let size = 5;
        tns_ar_filter(&mut spec, start, size, -1, &lpc).unwrap();

        // Reference: process region in reverse order (high→low).
        let mut rev: Vec<f64> = region.iter().rev().copied().collect();
        let want_rev = ref_ar_filter(&rev, &lpc);
        rev.copy_from_slice(&want_rev);
        let want: Vec<f64> = rev.into_iter().rev().collect();

        assert_eq!(spec[0], 100.0, "lower sentinel untouched");
        assert_eq!(spec[6], 200.0, "upper sentinel untouched");
        for (i, w) in want.iter().enumerate() {
            assert!(
                (spec[1 + i] - w).abs() < 1e-12,
                "idx {i}: {} vs {w}",
                spec[1 + i]
            );
        }
    }

    #[test]
    fn ar_filter_only_touches_targeted_region() {
        let lpc = [1.0, 0.5];
        let mut spec = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        // Filter only indices 2..=4 (size 3, upward).
        tns_ar_filter(&mut spec, 2, 3, 1, &lpc).unwrap();
        assert_eq!(spec[0], 1.0);
        assert_eq!(spec[1], 2.0);
        assert_eq!(spec[5], 6.0);
        // Region recomputed independently.
        let want = ref_ar_filter(&[3.0, 4.0, 5.0], &lpc);
        for i in 0..3 {
            assert!((spec[2 + i] - want[i]).abs() < 1e-12);
        }
    }

    #[test]
    fn ar_filter_zero_size_is_noop() {
        let mut spec = [1.0, 2.0, 3.0];
        let before = spec;
        tns_ar_filter(&mut spec, 0, 0, 1, &[1.0, 0.5]).unwrap();
        assert_eq!(spec, before);
    }

    #[test]
    fn ar_filter_rejects_empty_lpc() {
        let mut spec = [1.0, 2.0];
        assert!(matches!(
            tns_ar_filter(&mut spec, 0, 2, 1, &[]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn ar_filter_rejects_bad_inc() {
        let mut spec = [1.0, 2.0];
        assert!(matches!(
            tns_ar_filter(&mut spec, 0, 2, 0, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            tns_ar_filter(&mut spec, 0, 2, 2, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn ar_filter_rejects_out_of_bounds_walk() {
        let mut spec = [1.0, 2.0, 3.0];
        // start in range but size overruns the top.
        assert!(matches!(
            tns_ar_filter(&mut spec, 1, 5, 1, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
        // downward walk underruns below 0.
        assert!(matches!(
            tns_ar_filter(&mut spec, 1, 3, -1, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
        // start past the end.
        assert!(matches!(
            tns_ar_filter(&mut spec, 3, 1, 1, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    #[test]
    fn ar_filter_end_to_end_from_wire_coef() {
        // Decode a wire TNS filter to LPC, then shape a spectrum.
        // Confirms the lpc_step_up output drives tns_ar_filter without
        // any glue. coef_res_bits = 4, coef_compress = 0, order 2.
        let wire = [3_u32, 0xE]; // one positive, one negative reflection
        let lpc = tns_decode_coef_to_lpc(4, 0, &wire).unwrap();
        assert_eq!(lpc.len(), 3);
        assert_eq!(lpc[0], 1.0);
        let x = [0.3, -0.9, 1.2, 0.4, -0.6, 0.1];
        let want = ref_ar_filter(&x, &lpc);
        let mut spec = x;
        tns_ar_filter(&mut spec, 0, x.len(), 1, &lpc).unwrap();
        for (g, w) in spec.iter().zip(want.iter()) {
            assert!((g - w).abs() < 1e-12);
        }
    }

    // ---------- tns_ma_filter (analysis / all-zero) ----------

    /// Reference all-zero (analysis) filter for an upward, in-order
    /// region: y(n) = x(n) + Σ lpc[k]·x(n-k), zero-seeded history.
    fn ref_ma_filter(x: &[f64], lpc: &[f64]) -> Vec<f64> {
        let order = lpc.len() - 1;
        let mut y = vec![0.0; x.len()];
        for n in 0..x.len() {
            let mut acc = x[n];
            for k in 1..=order {
                if n >= k {
                    acc += lpc[k] * x[n - k];
                }
            }
            y[n] = acc;
        }
        y
    }

    #[test]
    fn ma_filter_order_zero_is_identity() {
        let mut spec = [0.3, -0.9, 1.2, 0.4];
        let before = spec;
        let n = spec.len();
        tns_ma_filter(&mut spec, 0, n, 1, &[1.0]).unwrap();
        assert_eq!(spec, before);
    }

    #[test]
    fn ma_filter_matches_reference_upward() {
        let lpc = [1.0, 0.5, -0.25];
        let x = [0.3, -0.9, 1.2, 0.4, -0.6, 0.1];
        let want = ref_ma_filter(&x, &lpc);
        let mut spec = x;
        tns_ma_filter(&mut spec, 0, x.len(), 1, &lpc).unwrap();
        for (g, w) in spec.iter().zip(want.iter()) {
            assert!((g - w).abs() < 1e-12, "got {g} want {w}");
        }
    }

    #[test]
    fn ma_then_ar_is_identity() {
        // §4.6.7.4.1: the analysis filter followed by the synthesis
        // filter (same region, same lpc) reconstructs the input exactly.
        let lpc = tns_decode_coef_to_lpc(4, 0, &[3, 0xE]).unwrap();
        let x = [0.7, -0.2, 1.1, -1.3, 0.05, 0.9, -0.4];
        let mut spec = x;
        tns_ma_filter(&mut spec, 0, x.len(), 1, &lpc).unwrap();
        tns_ar_filter(&mut spec, 0, x.len(), 1, &lpc).unwrap();
        for (g, w) in spec.iter().zip(x.iter()) {
            assert!((g - w).abs() < 1e-12, "ma∘ar not identity: {g} vs {w}");
        }
    }

    #[test]
    fn ma_then_ar_is_identity_downward() {
        // Same inverse relationship for the downward (direction=1) walk.
        let lpc = [1.0, -0.4, 0.2];
        let x = [0.7, -0.2, 1.1, -1.3, 0.05];
        let mut spec = x;
        let end = x.len();
        tns_ma_filter(&mut spec, end - 1, end, -1, &lpc).unwrap();
        tns_ar_filter(&mut spec, end - 1, end, -1, &lpc).unwrap();
        for (g, w) in spec.iter().zip(x.iter()) {
            assert!((g - w).abs() < 1e-12);
        }
    }

    #[test]
    fn ma_filter_rejects_bad_args() {
        let mut spec = [1.0, 2.0, 3.0];
        assert!(matches!(
            tns_ma_filter(&mut spec, 0, 1, 2, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            tns_ma_filter(&mut spec, 1, 5, 1, &[1.0, 0.5]),
            Err(Error::TnsCoefOutOfRange)
        ));
        assert!(matches!(
            tns_ma_filter(&mut spec, 0, 1, 1, &[]),
            Err(Error::TnsCoefOutOfRange)
        ));
    }
}
