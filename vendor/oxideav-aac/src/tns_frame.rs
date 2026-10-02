//! §4.6.9.3 `tns_decode_frame()` — per-frame Temporal Noise Shaping
//! orchestration.
//!
//! This module chains the three TNS building blocks that previous
//! rounds landed into the spec's outer per-frame loop:
//!
//! * [`crate::tns_data`] — the Table 4.54 wire parser that yields the
//!   per-window `n_filt` / `coef_res` and per-filter `length` /
//!   `order` / `direction` / `coef_compress` / `coef[]` fields;
//! * [`crate::tns_coef::tns_decode_coef_to_lpc`] — the §4.6.9.3
//!   `tns_decode_coef()` inverse-quantisation + conversion-to-LPC
//!   step-up;
//! * [`crate::tns_coef::tns_ar_filter`] — the §4.6.9.3
//!   `tns_ar_filter()` all-pole IIR pass over a strided spectral
//!   region.
//!
//! The orchestration follows the §4.6.9.3 pseudocode:
//!
//! ```text
//! tns_decode_frame()
//! {
//!     for (w = 0; w < num_windows; w++) {
//!         bottom = num_swb;
//!         for (f = 0; f < n_filt[w]; f++) {
//!             top = bottom;
//!             bottom = max( top - length[w][f], 0 );
//!             tns_order = min( order[w][f], TNS_MAX_ORDER );
//!             if (!tns_order) continue;
//!             tns_decode_coef( tns_order, coef_res[w]+3,
//!                              coef_compress[w][f], coef[w][f], lpc[] );
//!             start = swb_offset[min(bottom, TNS_MAX_BANDS, max_sfb)];
//!             end   = swb_offset[min(top,    TNS_MAX_BANDS, max_sfb)];
//!             if ((size = end - start) <= 0) continue;
//!             if (direction[w][f]) { inc = -1; start = end - 1; }
//!             else                 { inc =  1; }
//!             tns_ar_filter( &spec[w][start], size, inc, lpc[], tns_order );
//!         }
//!     }
//! }
//! ```
//!
//! Filter regions are sliced top-down: the first transmitted filter
//! covers the topmost `length[w][0]` scalefactor bands (counting down
//! from `num_swb`), the next filter covers the `length[w][1]` bands
//! immediately below, and so on, with `bottom` clamped at band 0. The
//! band → coefficient-index mapping goes through the
//! [`crate::swb_offset`] tables, with each lookup index clamped by the
//! three-way `min(band, TNS_MAX_BANDS, max_sfb)`
//! ([`crate::tns_max::clamp_tns_band`]); the filter order is clamped
//! by `TNS_MAX_ORDER` ([`crate::tns_max::clamp_tns_order`]). Both
//! caps are object-type-dependent (Tables 4.102 / 4.103).
//!
//! Scope: the canonical 1024-line long / 8 × 128-line short frames
//! that the [`crate::swb_offset`] tables cover. The ER AAC LD
//! 480/512-line frames (Tables 4.119 / 4.120 band caps, dedicated
//! `swb_offset` tables) remain deferred until the LD reconstruction
//! path is wired, matching the standing `int_tns_decode_coef()`
//! deferral.

use crate::ics_info::WindowSequence;
use crate::swb_offset::{
    long_window_offsets, short_window_offsets, LONG_WINDOW_LEN, SHORT_WINDOW_LEN,
};
use crate::tns_coef::{tns_ar_filter, tns_decode_coef_to_lpc, tns_ma_filter};
use crate::tns_data::{num_windows, TnsData};
use crate::tns_max::{clamp_tns_band, clamp_tns_order};
use crate::{Error, Result};

/// Apply §4.6.9.3 `tns_decode_frame()` to one channel's dequantised
/// spectrum, in place.
///
/// ## Inputs
///
/// * `spec` — the channel's full-frame coefficient buffer, windows
///   concatenated in order: `num_windows × window_len` samples, i.e.
///   `8 × 128 = 1024` for `EIGHT_SHORT_SEQUENCE` and `1 × 1024`
///   otherwise. Window `w` occupies
///   `spec[w * window_len .. (w + 1) * window_len]` (the pseudocode's
///   `spec[w][..]`).
/// * `tns` — the parsed [`TnsData`] block for this channel. Its
///   window count must match `window_sequence` (which
///   [`TnsData::parse`] guarantees when called under the same
///   sequence).
/// * `window_sequence` — the surrounding `ics_info()` window
///   sequence; selects `num_windows`, `window_len`, the
///   [`crate::swb_offset`] table, and the short/long columns of
///   Tables 4.102 / 4.103.
/// * `max_sfb` — the surrounding `ics_info()` field; third operand of
///   the §4.6.9.3 band clamp.
/// * `aot` — `audioObjectType` (Table 1.17); selects the
///   `TNS_MAX_ORDER` row and the PQF / non-PQF `TNS_MAX_BANDS`
///   columns.
/// * `fs_index` — `samplingFrequencyIndex` (Table 1.18, `0..=11`);
///   selects the `swb_offset` table and the `TNS_MAX_BANDS` row.
///
/// ## Errors
///
/// * [`Error::TnsFrameInvalid`] — `spec.len()` is not
///   `num_windows × window_len`; `tns.windows.len()` disagrees with
///   `window_sequence`; or a filter's `coef` vector is shorter than
///   its `TNS_MAX_ORDER`-clamped `tns_order` (a fabricated
///   structure — the wire parser always emits `coef.len() == order`).
/// * [`Error::IcsInfoUnsupportedSampleRateIndex`] — `fs_index` has no
///   `swb_offset` / `TNS_MAX_BANDS` entry (`>= 12`).
/// * [`Error::TnsCoefOutOfRange`] — a wire `coef[i]` magnitude does
///   not fit `coef_res2 = coef_res_bits − coef_compress` bits
///   (propagated from [`tns_decode_coef_to_lpc`]).
///
/// An order-0 filter and an empty (clamped-away) region are
/// well-defined no-ops per the pseudocode's `continue` arms; a
/// `TnsData` with no filters at all leaves `spec` untouched.
pub fn tns_decode_frame(
    spec: &mut [f64],
    tns: &TnsData,
    window_sequence: WindowSequence,
    max_sfb: u8,
    aot: u8,
    fs_index: u8,
) -> Result<()> {
    tns_frame_filter(
        spec,
        tns,
        window_sequence,
        max_sfb,
        aot,
        fs_index,
        TnsFilterKind::Synthesis,
    )
}

/// §4.6.7.4.1 TNS **analysis** pass — the same per-window / per-filter
/// region walk as [`tns_decode_frame`], but applying the all-zero
/// [`tns_ma_filter`] (the inverse of the §4.6.9.3 all-pole synthesis
/// filter) instead.
///
/// Figure 4.30 requires this forward filter inside the LTP loop: the
/// LTP-predicted spectrum `X_est = MDCT(x_est)` must be moved into the
/// noise-shaped residual domain (the domain the transmitted `Y_rec`
/// lives in, *before* TNS synthesis) so that `X_rec = X_est + Y_rec`
/// adds like-for-like. The subsequent §4.6.9 TNS synthesis pass over
/// `X_rec` then undoes the analysis on the LTP contribution while
/// shaping the residual, exactly as the all-pole filter inverts the
/// all-zero one over a shared region.
///
/// Inputs, scope and errors mirror [`tns_decode_frame`]; the only
/// difference is the filter polarity. When `tns` carries no filters
/// (or only order-0 / empty-region filters) the spectrum is untouched,
/// so a channel without TNS needs no analysis pass.
pub fn tns_analysis_frame(
    spec: &mut [f64],
    tns: &TnsData,
    window_sequence: WindowSequence,
    max_sfb: u8,
    aot: u8,
    fs_index: u8,
) -> Result<()> {
    tns_frame_filter(
        spec,
        tns,
        window_sequence,
        max_sfb,
        aot,
        fs_index,
        TnsFilterKind::Analysis,
    )
}

/// Which TNS filter polarity [`tns_frame_filter`] applies over each
/// region: the §4.6.9.3 all-pole synthesis filter (the normal decode
/// path) or the §4.6.7.4.1 all-zero analysis filter (the LTP loop).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TnsFilterKind {
    /// All-pole [`tns_ar_filter`] — §4.6.9.3 decode.
    Synthesis,
    /// All-zero [`tns_ma_filter`] — §4.6.7.4.1 LTP-loop analysis.
    Analysis,
}

/// Shared §4.6.9.3 region walk for both TNS polarities. Identical band
/// clamping, coefficient decode and region selection; only the final
/// per-region filter call differs (`kind`).
fn tns_frame_filter(
    spec: &mut [f64],
    tns: &TnsData,
    window_sequence: WindowSequence,
    max_sfb: u8,
    aot: u8,
    fs_index: u8,
    kind: TnsFilterKind,
) -> Result<()> {
    let windows = num_windows(window_sequence);
    let (window_len, offsets) = if window_sequence.is_eight_short() {
        (SHORT_WINDOW_LEN as usize, short_window_offsets(fs_index)?)
    } else {
        (LONG_WINDOW_LEN as usize, long_window_offsets(fs_index)?)
    };

    if tns.windows.len() != windows {
        return Err(Error::TnsFrameInvalid);
    }
    if spec.len() != windows * window_len {
        return Err(Error::TnsFrameInvalid);
    }

    // `num_swb + 1` entries per swb_offset table; the top band index
    // (the pseudocode's initial `bottom = num_swb`) is the sentinel
    // slot, so every clamped lookup below stays in bounds.
    let num_swb = offsets.len() - 1;

    for (w, tns_window) in tns.windows.iter().enumerate() {
        let coef_res_bits = 3 + u32::from(tns_window.coef_res);
        let window_spec = &mut spec[w * window_len..(w + 1) * window_len];

        let mut bottom = num_swb;
        for filter in &tns_window.filters {
            let top = bottom;
            bottom = top.saturating_sub(filter.length as usize);

            let tns_order = clamp_tns_order(filter.order, aot, window_sequence, fs_index)? as usize;
            if tns_order == 0 {
                continue;
            }
            if filter.coef.len() < tns_order {
                return Err(Error::TnsFrameInvalid);
            }

            // tns_decode_coef( tns_order, coef_res[w]+3,
            //                  coef_compress[w][f], coef[w][f], lpc[] )
            // — only the first `tns_order` transmitted magnitudes
            // participate when the wire `order` exceeded the cap.
            let coef: Vec<u32> = filter.coef[..tns_order]
                .iter()
                .map(|&c| u32::from(c))
                .collect();
            let lpc =
                tns_decode_coef_to_lpc(coef_res_bits, u32::from(filter.coef_compress), &coef)?;

            // Band indices are at most `num_swb` (bottom/top start
            // there and only decrease), so the u8 narrowing is exact:
            // every standard table has num_swb <= 51.
            let start_band = clamp_tns_band(bottom as u8, max_sfb, aot, window_sequence, fs_index)?;
            let end_band = clamp_tns_band(top as u8, max_sfb, aot, window_sequence, fs_index)?;
            let start = offsets[start_band as usize] as usize;
            let end = offsets[end_band as usize] as usize;
            if end <= start {
                continue;
            }
            let size = end - start;

            let (filter_start, inc) = if filter.direction {
                (end - 1, -1)
            } else {
                (start, 1)
            };
            match kind {
                TnsFilterKind::Synthesis => {
                    tns_ar_filter(window_spec, filter_start, size, inc, &lpc)?;
                }
                TnsFilterKind::Analysis => {
                    tns_ma_filter(window_spec, filter_start, size, inc, &lpc)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tns_data::{TnsFilter, TnsWindow};
    use crate::tns_max::{tns_max_bands, tns_max_order, AOT_AAC_LC, AOT_AAC_MAIN};

    /// 48 kHz — long-window table has 49 SWBs (sentinel 1024), short
    /// has 14 (sentinel 128).
    const FS_48K: u8 = 3;

    fn ramp(len: usize) -> Vec<f64> {
        (0..len).map(|i| (i % 97) as f64 * 0.25 - 12.0).collect()
    }

    fn long_window(filters: Vec<TnsFilter>, coef_res: bool) -> TnsData {
        TnsData {
            windows: vec![TnsWindow { coef_res, filters }],
        }
    }

    fn no_filter_window() -> TnsWindow {
        TnsWindow {
            coef_res: false,
            filters: vec![],
        }
    }

    // ===== no-op paths =====

    #[test]
    fn empty_tns_data_leaves_spectrum_untouched() {
        let mut spec = ramp(1024);
        let want = spec.clone();
        let tns = long_window(vec![], false);
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            49,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    #[test]
    fn order_zero_filter_is_a_no_op() {
        let mut spec = ramp(1024);
        let want = spec.clone();
        let tns = long_window(
            vec![TnsFilter {
                length: 49,
                order: 0,
                direction: false,
                coef_compress: false,
                coef: vec![],
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            49,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    #[test]
    fn zero_length_region_is_a_no_op() {
        // length = 0 → bottom == top → end == start → `continue` arm.
        let mut spec = ramp(1024);
        let want = spec.clone();
        let tns = long_window(
            vec![TnsFilter {
                length: 0,
                order: 2,
                direction: false,
                coef_compress: false,
                coef: vec![1, 2],
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            49,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    // ===== single-filter long window: composition equivalence =====

    /// The orchestrator must produce exactly the manual composition
    /// `tns_decode_coef_to_lpc` + `tns_ar_filter` over the region the
    /// §4.6.9.3 band arithmetic selects.
    #[test]
    fn single_upward_filter_matches_manual_composition() {
        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1; // 49
        let length = 10_u8;
        let coef: Vec<u8> = vec![1, 7, 2]; // 3-bit wire magnitudes
        let order = coef.len() as u8;

        let mut spec = ramp(1024);
        let mut want = spec.clone();

        // Manual composition. max_sfb = num_swb and TNS_MAX_BANDS for
        // LC long @48k >= 40, so top clamps to min(49, cap, 49) and
        // bottom to min(39, cap, 49).
        let cap = tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;
        let top = num_swb.min(cap).min(num_swb);
        let bottom = (num_swb - length as usize).min(cap).min(num_swb);
        let start = offsets[bottom] as usize;
        let end = offsets[top] as usize;
        let coef_u32: Vec<u32> = coef.iter().map(|&c| u32::from(c)).collect();
        let lpc = tns_decode_coef_to_lpc(3, 0, &coef_u32).unwrap();
        tns_ar_filter(&mut want, start, end - start, 1, &lpc).unwrap();

        let tns = long_window(
            vec![TnsFilter {
                length,
                order,
                direction: false,
                coef_compress: false,
                coef,
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
        // The filter genuinely changed something inside the region.
        assert_ne!(spec[start..end], ramp(1024)[start..end]);
    }

    #[test]
    fn downward_filter_matches_manual_composition() {
        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1;
        let length = 8_u8;
        let coef: Vec<u8> = vec![3, 14, 9]; // 4-bit wire magnitudes
        let order = coef.len() as u8;

        let mut spec = ramp(1024);
        let mut want = spec.clone();

        let cap = tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;
        let top = num_swb.min(cap);
        let bottom = (num_swb - length as usize).min(cap);
        let start = offsets[bottom] as usize;
        let end = offsets[top] as usize;
        let coef_u32: Vec<u32> = coef.iter().map(|&c| u32::from(c)).collect();
        let lpc = tns_decode_coef_to_lpc(4, 0, &coef_u32).unwrap();
        // direction = 1 → inc = -1, start = end - 1.
        tns_ar_filter(&mut want, end - 1, end - start, -1, &lpc).unwrap();

        let tns = long_window(
            vec![TnsFilter {
                length,
                order,
                direction: true,
                coef_compress: false,
                coef,
            }],
            true, // coef_res = 1 → coef_res_bits = 4
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    // ===== region slicing =====

    #[test]
    fn filter_region_counts_down_from_top_band_and_leaves_rest_untouched() {
        // LC long @48 kHz: TNS_MAX_BANDS = 40 < num_swb = 49, so the
        // §4.6.9.3 three-way min clamps both region ends. length = 15
        // → bottom = 34, top = 49→40: the live region is
        // swb_offset[34]..swb_offset[40]; bands 40..49 are clamped
        // away entirely.
        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1;
        let cap = tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;
        assert_eq!(cap, 40);
        let length = 15_u8;
        let bottom = num_swb - length as usize; // 34, below the cap
        let start = offsets[bottom] as usize;
        let end = offsets[cap] as usize;

        let mut spec = ramp(1024);
        let before = spec.clone();
        let tns = long_window(
            vec![TnsFilter {
                length,
                order: 1,
                direction: false,
                coef_compress: false,
                coef: vec![2],
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        // Everything below swb_offset[bottom] is untouched.
        assert_eq!(spec[..start], before[..start]);
        // Everything above the TNS_MAX_BANDS clamp is untouched too.
        assert_eq!(spec[end..], before[end..]);
        // The surviving clamped region was genuinely filtered.
        assert_ne!(spec[start..end], before[start..end]);
    }

    #[test]
    fn second_filter_covers_bands_below_the_first() {
        // Two filters: f0 covers the top 14 bands, f1 the 7 bands
        // below them. Verify against a manual two-pass composition.
        // With TNS_MAX_BANDS = 40 < num_swb = 49 the f0 region top
        // clamps to band 40 while its bottom (35) survives, and the
        // f1 region (28..35) lies entirely below the cap.
        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1;
        let cap = tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;
        let (len0, len1) = (14_u8, 7_u8);

        let mut spec = ramp(1024);
        let mut want = spec.clone();

        let top0 = num_swb;
        let bottom0 = top0 - len0 as usize;
        let lpc0 = tns_decode_coef_to_lpc(3, 0, &[4]).unwrap();
        let s0 = offsets[bottom0.min(cap)] as usize;
        let e0 = offsets[top0.min(cap)] as usize;
        tns_ar_filter(&mut want, s0, e0 - s0, 1, &lpc0).unwrap();

        let top1 = bottom0;
        let bottom1 = top1 - len1 as usize;
        let lpc1 = tns_decode_coef_to_lpc(3, 0, &[7, 1]).unwrap();
        let s1 = offsets[bottom1.min(cap)] as usize;
        let e1 = offsets[top1.min(cap)] as usize;
        tns_ar_filter(&mut want, e1 - 1, e1 - s1, -1, &lpc1).unwrap();

        let tns = long_window(
            vec![
                TnsFilter {
                    length: len0,
                    order: 1,
                    direction: false,
                    coef_compress: false,
                    coef: vec![4],
                },
                TnsFilter {
                    length: len1,
                    order: 2,
                    direction: true,
                    coef_compress: false,
                    coef: vec![7, 1],
                },
            ],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
        // The two regions are disjoint and both genuinely filtered.
        assert!(s1 < e1 && e1 == s0 && s0 < e0);
    }

    #[test]
    fn length_overrun_saturates_bottom_at_band_zero() {
        // length = 63 (max 6-bit wire value) > num_swb → bottom = 0,
        // region = whole clamped spectrum.
        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1;
        let cap = tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;

        let mut spec = ramp(1024);
        let mut want = spec.clone();
        let lpc = tns_decode_coef_to_lpc(3, 0, &[5]).unwrap();
        let end = offsets[num_swb.min(cap)] as usize;
        tns_ar_filter(&mut want, 0, end, 1, &lpc).unwrap();

        let tns = long_window(
            vec![TnsFilter {
                length: 63,
                order: 1,
                direction: false,
                coef_compress: false,
                coef: vec![5],
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    // ===== clamps =====

    #[test]
    fn max_sfb_clamps_the_filter_region_top() {
        // max_sfb = 20 → end = swb_offset[20]; coefficients above it
        // must stay untouched even though the filter nominally covers
        // the top 30 bands.
        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1;
        let max_sfb = 20_u8;
        let end = offsets[max_sfb as usize] as usize;

        let mut spec = ramp(1024);
        let before = spec.clone();
        let tns = long_window(
            vec![TnsFilter {
                length: 30,
                order: 1,
                direction: false,
                coef_compress: false,
                coef: vec![6],
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            max_sfb,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec[end..], before[end..]);
        // bottom = 49 - 30 = 19 < max_sfb → a 1-band region survives
        // the clamp and is filtered.
        let start = offsets[(num_swb - 30).min(max_sfb as usize)] as usize;
        assert_ne!(spec[start..end], before[start..end]);
    }

    #[test]
    fn fully_clamped_region_is_a_no_op() {
        // bottom = 49 - 5 = 44 > max_sfb = 10 → both ends clamp to
        // swb_offset[10] → size = 0 → continue.
        let mut spec = ramp(1024);
        let want = spec.clone();
        let tns = long_window(
            vec![TnsFilter {
                length: 5,
                order: 1,
                direction: false,
                coef_compress: false,
                coef: vec![3],
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            10,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    #[test]
    fn wire_order_is_clamped_by_tns_max_order() {
        // AOT LC long → TNS_MAX_ORDER = 12. A wire order of 15 must
        // use only the first 12 transmitted magnitudes.
        let cap = tns_max_order(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;
        assert_eq!(cap, 12);
        let coef: Vec<u8> = (0..15).map(|i| (i % 8) as u8).collect();

        let offsets = long_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1;
        let band_cap =
            tns_max_bands(AOT_AAC_LC, WindowSequence::OnlyLong, FS_48K).unwrap() as usize;
        let length = 12_u8;
        let bottom = num_swb - length as usize;
        let start = offsets[bottom.min(band_cap)] as usize;
        let end = offsets[num_swb.min(band_cap)] as usize;

        let mut spec = ramp(1024);
        let mut want = spec.clone();
        let coef_u32: Vec<u32> = coef[..cap].iter().map(|&c| u32::from(c)).collect();
        let lpc = tns_decode_coef_to_lpc(3, 0, &coef_u32).unwrap();
        assert_eq!(lpc.len(), cap + 1);
        tns_ar_filter(&mut want, start, end - start, 1, &lpc).unwrap();

        let tns = long_window(
            vec![TnsFilter {
                length,
                order: 15,
                direction: false,
                coef_compress: false,
                coef,
            }],
            false,
        );
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, want);
    }

    #[test]
    fn aac_main_long_window_allows_order_up_to_20() {
        // Same wire order 15, but AOT Main (TNS_MAX_ORDER = 20 long):
        // all 15 magnitudes participate, so the output differs from
        // the LC-clamped run.
        let coef: Vec<u8> = (0..15).map(|i| ((i * 3) % 8) as u8).collect();
        let mk = |aot: u8| {
            let mut spec = ramp(1024);
            let tns = long_window(
                vec![TnsFilter {
                    length: 12,
                    order: 15,
                    direction: false,
                    coef_compress: false,
                    coef: coef.clone(),
                }],
                false,
            );
            tns_decode_frame(&mut spec, &tns, WindowSequence::OnlyLong, 49, aot, FS_48K).unwrap();
            spec
        };
        assert_ne!(mk(AOT_AAC_MAIN), mk(AOT_AAC_LC));
    }

    // ===== short windows =====

    #[test]
    fn short_sequence_filters_only_the_targeted_window() {
        // 8 × 128 frame; a single filter on window 3 must leave the
        // other 7 windows byte-identical.
        let offsets = short_window_offsets(FS_48K).unwrap();
        let num_swb = offsets.len() - 1; // 14
        let mut windows: Vec<TnsWindow> = (0..8).map(|_| no_filter_window()).collect();
        windows[3] = TnsWindow {
            coef_res: false,
            filters: vec![TnsFilter {
                length: num_swb as u8,
                order: 2,
                direction: false,
                coef_compress: false,
                coef: vec![1, 6],
            }],
        };
        let tns = TnsData { windows };

        let mut spec = ramp(1024);
        let before = spec.clone();
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::EightShort,
            num_swb as u8,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec[..3 * 128], before[..3 * 128]);
        assert_eq!(spec[4 * 128..], before[4 * 128..]);
        assert_ne!(spec[3 * 128..4 * 128], before[3 * 128..4 * 128]);

        // And window 3 matches the manual composition on its slice.
        let cap = tns_max_bands(AOT_AAC_LC, WindowSequence::EightShort, FS_48K).unwrap() as usize;
        let end = offsets[num_swb.min(cap)] as usize;
        let lpc = tns_decode_coef_to_lpc(3, 0, &[1, 6]).unwrap();
        let mut want_w3 = before[3 * 128..4 * 128].to_vec();
        tns_ar_filter(&mut want_w3, 0, end, 1, &lpc).unwrap();
        assert_eq!(spec[3 * 128..4 * 128], want_w3);
    }

    // ===== validation =====

    #[test]
    fn rejects_spectrum_length_mismatch() {
        let mut spec = ramp(512);
        let tns = long_window(vec![], false);
        assert!(matches!(
            tns_decode_frame(
                &mut spec,
                &tns,
                WindowSequence::OnlyLong,
                49,
                AOT_AAC_LC,
                FS_48K
            ),
            Err(Error::TnsFrameInvalid)
        ));
    }

    #[test]
    fn rejects_window_count_mismatch() {
        // 1 TnsWindow under EIGHT_SHORT_SEQUENCE (needs 8).
        let mut spec = ramp(1024);
        let tns = long_window(vec![], false);
        assert!(matches!(
            tns_decode_frame(
                &mut spec,
                &tns,
                WindowSequence::EightShort,
                14,
                AOT_AAC_LC,
                FS_48K
            ),
            Err(Error::TnsFrameInvalid)
        ));
    }

    #[test]
    fn rejects_coef_shorter_than_clamped_order() {
        let mut spec = ramp(1024);
        let tns = long_window(
            vec![TnsFilter {
                length: 10,
                order: 3,
                direction: false,
                coef_compress: false,
                coef: vec![1], // < clamped order 3
            }],
            false,
        );
        assert!(matches!(
            tns_decode_frame(
                &mut spec,
                &tns,
                WindowSequence::OnlyLong,
                49,
                AOT_AAC_LC,
                FS_48K
            ),
            Err(Error::TnsFrameInvalid)
        ));
    }

    #[test]
    fn rejects_unsupported_fs_index() {
        let mut spec = ramp(1024);
        let tns = long_window(vec![], false);
        assert!(matches!(
            tns_decode_frame(
                &mut spec,
                &tns,
                WindowSequence::OnlyLong,
                49,
                AOT_AAC_LC,
                12
            ),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(12))
        ));
    }

    #[test]
    fn propagates_coef_out_of_range_from_decode() {
        // coef_compress = 1 with coef_res = 0 → coef_res2 = 2 bits;
        // a magnitude of 4 overflows the field.
        let mut spec = ramp(1024);
        let tns = long_window(
            vec![TnsFilter {
                length: 10,
                order: 1,
                direction: false,
                coef_compress: true,
                coef: vec![4],
            }],
            false,
        );
        assert!(matches!(
            tns_decode_frame(
                &mut spec,
                &tns,
                WindowSequence::OnlyLong,
                49,
                AOT_AAC_LC,
                FS_48K
            ),
            Err(Error::TnsCoefOutOfRange)
        ));
    }

    // ===== §4.6.7.4.1 analysis pass =====

    #[test]
    fn analysis_then_synthesis_is_identity() {
        // The LTP-loop analysis filter (all-zero) followed by the §4.6.9
        // synthesis filter (all-pole), over the same frame, reconstructs
        // the spectrum exactly — the §4.6.7.4.1 invariant that lets the
        // single TNS synthesis pass after the LTP add undo the analysis
        // on X_est while shaping the residual.
        let tns = long_window(
            vec![
                TnsFilter {
                    length: 12,
                    order: 3,
                    direction: false,
                    coef_compress: false,
                    coef: vec![1, 7, 2],
                },
                TnsFilter {
                    length: 8,
                    order: 2,
                    direction: true,
                    coef_compress: false,
                    coef: vec![6, 3],
                },
            ],
            false,
        );
        let original = ramp(1024);
        let mut spec = original.clone();
        tns_analysis_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            49,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        // The analysis pass actually changed the spectrum.
        assert_ne!(spec, original);
        tns_decode_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            49,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        for (g, w) in spec.iter().zip(original.iter()) {
            assert!((g - w).abs() < 1e-9, "analysis∘synthesis drift: {g} vs {w}");
        }
    }

    #[test]
    fn analysis_no_filters_is_noop() {
        let tns = TnsData {
            windows: vec![no_filter_window()],
        };
        let original = ramp(1024);
        let mut spec = original.clone();
        tns_analysis_frame(
            &mut spec,
            &tns,
            WindowSequence::OnlyLong,
            49,
            AOT_AAC_LC,
            FS_48K,
        )
        .unwrap();
        assert_eq!(spec, original);
    }
}
