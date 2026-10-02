//! SBR high-frequency generation — ISO/IEC 14496-3 §4.6.18.6.
//!
//! Builds the `XHigh` subband matrix from the analysis-filterbank
//! output `XLow`:
//!
//! * **Patch construction** (§4.6.18.6.3 / Figure 4.48) — the
//!   `numPatches` / `patchStartSubband` / `patchNumSubbands` decision
//!   that maps consecutive low-band source ranges onto the SBR range,
//!   driven by `goalSb = NINT(2.048e6 / FsSBR)` and the `fMaster`
//!   grid, with the trailing small-patch trim.
//! * **Inverse filtering** (§4.6.18.6.2) — the covariance-method
//!   second-order linear prediction per low subband (`φk(i,j)` over
//!   `numTimeSlots·RATE + 6` samples, `d(k)` with `εInv = 1e-6`, the
//!   `α0(k)` / `α1(k)` solution, and the `|α| ≥ 4` reset), plus the
//!   Table 4.175 `newBw` transition function and the `bwArray` chirp
//!   blend (`0.75/0.25` attack, `0.90625/0.09375` decay, `< 0.015625`
//!   flush to zero).
//! * **HF generator** (§4.6.18.6.3) — `XHigh(k, l + tHFAdj) =
//!   XLow(p, …) + bw·α0(p)·XLow(p, l−1+…) + bw²·α1(p)·XLow(p, l−2+…)`
//!   over the patch mapping, with the chirp factor selected by the
//!   noise-floor band `g(k)`.
//!
//! Both `XLow` and `XHigh` are stored slot-major (`x[slot][band]`)
//! with the slot axis carrying the spec's absolute column index (the
//! `tHFGen`-slot history precedes the current frame, so spec index
//! `l + tHFAdj` is a direct column index).
//!
//! ## Provenance
//!
//! Every formula, constant, and branch is from the §4.6.18.6 text,
//! Table 4.175, and the Figure 4.48 flowchart of the staged spec. No
//! part of this implementation is derived from any external decoder.

use crate::sbr_freq_bands::HiLoTables;
use crate::sbr_qmf::Complex;
use crate::{Error, Result};

/// `tHFAdj = 2` — the envelope-adjuster offset (§4.6.18.5).
pub const T_HF_ADJ: usize = 2;

/// `tHFGen = 8` — the HF-generator offset (§4.6.18.5).
pub const T_HF_GEN: usize = 8;

/// The §4.6.18.6.2 relaxation parameter `εInv`.
pub const EPS_INV: f64 = 1e-6;

/// §4.6.18.3.6: `numPatches ≤ 5`.
pub const MAX_PATCHES: usize = 5;

/// The §4.6.18.6.3 / Figure 4.48 patch layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patches {
    /// `patchStartSubband(i)` — first source QMF subband of patch `i`.
    pub start: Vec<usize>,
    /// `patchNumSubbands(i)` — subband count of patch `i`.
    pub num: Vec<usize>,
}

impl Patches {
    /// `numPatches`.
    #[inline]
    #[must_use]
    pub fn num_patches(&self) -> usize {
        self.num.len()
    }

    /// The §4.6.18.3.2.3 patch borders: `patchBorders(0) = kx`,
    /// `patchBorders(k) = patchBorders(k-1) + patchNumSubbands(k-1)`.
    #[must_use]
    pub fn borders(&self, k_x: i32) -> Vec<i32> {
        let mut b = Vec::with_capacity(self.num.len() + 1);
        b.push(k_x);
        for &n in &self.num {
            b.push(b[b.len() - 1] + n as i32);
        }
        b
    }
}

/// Figure 4.48 — patch construction.
///
/// `f_master` is the §4.6.18.3.2.1 master table (`fMaster(0..=NMaster)`),
/// `k0` its first subband, `k_x` / `m` the SBR range, and `fs_sbr` the
/// SBR internal rate driving `goalSb = NINT(2.048e6 / FsSBR)`.
pub fn build_patches(f_master: &[i32], k0: i32, k_x: i32, m: i32, fs_sbr: u32) -> Result<Patches> {
    if f_master.len() < 2 || fs_sbr == 0 {
        return Err(Error::SbrFreqBandInvalid);
    }
    let n_master = f_master.len() - 1;

    let mut msb = k0;
    let mut usb = k_x;
    let mut start = Vec::new();
    let mut num = Vec::new();

    // goalSb = NINT(2.048e6 / Fs).
    let goal_sb = ((2.0 * 2.048e6 / f64::from(fs_sbr) + 1.0) / 2.0).floor() as i32;
    // k: the first master index at/after goalSb (NMaster if goalSb is
    // past the SBR stop border).
    let mut k = if goal_sb < k_x + m {
        let mut kk = 0usize;
        for (i, &f) in f_master.iter().enumerate() {
            if f < goal_sb {
                kk = i + 1;
            } else {
                break;
            }
        }
        kk
    } else {
        n_master
    };

    let mut sb;
    let mut guard = 0usize;
    loop {
        guard += 1;
        if guard > 64 {
            return Err(Error::SbrFreqBandInvalid);
        }
        // Walk j downward from k until the patch source fits under the
        // first master subband: sb <= k0 - 1 + msb - odd.
        let mut j = k;
        let odd = loop {
            if j >= f_master.len() {
                return Err(Error::SbrFreqBandInvalid);
            }
            sb = f_master[j];
            let odd = (sb - 2 + k0).rem_euclid(2);
            if sb <= k0 - 1 + msb - odd {
                break odd;
            }
            if j == 0 {
                return Err(Error::SbrFreqBandInvalid);
            }
            j -= 1;
        };

        let n = (sb - usb).max(0);
        let s = k0 - odd - n;
        if n > 0 {
            if s < 0 || start.len() >= MAX_PATCHES {
                return Err(Error::SbrFreqBandInvalid);
            }
            start.push(s as usize);
            num.push(n as usize);
            usb = sb;
            msb = sb;
        } else {
            msb = k_x;
        }

        if f_master[k] - sb < 3 {
            k = n_master;
        }
        if sb == k_x + m {
            break;
        }
    }

    // Trailing small-patch trim: drop a final patch narrower than 3
    // subbands when more than one patch was built.
    if num.len() > 1 && *num.last().unwrap() < 3 {
        num.pop();
        start.pop();
    }

    Ok(Patches { start, num })
}

/// Table 4.175 — `newBw(bs_invf_mode´, bs_invf_mode)`. Row is the
/// previous frame's mode, column the current one (both `0..=3` for
/// Off / Low / Intermediate / Strong).
#[must_use]
pub fn new_bw(prev_mode: u8, cur_mode: u8) -> f64 {
    const TABLE: [[f64; 4]; 4] = [
        [0.0, 0.6, 0.9, 0.98],
        [0.6, 0.75, 0.9, 0.98],
        [0.0, 0.75, 0.9, 0.98],
        [0.0, 0.75, 0.9, 0.98],
    ];
    TABLE[usize::from(prev_mode.min(3))][usize::from(cur_mode.min(3))]
}

/// §4.6.18.6.2 chirp-factor update: one `bwArray` entry per noise
/// band. `prev_invf` / `prev_bw` are the previous SBR frame's values
/// (all zero for the first frame).
#[must_use]
pub fn chirp_factors(cur_invf: &[u8], prev_invf: &[u8], prev_bw: &[f64]) -> Vec<f64> {
    cur_invf
        .iter()
        .enumerate()
        .map(|(i, &cur)| {
            let prev_mode = prev_invf.get(i).copied().unwrap_or(0);
            let bw_prev = prev_bw.get(i).copied().unwrap_or(0.0);
            let nb = new_bw(prev_mode, cur);
            let temp = if nb < bw_prev {
                0.75 * nb + 0.25 * bw_prev
            } else {
                0.90625 * nb + 0.09375 * bw_prev
            };
            if temp < 0.015625 {
                0.0
            } else {
                temp
            }
        })
        .collect()
}

/// §4.6.18.6.2 covariance-method prediction coefficients
/// `(α0(k), α1(k))` for low subband `k`.
///
/// `x_low` is slot-major with the spec's absolute column index (the
/// covariance windows over `n − i + tHFAdj` for
/// `0 ≤ n < n_slots_frame + 6`), so `x_low` must carry at least
/// `n_slots_frame + 6 + tHFAdj` columns.
pub fn prediction_coefficients(
    x_low: &[[Complex; 32]],
    k: usize,
    n_slots_frame: usize,
) -> Result<(Complex, Complex)> {
    if k >= 32 || x_low.len() < n_slots_frame + 6 + T_HF_ADJ {
        return Err(Error::SbrFreqBandInvalid);
    }
    // φk(i, j) = Σ_n XLow(k, n - i + tHFAdj) · XLow*(k, n - j + tHFAdj).
    let phi = |i: usize, j: usize| -> Complex {
        let mut acc = Complex::default();
        for n in 0..(n_slots_frame + 6) {
            let a = x_low[n + T_HF_ADJ - i][k];
            let b = x_low[n + T_HF_ADJ - j][k];
            acc += a * b.conj();
        }
        acc
    };
    let phi01 = phi(0, 1);
    let phi02 = phi(0, 2);
    let phi11 = phi(1, 1);
    let phi12 = phi(1, 2);
    let phi22 = phi(2, 2);

    // d(k) = φ(2,2)·φ(1,1) − |φ(1,2)|² / (1 + εInv). φ(1,1) / φ(2,2)
    // are real by construction.
    let d = phi22.re * phi11.re - phi12.norm_sqr() / (1.0 + EPS_INV);

    let alpha1 = if d != 0.0 {
        let numer = phi01 * phi12 - phi02 * phi11.re;
        Complex::new(numer.re / d, numer.im / d)
    } else {
        Complex::default()
    };
    let alpha0 = if phi11.re != 0.0 {
        let numer = phi01 + alpha1 * phi12.conj();
        Complex::new(-numer.re / phi11.re, -numer.im / phi11.re)
    } else {
        Complex::default()
    };

    // If either magnitude reaches 4, both coefficients reset to zero.
    if alpha0.norm_sqr() >= 16.0 || alpha1.norm_sqr() >= 16.0 {
        return Ok((Complex::default(), Complex::default()));
    }
    Ok((alpha0, alpha1))
}

/// §4.6.18.6.3 — generate `XHigh` from `XLow` over the patch mapping.
///
/// * `x_low` — slot-major analysis output (spec absolute columns).
/// * `patches` — the Figure 4.48 layout.
/// * `bw_array` — the per-noise-band chirp factors.
/// * `bands` — the derived frequency tables (`fTableNoise`, `k_x`).
/// * `l_range` — the spec's `RATE·tE(0) .. RATE·tE(LE)` column range
///   (exclusive end, *before* the `tHFAdj` offset).
/// * `n_slots_frame` — `numTimeSlots · RATE` (covariance length).
///
/// Returns `XHigh` with the same slot-major layout and column count as
/// `x_low` (bands outside the patched range stay zero).
pub fn generate_hf(
    x_low: &[[Complex; 32]],
    patches: &Patches,
    bw_array: &[f64],
    bands: &HiLoTables,
    l_range: core::ops::Range<i32>,
    n_slots_frame: usize,
) -> Result<Vec<[Complex; 64]>> {
    let k_x = bands.k_x;
    let mut x_high = vec![[Complex::default(); 64]; x_low.len()];

    // α cache per source subband (a subband may feed several patches).
    let mut alphas: [Option<(Complex, Complex)>; 32] = [None; 32];

    // g(k): the noise band containing QMF subband k.
    let g_of = |k: i32| -> Result<usize> {
        let nb = &bands.f_table_noise;
        for i in 0..nb.len() - 1 {
            if nb[i] <= k && k < nb[i + 1] {
                return Ok(i);
            }
        }
        Err(Error::SbrFreqBandInvalid)
    };

    let mut k_off = 0usize;
    for (i, (&p_start, &p_num)) in patches.start.iter().zip(patches.num.iter()).enumerate() {
        let _ = i;
        for x in 0..p_num {
            let k = k_x as usize + x + k_off;
            let p = p_start + x;
            if k >= 64 || p >= 32 {
                return Err(Error::SbrFreqBandInvalid);
            }
            let (a0, a1) = match alphas[p] {
                Some(a) => a,
                None => {
                    let a = prediction_coefficients(x_low, p, n_slots_frame)?;
                    alphas[p] = Some(a);
                    a
                }
            };
            let bw = *bw_array
                .get(g_of(k as i32)?)
                .ok_or(Error::SbrFreqBandInvalid)?;
            let bw2 = bw * bw;
            for l in l_range.clone() {
                let c = usize::try_from(l).map_err(|_| Error::SbrFreqBandInvalid)? + T_HF_ADJ;
                if c >= x_low.len() || c < 2 {
                    return Err(Error::SbrFreqBandInvalid);
                }
                x_high[c][k] =
                    x_low[c][p] + (a0 * bw) * x_low[c - 1][p] + (a1 * bw2) * x_low[c - 2][p];
            }
        }
        k_off += p_num;
    }
    Ok(x_high)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Table 4.175 spot values.
    #[test]
    fn new_bw_table() {
        assert_eq!(new_bw(0, 0), 0.0);
        assert_eq!(new_bw(0, 1), 0.6);
        assert_eq!(new_bw(1, 0), 0.6);
        assert_eq!(new_bw(1, 1), 0.75);
        assert_eq!(new_bw(2, 0), 0.0);
        assert_eq!(new_bw(2, 1), 0.75);
        assert_eq!(new_bw(3, 3), 0.98);
        assert_eq!(new_bw(0, 2), 0.9);
    }

    /// Chirp blend: rising values take the 0.90625/0.09375 mix,
    /// falling values the 0.75/0.25 mix, and tiny results flush to 0.
    #[test]
    fn chirp_blend_and_flush() {
        // First frame: prev all zero. newBw(0, 3) = 0.98 rising:
        // 0.90625·0.98 = 0.888125.
        let bw = chirp_factors(&[3], &[0], &[0.0]);
        assert!((bw[0] - 0.888125).abs() < 1e-12);
        // Falling: newBw(3, 0) = 0.0 < prev 0.888125:
        // 0.25·0.888125 = 0.22203125.
        let bw2 = chirp_factors(&[0], &[3], &bw);
        assert!((bw2[0] - 0.22203125).abs() < 1e-12);
        // Repeated Off decays geometrically to below 0.015625 → 0.
        let mut cur = bw2;
        for _ in 0..4 {
            cur = chirp_factors(&[0], &[0], &cur);
        }
        assert_eq!(cur[0], 0.0);
    }

    /// Figure 4.48 on a hand-walked geometry: fMaster = 8..=24 step 2,
    /// k0 = kx = 8, M = 16, goalSb past the range.
    #[test]
    fn patch_construction_hand_walked() {
        let f_master: Vec<i32> = (0..=8).map(|i| 8 + 2 * i).collect();
        // fs_sbr small enough that goalSb = NINT(2.048e6/fs) ≥ 24.
        let p = build_patches(&f_master, 8, 8, 16, 85_000).unwrap();
        // Iter 1: sb = 14 → patch (start 2, num 6);
        // iter 2: sb = 20 → patch (2, 6); iter 3: sb = 24 → (4, 4).
        assert_eq!(p.start, vec![2, 2, 4]);
        assert_eq!(p.num, vec![6, 6, 4]);
        assert_eq!(p.borders(8), vec![8, 14, 20, 24]);
    }

    /// The patch trim drops a trailing patch narrower than 3 subbands.
    #[test]
    fn patch_trim_drops_small_tail() {
        // fMaster reaching kx + M = 22 with a final 2-wide step.
        let f_master = vec![8, 10, 12, 14, 16, 20, 22];
        let p = build_patches(&f_master, 8, 8, 14, 85_000).unwrap();
        // Walk: msb=8,usb=8 → sb=14 (odd 0) num 6 start 2;
        // then sb=20? 20 ≤ 7+14-0=21 → num 6 start 2; then sb=22:
        // 22 ≤ 7+20-0=27 → num 2 start 6 → trimmed.
        assert_eq!(p.num, vec![6, 6]);
        assert_eq!(p.start, vec![2, 2]);
    }

    /// Patch invariants on a spec-derived master table (44.1 kHz
    /// HE-AAC geometry).
    #[test]
    fn patch_invariants_on_derived_master() {
        let fs_sbr = 44_100;
        let k0 = crate::sbr_freq_bands::k0(fs_sbr, 5).unwrap();
        let k2 = crate::sbr_freq_bands::k2(fs_sbr, 5, k0).unwrap();
        let fm = crate::sbr_freq_bands::master_table(k0, k2, 2, true).unwrap();
        let bands = HiLoTables::derive(&fm, 0, 2).unwrap();
        let p = build_patches(&fm, k0, bands.k_x, bands.m, fs_sbr).unwrap();
        assert!(p.num_patches() >= 1 && p.num_patches() <= MAX_PATCHES);
        for (&s, &n) in p.start.iter().zip(p.num.iter()) {
            assert!(n > 0);
            // Source range lies below the first master subband.
            assert!((s + n) as i32 <= k0);
        }
        // Borders start at kx and stay within kx + M.
        let borders = p.borders(bands.k_x);
        assert_eq!(borders[0], bands.k_x);
        assert!(*borders.last().unwrap() <= bands.k_x + bands.m);
    }

    /// Build a slot-major XLow whose band `k` carries an exact
    /// second-order recursion `x[n] = a1·x[n-1] + a2·x[n-2]`.
    fn ar2_xlow(k: usize, a1: Complex, a2: Complex, cols: usize) -> Vec<[Complex; 32]> {
        let mut x = vec![[Complex::default(); 32]; cols];
        x[0][k] = Complex::new(1.0, 0.3);
        x[1][k] = Complex::new(0.2, -0.5);
        for n in 2..cols {
            let v = a1 * x[n - 1][k] + a2 * x[n - 2][k];
            x[n][k] = v;
        }
        x
    }

    /// The covariance method recovers an exact AR(2) recursion:
    /// α0 = −a1, α1 = −a2.
    #[test]
    fn prediction_recovers_ar2() {
        let a1 = Complex::new(0.9, 0.1);
        let a2 = Complex::new(-0.5, 0.05);
        let x = ar2_xlow(3, a1, a2, 40);
        let (al0, al1) = prediction_coefficients(&x, 3, 32).unwrap();
        // The εInv = 1e-6 relaxation perturbs the exact solution by
        // O(εInv), so the recovery is pinned to that scale.
        assert!((al0 + a1).norm_sqr() < 1e-10, "{al0:?}");
        assert!((al1 + a2).norm_sqr() < 1e-10, "{al1:?}");
    }

    /// |α| ≥ 4 resets both coefficients.
    #[test]
    fn prediction_resets_large_coefficients() {
        // An unstable recursion with |a1| > 4 forces the reset.
        let a1 = Complex::new(4.5, 0.0);
        let a2 = Complex::new(0.0, 0.0);
        let mut x = vec![[Complex::default(); 32]; 40];
        x[0][0] = Complex::new(1e-6, 0.0);
        for n in 1..40 {
            let v = a1 * x[n - 1][0];
            x[n][0] = v;
        }
        let _ = a2;
        let (al0, al1) = prediction_coefficients(&x, 0, 32).unwrap();
        assert_eq!(al0, Complex::default());
        assert_eq!(al1, Complex::default());
    }

    fn tiny_bands() -> HiLoTables {
        HiLoTables {
            f_table_high: vec![8, 12, 16],
            f_table_low: vec![8, 16],
            f_table_noise: vec![8, 16],
            m: 8,
            k_x: 8,
        }
    }

    /// bw = 0 copies the source band; bw = 1 on a perfectly
    /// predictable source whitens it to (near) zero.
    #[test]
    fn generate_copies_and_whitens() {
        let a1 = Complex::new(0.8, 0.2);
        let a2 = Complex::new(-0.4, 0.0);
        let x = ar2_xlow(2, a1, a2, 40);
        let patches = Patches {
            start: vec![2],
            num: vec![8],
        };
        let bands = tiny_bands();
        // bw = 0: XHigh(k) == XLow(p) on the generated range. Patch
        // maps source 2..10 → 8..16; k = 8 comes from p = 2.
        let hi = generate_hf(&x, &patches, &[0.0], &bands, 0..32, 32).unwrap();
        for l in 0..32usize {
            let c = l + T_HF_ADJ;
            assert_eq!(hi[c][8], x[c][2]);
        }
        // bw = 1: the inverse filter cancels the AR(2) recursion (to
        // the O(εInv) accuracy of the relaxed covariance solution).
        let hi = generate_hf(&x, &patches, &[1.0], &bands, 0..32, 32).unwrap();
        let sig: f64 = (0..32).map(|l| x[l + T_HF_ADJ][2].norm_sqr()).sum();
        let res: f64 = (0..32).map(|l| hi[l + T_HF_ADJ][8].norm_sqr()).sum();
        assert!(res < 1e-10 * sig, "residual {res} vs signal {sig}");
        // Un-patched bands stay zero.
        for col in &hi {
            assert_eq!(col[20], Complex::default());
        }
    }
}
