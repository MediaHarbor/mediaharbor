//! Per-channel "decoded spectrum" pipeline stage — ISO/IEC 14496-3
//! §4.6.3.3 `quant_to_spec()` + the parse → dequant → scalefactor →
//! TNS composition.
//!
//! This module chains the per-tool reconstruction primitives into
//! the channel-level stage that ends one step short of the
//! filterbank (§4.6.11 IMDCT + window-overlap-add, a follow-up
//! round):
//!
//! 1. **Pulse fix-up** (§4.6.3.3) — when `pulse_data_present`, fold
//!    the `±pulse_amp` corrections into `x_quant` via
//!    [`crate::swb_offset::apply_pulse_data`] (long windows only,
//!    per Table 4.50 Note 1).
//! 2. **Scalefactor accumulation** (§4.6.2.3.2 / §4.6.8.1.4 /
//!    §4.6.13) — [`crate::scale_factor_data::accumulate`].
//! 3. **Inverse quantization + rescaling** (§4.6.1.3 / §4.6.2.3.3)
//!    — [`crate::dequant::rescale_spectrum`].
//! 4. **De-interleaving** (§4.6.3.3 `quant_to_spec()`) — from the
//!    §4.5.2.3.5 group-interleaved transmission order to the
//!    window-major `spec[w][k]` layout that TNS and the filterbank
//!    consume ([`quant_to_spec`]).
//! 5. **TNS** (§4.6.9) — when `tns_data_present`,
//!    [`crate::tns_frame::tns_decode_frame`] over the de-interleaved
//!    spectrum.
//!
//! ## §4.6.3.3 `quant_to_spec()`
//!
//! ```text
//! quant_to_spec() {
//!     k = 0;
//!     for (g = 0; g < num_window_groups; g++ ) {
//!         j = 0;
//!         for (sfb = 0; sfb < num_swb; sfb++) {
//!             width = swb_offset[sfb+1] - swb_offset[sfb];
//!             for (win = 0; win < window_group_length[g]; win++) {
//!                 for (bin = 0; bin < width; bin++) {
//!                     spec[win+k][bin+j] = x_quant[g][win][sfb][bin];
//!                 }
//!             }
//!             j += width;
//!         }
//!         k += window_group_length[g];
//!     }
//! }
//! ```
//!
//! The interleaved source reads linearly in exactly the loop order
//! (`g`, `sfb`, `win`, `bin`) because §4.5.2.3.5 stores each
//! virtual scalefactor band as the concatenated per-window
//! scalefactor-window-band slices. For the long window sequences
//! (`num_window_groups == 1`, `window_group_length[0] == 1`) the
//! mapping degenerates to an identity copy.
//!
//! ## Scope
//!
//! Intensity stereo (§4.6.8.2), M/S (§4.6.8.1), and PNS (§4.6.13)
//! reconstruction are channel-*pair* / noise-synthesis tools that
//! slot between steps 4 and 5 (de-interleave → joint-stereo → TNS).
//! The M/S de-matrix is implemented as
//! [`crate::ms_stereo::apply_ms_stereo`], a CPE-level pass over the
//! two channels' de-interleaved spectra; this single-channel stage
//! does not invoke it (the caller runs it on the pair before each
//! channel's TNS). Intensity stereo and PNS synthesis remain
//! follow-ups, so intensity / `NOISE_HCB` bands still come out as
//! silence here. The Main-profile predictor (§4.6.7) and LTP
//! (§4.6.6) are likewise deferred.

use crate::dequant::rescale_spectrum;
use crate::ics_body::IcsBody;
use crate::ics_info::IcsInfo;
use crate::scale_factor_data::accumulate;
use crate::spectral_data::SpectralData;
use crate::swb_offset::{
    apply_pulse_data, long_window_offsets, short_window_offsets, LONG_WINDOW_LEN, SHORT_WINDOW_LEN,
};
use crate::tns_frame::tns_decode_frame;
use crate::{Error, Result};

/// §4.6.3.3 `quant_to_spec()` — de-interleave per-group
/// transmission-order coefficient buffers into the window-major
/// `spec[w][k]` layout (windows concatenated:
/// `spec[w * window_len + k]`).
///
/// * `groups` — one buffer per window group in the §4.5.2.3.5
///   interleaved order, each spanning the full group
///   (`window_group_length[g] × 128` short, `1024` long) — the
///   shape produced by [`SpectralData::parse`] and preserved by
///   [`rescale_spectrum`].
/// * `ics_info` / `fs_index` — grouping and the Table 4.129-family
///   `swb_offset` table.
///
/// Errors:
///
/// * [`Error::IcsInfoUnsupportedSampleRateIndex`] — `fs_index`
///   outside the SWB-table range.
/// * [`Error::QuantToSpecInvalid`] — group count or a group buffer
///   length disagreeing with the `ics_info` grouping, or a grouping
///   whose `window_group_length` sum is not `num_windows`.
pub fn quant_to_spec(groups: &[Vec<f64>], ics_info: &IcsInfo, fs_index: u8) -> Result<Vec<f64>> {
    let (window_len, offsets) = if ics_info.window_sequence.is_eight_short() {
        (SHORT_WINDOW_LEN as usize, short_window_offsets(fs_index)?)
    } else {
        (LONG_WINDOW_LEN as usize, long_window_offsets(fs_index)?)
    };
    let num_swb = offsets.len() - 1;
    let num_windows = ics_info.num_windows as usize;
    let num_groups = ics_info.num_window_groups as usize;

    if groups.len() != num_groups
        || ics_info.window_group_length.len() != num_groups
        || ics_info
            .window_group_length
            .iter()
            .map(|&w| w as usize)
            .sum::<usize>()
            != num_windows
    {
        return Err(Error::QuantToSpecInvalid);
    }

    let mut spec = vec![0.0f64; num_windows * window_len];
    // `k` in the pseudocode: index of the group's first window.
    let mut window_base = 0usize;
    for (g, group) in groups.iter().enumerate() {
        let wgl = ics_info.window_group_length[g] as usize;
        if group.len() != wgl * window_len {
            return Err(Error::QuantToSpecInvalid);
        }
        // The interleaved buffer reads linearly in (sfb, win, bin)
        // order; `j` is the in-window coefficient offset of the
        // current scalefactor window band.
        let mut src = group.iter();
        let mut j = 0usize;
        for sfb in 0..num_swb {
            let width = (offsets[sfb + 1] - offsets[sfb]) as usize;
            for win in 0..wgl {
                let dst = (window_base + win) * window_len + j;
                for bin in 0..width {
                    // group.len() == wgl * window_len == wgl * sum of
                    // widths, so the iterator yields exactly enough.
                    spec[dst + bin] = *src.next().expect("group length checked above");
                }
            }
            j += width;
        }
        window_base += wgl;
    }
    Ok(spec)
}

/// Decode one channel's spectrum: pulse fix-up → scalefactor
/// accumulation → inverse quantization + rescaling →
/// `quant_to_spec()` → TNS.
///
/// * `body` — the parsed Table 4.50 channel body
///   ([`IcsBody::parse`] / [`IcsBody::parse_with_ics_info`]).
/// * `ics_info` — the channel's `ics_info()`; pass
///   `body.ics_info.as_ref().unwrap()` for the inline form or the
///   externally-held shared `IcsInfo` for the CPE
///   `common_window == 1` form.
/// * `spectral` — the channel's parsed Table 4.56 spectrum
///   ([`SpectralData::parse`] resumed at
///   `body.spectral_data_bit_offset`).
/// * `aot` / `fs_index` — `audioObjectType` and
///   `samplingFrequencyIndex`, driving the TNS clamp tables and the
///   `swb_offset` selection.
///
/// Returns the window-major decoded spectrum (`num_windows ×
/// window_len` = 1024 coefficients, window `w` at
/// `spec[w * window_len ..]`) — the §4.6.11 filterbank's input.
///
/// Errors propagate from the composed stages: see
/// [`apply_pulse_data`], [`accumulate`], [`rescale_spectrum`],
/// [`quant_to_spec`], and [`tns_decode_frame`].
pub fn decode_channel_spectrum(
    body: &IcsBody,
    ics_info: &IcsInfo,
    spectral: &SpectralData,
    aot: u8,
    fs_index: u8,
) -> Result<Vec<f64>> {
    // 1. §4.6.3.3 pulse fix-up on the quantised spectrum (long
    //    windows only — the parser already rejects the EIGHT_SHORT
    //    combination, and a long sequence has exactly one group).
    let x_quant: &SpectralData = &if let Some(pd) = &body.pulse_data {
        let mut patched = spectral.clone();
        let group0 = patched.x_quant.first_mut().ok_or(Error::DequantInvalid)?;
        apply_pulse_data(group0, fs_index, pd)?;
        patched
    } else {
        spectral.clone()
    };

    // 2. §4.6.2.3.2 scalefactor accumulation.
    let scale_factors = accumulate(
        &body.scale_factor_data,
        &body.section_data.sfb_cb,
        body.global_gain,
    )?;

    // 3. §4.6.1.3 + §4.6.2.3.3 inverse quantization + rescaling.
    let rescaled = rescale_spectrum(
        x_quant,
        &scale_factors,
        &body.section_data.sfb_cb,
        ics_info,
        fs_index,
    )?;

    // 4. §4.6.3.3 quant_to_spec() de-interleaving.
    let mut spec = quant_to_spec(&rescaled, ics_info, fs_index)?;

    // 5. §4.6.9 TNS.
    if let Some(tns) = &body.tns_data {
        tns_decode_frame(
            &mut spec,
            tns,
            ics_info.window_sequence,
            ics_info.max_sfb,
            aot,
            fs_index,
        )?;
    }
    Ok(spec)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::{WindowSequence, WindowShape};

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
            num_swb: crate::ics_info::NUM_SWB_SHORT_WINDOW[4],
        }
    }

    // ===== quant_to_spec =====

    #[test]
    fn quant_to_spec_long_is_identity() {
        let info = long_ics_info(10);
        let group: Vec<f64> = (0..1024).map(|i| i as f64 * 0.5 - 100.0).collect();
        let spec = quant_to_spec(core::slice::from_ref(&group), &info, 4).unwrap();
        assert_eq!(spec, group);
    }

    #[test]
    fn quant_to_spec_short_deinterleaves_grouped_windows() {
        // Grouping 5 + 3 at fs_index 4 (short band 0 is 4 wide,
        // band 1 is 4 wide, ...). Place markers at known
        // (g, sfb, win, bin) coordinates and verify their
        // window-major destinations.
        let info = short_ics_info(2, vec![5, 3]);
        let mut g0 = vec![0.0f64; 5 * 128];
        let mut g1 = vec![0.0f64; 3 * 128];
        // (g=0, sfb=0, win=2, bin=1): interleaved index
        // 0 + 2*4 + 1 = 9 -> spec window 2, coefficient 1.
        g0[9] = 1.0;
        // (g=0, sfb=1, win=4, bin=3): interleaved index
        // 5*4 + 4*4 + 3 = 39 -> spec window 4, coefficient 4+3.
        g0[39] = 2.0;
        // (g=1, sfb=0, win=0, bin=0): -> spec window 5 (groups 0..4
        // are group 0), coefficient 0.
        g1[0] = 3.0;
        // (g=1, sfb=1, win=2, bin=2): interleaved index
        // 3*4 + 2*4 + 2 = 22 -> spec window 7, coefficient 6.
        g1[22] = 4.0;
        let spec = quant_to_spec(&[g0, g1], &info, 4).unwrap();
        assert_eq!(spec.len(), 1024);
        assert_eq!(spec[2 * 128 + 1], 1.0);
        assert_eq!(spec[4 * 128 + 7], 2.0);
        assert_eq!(spec[5 * 128], 3.0);
        assert_eq!(spec[7 * 128 + 6], 4.0);
        let placed = spec.iter().filter(|&&v| v != 0.0).count();
        assert_eq!(placed, 4);
    }

    #[test]
    fn quant_to_spec_short_full_table_round_trips_every_coefficient() {
        // Tag every interleaved coefficient with a unique value and
        // verify the de-interleave is a permutation reaching all
        // 1024 slots.
        let info = short_ics_info(14, vec![1, 2, 1, 4]);
        let mut groups = Vec::new();
        let mut tag = 1.0f64;
        for &wgl in &info.window_group_length {
            let mut g = vec![0.0f64; wgl as usize * 128];
            for slot in g.iter_mut() {
                *slot = tag;
                tag += 1.0;
            }
            groups.push(g);
        }
        let spec = quant_to_spec(&groups, &info, 4).unwrap();
        let mut seen: Vec<f64> = spec.clone();
        seen.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let want: Vec<f64> = (1..=1024).map(|i| i as f64).collect();
        assert_eq!(seen, want);
    }

    #[test]
    fn quant_to_spec_rejects_shape_mismatches() {
        let info = short_ics_info(2, vec![5, 3]);
        // Wrong group count.
        assert!(matches!(
            quant_to_spec(&[vec![0.0; 5 * 128]], &info, 4),
            Err(Error::QuantToSpecInvalid)
        ));
        // Wrong group buffer length.
        assert!(matches!(
            quant_to_spec(&[vec![0.0; 5 * 128], vec![0.0; 2 * 128]], &info, 4),
            Err(Error::QuantToSpecInvalid)
        ));
        // Grouping that does not sum to num_windows.
        let bad = short_ics_info(2, vec![5, 2]);
        assert!(matches!(
            quant_to_spec(&[vec![0.0; 5 * 128], vec![0.0; 2 * 128]], &bad, 4),
            Err(Error::QuantToSpecInvalid)
        ));
        // Unsupported fs_index propagates.
        let info = long_ics_info(2);
        assert!(matches!(
            quant_to_spec(&[vec![0.0; 1024]], &info, 12),
            Err(Error::IcsInfoUnsupportedSampleRateIndex(12))
        ));
    }
}
