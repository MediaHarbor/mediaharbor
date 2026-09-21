//! §4.6.13 Perceptual Noise Substitution (PNS) synthesis — ISO/IEC
//! 14496-3.
//!
//! PNS replaces the Huffman-coded / inverse-quantised spectrum of a
//! noise-like scalefactor band with a freshly generated random vector
//! scaled to a transmitted target energy. It is the third channel /
//! noise tool in the §4.6 decode chain (after M/S and before intensity
//! stereo), signalled by the pseudo codebook `NOISE_HCB` (13) in a
//! band's `sfb_cb`. Because no spectral coefficients are transmitted
//! for such a band (during Huffman decoding `NOISE_HCB` is treated
//! exactly like `ZERO_HCB`, §4.6.13.5), the in-band coefficients arrive
//! as silence and this tool fills them.
//!
//! ## §4.6.13.3 decoding process
//!
//! The energy of a noise band is carried by `noise_nrg[g][sfb]` — a
//! value coded *exactly like a scalefactor* (Huffman-DPCM, with the
//! first PNS band of the frame sent as a 9-bit literal) on its own DPCM
//! track seeded at `global_gain - NOISE_OFFSET - 256`
//! (`NOISE_OFFSET == 90`). That accumulation is the upstream job of
//! [`crate::scale_factor_data::accumulate`]; this module consumes the
//! absolute `noise_nrg[g][sfb]` it produces.
//!
//! The per-band synthesis (ISO/IEC 14496-3:2009 §4.6.13.3 pseudo code)
//! is:
//!
//! ```text
//! size     = swb_offset[sfb+1] - swb_offset[sfb];
//! gen_rand_vector(&spec[..], size);          /* random vector       */
//! nrg = 0; for i in 0..size { nrg += spec[i]*spec[i]; }
//! sqrt_nrg = sqrt(nrg);
//! scale   *= 2.0^(0.25 * noise_nrg[g][sfb]) / sqrt_nrg;
//! for i in 0..size { spec[i] *= scale; }
//! ```
//!
//! The 2009 revision normalises by the **measured** energy of the
//! generated vector (`sqrt_nrg = sqrt(Σ spec²)`) rather than by an
//! assumed per-sample average energy `MEAN_NRG` (the 2001 form). This
//! removes the dependence on any particular generator's variance: the
//! band that comes out has L2 norm
//!
//! ```text
//! ‖spec‖₂ = sqrt(Σ (spec[i]·scale)²)
//!         = scale · sqrt_nrg
//!         = 2.0^(0.25 · noise_nrg[g][sfb])
//! ```
//!
//! exactly, independent of which random vector was drawn (as long as it
//! is non-zero). The target energy is therefore **spec-determined and
//! deterministic**; only the per-coefficient *phase* of the band is a
//! function of the generator, which the standard deliberately leaves
//! open ("a suitable random number generator can be realized using one
//! multiplication/accumulation per random value", §4.6.13.3). The
//! `2.0^(0.25·noise_nrg)` energy ladder is the same per-quarter-step
//! gain as the §4.6.2.3.3 scalefactor gain.
//!
//! ## Generator
//!
//! [`gen_rand_vector`] is the default generator: a 32-bit
//! multiply-accumulate LCG mapped to signed `f64` values in
//! `[-1.0, 1.0)`, one multiply-accumulate per coefficient as the spec
//! suggests. Because the final scaling normalises away the generator's
//! amplitude, only its *zero-sum-of-squares-avoidance* matters for
//! correctness (a band of length ≥ 1 from this generator always has a
//! non-zero sum of squares). [`apply_pns`] takes the generator as a
//! closure so a caller can substitute a different (e.g. bit-exact
//! reference) source without changing the synthesis maths.
//!
//! ## §4.6.13.3 channel-pair correlation (`ms_used`)
//!
//! For a channel pair, if the **same** `(group, sfb)` is `NOISE_HCB` in
//! **both** channels and the band's `ms_used` bit is set (or
//! `ms_mask_present == 2`), the *same* random vector is used for both
//! channels (correlated noise); otherwise each channel draws its own
//! (independent noise). No M/S de-matrix is applied to such a band —
//! PNS and M/S are mutually exclusive (§4.6.13.5), so a set `ms_used`
//! bit on a both-channels-noise band selects the shared vector rather
//! than an M/S reconstruction. [`apply_pns_pair`] implements this.
//!
//! ## Decoder block order
//!
//! Per §4.6 the noise components are injected into the output spectrum
//! **prior to** the §4.6.9 TNS step (§4.6.13.5), on the de-interleaved,
//! window-major spectrum produced by
//! [`crate::decoded_spectrum::quant_to_spec`]
//! (`spec[w * window_len + k]`). The per-band coefficient extent
//! `swb_offset[sfb+1]-swb_offset[sfb]` and the
//! `(group, in-group window) → absolute window` mapping match the rest
//! of the pipeline.
//!
//! ## Scope
//!
//! This module is the §4.6.13.3 forward (`global_gain`-seeded) noise
//! synthesis only. The RVLC backward-DPCM noise-energy decode
//! (§4.6.13.3, error-resilient profiles) and the scalable-coder
//! integration (§4.6.13.6) are separate follow-ups; the `noise_nrg`
//! track itself is produced upstream by
//! [`crate::scale_factor_data::accumulate`].

use crate::ics_info::IcsInfo;
use crate::section_data::NOISE_HCB;
use crate::swb_offset::{
    long_window_offsets, short_window_offsets, LONG_WINDOW_LEN, SHORT_WINDOW_LEN,
};
use crate::{Error, Result};

/// §4.6.13.3 `is_noise(group,sfb)` — the noise-band predicate.
///
/// `true` ⇔ the band's `sfb_cb` is `NOISE_HCB` (13); the band carries a
/// `noise_nrg` energy in place of a scalefactor and no spectral
/// coefficients, and is filled by this tool.
pub fn is_noise(cb: u8) -> bool {
    cb == NOISE_HCB
}

/// §4.6.13.3 `2.0^(0.25 * noise_nrg)` — the target L2 norm of a noise
/// band.
///
/// The same per-quarter-step gain ladder as the §4.6.2.3.3 scalefactor
/// gain `2^(0.25·(sf−100))`; the absolute `noise_nrg[g][sfb]` plays the
/// role of a scalefactor for the noise energy. After
/// measured-energy normalisation a synthesised band has exactly this
/// L2 norm.
pub fn noise_target_norm(noise_nrg: i32) -> f64 {
    2.0f64.powf(0.25 * noise_nrg as f64)
}

/// Default `gen_rand_vector(addr, size)` (§4.6.13.3): fill `out` with
/// `out.len()` signed pseudo-random values in `[-1.0, 1.0)` using one
/// multiply-accumulate per value.
///
/// `state` is the generator's running 32-bit register; pass the same
/// `&mut state` across calls within a frame so independent bands draw
/// independent vectors. The amplitude is irrelevant to the final
/// output — [`apply_pns`] normalises by the measured energy — so this
/// only needs to produce a vector whose sum of squares is non-zero,
/// which it always does for a non-empty band.
///
/// The recurrence is a 32-bit linear congruential step
/// (`state = state * 1664525 + 1013904223`, both wrapping) whose high
/// bits are mapped to a signed fraction. This is one of the "suitable
/// random number generators" the spec admits; callers needing a
/// different source pass their own closure to [`apply_pns`].
pub fn gen_rand_vector(out: &mut [f64], state: &mut u32) {
    for v in out.iter_mut() {
        // One multiply-accumulate per random value (§4.6.13.3).
        *state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        // Map the 32-bit register to a signed fraction in [-1, 1).
        // (state as i32) ranges over the full signed 32-bit interval;
        // dividing by 2^31 yields [-1.0, 1.0).
        *v = (*state as i32) as f64 / 2_147_483_648.0;
    }
}

/// Synthesise one PNS band in place from a pre-filled random vector.
///
/// `band` is the generated random vector (length `size`); on return it
/// holds the energy-normalised noise band whose L2 norm is exactly
/// `2.0^(0.25 · noise_nrg)` (§4.6.13.3, 2009 measured-energy form).
///
/// If the random vector is all-zero (sum of squares zero) the band
/// cannot be normalised; per §4.6.13.3 a suitable generator yields a
/// non-zero sum of squares, so this leaves an all-zero band untouched
/// (the only energy-preserving choice) rather than dividing by zero.
fn normalise_band(band: &mut [f64], noise_nrg: i32) {
    let nrg: f64 = band.iter().map(|&x| x * x).sum();
    if nrg <= 0.0 {
        return;
    }
    let sqrt_nrg = nrg.sqrt();
    let scale = noise_target_norm(noise_nrg) / sqrt_nrg;
    for x in band.iter_mut() {
        *x *= scale;
    }
}

/// A single channel's de-interleaved spectrum plus the per-band
/// codebooks and noise energies the §4.6.13.3 synthesis needs.
///
/// `spec` is the window-major decoded spectrum
/// (`num_windows × window_len`) produced by
/// [`crate::decoded_spectrum::quant_to_spec`]; noise bands arrive as
/// silence and are overwritten in place. `sfb_cb[g][sfb]` selects which
/// bands are noise-coded; `noise_nrg[g][sfb]` is the absolute energy
/// (§4.6.13.3) for each noise band (consulted only where
/// `sfb_cb == NOISE_HCB`).
#[derive(Debug)]
pub struct PnsChannel<'a> {
    /// Window-major channel spectrum; noise bands overwritten in place.
    pub spec: &'a mut [f64],
    /// Per-band `sfb_cb[g][sfb]`.
    pub sfb_cb: &'a [Vec<u8>],
    /// Absolute `noise_nrg[g][sfb]` (§4.6.13.3).
    pub noise_nrg: &'a [Vec<i32>],
}

/// Validate that a channel's spectrum / `sfb_cb` / `noise_nrg` shapes
/// agree with `ics_info`, returning the window geometry.
fn channel_geometry<'a>(
    spec_len: usize,
    sfb_cb: &[Vec<u8>],
    noise_nrg: &[Vec<i32>],
    ics_info: &IcsInfo,
    fs_index: u8,
) -> Result<(usize, &'a [u16])> {
    let (window_len, offsets) = if ics_info.window_sequence.is_eight_short() {
        (SHORT_WINDOW_LEN as usize, short_window_offsets(fs_index)?)
    } else {
        (LONG_WINDOW_LEN as usize, long_window_offsets(fs_index)?)
    };
    let num_swb = offsets.len() - 1;
    let num_windows = ics_info.num_windows as usize;
    let num_groups = ics_info.num_window_groups as usize;
    let max_sfb = ics_info.max_sfb as usize;

    if spec_len != num_windows * window_len {
        return Err(Error::PnsInvalid);
    }
    if ics_info.window_group_length.len() != num_groups
        || ics_info
            .window_group_length
            .iter()
            .map(|&w| w as usize)
            .sum::<usize>()
            != num_windows
    {
        return Err(Error::PnsInvalid);
    }
    if max_sfb > num_swb {
        return Err(Error::PnsInvalid);
    }
    if sfb_cb.len() != num_groups || noise_nrg.len() != num_groups {
        return Err(Error::PnsInvalid);
    }
    for g in 0..num_groups {
        if sfb_cb[g].len() < max_sfb || noise_nrg[g].len() < max_sfb {
            return Err(Error::PnsInvalid);
        }
    }
    Ok((window_len, offsets))
}

/// Apply the §4.6.13.3 noise substitution to one channel in place.
///
/// For every `(group, sfb)` whose `sfb_cb` is `NOISE_HCB`, every window
/// of the group has its in-band coefficients replaced by a fresh random
/// vector (drawn via `rng`) scaled to L2 norm
/// `2.0^(0.25 · noise_nrg[g][sfb])`. Non-noise bands are left exactly
/// as they arrive.
///
/// * `chan` — the channel spectrum, codebooks, and noise energies
///   ([`PnsChannel`]).
/// * `ics_info` — supplies `num_window_groups`, `window_group_length`,
///   `max_sfb`, and the window geometry.
/// * `fs_index` — `samplingFrequencyIndex`, selecting the `swb_offset`
///   table.
/// * `rng` — `gen_rand_vector(out)` fills `out` with a fresh random
///   vector; called once per `(group, window, noise sfb)`. Use a
///   stateful closure over [`gen_rand_vector`] for the default
///   generator.
///
/// Returns [`Error::PnsInvalid`] if the buffer / `sfb_cb` / `noise_nrg`
/// shapes disagree with `ics_info` (see the variant docs). When no band
/// is noise-coded the spectrum is left untouched.
pub fn apply_pns<F>(
    chan: &mut PnsChannel<'_>,
    ics_info: &IcsInfo,
    fs_index: u8,
    mut rng: F,
) -> Result<()>
where
    F: FnMut(&mut [f64]),
{
    let PnsChannel {
        spec,
        sfb_cb,
        noise_nrg,
    } = chan;

    let (window_len, offsets) =
        channel_geometry(spec.len(), sfb_cb, noise_nrg, ics_info, fs_index)?;
    let num_groups = ics_info.num_window_groups as usize;
    let max_sfb = ics_info.max_sfb as usize;

    let mut window_base = 0usize;
    for g in 0..num_groups {
        let wgl = ics_info.window_group_length[g] as usize;
        for sfb in 0..max_sfb {
            if !is_noise(sfb_cb[g][sfb]) {
                continue;
            }
            let start = offsets[sfb] as usize;
            let end = offsets[sfb + 1] as usize;
            let nrg = noise_nrg[g][sfb];
            for b in 0..wgl {
                let base = (window_base + b) * window_len;
                let band = &mut spec[base + start..base + end];
                rng(band);
                normalise_band(band, nrg);
            }
        }
        window_base += wgl;
    }

    Ok(())
}

/// Apply the §4.6.13.3 noise substitution to a channel pair in place,
/// honouring the shared-random-vector correlation rule.
///
/// Both channels share the `common_window` `ics_info`. For each
/// `(group, sfb)`:
///
/// * If `NOISE_HCB` in **both** channels and the band's `ms_used` bit
///   is set (`ms_mask_present == true`, i.e. `ms_mask_present == 1`),
///   **or** `all_shared` is set (`ms_mask_present == 2`, "all bands
///   shared"), the **same** random vector is generated once and scaled
///   independently into each channel (correlated noise; no M/S
///   de-matrix — PNS and M/S are mutually exclusive, §4.6.13.5).
/// * Otherwise each noise band draws an independent vector.
///
/// `rng(out)` fills `out` with a fresh random vector.
///
/// * `left` / `right` — the two channels' spectra, codebooks, and noise
///   energies.
/// * `ms_mask_present` — `true` when a per-band `ms_used` mask is
///   present (`ms_mask_present == 1`).
/// * `all_shared` — `true` when `ms_mask_present == 2` (every band's
///   noise vector is shared); when set, `ms_used` is not consulted.
/// * `ms_used` — `ms_used[g][sfb]`; consulted only when `ms_mask_present`
///   is `true` and `all_shared` is `false`. Pass an empty slice
///   otherwise.
/// * `ics_info` / `fs_index` — shared window geometry and `swb_offset`
///   table.
///
/// Returns [`Error::PnsInvalid`] on any shape mismatch.
#[allow(clippy::too_many_arguments)]
pub fn apply_pns_pair<F>(
    left: &mut PnsChannel<'_>,
    right: &mut PnsChannel<'_>,
    ms_mask_present: bool,
    all_shared: bool,
    ms_used: &[Vec<bool>],
    ics_info: &IcsInfo,
    fs_index: u8,
    mut rng: F,
) -> Result<()>
where
    F: FnMut(&mut [f64]),
{
    let (window_len, offsets) = channel_geometry(
        left.spec.len(),
        left.sfb_cb,
        left.noise_nrg,
        ics_info,
        fs_index,
    )?;
    // Right channel must match the same geometry.
    let (rwindow_len, _) = channel_geometry(
        right.spec.len(),
        right.sfb_cb,
        right.noise_nrg,
        ics_info,
        fs_index,
    )?;
    if rwindow_len != window_len {
        return Err(Error::PnsInvalid);
    }

    let num_groups = ics_info.num_window_groups as usize;
    let max_sfb = ics_info.max_sfb as usize;

    if ms_mask_present && !all_shared {
        if ms_used.len() != num_groups {
            return Err(Error::PnsInvalid);
        }
        for row in ms_used {
            if row.len() < max_sfb {
                return Err(Error::PnsInvalid);
            }
        }
    }

    let mut window_base = 0usize;
    for g in 0..num_groups {
        let wgl = ics_info.window_group_length[g] as usize;
        for sfb in 0..max_sfb {
            let l_noise = is_noise(left.sfb_cb[g][sfb]);
            let r_noise = is_noise(right.sfb_cb[g][sfb]);
            if !l_noise && !r_noise {
                continue;
            }
            let start = offsets[sfb] as usize;
            let end = offsets[sfb + 1] as usize;
            let size = end - start;
            // Shared vector only when both channels are noise on this
            // band AND the band signals correlation. The `ms_used`
            // lookup is gated on `ms_mask_present` (and validated
            // above), so the `.get()` chain only matters defensively.
            let band_ms_used = ms_used
                .get(g)
                .and_then(|row| row.get(sfb))
                .copied()
                .unwrap_or(false);
            let shared = l_noise && r_noise && (all_shared || (ms_mask_present && band_ms_used));
            for b in 0..wgl {
                let base = (window_base + b) * window_len;
                if shared {
                    // One random vector, scaled independently into both
                    // channels (§4.6.13.3 correlated-noise path).
                    let mut vec = vec![0.0f64; size];
                    rng(&mut vec);
                    let lband = &mut left.spec[base + start..base + end];
                    lband.copy_from_slice(&vec);
                    normalise_band(lband, left.noise_nrg[g][sfb]);
                    let rband = &mut right.spec[base + start..base + end];
                    rband.copy_from_slice(&vec);
                    normalise_band(rband, right.noise_nrg[g][sfb]);
                } else {
                    if l_noise {
                        let lband = &mut left.spec[base + start..base + end];
                        rng(lband);
                        normalise_band(lband, left.noise_nrg[g][sfb]);
                    }
                    if r_noise {
                        let rband = &mut right.spec[base + start..base + end];
                        rng(rband);
                        normalise_band(rband, right.noise_nrg[g][sfb]);
                    }
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
    use crate::section_data::{INTENSITY_HCB, ZERO_HCB};

    const FS_48000: u8 = 3;

    /// Build a minimal long-window `IcsInfo` (one group of one window,
    /// `max_sfb` bands) for synthesis tests at fs_index 3 (48 kHz).
    fn long_ics(max_sfb: u8) -> IcsInfo {
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
            num_swb: crate::ics_info::NUM_SWB_LONG_WINDOW[FS_48000 as usize],
        }
    }

    /// L2 norm of a slice.
    fn norm(s: &[f64]) -> f64 {
        s.iter().map(|&x| x * x).sum::<f64>().sqrt()
    }

    #[test]
    fn noise_target_norm_is_quarter_step_ladder() {
        assert!((noise_target_norm(0) - 1.0).abs() < 1e-12);
        // +4 in noise_nrg doubles the target norm (2^(0.25*4) = 2).
        assert!((noise_target_norm(4) - 2.0).abs() < 1e-12);
        assert!((noise_target_norm(-4) - 0.5).abs() < 1e-12);
    }

    #[test]
    fn gen_rand_vector_is_signed_and_nonzero_energy() {
        let mut state = 1u32;
        let mut v = vec![0.0f64; 16];
        gen_rand_vector(&mut v, &mut state);
        // Values lie in [-1, 1) and the sum of squares is non-zero.
        assert!(v.iter().all(|&x| (-1.0..1.0).contains(&x)));
        assert!(v.iter().map(|&x| x * x).sum::<f64>() > 0.0);
        // Signed: at least one negative and one positive over 16 draws.
        assert!(v.iter().any(|&x| x < 0.0));
        assert!(v.iter().any(|&x| x > 0.0));
    }

    #[test]
    fn synthesised_band_has_exact_target_norm() {
        // fs_index 3 (48 kHz) long window, single noise band at sfb 0.
        let fs = 3u8;
        let ics = long_ics(1);
        let offsets = long_window_offsets(fs).unwrap();
        let mut spec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let sfb_cb = vec![vec![NOISE_HCB]];
        let noise_nrg = vec![vec![8i32]]; // target norm = 2^(0.25*8) = 4.0
        let mut state = 12345u32;
        let mut chan = PnsChannel {
            spec: &mut spec,
            sfb_cb: &sfb_cb,
            noise_nrg: &noise_nrg,
        };
        apply_pns(&mut chan, &ics, fs, |out| gen_rand_vector(out, &mut state)).unwrap();
        let start = offsets[0] as usize;
        let end = offsets[1] as usize;
        let got = norm(&spec[start..end]);
        assert!((got - 4.0).abs() < 1e-9, "band L2 norm {got} != target 4.0");
        // Coefficients outside the noise band are untouched (silent).
        assert!(spec[end..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn non_noise_bands_left_untouched() {
        let fs = 3u8;
        let ics = long_ics(2);
        let offsets = long_window_offsets(fs).unwrap();
        let mut spec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        // Seed band 1 (a non-noise band) with a recognisable value.
        let b1 = offsets[1] as usize;
        spec[b1] = 7.5;
        let sfb_cb = vec![vec![NOISE_HCB, ZERO_HCB]];
        let noise_nrg = vec![vec![0i32, 0i32]];
        let mut state = 1u32;
        let mut chan = PnsChannel {
            spec: &mut spec,
            sfb_cb: &sfb_cb,
            noise_nrg: &noise_nrg,
        };
        apply_pns(&mut chan, &ics, fs, |out| gen_rand_vector(out, &mut state)).unwrap();
        assert_eq!(spec[b1], 7.5, "non-noise band must be untouched");
        // The noise band (band 0) was filled.
        assert!(norm(&spec[offsets[0] as usize..b1]) > 0.0);
    }

    #[test]
    fn shape_mismatch_is_rejected() {
        let fs = 3u8;
        let ics = long_ics(1);
        let mut spec = vec![0.0f64; 10]; // wrong length
        let sfb_cb = vec![vec![NOISE_HCB]];
        let noise_nrg = vec![vec![0i32]];
        let mut chan = PnsChannel {
            spec: &mut spec,
            sfb_cb: &sfb_cb,
            noise_nrg: &noise_nrg,
        };
        let r = apply_pns(&mut chan, &ics, fs, |_| {});
        assert!(matches!(r, Err(Error::PnsInvalid)));
    }

    #[test]
    fn pair_shared_vector_correlates_noise() {
        // Both channels noise at sfb 0 with the SAME noise_nrg and the
        // ms_used bit set → shared random vector → the two bands are
        // identical (correlated noise), since equal target norms scale
        // the same source vector by the same factor.
        let fs = 3u8;
        let ics = long_ics(1);
        let offsets = long_window_offsets(fs).unwrap();
        let mut lspec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let mut rspec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let sfb_cb = vec![vec![NOISE_HCB]];
        let noise_nrg = vec![vec![4i32]];
        let ms_used = vec![vec![true]];
        let mut state = 999u32;
        {
            let mut l = PnsChannel {
                spec: &mut lspec,
                sfb_cb: &sfb_cb,
                noise_nrg: &noise_nrg,
            };
            let mut r = PnsChannel {
                spec: &mut rspec,
                sfb_cb: &sfb_cb,
                noise_nrg: &noise_nrg,
            };
            apply_pns_pair(&mut l, &mut r, true, false, &ms_used, &ics, fs, |out| {
                gen_rand_vector(out, &mut state)
            })
            .unwrap();
        }
        let start = offsets[0] as usize;
        let end = offsets[1] as usize;
        for i in start..end {
            assert!(
                (lspec[i] - rspec[i]).abs() < 1e-12,
                "shared-vector bands must be identical at {i}"
            );
        }
        assert!((norm(&lspec[start..end]) - noise_target_norm(4)).abs() < 1e-9);
    }

    #[test]
    fn pair_independent_when_mask_absent() {
        // Both channels noise but ms_mask_present == false → independent
        // vectors → the two bands differ (overwhelmingly likely for a
        // 96-bin band; we assert they are not bit-identical).
        let fs = 3u8;
        let ics = long_ics(1);
        let offsets = long_window_offsets(fs).unwrap();
        let mut lspec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let mut rspec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let sfb_cb = vec![vec![NOISE_HCB]];
        let noise_nrg = vec![vec![4i32]];
        let mut state = 7u32;
        {
            let mut l = PnsChannel {
                spec: &mut lspec,
                sfb_cb: &sfb_cb,
                noise_nrg: &noise_nrg,
            };
            let mut r = PnsChannel {
                spec: &mut rspec,
                sfb_cb: &sfb_cb,
                noise_nrg: &noise_nrg,
            };
            apply_pns_pair(&mut l, &mut r, false, false, &[], &ics, fs, |out| {
                gen_rand_vector(out, &mut state)
            })
            .unwrap();
        }
        let start = offsets[0] as usize;
        let end = offsets[1] as usize;
        let identical = (start..end).all(|i| lspec[i] == rspec[i]);
        assert!(!identical, "independent draws must not be bit-identical");
        // Both still hit the exact target norm.
        assert!((norm(&lspec[start..end]) - noise_target_norm(4)).abs() < 1e-9);
        assert!((norm(&rspec[start..end]) - noise_target_norm(4)).abs() < 1e-9);
    }

    #[test]
    fn pair_one_channel_noise_ignores_ms_used() {
        // Only the right channel is noise at sfb 0; ms_used is set but
        // must be ignored (§4.6.13.3) — the left band stays silent and
        // only the right is filled, independently.
        let fs = 3u8;
        let ics = long_ics(1);
        let offsets = long_window_offsets(fs).unwrap();
        let mut lspec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let mut rspec = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let lcb = vec![vec![ZERO_HCB]];
        let rcb = vec![vec![NOISE_HCB]];
        let lnrg = vec![vec![0i32]];
        let rnrg = vec![vec![4i32]];
        let ms_used = vec![vec![true]];
        let mut state = 42u32;
        {
            let mut l = PnsChannel {
                spec: &mut lspec,
                sfb_cb: &lcb,
                noise_nrg: &lnrg,
            };
            let mut r = PnsChannel {
                spec: &mut rspec,
                sfb_cb: &rcb,
                noise_nrg: &rnrg,
            };
            apply_pns_pair(&mut l, &mut r, true, false, &ms_used, &ics, fs, |out| {
                gen_rand_vector(out, &mut state)
            })
            .unwrap();
        }
        let start = offsets[0] as usize;
        let end = offsets[1] as usize;
        assert!(
            lspec[start..end].iter().all(|&x| x == 0.0),
            "non-noise left band must stay silent"
        );
        assert!((norm(&rspec[start..end]) - noise_target_norm(4)).abs() < 1e-9);
    }

    #[test]
    fn is_noise_predicate() {
        assert!(is_noise(NOISE_HCB));
        assert!(!is_noise(ZERO_HCB));
        assert!(!is_noise(INTENSITY_HCB));
        assert!(!is_noise(1));
    }
}
