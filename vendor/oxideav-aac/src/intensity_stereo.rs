//! §4.6.8.2 Intensity Stereo (IS) decoding — ISO/IEC 14496-3.
//!
//! Intensity stereo is the second joint-channel tool of a channel
//! pair (the first being M/S, [`crate::ms_stereo`]). Where M/S
//! reconstructs both channels from a mid/side basis, intensity stereo
//! derives the **right** channel entirely from the **left** channel
//! by a single per-band real scale, exploiting the ear's reduced
//! sensitivity to phase at high frequencies. The left channel is
//! untouched.
//!
//! ## §4.6.8.2.3 decoding process
//!
//! Intensity stereo is signalled by the pseudo codebooks
//! `INTENSITY_HCB` (15, in-phase) and `INTENSITY_HCB2` (14,
//! out-of-phase) appearing in the **right** channel's `sfb_cb` (their
//! use in a left channel is illegal). For each intensity-coded band a
//! transmitted *intensity stereo position* `is_pos[g][sfb]` replaces
//! the right channel's scalefactor; the §4.6.8.2.3 reconstruction is
//!
//! ```text
//! is_intensity(g,sfb)    = +1  if right sfb_cb == INTENSITY_HCB  (15)
//!                          -1  if right sfb_cb == INTENSITY_HCB2 (14)
//!                           0  otherwise
//! invert_intensity(g,sfb)= 1 - 2*ms_used[g][sfb]  if ms_mask_present == 1
//!                                                  (and aot != AAC scalable)
//!                          +1                       otherwise
//! scale = is_intensity(g,sfb) * invert_intensity(g,sfb)
//!         * 0.5^(0.25 * is_pos[g][sfb]);
//! for (i = 0; i < swb_offset[sfb+1]-swb_offset[sfb]; i++)
//!     r_spec[g][b][sfb][i] = scale * l_spec[g][b][sfb][i];
//! ```
//!
//! The `0.5^(0.25·is_pos)` magnitude is the same per-quarter-step gain
//! ladder as the §4.6.2.3.3 scalefactor gain `2^(0.25·(sf−100))` (the
//! intensity position plays the role of a scalefactor difference); the
//! `is_intensity` factor carries the in/out-of-phase sign of the
//! codebook and `invert_intensity` flips it when the band's `ms_used`
//! bit is set under a per-band M/S mask (`ms_mask_present == 1`). This
//! is a deterministic algebraic reconstruction — no rounding tables and
//! no RNG — so an intensity-coded band comes out byte-exact.
//!
//! ## Mutual exclusion (§4.6.8.1.3 note / §4.6.8.2.3 / §4.6.13.3)
//!
//! M/S, intensity stereo, and PNS are mutually exclusive on any one
//! `(group, sfb)`. This tool only ever rewrites the right channel of a
//! band whose **right** `sfb_cb` is an intensity book; M/S already
//! skips those bands (it consults the right channel's intensity status
//! via `is_intensity`). A band that is `NOISE_HCB` cannot also be an
//! intensity book, so no extra noise guard is needed here — the
//! `is_intensity` predicate is `0` for every non-intensity codebook
//! and the band is left as the inverse-quantised passthrough.
//!
//! ## Decoder block order
//!
//! Per §4.6 the channel-pair / noise tools run inverse-quant → M/S →
//! PNS → intensity → TNS on the **de-interleaved, window-major**
//! spectrum produced by [`crate::decoded_spectrum::quant_to_spec`]
//! (`spec[w * window_len + k]`), so this pass runs after
//! [`crate::ms_stereo::apply_ms_stereo`] and before the §4.6.9 TNS
//! filter. The per-band coefficient extent
//! `swb_offset[sfb+1]-swb_offset[sfb]` and the
//! `(group, in-group window) → absolute window` mapping match the rest
//! of the pipeline.
//!
//! ## Scope
//!
//! This module is the intensity-stereo / left-to-right derivation only.
//! The dependently-switched coupling channel contribution of the
//! "intensity stereo / coupling" tool (§4.6.8.2.1, fed by a CCE) and
//! PNS synthesis (§4.6.13) are separate follow-ups. The
//! `is_position[g][sfb]` track itself is produced upstream by the
//! §4.6.8.1.4 DPCM accumulator
//! ([`crate::scale_factor_data::accumulate`]).

use crate::ics_info::IcsInfo;
use crate::section_data::{INTENSITY_HCB, INTENSITY_HCB2};
use crate::swb_offset::{
    long_window_offsets, short_window_offsets, LONG_WINDOW_LEN, SHORT_WINDOW_LEN,
};
use crate::{Error, Result};

/// §4.6.8.2.3 `is_intensity(g,sfb)` — the in/out-of-phase sign of an
/// intensity band, keyed on the **right** channel codebook.
///
/// `+1` for `INTENSITY_HCB` (15, in-phase), `-1` for `INTENSITY_HCB2`
/// (14, out-of-phase), `0` for any non-intensity codebook (the band is
/// not intensity-coded and is left untouched).
pub fn is_intensity(right_cb: u8) -> i32 {
    match right_cb {
        INTENSITY_HCB => 1,
        INTENSITY_HCB2 => -1,
        _ => 0,
    }
}

/// §4.6.8.2.3 `invert_intensity(g,sfb)` — the phase-reversal factor.
///
/// Returns `1 - 2*ms_used` (i.e. `+1` when `ms_used == false`, `-1`
/// when `true`) under a per-band M/S mask (`ms_mask_present == 1`) for
/// a non-scalable GA decoder; `+1` otherwise. Because M/S and
/// intensity are mutually exclusive on a band, a set `ms_used` bit on
/// an intensity band carries the §4.6.8.2.3 phase reversal rather than
/// an M/S de-matrix.
pub fn invert_intensity(per_band_mask: bool, ms_used: bool) -> i32 {
    if per_band_mask {
        1 - 2 * (ms_used as i32)
    } else {
        1
    }
}

/// §4.6.8.2.3 `0.5^(0.25 * is_pos)` — the intensity-position gain.
///
/// The same per-quarter-step ladder as the §4.6.2.3.3 scalefactor gain
/// but on a base of `1/2`; a larger position attenuates the derived
/// right channel.
pub fn intensity_gain(is_pos: i32) -> f64 {
    0.5f64.powf(0.25 * is_pos as f64)
}

/// A channel pair's de-interleaved spectra plus the right-channel
/// codebooks and intensity positions the §4.6.8.2.3 derivation needs.
///
/// `left` / `right` are the window-major decoded spectra
/// (`num_windows × window_len`) produced by
/// [`crate::decoded_spectrum::quant_to_spec`]. `left` is read only;
/// for every intensity-coded band `right` is overwritten with
/// `scale · left`. Bands whose right `sfb_cb` is not an intensity book
/// are left exactly as they arrive (the inverse-quantised passthrough).
///
/// `right_sfb_cb` is the right channel's `sfb_cb[g][sfb]` (from its
/// [`crate::section_data::SectionData`]); it both selects which bands
/// are intensity-coded and supplies the in/out-of-phase sign.
///
/// `is_pos` is the right channel's absolute `is_pos[g][sfb]` track
/// (§4.6.8.1.4, from [`crate::scale_factor_data::accumulate`]). Only
/// the entries at intensity-coded `(g, sfb)` are consulted.
#[derive(Debug)]
pub struct IntensityPairSpectra<'a> {
    /// First ("left") channel spectrum — read only.
    pub left: &'a [f64],
    /// Second ("right") channel spectrum — derived from `left` on
    /// intensity bands, untouched elsewhere.
    pub right: &'a mut [f64],
    /// Right channel `sfb_cb[g][sfb]`.
    pub right_sfb_cb: &'a [Vec<u8>],
    /// Right channel absolute `is_pos[g][sfb]` (§4.6.8.1.4).
    pub is_pos: &'a [Vec<i32>],
}

/// Apply the §4.6.8.2.3 intensity-stereo left→right derivation in place.
///
/// * `pair` — the channel-pair spectra, right-channel codebooks, and
///   intensity positions ([`IntensityPairSpectra`]).
/// * `ms_mask_present` — `true` ⇔ the CPE carries a per-band `ms_used`
///   mask (`ms_mask_present == 1`); selects the §4.6.8.2.3
///   `invert_intensity` phase-reversal branch.
/// * `ms_used` — `ms_used[g][sfb]` (one row per window group, each at
///   least `max_sfb` long). Consulted only when `ms_mask_present` is
///   `true`; pass an empty slice otherwise.
/// * `ics_info` — the shared `common_window` `ics_info()`; supplies
///   `num_window_groups`, `window_group_length`, `max_sfb`, and the
///   window geometry.
/// * `fs_index` — `samplingFrequencyIndex`, selecting the `swb_offset`
///   table.
///
/// Returns [`Error::IntensityStereoInvalid`] if the buffer / mask /
/// `sfb_cb` / `is_pos` shapes disagree with `ics_info` (see the variant
/// docs). When no band is intensity-coded the right buffer is left
/// untouched.
pub fn apply_intensity_stereo(
    pair: &mut IntensityPairSpectra<'_>,
    ms_mask_present: bool,
    ms_used: &[Vec<bool>],
    ics_info: &IcsInfo,
    fs_index: u8,
) -> Result<()> {
    let IntensityPairSpectra {
        left,
        right,
        right_sfb_cb,
        is_pos,
    } = pair;

    let (window_len, offsets) = if ics_info.window_sequence.is_eight_short() {
        (SHORT_WINDOW_LEN as usize, short_window_offsets(fs_index)?)
    } else {
        (LONG_WINDOW_LEN as usize, long_window_offsets(fs_index)?)
    };
    let num_swb = offsets.len() - 1;
    let num_windows = ics_info.num_windows as usize;
    let num_groups = ics_info.num_window_groups as usize;
    let max_sfb = ics_info.max_sfb as usize;

    // Geometry consistency: both channels share the common_window
    // ics_info, so both spectra are num_windows × window_len.
    let expected = num_windows * window_len;
    if left.len() != expected || right.len() != expected {
        return Err(Error::IntensityStereoInvalid);
    }
    if ics_info.window_group_length.len() != num_groups
        || ics_info
            .window_group_length
            .iter()
            .map(|&w| w as usize)
            .sum::<usize>()
            != num_windows
    {
        return Err(Error::IntensityStereoInvalid);
    }
    // max_sfb must not exceed the band count of the active window.
    if max_sfb > num_swb {
        return Err(Error::IntensityStereoInvalid);
    }
    // The right channel drives both the intensity predicate and the
    // is_pos lookup, so both tables must cover every (g, sfb).
    if right_sfb_cb.len() != num_groups || is_pos.len() != num_groups {
        return Err(Error::IntensityStereoInvalid);
    }
    for g in 0..num_groups {
        if right_sfb_cb[g].len() < max_sfb || is_pos[g].len() < max_sfb {
            return Err(Error::IntensityStereoInvalid);
        }
    }
    // A per-band mask needs a full ms_used[g][sfb]; otherwise it is
    // ignored.
    if ms_mask_present {
        if ms_used.len() != num_groups {
            return Err(Error::IntensityStereoInvalid);
        }
        for row in ms_used {
            if row.len() < max_sfb {
                return Err(Error::IntensityStereoInvalid);
            }
        }
    }

    let mut window_base = 0usize;
    for g in 0..num_groups {
        let wgl = ics_info.window_group_length[g] as usize;
        for sfb in 0..max_sfb {
            let sign = is_intensity(right_sfb_cb[g][sfb]);
            if sign == 0 {
                // Not an intensity band: leave the right channel as the
                // inverse-quantised / M/S-reconstructed passthrough.
                continue;
            }
            let used = ms_mask_present && ms_used[g][sfb];
            let inv = invert_intensity(ms_mask_present, used);
            let scale = sign as f64 * inv as f64 * intensity_gain(is_pos[g][sfb]);
            let start = offsets[sfb] as usize;
            let end = offsets[sfb + 1] as usize;
            for b in 0..wgl {
                let base = (window_base + b) * window_len;
                for i in start..end {
                    right[base + i] = scale * left[base + i];
                }
            }
        }
        window_base += wgl;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::{WindowSequence, WindowShape};
    use crate::section_data::{NOISE_HCB, ZERO_HCB};

    const FS_44100: u8 = 4;
    const SPECTRUM_CB: u8 = 2;

    fn long_ics_info(max_sfb: u8) -> IcsInfo {
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::OnlyLong,
            window_shape: WindowShape::Sine,
            max_sfb,
            scale_factor_grouping: None,
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: false,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 1,
            num_window_groups: 1,
            window_group_length: vec![1],
            num_swb: crate::ics_info::NUM_SWB_LONG_WINDOW[FS_44100 as usize],
        }
    }

    fn short_ics_info(max_sfb: u8, window_group_length: Vec<u8>) -> IcsInfo {
        let num_window_groups = window_group_length.len() as u8;
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::EightShort,
            window_shape: WindowShape::Sine,
            max_sfb,
            scale_factor_grouping: Some(0),
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: false,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 8,
            num_window_groups,
            window_group_length,
            num_swb: crate::ics_info::NUM_SWB_SHORT_WINDOW[FS_44100 as usize],
        }
    }

    fn plain_cb(num_groups: usize, max_sfb: usize) -> Vec<Vec<u8>> {
        vec![vec![SPECTRUM_CB; max_sfb]; num_groups]
    }

    fn zero_pos(num_groups: usize, max_sfb: usize) -> Vec<Vec<i32>> {
        vec![vec![0i32; max_sfb]; num_groups]
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        left: &[f64],
        right: &mut [f64],
        ms_mask_present: bool,
        ms_used: &[Vec<bool>],
        right_sfb_cb: &[Vec<u8>],
        is_pos: &[Vec<i32>],
        ics_info: &IcsInfo,
        fs_index: u8,
    ) -> Result<()> {
        let mut pair = IntensityPairSpectra {
            left,
            right,
            right_sfb_cb,
            is_pos,
        };
        apply_intensity_stereo(&mut pair, ms_mask_present, ms_used, ics_info, fs_index)
    }

    #[test]
    fn is_intensity_sign() {
        assert_eq!(is_intensity(INTENSITY_HCB), 1);
        assert_eq!(is_intensity(INTENSITY_HCB2), -1);
        assert_eq!(is_intensity(SPECTRUM_CB), 0);
        assert_eq!(is_intensity(NOISE_HCB), 0);
        assert_eq!(is_intensity(ZERO_HCB), 0);
    }

    #[test]
    fn invert_intensity_branches() {
        // No per-band mask: always +1 regardless of ms_used.
        assert_eq!(invert_intensity(false, false), 1);
        assert_eq!(invert_intensity(false, true), 1);
        // Per-band mask: 1 - 2*ms_used.
        assert_eq!(invert_intensity(true, false), 1);
        assert_eq!(invert_intensity(true, true), -1);
    }

    #[test]
    fn intensity_gain_quarter_ladder() {
        // 0.5^0 = 1.
        assert!((intensity_gain(0) - 1.0).abs() < 1e-12);
        // 0.5^(0.25*4) = 0.5^1 = 0.5.
        assert!((intensity_gain(4) - 0.5).abs() < 1e-12);
        // 0.5^(0.25*8) = 0.25.
        assert!((intensity_gain(8) - 0.25).abs() < 1e-12);
        // negative position amplifies: 0.5^(-1) = 2.
        assert!((intensity_gain(-4) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn in_phase_pos_zero_copies_left() {
        // INTENSITY_HCB, is_pos = 0, no mask → scale = +1: right == left.
        let info = long_ics_info(3);
        let max_sfb = 3;
        let off = long_window_offsets(FS_44100).unwrap();
        let n = LONG_WINDOW_LEN as usize;
        let mut left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        for (k, (l, r)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            if k >= off[0] as usize && k < off[3] as usize {
                *l = (k as f64) * 0.5 - 3.0;
                *r = 999.0; // garbage to be overwritten
            }
        }
        let mut cb = plain_cb(1, max_sfb);
        cb[0][1] = INTENSITY_HCB; // make sfb 1 intensity-coded
        let pos = zero_pos(1, max_sfb);

        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();

        // sfb 1 derived from left; sfb 0 and 2 untouched (still garbage).
        for (r, l) in right
            .iter()
            .zip(left.iter())
            .take(off[2] as usize)
            .skip(off[1] as usize)
        {
            assert!((r - l).abs() < 1e-12);
        }
        for &r in right.iter().take(off[1] as usize).skip(off[0] as usize) {
            assert_eq!(r, 999.0);
        }
        for &r in right.iter().take(off[3] as usize).skip(off[2] as usize) {
            assert_eq!(r, 999.0);
        }
    }

    #[test]
    fn out_of_phase_negates() {
        // INTENSITY_HCB2 (sign -1), is_pos = 0, no mask → scale = -1.
        let info = long_ics_info(2);
        let off = long_window_offsets(FS_44100).unwrap();
        let n = LONG_WINDOW_LEN as usize;
        let mut left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        for l in left.iter_mut().take(off[1] as usize).skip(off[0] as usize) {
            *l = 7.0;
        }
        let mut cb = plain_cb(1, 2);
        cb[0][0] = INTENSITY_HCB2;
        let pos = zero_pos(1, 2);

        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();

        for &r in right.iter().take(off[1] as usize).skip(off[0] as usize) {
            assert!((r + 7.0).abs() < 1e-12);
        }
    }

    #[test]
    fn position_scales_gain() {
        // is_pos = 4 → gain 0.5; in-phase, no mask → right = 0.5 * left.
        let info = long_ics_info(1);
        let off = long_window_offsets(FS_44100).unwrap();
        let n = LONG_WINDOW_LEN as usize;
        let mut left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        for l in left.iter_mut().take(off[1] as usize).skip(off[0] as usize) {
            *l = 16.0;
        }
        let mut cb = plain_cb(1, 1);
        cb[0][0] = INTENSITY_HCB;
        let mut pos = zero_pos(1, 1);
        pos[0][0] = 4;

        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();

        for &r in right.iter().take(off[1] as usize).skip(off[0] as usize) {
            assert!((r - 8.0).abs() < 1e-12);
        }
    }

    #[test]
    fn ms_used_inverts_phase_under_mask() {
        // INTENSITY_HCB (sign +1) with a set ms_used bit under a
        // per-band mask flips to -1: right = -left.
        let info = long_ics_info(1);
        let off = long_window_offsets(FS_44100).unwrap();
        let n = LONG_WINDOW_LEN as usize;
        let mut left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        for l in left.iter_mut().take(off[1] as usize).skip(off[0] as usize) {
            *l = 3.0;
        }
        let mut cb = plain_cb(1, 1);
        cb[0][0] = INTENSITY_HCB;
        let pos = zero_pos(1, 1);
        let ms_used = vec![vec![true]];

        run(
            &left, &mut right, true, &ms_used, &cb, &pos, &info, FS_44100,
        )
        .unwrap();

        for &r in right.iter().take(off[1] as usize).skip(off[0] as usize) {
            assert!((r + 3.0).abs() < 1e-12);
        }
    }

    #[test]
    fn ms_used_ignored_without_mask() {
        // Same set ms_used bit but ms_mask_present == false → +1 (the
        // mask is not consulted; invert_intensity is +1).
        let info = long_ics_info(1);
        let off = long_window_offsets(FS_44100).unwrap();
        let n = LONG_WINDOW_LEN as usize;
        let mut left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        for l in left.iter_mut().take(off[1] as usize).skip(off[0] as usize) {
            *l = 3.0;
        }
        let mut cb = plain_cb(1, 1);
        cb[0][0] = INTENSITY_HCB;
        let pos = zero_pos(1, 1);

        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();

        for &r in right.iter().take(off[1] as usize).skip(off[0] as usize) {
            assert!((r - 3.0).abs() < 1e-12);
        }
    }

    #[test]
    fn non_intensity_bands_untouched() {
        // No intensity codebook anywhere → right is left exactly as-is.
        let info = long_ics_info(4);
        let n = LONG_WINDOW_LEN as usize;
        let left = vec![1.0f64; n];
        let mut right = vec![42.0f64; n];
        let cb = plain_cb(1, 4); // all spectrum books
        let pos = zero_pos(1, 4);

        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();

        assert!(right.iter().all(|&v| v == 42.0));
    }

    #[test]
    fn short_window_grouping() {
        // Two groups of 4 + 4 short windows; intensity on sfb 0 of
        // group 1. Every window of that group derives right from left.
        let wgl = vec![4u8, 4u8];
        let info = short_ics_info(2, wgl.clone());
        let off = short_window_offsets(FS_44100).unwrap();
        let wlen = SHORT_WINDOW_LEN as usize;
        let n = 8 * wlen;
        let mut left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        for w in 0..8 {
            for k in (off[0] as usize)..(off[1] as usize) {
                left[w * wlen + k] = (w as f64) + 1.0;
            }
        }
        let mut cb = plain_cb(2, 2);
        cb[1][0] = INTENSITY_HCB; // group 1 sfb 0 in-phase
        let pos = zero_pos(2, 2);

        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();

        // Group 0 (windows 0..4) untouched, group 1 (windows 4..8)
        // derived as right = left (in-phase, pos 0).
        for w in 0..4 {
            for k in (off[0] as usize)..(off[1] as usize) {
                assert_eq!(right[w * wlen + k], 0.0);
            }
        }
        for w in 4..8 {
            for k in (off[0] as usize)..(off[1] as usize) {
                assert!((right[w * wlen + k] - left[w * wlen + k]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn rejects_length_mismatch() {
        let info = long_ics_info(1);
        let left = vec![0.0f64; 10];
        let mut right = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let cb = plain_cb(1, 1);
        let pos = zero_pos(1, 1);
        let e = run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100);
        assert_eq!(e, Err(Error::IntensityStereoInvalid));
    }

    #[test]
    fn rejects_short_is_pos() {
        let info = long_ics_info(3);
        let n = LONG_WINDOW_LEN as usize;
        let left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        let cb = plain_cb(1, 3);
        let pos = zero_pos(1, 2); // too short
        let e = run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100);
        assert_eq!(e, Err(Error::IntensityStereoInvalid));
    }

    #[test]
    fn rejects_missing_ms_used_under_mask() {
        let info = long_ics_info(1);
        let n = LONG_WINDOW_LEN as usize;
        let left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        let cb = plain_cb(1, 1);
        let pos = zero_pos(1, 1);
        // ms_mask_present true but ms_used empty → reject.
        let e = run(&left, &mut right, true, &[], &cb, &pos, &info, FS_44100);
        assert_eq!(e, Err(Error::IntensityStereoInvalid));
    }

    #[test]
    fn rejects_max_sfb_over_num_swb() {
        let mut info = long_ics_info(1);
        info.max_sfb = 99; // exceeds long-window band count
        let n = LONG_WINDOW_LEN as usize;
        let left = vec![0.0f64; n];
        let mut right = vec![0.0f64; n];
        let cb = vec![vec![SPECTRUM_CB; 99]];
        let pos = vec![vec![0i32; 99]];
        let e = run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100);
        assert_eq!(e, Err(Error::IntensityStereoInvalid));
    }

    #[test]
    fn no_intensity_band_leaves_right_untouched() {
        // Empty/degenerate: max_sfb 0 → nothing to scan.
        let mut info = long_ics_info(0);
        info.max_sfb = 0;
        let n = LONG_WINDOW_LEN as usize;
        let left = vec![1.0f64; n];
        let mut right = vec![5.0f64; n];
        let cb: Vec<Vec<u8>> = vec![vec![]];
        let pos: Vec<Vec<i32>> = vec![vec![]];
        run(&left, &mut right, false, &[], &cb, &pos, &info, FS_44100).unwrap();
        assert!(right.iter().all(|&v| v == 5.0));
    }
}
