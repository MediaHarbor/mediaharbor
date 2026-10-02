//! PS stereo processing — ISO/IEC 14496-3:2009 §8.6.4.6.
//!
//! Converts the mono hybrid signal `s_k(n)` and its de-correlation
//! `d_k(n)` into left/right hybrid signals through the 2×2 mixing
//!
//! ```text
//! l_k(n) = H11(k,n)·s_k(n) + H21(k,n)·d_k(n)
//! r_k(n) = H12(k,n)·s_k(n) + H22(k,n)·d_k(n)
//! ```
//!
//! Per parameter position (envelope border) the vectors `h11..h22`
//! are derived per stereo band from the dequantized cues:
//!
//! * IID: `c(b) = 10^(iid(b)/20)` on the Table 8.25 (default) or
//!   8.26 (fine) dB grid;
//! * ICC: `ρ(b)` on the Table 8.28 grid, driving **mixing procedure
//!   Ra** (`icc_mode 0..2`: scale factors `c1 = √(2/(1+c²))`,
//!   `c2 = √2·c/√(1+c²)`, rotation `α = ½·arccos(ρ)`,
//!   `β = α·(c1−c2)/√2`) or **Rb** (`icc_mode 3..5`: `ρ` floored at
//!   0.05, `α = ½·arctan(2cρ/(c²−1))` with the `c = 1` and
//!   modulo-π/2 corrections, `μ`/`γ` per §8.6.4.6.2.2);
//! * IPD/OPD (§8.6.4.6.3.2, when enabled): the three-position
//!   smoothing `φ = ∠(¼e^(j·prev2) + ½e^(j·prev1) + e^(j·cur))` on
//!   the Table 8.31 `π/4` ladder, applied as `e^(jφ1)` on `h11/h21`
//!   and `e^(jφ2)` (`φ2 = φ_opd − φ_ipd`) on `h12/h22`; the
//!   `*`-marked negative-frequency hybrid channels take the complex
//!   conjugate.
//!
//! Between borders the four H matrices are linearly interpolated
//! (§8.6.4.6.4), the first region interpolating from the previous
//! frame's final coefficients (zeros on the very first frame), the
//! region after the last border holding. FIX_BORDERS positions are
//! `⌊32·(e+1)/num_env⌋ − 1`; VAR_BORDERS come from the bitstream.
//! `num_env == 0` holds the previous frame's coefficients for the
//! whole frame (§8.6.4.6.5). A stereo-band-count switch (Table 8.47)
//! re-maps the retained coefficients through Tables 8.45 / 8.46.
//!
//! All truth from ISO/IEC 14496-3:2009 §8.6.4.6 staged under
//! `docs/audio/aac/`.

use crate::ps_data::{PsData, PsIndices};
use crate::ps_hybrid::{HybridConfig, NUM_QMF_SLOTS};
use crate::ps_map::{conjugate_flags, map_indices, parameter_map, MAP_20_TO_34, MAP_34_TO_20};
use crate::sbr_qmf::Complex;
use crate::{Error, Result};

/// Table 8.25 — default IID quantization grid, dB, index −7..7.
const IID_DB_COARSE: [f64; 15] = [
    -25.0, -18.0, -14.0, -10.0, -7.0, -4.0, -2.0, 0.0, 2.0, 4.0, 7.0, 10.0, 14.0, 18.0, 25.0,
];

/// Table 8.26 — fine IID quantization grid, dB, index −15..15.
const IID_DB_FINE: [f64; 31] = [
    -50.0, -45.0, -40.0, -35.0, -30.0, -25.0, -22.0, -19.0, -16.0, -13.0, -10.0, -8.0, -6.0, -4.0,
    -2.0, 0.0, 2.0, 4.0, 6.0, 8.0, 10.0, 13.0, 16.0, 19.0, 22.0, 25.0, 30.0, 35.0, 40.0, 45.0,
    50.0,
];

/// Table 8.28 — ICC quantization grid `ρ`.
const ICC_RHO: [f64; 8] = [1.0, 0.937, 0.84118, 0.60092, 0.36764, 0.0, -0.589, -1.0];

/// One stereo band's mixing coefficients.
type H4 = [Complex; 4]; // h11, h12, h21, h22

/// A stereo pair of hybrid-domain frames.
pub type HybridPair = (Vec<Vec<Complex>>, Vec<Vec<Complex>>);

/// §8.6.4.6 stereo processor: per-band coefficient state across
/// frames plus the IPD/OPD smoothing history.
#[derive(Debug, Clone)]
pub struct PsStereo {
    /// Stereo band count in force (20 or 34).
    n_bands: usize,
    /// `H(·, n_{−1})` — coefficients at the previous frame's last
    /// slot, per stereo band.
    h_prev: Vec<H4>,
    /// IPD/OPD angle history: `[.., e−1]` and `[.., e]` positions
    /// (radians), per stereo band.
    ipd_hist: [Vec<f64>; 2],
    opd_hist: [Vec<f64>; 2],
}

impl PsStereo {
    /// Fresh state (first frame interpolates from zero coefficients).
    #[must_use]
    pub fn new(n_bands: usize) -> Self {
        PsStereo {
            n_bands,
            h_prev: vec![[Complex::default(); 4]; n_bands],
            ipd_hist: [vec![0.0; n_bands], vec![0.0; n_bands]],
            opd_hist: [vec![0.0; n_bands], vec![0.0; n_bands]],
        }
    }

    /// Table 8.47 — switch the stereo band count, re-mapping the
    /// retained coefficients through Table 8.45 / 8.46 and resetting
    /// the phase-smoothing history.
    pub fn switch_bands(&mut self, n_bands: usize) {
        if n_bands == self.n_bands {
            return;
        }
        let map = |vals: Vec<f64>| -> Vec<f64> {
            if n_bands == 34 {
                MAP_20_TO_34.iter().map(|m| m.apply_f64(&vals)).collect()
            } else {
                MAP_34_TO_20.iter().map(|m| m.apply_f64(&vals)).collect()
            }
        };
        let mut new_h = vec![[Complex::default(); 4]; n_bands];
        for c in 0..4 {
            let re: Vec<f64> = self.h_prev.iter().map(|h| h[c].re).collect();
            let im: Vec<f64> = self.h_prev.iter().map(|h| h[c].im).collect();
            let re = map(re);
            let im = map(im);
            for (b, h) in new_h.iter_mut().enumerate() {
                h[c] = Complex::new(re[b], im[b]);
            }
        }
        self.h_prev = new_h;
        self.n_bands = n_bands;
        self.ipd_hist = [vec![0.0; n_bands], vec![0.0; n_bands]];
        self.opd_hist = [vec![0.0; n_bands], vec![0.0; n_bands]];
    }

    /// The stereo band count in force.
    #[must_use]
    pub fn n_bands(&self) -> usize {
        self.n_bands
    }

    /// Process one stereo frame: mix `s` (mono hybrid) and `d`
    /// (de-correlated hybrid) into `(l, r)` hybrid signals per the
    /// resolved parameters. `config` must agree with `n_bands`.
    pub fn process(
        &mut self,
        ps: &PsData,
        idx: &PsIndices,
        config: HybridConfig,
        s: &[Vec<Complex>],
        d: &[Vec<Complex>],
    ) -> Result<HybridPair> {
        let nb = self.n_bands;
        let expected = match config {
            HybridConfig::Bands1020 => 20,
            HybridConfig::Bands34 => 34,
        };
        if expected != nb || s.len() != NUM_QMF_SLOTS || d.len() != NUM_QMF_SLOTS {
            return Err(Error::PsDataInvalid);
        }
        let b_k = parameter_map(config);
        let conj_k = conjugate_flags(config);
        let nr_hyb = config.nr_bands();
        if s.iter().chain(d.iter()).any(|row| row.len() != nr_hyb) {
            return Err(Error::PsDataInvalid);
        }

        // Per-slot H matrices, per stereo band.
        let mut h_slots = vec![vec![[Complex::default(); 4]; nb]; NUM_QMF_SLOTS];

        if ps.num_env == 0 {
            // §8.6.4.6.5: hold the previous coefficients all frame.
            for slot in h_slots.iter_mut() {
                slot.copy_from_slice(&self.h_prev);
            }
        } else {
            // Envelope borders n_e.
            let borders: Vec<usize> = if ps.frame_class {
                ps.border_position
                    .iter()
                    .map(|&b| usize::from(b).min(NUM_QMF_SLOTS - 1))
                    .collect()
            } else {
                (0..ps.num_env)
                    .map(|e| NUM_QMF_SLOTS * (e + 1) / ps.num_env - 1)
                    .collect()
            };

            let mut h_from = self.h_prev.clone();
            let mut n_from: isize = -1; // "border" behind slot 0
            for (e, &n_e) in borders.iter().enumerate() {
                let h_to = self.envelope_h(ps, idx, e)?;
                // §8.6.4.6.4: first region divides by n_0 with
                // multiplier n; later regions by (n_e − n_{e−1}).
                let (den, base) = if e == 0 {
                    (n_e.max(1) as f64, 0isize)
                } else {
                    (((n_e as isize - n_from).max(1)) as f64, n_from)
                };
                let lo = ((n_from + 1).max(0)) as usize;
                let hi = n_e.min(NUM_QMF_SLOTS - 1);
                for (n, slot) in h_slots.iter_mut().enumerate().take(hi + 1).skip(lo) {
                    let t = (n as isize - base) as f64 / den;
                    for (b, cell) in slot.iter_mut().enumerate() {
                        for c in 0..4 {
                            cell[c] = h_from[b][c] + (h_to[b][c] - h_from[b][c]) * t;
                        }
                    }
                }
                h_from = h_to;
                n_from = n_e as isize;
            }
            // Region after the last border: hold.
            let lo = ((n_from + 1).max(0)) as usize;
            for slot in h_slots.iter_mut().skip(lo) {
                slot.copy_from_slice(&h_from);
            }
            self.h_prev = h_from;
        }

        // Mix.
        let mut l = vec![vec![Complex::default(); nr_hyb]; NUM_QMF_SLOTS];
        let mut r = vec![vec![Complex::default(); nr_hyb]; NUM_QMF_SLOTS];
        for n in 0..NUM_QMF_SLOTS {
            for k in 0..nr_hyb {
                let b = usize::from(b_k[k]);
                let mut h = h_slots[n][b];
                if conj_k.contains(&k) {
                    for c in h.iter_mut() {
                        *c = c.conj();
                    }
                }
                l[n][k] = h[0] * s[n][k] + h[2] * d[n][k];
                r[n][k] = h[1] * s[n][k] + h[3] * d[n][k];
            }
        }
        Ok((l, r))
    }

    /// Derive `h11..h22` per stereo band for envelope `e`
    /// (§8.6.4.6.2 + §8.6.4.6.3), advancing the phase history.
    fn envelope_h(&mut self, ps: &PsData, idx: &PsIndices, e: usize) -> Result<Vec<H4>> {
        let nb = self.n_bands;

        // Map the parameter vectors to the stereo band count; a
        // disabled parameter kind is index 0 (§8.5.2 defaults).
        let iid = match idx.iid.get(e) {
            Some(v) => map_indices(v, nb),
            None => vec![0; nb],
        };
        let icc = match idx.icc.get(e) {
            Some(v) => map_indices(v, nb),
            None => vec![0; nb],
        };
        if iid.len() != nb || icc.len() != nb {
            return Err(Error::PsDataInvalid);
        }

        let fine = ps.config.iid_quant_fine();
        let rb = ps.config.icc_mode >= 3;

        let mut out = vec![[Complex::default(); 4]; nb];
        for b in 0..nb {
            let iid_db = if fine {
                *IID_DB_FINE
                    .get((iid[b] + 15) as usize)
                    .ok_or(Error::PsDataInvalid)?
            } else {
                *IID_DB_COARSE
                    .get((iid[b] + 7) as usize)
                    .ok_or(Error::PsDataInvalid)?
            };
            let c = 10f64.powf(iid_db / 20.0);
            let rho = *ICC_RHO.get(icc[b] as usize).ok_or(Error::PsDataInvalid)?;

            let (h11, h12, h21, h22) = if rb { mix_rb(c, rho) } else { mix_ra(c, rho) };
            out[b] = [
                Complex::new(h11, 0.0),
                Complex::new(h12, 0.0),
                Complex::new(h21, 0.0),
                Complex::new(h22, 0.0),
            ];
        }

        if ps.enable_ipdopd {
            // Zero-extended, band-count-mapped phase indices.
            let nr = ps.config.nr_ipdopd_par();
            let native = if ps.config.iid_mode % 3 == 0 {
                10
            } else if ps.config.iid_mode % 3 == 1 {
                20
            } else {
                34
            };
            let extend = |v: Option<&Vec<i32>>| -> Vec<f64> {
                let mut full = vec![0i32; native];
                if let Some(v) = v {
                    full[..nr.min(v.len())].copy_from_slice(&v[..nr.min(v.len())]);
                }
                map_indices(&full, nb)
                    .iter()
                    .map(|&i| f64::from(i) * core::f64::consts::FRAC_PI_4)
                    .collect()
            };
            let ipd_cur = extend(idx.ipd.get(e));
            let opd_cur = extend(idx.opd.get(e));
            for b in 0..nb {
                let sm = |h: &[Vec<f64>; 2], cur: f64| -> f64 {
                    let mut acc = Complex::default();
                    for (w, ang) in [(0.25, h[0][b]), (0.5, h[1][b]), (1.0, cur)] {
                        let (si, co) = ang.sin_cos();
                        acc += Complex::new(co * w, si * w);
                    }
                    acc.im.atan2(acc.re)
                };
                let phi_opd = sm(&self.opd_hist, opd_cur[b]);
                let phi_ipd = sm(&self.ipd_hist, ipd_cur[b]);
                let phi1 = phi_opd;
                let phi2 = phi_opd - phi_ipd;
                let (s1, c1) = phi1.sin_cos();
                let (s2, c2) = phi2.sin_cos();
                let r1 = Complex::new(c1, s1);
                let r2 = Complex::new(c2, s2);
                out[b][0] = out[b][0] * r1;
                out[b][2] = out[b][2] * r1;
                out[b][1] = out[b][1] * r2;
                out[b][3] = out[b][3] * r2;
            }
            // Advance the history.
            self.ipd_hist[0] = core::mem::take(&mut self.ipd_hist[1]);
            self.ipd_hist[1] = ipd_cur;
            self.opd_hist[0] = core::mem::take(&mut self.opd_hist[1]);
            self.opd_hist[1] = opd_cur;
        }
        Ok(out)
    }
}

/// §8.6.4.6.2.1 mixing procedure Ra.
fn mix_ra(c: f64, rho: f64) -> (f64, f64, f64, f64) {
    let denom = (1.0 + c * c).sqrt();
    let c1 = core::f64::consts::SQRT_2 / denom;
    let c2 = core::f64::consts::SQRT_2 * c / denom;
    let alpha = 0.5 * rho.clamp(-1.0, 1.0).acos();
    let beta = alpha * (c1 - c2) / core::f64::consts::SQRT_2;
    (
        (alpha + beta).cos() * c2,
        (beta - alpha).cos() * c1,
        (alpha + beta).sin() * c2,
        (beta - alpha).sin() * c1,
    )
}

/// §8.6.4.6.2.2 mixing procedure Rb.
fn mix_rb(c: f64, rho: f64) -> (f64, f64, f64, f64) {
    let rho = rho.max(0.05);
    let mut alpha = if (c - 1.0).abs() < 1e-12 {
        core::f64::consts::FRAC_PI_4
    } else {
        0.5 * (2.0 * c * rho / (c * c - 1.0)).atan()
    };
    // Modulo correction into [0, π/2).
    alpha -= (alpha / core::f64::consts::FRAC_PI_2).floor() * core::f64::consts::FRAC_PI_2;
    let mu = 1.0 + (4.0 * rho * rho - 4.0) / (c + 1.0 / c).powi(2);
    let gamma = ((1.0 - mu) / (1.0 + mu)).max(0.0).sqrt().atan();
    let s2 = core::f64::consts::SQRT_2;
    (
        s2 * alpha.cos() * gamma.cos(),
        s2 * alpha.sin() * gamma.cos(),
        -s2 * alpha.sin() * gamma.sin(),
        s2 * alpha.cos() * gamma.sin(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ps_data::PsIndexState;
    use oxideav_core::bits::{BitReader, BitWriter};

    /// Build a one-envelope FIX ps_data with the given uniform IID /
    /// ICC index (coarse grid, 10 pars each) and resolve it.
    fn ps_with(iid_idx: i32, icc_idx: i32) -> (PsData, PsIndices) {
        let mut w = BitWriter::new();
        w.write_bit(true); // header
        w.write_bit(true); // enable_iid
        w.write_u32(0, 3); // iid_mode 0
        w.write_bit(true); // enable_icc
        w.write_u32(0, 3); // icc_mode 0
        w.write_bit(false); // enable_ext
        w.write_bit(false); // FIX
        w.write_u32(1, 2); // num_env = 1
        w.write_bit(false); // iid freq
        for b in 0..10 {
            // First band carries the index, the rest delta 0.
            let (len, code) = crate::ps_huffman::HUFF_IID_DF[(iid_idx + 14) as usize];
            if b == 0 {
                w.write_u32(code, u32::from(len));
            } else {
                let (l0, c0) = crate::ps_huffman::HUFF_IID_DF[14];
                w.write_u32(c0, u32::from(l0));
            }
        }
        w.write_bit(false); // icc freq
        for b in 0..10 {
            let (len, code) = crate::ps_huffman::HUFF_ICC_DF[(icc_idx + 7) as usize];
            if b == 0 {
                w.write_u32(code, u32::from(len));
            } else {
                let (l0, c0) = crate::ps_huffman::HUFF_ICC_DF[7];
                w.write_u32(c0, u32::from(l0));
            }
        }
        let bytes = w.finish();
        let mut r = BitReader::new(&bytes);
        let ps = PsData::parse(&mut r, None).unwrap().unwrap();
        let mut st = PsIndexState::default();
        let idx = ps.resolve(&mut st).unwrap();
        (ps, idx)
    }

    fn ones(nr: usize) -> Vec<Vec<Complex>> {
        (0..NUM_QMF_SLOTS)
            .map(|_| vec![Complex::new(1.0, 0.0); nr])
            .collect()
    }

    fn zeros(nr: usize) -> Vec<Vec<Complex>> {
        (0..NUM_QMF_SLOTS)
            .map(|_| vec![Complex::default(); nr])
            .collect()
    }

    /// ICC = 1 (index 0) makes α = β = 0: the mix is pure IID panning
    /// `l = c2·s`, `r = c1·s`, `d` unused. Pin the exact §8.6.4.6.2.1
    /// scale factors at the last slot (interpolation complete).
    #[test]
    fn pure_iid_panning_matches_scale_factors() {
        let config = HybridConfig::Bands1020;
        let (ps, idx) = ps_with(7, 0); // +25 dB
        let mut st = PsStereo::new(20);
        let s = ones(config.nr_bands());
        let d = zeros(config.nr_bands());
        let (l, r) = st.process(&ps, &idx, config, &s, &d).unwrap();
        let c = 10f64.powf(25.0 / 20.0);
        let c1 = core::f64::consts::SQRT_2 / (1.0 + c * c).sqrt();
        let c2 = c * c1;
        // Hybrid channel 20 (stereo band 16), final slot: h fully
        // interpolated to the envelope value.
        let n = NUM_QMF_SLOTS - 1;
        assert!((l[n][20].re - c2).abs() < 1e-12, "{} vs {c2}", l[n][20].re);
        assert!((r[n][20].re - c1).abs() < 1e-12);
        assert!(l[n][20].im.abs() < 1e-15 && r[n][20].im.abs() < 1e-15);
        // Left is 25 dB louder.
        let ratio = 20.0 * (l[n][20].re / r[n][20].re).log10();
        assert!((ratio - 25.0).abs() < 1e-9);
    }

    /// ICC = −1 (index 7) with IID 0: α = π/2, the channels are the
    /// anti-phase de-correlated pair `l = d`, `r = −d` (§8.6.4.6.2.1).
    #[test]
    fn full_anticorrelation_uses_decorrelated_signal() {
        let config = HybridConfig::Bands1020;
        let (ps, idx) = ps_with(0, 7);
        let mut st = PsStereo::new(20);
        let s = ones(config.nr_bands());
        let d = ones(config.nr_bands());
        let (l, r) = st.process(&ps, &idx, config, &s, &d).unwrap();
        let n = NUM_QMF_SLOTS - 1;
        // h11 = cos(π/2) = 0, h21 = sin(π/2) = 1 → l = d.
        assert!((l[n][20].re - 1.0).abs() < 1e-12);
        // h12 = cos(−π/2) = 0, h22 = sin(−π/2) = −1 → r = −d.
        assert!((r[n][20].re + 1.0).abs() < 1e-12);
    }

    /// The first region interpolates from zero (fresh state) to the
    /// envelope coefficients linearly in n/n_0.
    #[test]
    fn first_region_interpolates_from_zero() {
        let config = HybridConfig::Bands1020;
        let (ps, idx) = ps_with(0, 0); // IID 0 dB, ICC 1 → h11 = h12 = 1
        let mut st = PsStereo::new(20);
        let s = ones(config.nr_bands());
        let d = zeros(config.nr_bands());
        let (l, _r) = st.process(&ps, &idx, config, &s, &d).unwrap();
        // num_env = 1, FIX → n_0 = 31; H(n) = n/31 · h.
        for (n, row) in l.iter().enumerate() {
            let expect = n as f64 / 31.0;
            assert!(
                (row[20].re - expect).abs() < 1e-12,
                "slot {n}: {} vs {expect}",
                row[20].re
            );
        }
        // A second identical frame is flat at the full value.
        let (l2, _) = st.process(&ps, &idx, config, &s, &d).unwrap();
        for row in &l2 {
            assert!((row[20].re - 1.0).abs() < 1e-12);
        }
    }

    /// num_env = 0 holds the previous coefficients for the whole
    /// frame.
    #[test]
    fn zero_envelopes_hold_previous_coefficients() {
        let config = HybridConfig::Bands1020;
        let (ps, idx) = ps_with(7, 0);
        let mut st = PsStereo::new(20);
        let s = ones(config.nr_bands());
        let d = zeros(config.nr_bands());
        let _ = st.process(&ps, &idx, config, &s, &d).unwrap();
        // Hold frame: num_env = 0 (frame_class FIX, num_env_idx 0).
        let mut hold = ps.clone();
        hold.num_env = 0;
        let empty = PsIndices::default();
        let (l, r) = st.process(&hold, &empty, config, &s, &d).unwrap();
        let c = 10f64.powf(25.0 / 20.0);
        let c1 = core::f64::consts::SQRT_2 / (1.0 + c * c).sqrt();
        for row in &l {
            assert!((row[20].re - c * c1).abs() < 1e-12);
        }
        for row in &r {
            assert!((row[20].re - c1).abs() < 1e-12);
        }
    }

    /// Rb at c = 1, ρ = 1: α = π/4, μ = 1, γ = 0 → an energy-
    /// preserving 45° rotation of the mono signal (`h11 = h12 = 1`,
    /// `h21 = h22 = 0`).
    #[test]
    fn rb_identity_point() {
        let (h11, h12, h21, h22) = mix_rb(1.0, 1.0);
        assert!((h11 - 1.0).abs() < 1e-12);
        assert!((h12 - 1.0).abs() < 1e-12);
        assert!(h21.abs() < 1e-12);
        assert!(h22.abs() < 1e-12);
    }

    /// Ra preserves total energy: |h11|² + |h12|² + |h21|² + |h22|²
    /// = c1² + c2² = 2 for every cue combination.
    #[test]
    fn ra_energy_invariant() {
        for iid in -7..=7 {
            for &rho in &ICC_RHO {
                let c = 10f64.powf(IID_DB_COARSE[(iid + 7) as usize] / 20.0);
                let (a, b, x, y) = mix_ra(c, rho);
                let e = a * a + b * b + x * x + y * y;
                assert!((e - 2.0).abs() < 1e-12, "iid {iid} rho {rho}: {e}");
            }
        }
    }
}
