//! §4.6.8.1 M/S (mid/side) stereo de-matrix — ISO/IEC 14496-3.
//!
//! M/S joint channel coding operates on a channel pair. On a
//! per-spectral-coefficient basis the decoder reconstructs the
//! left/right vector by either the identity matrix (M/S off for the
//! band) or the inverse M/S matrix (M/S on):
//!
//! ```text
//! [ l ]   [ 1  0 ] [ l ]          [ l ]   [ 1   1 ] [ m ]
//! [ r ] = [ 0  1 ] [ r ]    or    [ r ] = [ 1  -1 ] [ s ]
//! ```
//!
//! With `m` carried in the left slot and `s` in the right slot, the
//! §4.6.8.1.3 decoding pseudo code is the in-place de-matrix:
//!
//! ```text
//! if (mask_present >= 1) {
//!     for (g=0; g<num_window_groups; g++)
//!       for (b=0; b<window_group_length[g]; b++)
//!         for (sfb=0; sfb<max_sfb; sfb++)
//!           if ((ms_used[g][sfb] || mask_present == 2) &&
//!                !is_intensity(g,sfb) && !is_noise(g,sfb))
//!             for (i=0; i< swb_offset[sfb+1]-swb_offset[sfb]; i++) {
//!                 tmp        = l_spec[..i] - r_spec[..i];
//!                 l_spec[..i]= l_spec[..i] + r_spec[..i];
//!                 r_spec[..i]= tmp;
//!             }
//! }
//! ```
//!
//! i.e. `l' = m + s`, `r' = m - s`. The de-matrix is exact and fully
//! determined by the bitstream — no rounding or RNG is involved — so
//! a band whose `ms_used` is set is reconstructed byte-exactly.
//!
//! ## Mutual exclusion (§4.6.8.1.3 note / §4.6.8.2.3 / §4.6.13.5)
//!
//! M/S, intensity stereo, and perceptual noise substitution are
//! mutually exclusive on any one `(group, sfb)`. The de-matrix is
//! therefore suppressed when the band is intensity-coded
//! (`is_intensity`) or noise-substituted (`is_noise`):
//!
//! * `is_intensity(g,sfb)` keys on the **right** channel codebook
//!   (`INTENSITY_HCB` / `INTENSITY_HCB2`, 15 / 14); use of an
//!   intensity codebook in a left channel is illegal (§4.6.8.2.3),
//!   so only the right channel's `sfb_cb` is consulted.
//! * `is_noise(g,sfb)` keys on `NOISE_HCB` (13). A band is excluded
//!   from the M/S de-matrix when **either** channel marks it noise:
//!   if only one channel is PNS the `ms_used` setting is not
//!   evaluated for noise correlation (§4.6.13.3), and a band that is
//!   PNS in one channel and a real spectrum in the other still must
//!   not be de-matrixed against substituted noise.
//!
//! ## Geometry
//!
//! The de-matrix runs on the **de-interleaved, window-major**
//! spectrum produced by [`crate::decoded_spectrum::quant_to_spec`]
//! (`spec[w * window_len + k]`), before TNS — the decoder block
//! order is inverse-quant → M/S → PNS → intensity → TNS (§4.6, the
//! "intensity stereo / coupling" and "TNS" tool descriptions). The
//! per-band coefficient extent `swb_offset[sfb+1]-swb_offset[sfb]`
//! is the same scalefactor-window-band geometry the rest of the
//! pipeline uses; window index `w` of group `g`, in-group window `b`
//! is `window_base(g) + b`.
//!
//! ## Scope
//!
//! This module is the M/S tool only. Intensity stereo (§4.6.8.2) and
//! PNS (§4.6.13) synthesis — the other two joint/noise tools that
//! share the `ms_used` mask — are separate follow-ups; here their
//! bands are merely *excluded* from the de-matrix. `mask_present`
//! value `3` is reserved and rejected.

use crate::ics_info::IcsInfo;
use crate::section_data::{INTENSITY_HCB, INTENSITY_HCB2, NOISE_HCB};
use crate::swb_offset::{
    long_window_offsets, short_window_offsets, LONG_WINDOW_LEN, SHORT_WINDOW_LEN,
};
use crate::{Error, Result};

/// §4.6.8.1.2 `ms_mask_present` — the two-bit field decoded from the
/// CPE header that governs whether (and how) the M/S de-matrix runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MsMaskPresent {
    /// `00` — all zeros: no M/S de-matrix on any band.
    AllZeros,
    /// `01` — a `max_sfb`-band `ms_used` mask follows in the
    /// bitstream; the de-matrix runs per set bit.
    Mask,
    /// `10` — all ones: the de-matrix runs on every band that is not
    /// intensity- or noise-coded, regardless of `ms_used` content.
    AllOnes,
}

impl MsMaskPresent {
    /// Decode the raw two-bit `ms_mask_present` field.
    ///
    /// Returns [`Error::MsStereoInvalid`] for the reserved value `3`.
    pub fn from_bits(bits: u8) -> Result<Self> {
        match bits {
            0 => Ok(MsMaskPresent::AllZeros),
            1 => Ok(MsMaskPresent::Mask),
            2 => Ok(MsMaskPresent::AllOnes),
            _ => Err(Error::MsStereoInvalid),
        }
    }

    /// The wire value (`0`/`1`/`2`) — the inverse of [`Self::from_bits`].
    pub fn to_bits(self) -> u8 {
        match self {
            MsMaskPresent::AllZeros => 0,
            MsMaskPresent::Mask => 1,
            MsMaskPresent::AllOnes => 2,
        }
    }

    /// `mask_present >= 1` — whether the §4.6.8.1.3 outer guard is
    /// entered at all.
    pub fn is_active(self) -> bool {
        !matches!(self, MsMaskPresent::AllZeros)
    }
}

/// `true` ⇔ the band's right-channel codebook is an intensity book
/// (`INTENSITY_HCB` / `INTENSITY_HCB2`) — the §4.6.8.2.3
/// `is_intensity(g,sfb)` predicate restricted to "is it intensity at
/// all" (the M/S guard only needs the boolean, not the ±1 sign).
fn is_intensity_cb(cb: u8) -> bool {
    cb == INTENSITY_HCB || cb == INTENSITY_HCB2
}

/// `true` ⇔ the band's codebook is `NOISE_HCB` — the §4.6.13.3
/// `is_noise(g,sfb)` predicate.
fn is_noise_cb(cb: u8) -> bool {
    cb == NOISE_HCB
}

/// A channel pair's de-interleaved spectra plus the per-channel
/// codebook assignments the M/S de-matrix needs.
///
/// `left` / `right` are the window-major decoded spectra
/// (`num_windows × window_len`) produced by
/// [`crate::decoded_spectrum::quant_to_spec`], **pre-TNS**. On entry
/// `left` holds the mid (`m`) and `right` the side (`s`) for every
/// M/S-active band; [`apply_ms_stereo`] overwrites them with the
/// reconstructed left/right channels.
///
/// `left_sfb_cb` / `right_sfb_cb` are each channel's `sfb_cb[g][sfb]`
/// (from its [`crate::section_data::SectionData`]); they drive the
/// intensity (right) / noise (either) exclusions.
#[derive(Debug)]
pub struct ChannelPairSpectra<'a> {
    /// First ("left") channel spectrum — mid on entry, left on return.
    pub left: &'a mut [f64],
    /// Second ("right") channel spectrum — side on entry, right on return.
    pub right: &'a mut [f64],
    /// Left channel `sfb_cb[g][sfb]`.
    pub left_sfb_cb: &'a [Vec<u8>],
    /// Right channel `sfb_cb[g][sfb]`.
    pub right_sfb_cb: &'a [Vec<u8>],
}

/// Apply the §4.6.8.1.3 M/S de-matrix in place to a channel pair.
///
/// * `pair` — the channel-pair spectra and per-channel codebooks
///   ([`ChannelPairSpectra`]).
/// * `ms_mask_present` — the decoded CPE [`MsMaskPresent`].
/// * `ms_used` — `ms_used[g][sfb]` (one row per window group, each
///   at least `max_sfb` long). Ignored when `ms_mask_present` is
///   [`MsMaskPresent::AllZeros`] or [`MsMaskPresent::AllOnes`]; pass
///   an empty slice in those cases.
/// * `ics_info` — the shared `common_window` `ics_info()`; supplies
///   `num_window_groups`, `window_group_length`, `max_sfb`, and the
///   window geometry.
/// * `fs_index` — `samplingFrequencyIndex`, selecting the
///   `swb_offset` table.
///
/// When `ms_mask_present` is [`MsMaskPresent::AllZeros`] the buffers
/// are left untouched (identity matrix on every band).
///
/// Returns [`Error::MsStereoInvalid`] if the buffer / mask / `sfb_cb`
/// shapes disagree with `ics_info` (see the variant docs).
pub fn apply_ms_stereo(
    pair: &mut ChannelPairSpectra<'_>,
    ms_mask_present: MsMaskPresent,
    ms_used: &[Vec<bool>],
    ics_info: &IcsInfo,
    fs_index: u8,
) -> Result<()> {
    let ChannelPairSpectra {
        left,
        right,
        left_sfb_cb,
        right_sfb_cb,
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
        return Err(Error::MsStereoInvalid);
    }
    if ics_info.window_group_length.len() != num_groups
        || ics_info
            .window_group_length
            .iter()
            .map(|&w| w as usize)
            .sum::<usize>()
            != num_windows
    {
        return Err(Error::MsStereoInvalid);
    }
    // max_sfb must not exceed the band count of the active window.
    if max_sfb > num_swb {
        return Err(Error::MsStereoInvalid);
    }
    if left_sfb_cb.len() != num_groups || right_sfb_cb.len() != num_groups {
        return Err(Error::MsStereoInvalid);
    }
    // The per-band exclusions read sfb_cb[g][sfb] for sfb < max_sfb.
    for cb in left_sfb_cb.iter().chain(right_sfb_cb.iter()) {
        if cb.len() < max_sfb {
            return Err(Error::MsStereoInvalid);
        }
    }
    // `Mask` needs a full ms_used[g][sfb]; the other modes ignore it.
    if ms_mask_present == MsMaskPresent::Mask {
        if ms_used.len() != num_groups {
            return Err(Error::MsStereoInvalid);
        }
        for row in ms_used {
            if row.len() < max_sfb {
                return Err(Error::MsStereoInvalid);
            }
        }
    }

    if !ms_mask_present.is_active() {
        // mask_present == 0: identity matrix everywhere, nothing to do.
        return Ok(());
    }
    let all_ones = ms_mask_present == MsMaskPresent::AllOnes;

    let mut window_base = 0usize;
    for g in 0..num_groups {
        let wgl = ics_info.window_group_length[g] as usize;
        for sfb in 0..max_sfb {
            let band_on = all_ones || ms_used[g][sfb];
            if !band_on {
                continue;
            }
            // §4.6.8.1.3: intensity is keyed on the right channel,
            // noise on either channel; both suppress M/S.
            if is_intensity_cb(right_sfb_cb[g][sfb])
                || is_noise_cb(left_sfb_cb[g][sfb])
                || is_noise_cb(right_sfb_cb[g][sfb])
            {
                continue;
            }
            let start = offsets[sfb] as usize;
            let end = offsets[sfb + 1] as usize;
            for b in 0..wgl {
                let base = (window_base + b) * window_len;
                for i in start..end {
                    let l = left[base + i];
                    let r = right[base + i];
                    left[base + i] = l + r;
                    right[base + i] = l - r;
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
    use crate::section_data::ZERO_HCB;

    const FS_44100: u8 = 4;

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

    /// `sfb_cb` rows defaulting to a real spectrum book (here `2`).
    fn plain_cb(num_groups: usize, max_sfb: usize) -> Vec<Vec<u8>> {
        vec![vec![2u8; max_sfb]; num_groups]
    }

    /// Positional wrapper over [`apply_ms_stereo`] that bundles the
    /// channel pair into a [`ChannelPairSpectra`], keeping the test
    /// bodies terse.
    #[allow(clippy::too_many_arguments)]
    fn run(
        left: &mut [f64],
        right: &mut [f64],
        ms_mask_present: MsMaskPresent,
        ms_used: &[Vec<bool>],
        left_sfb_cb: &[Vec<u8>],
        right_sfb_cb: &[Vec<u8>],
        ics_info: &IcsInfo,
        fs_index: u8,
    ) -> Result<()> {
        let mut pair = ChannelPairSpectra {
            left,
            right,
            left_sfb_cb,
            right_sfb_cb,
        };
        apply_ms_stereo(&mut pair, ms_mask_present, ms_used, ics_info, fs_index)
    }

    #[test]
    fn from_bits_roundtrip() {
        for (bits, m) in [
            (0u8, MsMaskPresent::AllZeros),
            (1, MsMaskPresent::Mask),
            (2, MsMaskPresent::AllOnes),
        ] {
            assert_eq!(MsMaskPresent::from_bits(bits).unwrap(), m);
            assert_eq!(m.to_bits(), bits);
        }
        assert!(matches!(
            MsMaskPresent::from_bits(3),
            Err(Error::MsStereoInvalid)
        ));
    }

    #[test]
    fn all_zeros_is_identity() {
        let ics = long_ics_info(4);
        let mut l = vec![1.0f64; 1024];
        let mut r = vec![2.0f64; 1024];
        let cb = plain_cb(1, 4);
        run(
            &mut l,
            &mut r,
            MsMaskPresent::AllZeros,
            &[],
            &cb,
            &cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        assert!(l.iter().all(|&x| x == 1.0));
        assert!(r.iter().all(|&x| x == 2.0));
    }

    #[test]
    fn all_ones_dematrixes_every_band() {
        // max_sfb = 2; long-window band 0 = bins 0..4, band 1 = 4..8.
        let ics = long_ics_info(2);
        let offsets = long_window_offsets(FS_44100).unwrap();
        assert_eq!(offsets[0], 0);
        let mut l = vec![0.0f64; 1024];
        let mut r = vec![0.0f64; 1024];
        // m = 3, s = 1 in the first two bands → l' = 4, r' = 2.
        let band_end = offsets[2] as usize;
        for x in l.iter_mut().take(band_end) {
            *x = 3.0;
        }
        for x in r.iter_mut().take(band_end) {
            *x = 1.0;
        }
        let cb = plain_cb(1, 2);
        run(
            &mut l,
            &mut r,
            MsMaskPresent::AllOnes,
            &[],
            &cb,
            &cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        for i in 0..band_end {
            assert_eq!(l[i], 4.0, "l'[{i}]");
            assert_eq!(r[i], 2.0, "r'[{i}]");
        }
        // Bands at/above max_sfb are untouched.
        assert_eq!(l[band_end], 0.0);
        assert_eq!(r[band_end], 0.0);
    }

    #[test]
    fn mask_gates_per_band() {
        let ics = long_ics_info(2);
        let offsets = long_window_offsets(FS_44100).unwrap();
        let b0 = offsets[1] as usize; // end of band 0
        let b1 = offsets[2] as usize; // end of band 1
        let mut l = vec![0.0f64; 1024];
        let mut r = vec![0.0f64; 1024];
        for x in l.iter_mut().take(b1) {
            *x = 5.0;
        }
        for x in r.iter_mut().take(b1) {
            *x = 1.0;
        }
        let cb = plain_cb(1, 2);
        // band 0 on, band 1 off.
        let ms_used = vec![vec![true, false]];
        run(
            &mut l,
            &mut r,
            MsMaskPresent::Mask,
            &ms_used,
            &cb,
            &cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        // band 0 de-matrixed: l'=6, r'=4.
        for i in 0..b0 {
            assert_eq!(l[i], 6.0);
            assert_eq!(r[i], 4.0);
        }
        // band 1 untouched.
        for i in b0..b1 {
            assert_eq!(l[i], 5.0);
            assert_eq!(r[i], 1.0);
        }
    }

    #[test]
    fn intensity_band_excluded() {
        let ics = long_ics_info(1);
        let offsets = long_window_offsets(FS_44100).unwrap();
        let b0 = offsets[1] as usize;
        let mut l = vec![3.0f64; 1024];
        let mut r = vec![1.0f64; 1024];
        let left_cb = plain_cb(1, 1);
        // Right channel band 0 is intensity (15) → no de-matrix.
        let mut right_cb = plain_cb(1, 1);
        right_cb[0][0] = INTENSITY_HCB;
        run(
            &mut l,
            &mut r,
            MsMaskPresent::AllOnes,
            &[],
            &left_cb,
            &right_cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        for i in 0..b0 {
            assert_eq!(l[i], 3.0);
            assert_eq!(r[i], 1.0);
        }
    }

    #[test]
    fn noise_band_excluded_from_either_channel() {
        let ics = long_ics_info(1);
        let offsets = long_window_offsets(FS_44100).unwrap();
        let b0 = offsets[1] as usize;
        // Left channel band 0 is noise (13) → no de-matrix even though
        // the right channel is a real spectrum.
        let mut left_cb = plain_cb(1, 1);
        left_cb[0][0] = NOISE_HCB;
        let right_cb = plain_cb(1, 1);
        let mut l = vec![3.0f64; 1024];
        let mut r = vec![1.0f64; 1024];
        run(
            &mut l,
            &mut r,
            MsMaskPresent::AllOnes,
            &[],
            &left_cb,
            &right_cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        for i in 0..b0 {
            assert_eq!(l[i], 3.0);
            assert_eq!(r[i], 1.0);
        }
    }

    #[test]
    fn short_window_grouping_applies_per_window() {
        // Two groups: lengths [3, 5] summing to 8 short windows.
        let ics = short_ics_info(2, vec![3, 5]);
        let offsets = short_window_offsets(FS_44100).unwrap();
        let win = SHORT_WINDOW_LEN as usize;
        let bandlen = offsets[1] as usize; // band 0 width
        let mut l = vec![0.0f64; 8 * win];
        let mut r = vec![0.0f64; 8 * win];
        // Seed band 0 of every window with m=2, s=1.
        for w in 0..8 {
            for i in 0..bandlen {
                l[w * win + i] = 2.0;
                r[w * win + i] = 1.0;
            }
        }
        let cb = plain_cb(2, 2);
        // group 0 band 0 on; everything else off.
        let ms_used = vec![vec![true, false], vec![false, false]];
        run(
            &mut l,
            &mut r,
            MsMaskPresent::Mask,
            &ms_used,
            &cb,
            &cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        // Group 0 = windows 0,1,2 → de-matrixed (l'=3, r'=1).
        for w in 0..3 {
            for i in 0..bandlen {
                assert_eq!(l[w * win + i], 3.0, "win {w} band0");
                assert_eq!(r[w * win + i], 1.0);
            }
        }
        // Group 1 = windows 3..8 → untouched.
        for w in 3..8 {
            for i in 0..bandlen {
                assert_eq!(l[w * win + i], 2.0, "win {w} band0");
                assert_eq!(r[w * win + i], 1.0);
            }
        }
    }

    #[test]
    fn shape_mismatch_rejected() {
        let ics = long_ics_info(2);
        let cb = plain_cb(1, 2);
        let mut l = vec![0.0f64; 512]; // wrong length
        let mut r = vec![0.0f64; 1024];
        assert!(matches!(
            run(
                &mut l,
                &mut r,
                MsMaskPresent::AllOnes,
                &[],
                &cb,
                &cb,
                &ics,
                FS_44100,
            ),
            Err(Error::MsStereoInvalid)
        ));
    }

    #[test]
    fn mask_mode_requires_full_ms_used() {
        let ics = long_ics_info(3);
        let cb = plain_cb(1, 3);
        let mut l = vec![0.0f64; 1024];
        let mut r = vec![0.0f64; 1024];
        // ms_used row too short for max_sfb = 3.
        let ms_used = vec![vec![true, false]];
        assert!(matches!(
            run(
                &mut l,
                &mut r,
                MsMaskPresent::Mask,
                &ms_used,
                &cb,
                &cb,
                &ics,
                FS_44100,
            ),
            Err(Error::MsStereoInvalid)
        ));
    }

    #[test]
    fn dematrix_is_exactly_invertible_for_integers() {
        // l' = m+s, r' = m-s recovers (m,s) = ((l'+r')/2,(l'-r')/2).
        let ics = long_ics_info(1);
        let offsets = long_window_offsets(FS_44100).unwrap();
        let b0 = offsets[1] as usize;
        let cb = plain_cb(1, 1);
        let mut l = vec![0.0f64; 1024];
        let mut r = vec![0.0f64; 1024];
        for i in 0..b0 {
            l[i] = (i as f64) * 0.5 - 7.0; // m
            r[i] = 3.0 - (i as f64) * 0.25; // s
        }
        let m: Vec<f64> = l[..b0].to_vec();
        let s: Vec<f64> = r[..b0].to_vec();
        run(
            &mut l,
            &mut r,
            MsMaskPresent::AllOnes,
            &[],
            &cb,
            &cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        for i in 0..b0 {
            assert_eq!(l[i], m[i] + s[i]);
            assert_eq!(r[i], m[i] - s[i]);
        }
    }

    #[test]
    fn zero_hcb_bands_still_dematrix() {
        // A ZERO_HCB band carries no transmitted spectrum but is not
        // intensity/noise, so the M/S guard does not exclude it (its
        // coefficients are simply 0 on both sides → stays 0).
        let ics = long_ics_info(1);
        let offsets = long_window_offsets(FS_44100).unwrap();
        let b0 = offsets[1] as usize;
        let mut left_cb = plain_cb(1, 1);
        left_cb[0][0] = ZERO_HCB;
        let mut right_cb = plain_cb(1, 1);
        right_cb[0][0] = ZERO_HCB;
        let mut l = vec![0.0f64; 1024];
        let mut r = vec![0.0f64; 1024];
        run(
            &mut l,
            &mut r,
            MsMaskPresent::AllOnes,
            &[],
            &left_cb,
            &right_cb,
            &ics,
            FS_44100,
        )
        .unwrap();
        for i in 0..b0 {
            assert_eq!(l[i], 0.0);
            assert_eq!(r[i], 0.0);
        }
    }
}
