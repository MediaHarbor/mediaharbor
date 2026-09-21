//! §4.6.1.3 inverse quantization + §4.6.2.3.3 scalefactor
//! application — ISO/IEC 14496-3.
//!
//! The first numeric reconstruction stage after the Table 4.56
//! `spectral_data()` wire walk: convert the quantised integer
//! spectrum `x_quant` into the rescaled real-valued spectrum
//! `x_rescal` that the downstream tools (TNS, filterbank) consume.
//!
//! ## §4.6.1.3 — inverse quantization
//!
//! The encoder's non-uniform quantizer is inverted per coefficient:
//!
//! ```text
//! x_invquant = Sign(x_quant) * |x_quant|^(4/3)
//! ```
//!
//! The maximum allowed absolute amplitude for `x_quant` is 8191
//! ([`crate::spectral_codebook::MAX_QUANT`]); the wire walker
//! already enforces it, so [`inverse_quantize`] accepts any `i32`
//! and leaves range policing to the parser.
//!
//! ## §4.6.2.3.3 — applying scalefactors
//!
//! Every scalefactor band is rescaled by the gain of its absolute
//! scalefactor:
//!
//! ```text
//! gain = 2^(0.25 * (sf[g][sfb] - SF_OFFSET))      SF_OFFSET = 100
//! x_rescal[...] = x_invquant[...] * gain
//! ```
//!
//! per the §4.6.2.3.3 pseudocode, with the same gain applied to all
//! grouped short windows of a (virtual) scalefactor band. The
//! band → coefficient mapping is the §4.5.2.3.4
//! [`crate::spectral_data::sect_sfb_offset`] derivation, so the
//! whole operation runs directly over the §4.5.2.3.5 interleaved
//! transmission-order buffers produced by
//! [`crate::spectral_data::SpectralData::parse`].
//!
//! Bands whose codebook carries no spectrum keep a `0.0` output:
//! `ZERO_HCB` bands and bands at or above `max_sfb` transmit
//! nothing (and the wire walker leaves their `x_quant` at 0), while
//! `NOISE_HCB` / intensity bands are reconstructed by the PNS /
//! intensity-stereo tools (§4.6.13 / §4.6.8) which are not part of
//! this stage — their [`AbsoluteScaleFactorEntry::NoiseNrg`] /
//! [`AbsoluteScaleFactorEntry::IsPos`] records are consumed (to
//! keep the wire-order lockstep) but produce no rescaled energy
//! here.

use crate::ics_info::IcsInfo;
use crate::scale_factor_data::{AbsoluteScaleFactorEntry, AbsoluteScaleFactors};
use crate::section_data::{Codebook, ZERO_HCB};
use crate::spectral_data::{sect_sfb_offset, SpectralData};
use crate::{Error, Result};

/// `SF_OFFSET` per §4.6.2.3.3 — the scalefactor that maps to unit
/// gain. "The constant SF_OFFSET must be set to 100."
pub const SF_OFFSET: i32 = 100;

/// §4.6.1.3 inverse quantization of one coefficient:
/// `Sign(x_quant) · |x_quant|^(4/3)`.
#[inline]
pub fn inverse_quantize(x_quant: i32) -> f64 {
    // |x|^(4/3) computed as |x| · |x|^(1/3): `cbrt` is correctly
    // rounded, so perfect cubes (and 0 / ±1) invert exactly, and the
    // general case avoids the representation error of the literal
    // exponent 4/3.
    let abs = f64::from(x_quant.unsigned_abs());
    let mag = abs * abs.cbrt();
    if x_quant < 0 {
        -mag
    } else {
        mag
    }
}

/// §4.6.2.3.3 `get_scale_factor_gain()`:
/// `2^(0.25 · (sf − SF_OFFSET))`.
#[inline]
pub fn scale_factor_gain(sf: u8) -> f64 {
    (0.25 * f64::from(i32::from(sf) - SF_OFFSET)).exp2()
}

/// Run §4.6.1.3 inverse quantization and §4.6.2.3.3 scalefactor
/// application over one channel's quantised spectrum.
///
/// * `spectral` — the per-group interleaved `x_quant` buffers from
///   [`SpectralData::parse`] (or the same shape with the §4.6.3.3
///   pulse fix-up already folded in via
///   [`crate::swb_offset::apply_pulse_data`]).
/// * `scale_factors` — the absolute per-band records from
///   [`crate::scale_factor_data::accumulate`].
/// * `sfb_cb` — the per-`(g, sfb)` codebook map from
///   [`crate::section_data::SectionData::parse`].
/// * `ics_info` / `fs_index` — drive the §4.5.2.3.4 band →
///   coefficient mapping and the group buffer shapes.
///
/// Returns the rescaled spectrum `x_rescal` in the same per-group
/// interleaved layout (and lengths) as the input `x_quant`.
///
/// Errors:
///
/// * [`Error::IcsInfoUnsupportedSampleRateIndex`] /
///   [`Error::SpectralDataInvalid`] — propagated from
///   [`sect_sfb_offset`] (`fs_index` out of range, `max_sfb` above
///   `num_swb`).
/// * [`Error::DequantInvalid`] — structural mismatch between the
///   three inputs: group counts disagreeing with
///   `num_window_groups`, a group buffer length disagreeing with
///   `window_group_length[g] × 128` (or 1024 long), or a
///   scalefactor-entry sequence that does not match the
///   non-`ZERO_HCB` codebook classification of `sfb_cb` (including
///   the reserved codebook 12, which carries a scalefactor on the
///   wire but has no spectrum semantics to rescale).
pub fn rescale_spectrum(
    spectral: &SpectralData,
    scale_factors: &AbsoluteScaleFactors,
    sfb_cb: &[Vec<u8>],
    ics_info: &IcsInfo,
    fs_index: u8,
) -> Result<Vec<Vec<f64>>> {
    let offsets = sect_sfb_offset(ics_info, fs_index)?;
    let num_groups = ics_info.num_window_groups as usize;
    if spectral.x_quant.len() != num_groups
        || scale_factors.entries.len() != num_groups
        || sfb_cb.len() != num_groups
    {
        return Err(Error::DequantInvalid);
    }

    let mut out = Vec::with_capacity(num_groups);
    for (g, group_offsets) in offsets.iter().enumerate() {
        let x_quant = &spectral.x_quant[g];
        let expected_len = if ics_info.window_sequence.is_eight_short() {
            ics_info.window_group_length[g] as usize * crate::swb_offset::SHORT_WINDOW_LEN as usize
        } else {
            crate::swb_offset::LONG_WINDOW_LEN as usize
        };
        if x_quant.len() != expected_len || sfb_cb[g].len() != ics_info.max_sfb as usize {
            return Err(Error::DequantInvalid);
        }

        let mut rescal = vec![0.0f64; x_quant.len()];
        let mut entries = scale_factors.entries[g].iter();
        for (sfb, &cb) in sfb_cb[g].iter().enumerate() {
            if cb == ZERO_HCB {
                continue;
            }
            let entry = entries.next().ok_or(Error::DequantInvalid)?;
            let kind = Codebook::from_value(cb);
            match entry {
                AbsoluteScaleFactorEntry::Sf(sf)
                    if matches!(
                        kind,
                        Codebook::Quad { .. } | Codebook::Pair { .. } | Codebook::Esc
                    ) =>
                {
                    let gain = scale_factor_gain(*sf);
                    let start = group_offsets[sfb] as usize;
                    let end = group_offsets[sfb + 1] as usize;
                    for k in start..end {
                        rescal[k] = inverse_quantize(x_quant[k]) * gain;
                    }
                }
                // PNS / intensity bands transmit no spectrum; their
                // §4.6.13 / §4.6.8 reconstruction happens in the
                // dedicated tools, not the rescale stage. Consume
                // the record to keep the wire-order lockstep.
                AbsoluteScaleFactorEntry::NoiseNrg(_) if kind.is_noise() => {}
                AbsoluteScaleFactorEntry::IsPos(_) if kind.is_intensity() => {}
                _ => return Err(Error::DequantInvalid),
            }
        }
        if entries.next().is_some() {
            return Err(Error::DequantInvalid);
        }
        out.push(rescal);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::{WindowSequence, WindowShape};
    use crate::section_data::{INTENSITY_HCB, NOISE_HCB};

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
            num_swb: crate::ics_info::NUM_SWB_LONG_WINDOW[4],
        }
    }

    // ===== inverse_quantize =====

    #[test]
    fn inverse_quantize_pins_exact_cubes() {
        // |x|^(4/3) is exact when |x| is a perfect cube:
        // 8 = 2^3 -> 2^4, 27 = 3^3 -> 3^4, 64 = 4^3 -> 4^4,
        // 729 = 3^6 -> 3^8, 4096 = 2^12 -> 2^16.
        assert_eq!(inverse_quantize(0), 0.0);
        assert_eq!(inverse_quantize(1), 1.0);
        assert_eq!(inverse_quantize(-1), -1.0);
        assert_eq!(inverse_quantize(8), 16.0);
        assert_eq!(inverse_quantize(-8), -16.0);
        assert_eq!(inverse_quantize(27), 81.0);
        assert_eq!(inverse_quantize(-27), -81.0);
        assert_eq!(inverse_quantize(64), 256.0);
        assert_eq!(inverse_quantize(729), 6561.0);
        assert_eq!(inverse_quantize(4096), 65536.0);
        assert_eq!(inverse_quantize(-4096), -65536.0);
    }

    #[test]
    fn inverse_quantize_is_odd_and_monotonic_up_to_max_quant() {
        let mut prev = 0.0;
        for x in 1..=8191 {
            let y = inverse_quantize(x);
            assert!(y > prev, "monotonic at {x}");
            assert_eq!(inverse_quantize(-x), -y, "odd symmetry at {x}");
            prev = y;
        }
        // 8191^(4/3) is a bit above 8191 * 8191^(1/3) ~ 164k.
        assert!(prev > 160_000.0 && prev < 170_000.0);
    }

    // ===== scale_factor_gain =====

    #[test]
    fn scale_factor_gain_pins_exact_powers() {
        // sf = SF_OFFSET -> 1; every +4 doubles, every -4 halves.
        assert_eq!(scale_factor_gain(100), 1.0);
        assert_eq!(scale_factor_gain(104), 2.0);
        assert_eq!(scale_factor_gain(108), 4.0);
        assert_eq!(scale_factor_gain(96), 0.5);
        assert_eq!(scale_factor_gain(92), 0.25);
        // sf = 0 -> 2^-25; sf = 255 -> 2^38.75.
        assert_eq!(scale_factor_gain(0), (-25.0f64).exp2());
        assert_eq!(scale_factor_gain(255), 38.75f64.exp2());
        // Quarter-step: sf = 101 -> 2^0.25.
        assert_eq!(scale_factor_gain(101), 0.25f64.exp2());
    }

    // ===== rescale_spectrum =====

    /// One long window, two bands on a spectrum book: band gains are
    /// applied per band over the swb ranges.
    #[test]
    fn rescale_applies_per_band_gain_over_swb_ranges() {
        // fs_index 4 long: bands 0 and 1 are 4 coefficients each.
        let info = long_ics_info(2);
        let sfb_cb = vec![vec![1u8, 1]];
        let mut x_quant = vec![0i32; 1024];
        x_quant[..8].copy_from_slice(&[1, -1, 0, 8, -8, 1, 0, -1]);
        let spectral = SpectralData {
            x_quant: vec![x_quant],
        };
        // Band 0 at sf 104 (gain 2), band 1 at sf 96 (gain 0.5).
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![
                AbsoluteScaleFactorEntry::Sf(104),
                AbsoluteScaleFactorEntry::Sf(96),
            ]],
        };
        let out = rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].len(), 1024);
        // Band 0: x_invquant * 2.
        assert_eq!(out[0][..4], [2.0, -2.0, 0.0, 32.0]);
        // Band 1: x_invquant * 0.5.
        assert_eq!(out[0][4..8], [-8.0, 0.5, 0.0, -0.5]);
        // Above max_sfb: all zero.
        assert!(out[0][8..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn rescale_leaves_noise_and_intensity_bands_at_zero() {
        let info = long_ics_info(3);
        let sfb_cb = vec![vec![NOISE_HCB, INTENSITY_HCB, 2]];
        let mut x_quant = vec![0i32; 1024];
        // Only band 2 (coefficients 8..12) carries spectrum.
        x_quant[8..12].copy_from_slice(&[1, 1, -1, 0]);
        let spectral = SpectralData {
            x_quant: vec![x_quant],
        };
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![
                AbsoluteScaleFactorEntry::NoiseNrg(-50),
                AbsoluteScaleFactorEntry::IsPos(3),
                AbsoluteScaleFactorEntry::Sf(100),
            ]],
        };
        let out = rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4).unwrap();
        assert!(out[0][..8].iter().all(|&v| v == 0.0));
        assert_eq!(out[0][8..12], [1.0, 1.0, -1.0, 0.0]);
    }

    #[test]
    fn rescale_skips_zero_hcb_bands_without_consuming_entries() {
        let info = long_ics_info(3);
        let sfb_cb = vec![vec![ZERO_HCB, 1, ZERO_HCB]];
        let mut x_quant = vec![0i32; 1024];
        x_quant[4..8].copy_from_slice(&[1, 0, 0, -1]);
        let spectral = SpectralData {
            x_quant: vec![x_quant],
        };
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![AbsoluteScaleFactorEntry::Sf(108)]],
        };
        let out = rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4).unwrap();
        assert_eq!(out[0][4..8], [4.0, 0.0, 0.0, -4.0]);
        assert!(out[0][..4].iter().all(|&v| v == 0.0));
        assert!(out[0][8..].iter().all(|&v| v == 0.0));
    }

    /// EIGHT_SHORT grouping: the same gain covers all grouped short
    /// windows of a virtual band (§4.6.2.3.3 "all coefficients in
    /// grouped scalefactor window bands ... same scalefactor").
    #[test]
    fn rescale_short_grouped_band_shares_one_gain() {
        let info = IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::EightShort,
            window_shape: WindowShape::Sine,
            max_sfb: 1,
            scale_factor_grouping: Some(0),
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: false,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 8,
            num_window_groups: 2,
            window_group_length: vec![5, 3],
            num_swb: crate::ics_info::NUM_SWB_SHORT_WINDOW[4],
        };
        let sfb_cb = vec![vec![1u8], vec![1u8]];
        // fs 4 short band 0 is 4 wide; virtual band = wgl * 4.
        let mut g0 = vec![0i32; 5 * 128];
        for (i, slot) in g0.iter_mut().take(20).enumerate() {
            *slot = if i % 2 == 0 { 1 } else { -1 };
        }
        let mut g1 = vec![0i32; 3 * 128];
        for slot in g1.iter_mut().take(12) {
            *slot = 8;
        }
        let spectral = SpectralData {
            x_quant: vec![g0, g1],
        };
        let sf = AbsoluteScaleFactors {
            entries: vec![
                vec![AbsoluteScaleFactorEntry::Sf(104)],
                vec![AbsoluteScaleFactorEntry::Sf(96)],
            ],
        };
        let out = rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4).unwrap();
        for (i, &v) in out[0].iter().take(20).enumerate() {
            let want = if i % 2 == 0 { 2.0 } else { -2.0 };
            assert_eq!(v, want, "g0[{i}]");
        }
        assert!(out[0][20..].iter().all(|&v| v == 0.0));
        for (i, &v) in out[1].iter().take(12).enumerate() {
            assert_eq!(v, 8.0, "g1[{i}]");
        }
        assert!(out[1][12..].iter().all(|&v| v == 0.0));
    }

    #[test]
    fn rescale_rejects_entry_codebook_mismatch() {
        let info = long_ics_info(1);
        let sfb_cb = vec![vec![1u8]];
        let spectral = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        // IsPos entry against a spectrum book.
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![AbsoluteScaleFactorEntry::IsPos(0)]],
        };
        assert!(matches!(
            rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4),
            Err(Error::DequantInvalid)
        ));
    }

    #[test]
    fn rescale_rejects_reserved_codebook_12() {
        let info = long_ics_info(1);
        let sfb_cb = vec![vec![12u8]];
        let spectral = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![AbsoluteScaleFactorEntry::Sf(100)]],
        };
        assert!(matches!(
            rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4),
            Err(Error::DequantInvalid)
        ));
    }

    #[test]
    fn rescale_rejects_surplus_and_missing_entries() {
        let info = long_ics_info(1);
        let sfb_cb = vec![vec![1u8]];
        let spectral = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        let missing = AbsoluteScaleFactors {
            entries: vec![vec![]],
        };
        assert!(matches!(
            rescale_spectrum(&spectral, &missing, &sfb_cb, &info, 4),
            Err(Error::DequantInvalid)
        ));
        let surplus = AbsoluteScaleFactors {
            entries: vec![vec![
                AbsoluteScaleFactorEntry::Sf(100),
                AbsoluteScaleFactorEntry::Sf(100),
            ]],
        };
        assert!(matches!(
            rescale_spectrum(&spectral, &surplus, &sfb_cb, &info, 4),
            Err(Error::DequantInvalid)
        ));
    }

    #[test]
    fn rescale_rejects_wrong_group_buffer_length() {
        let info = long_ics_info(1);
        let sfb_cb = vec![vec![1u8]];
        let spectral = SpectralData {
            x_quant: vec![vec![0i32; 512]],
        };
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![AbsoluteScaleFactorEntry::Sf(100)]],
        };
        assert!(matches!(
            rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4),
            Err(Error::DequantInvalid)
        ));
    }

    #[test]
    fn rescale_rejects_group_count_mismatch() {
        let info = long_ics_info(1);
        let sfb_cb = vec![vec![1u8], vec![1u8]];
        let spectral = SpectralData {
            x_quant: vec![vec![0i32; 1024]],
        };
        let sf = AbsoluteScaleFactors {
            entries: vec![vec![AbsoluteScaleFactorEntry::Sf(100)]],
        };
        assert!(matches!(
            rescale_spectrum(&spectral, &sf, &sfb_cb, &info, 4),
            Err(Error::DequantInvalid)
        ));
    }
}
