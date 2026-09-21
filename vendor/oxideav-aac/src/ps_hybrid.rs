//! PS hybrid filterbank — ISO/IEC 14496-3:2009 §8.6.4.3 / Annex 8.A.3.
//!
//! Parametric Stereo needs a finer frequency resolution at the bottom
//! of the spectrum than the 64-band QMF provides, so the lowest QMF
//! subbands are split further by 13-tap prototype filters (Tables
//! 8.36–8.38), producing the *hybrid* sub-subband domain:
//!
//! * **10/20 stereo bands** — QMF band 0 split by 8 (Type A, complex
//!   modulated) with the outer sub-subband pairs merged to 6 channels,
//!   QMF bands 1 and 2 split by 2 (Type B, cosine modulated); 71
//!   hybrid channels total (`6 + 2 + 2 + 61`).
//! * **34 stereo bands** — QMF band 0 split by 12, band 1 by 8, bands
//!   2–4 by 4 (all Type A); 91 hybrid channels (`12+8+4+4+4 + 59`).
//!
//! ```text
//! Type A: G_q^p[n] = g^p[n] · exp(j·2π/Q^p·(q+1/2)·(n−6))
//! Type B: G_q^p[n] = g^p[n] · cos(2π·q/Q^p·(n−6))
//! ```
//!
//! The prototypes are linear-phase with a 6-slot delay; per Annex
//! 8.A.3 the SBR combination feeds the filterbank 6 *look-ahead* QMF
//! slots (`XLow` beyond the current frame), so the hybrid output is
//! time-aligned with the QMF input at **zero net delay**: the unsplit
//! bands pass straight through and the split bands consume the
//! look-ahead. Filtering is the convolution
//! `y[n] = Σ_m G[m] · x[n+6−m]`, needing 6 history slots per split
//! band which [`PsHybrid`] threads across frames.
//!
//! ## Channel ordering (Figures 8.20 / 8.22)
//!
//! For the 10/20 configuration QMF band 0's eight Type-A outputs `q`
//! (sub-subband centres `(q+1/2)·π/8`, `q ≥ 4` the negative-frequency
//! mirrors) merge and reorder to six hybrid channels:
//! `s0 = q6, s1 = q7, s2 = q0, s3 = q1, s4 = q2+q5, s5 = q3+q4`.
//! QMF band 1's two Type-B outputs land **swapped** (`s6 = q1,
//! s7 = q0` — odd QMF bands are spectrally inverted), band 2's in
//! order (`s8 = q0, s9 = q1`). The 34-band configuration keeps every
//! split output in filter order (Figure 8.22).
//!
//! The synthesis (§8.6.4.7 / Figures 8.21, 8.23) is a plain adder:
//! sub-subbands of a split QMF band sum back into that band. Because
//! each prototype's sub-filters sum to a pure 6-slot delay (the
//! Type-A modulation phases cancel off-centre, the Type-B prototypes
//! vanish at the surviving off-centre taps), analysis followed by
//! synthesis reconstructs the input exactly — pinned by the tests.
//!
//! All truth from ISO/IEC 14496-3:2009 §8.6.4.3 / Annex 8.A staged
//! under `docs/audio/aac/`.

use crate::sbr_qmf::Complex;
use crate::{Error, Result};

/// QMF slots per PS stereo frame in the SBR combination
/// (`numQMFSlots = numTimeSlots · RATE`, Annex 8.A.3, 1024 framing).
pub const NUM_QMF_SLOTS: usize = 32;

/// Look-ahead slots supplied by the SBR low-band buffer (Annex 8.A.3).
pub const LOOKAHEAD: usize = 6;

/// Prototype filter length (§8.6.4.3).
const PROTO_LEN: usize = 13;

/// Table 8.37 — `g⁰[n]`, `Q⁰ = 8` (10/20 stereo bands, QMF band 0).
const G0_Q8: [f64; PROTO_LEN] = [
    0.00746082949812,
    0.02270420949825,
    0.04546865930473,
    0.07266113929591,
    0.09885108575264,
    0.11793710567217,
    0.125,
    0.11793710567217,
    0.09885108575264,
    0.07266113929591,
    0.04546865930473,
    0.02270420949825,
    0.00746082949812,
];

/// Table 8.37 — `g^{1,2}[n]`, `Q^{1,2} = 2` (10/20 bands, QMF 1–2).
const G12_Q2: [f64; PROTO_LEN] = [
    0.0,
    0.01899487526049,
    0.0,
    -0.07293139167538,
    0.0,
    0.30596630545168,
    0.5,
    0.30596630545168,
    0.0,
    -0.07293139167538,
    0.0,
    0.01899487526049,
    0.0,
];

/// Table 8.38 — `g⁰[n]`, `Q⁰ = 12` (34 stereo bands, QMF band 0).
const G0_Q12: [f64; PROTO_LEN] = [
    0.04081179924692,
    0.03812810994926,
    0.05144908135699,
    0.06399831151592,
    0.07428313801106,
    0.08100347892914,
    0.08333333333333,
    0.08100347892914,
    0.07428313801106,
    0.06399831151592,
    0.05144908135699,
    0.03812810994926,
    0.04081179924692,
];

/// Table 8.38 — `g¹[n]`, `Q¹ = 8` (34 bands, QMF band 1).
const G1_Q8: [f64; PROTO_LEN] = [
    0.01565675600122,
    0.03752716391991,
    0.05417891378782,
    0.08417044116767,
    0.10307344158036,
    0.12222452249753,
    0.125,
    0.12222452249753,
    0.10307344158036,
    0.08417044116767,
    0.05417891378782,
    0.03752716391991,
    0.01565675600122,
];

/// Table 8.38 — `g^{2,3,4}[n]`, `Q^{2,3,4} = 4` (34 bands, QMF 2–4).
const G234_Q4: [f64; PROTO_LEN] = [
    -0.05908211155639,
    -0.04871498374946,
    0.0,
    0.07778723915851,
    0.16486303567403,
    0.23279856662996,
    0.25,
    0.23279856662996,
    0.16486303567403,
    0.07778723915851,
    0.0,
    -0.04871498374946,
    -0.05908211155639,
];

/// The two §8.6.4.3 hybrid configurations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridConfig {
    /// 10 or 20 stereo bands: 71 hybrid channels, QMF bands 0–2 split.
    Bands1020,
    /// 34 stereo bands: 91 hybrid channels, QMF bands 0–4 split.
    Bands34,
}

impl HybridConfig {
    /// `NR_BANDS` — hybrid channel count (§8.6.4.5.1).
    #[must_use]
    pub fn nr_bands(&self) -> usize {
        match self {
            HybridConfig::Bands1020 => 71,
            HybridConfig::Bands34 => 91,
        }
    }

    /// Number of QMF bands that are split.
    fn split_bands(&self) -> usize {
        match self {
            HybridConfig::Bands1020 => 3,
            HybridConfig::Bands34 => 5,
        }
    }

    /// Split factor `Q^p` per split QMF band.
    fn q(&self, p: usize) -> usize {
        match self {
            HybridConfig::Bands1020 => [8, 2, 2][p],
            HybridConfig::Bands34 => [12, 8, 4, 4, 4][p],
        }
    }

    /// Prototype `g^p` per split QMF band.
    fn proto(&self, p: usize) -> &'static [f64; PROTO_LEN] {
        match self {
            HybridConfig::Bands1020 => [&G0_Q8, &G12_Q2, &G12_Q2][p],
            HybridConfig::Bands34 => [&G0_Q12, &G1_Q8, &G234_Q4, &G234_Q4, &G234_Q4][p],
        }
    }

    /// Whether split band `p` uses the Type-A (complex) modulation.
    fn type_a(&self, p: usize) -> bool {
        match self {
            HybridConfig::Bands1020 => p == 0,
            HybridConfig::Bands34 => true,
        }
    }
}

/// One channel's hybrid analysis/synthesis state: the 6 history slots
/// per split QMF band that the 13-tap convolution reaches into before
/// the current frame.
#[derive(Debug, Clone)]
pub struct PsHybrid {
    config: HybridConfig,
    /// `history[p][j]` — the previous frame's QMF slots `26..32` for
    /// split band `p` (`j = 0` is the oldest).
    history: Vec<[Complex; LOOKAHEAD]>,
}

impl PsHybrid {
    /// A fresh filterbank for `config` (zero history).
    #[must_use]
    pub fn new(config: HybridConfig) -> Self {
        PsHybrid {
            config,
            history: vec![[Complex::default(); LOOKAHEAD]; config.split_bands()],
        }
    }

    /// The active configuration.
    #[must_use]
    pub fn config(&self) -> HybridConfig {
        self.config
    }

    /// Switch configuration (a §8.6.4.6.1 stereo-band change resets
    /// the filter state instantaneously).
    pub fn reset(&mut self, config: HybridConfig) {
        self.config = config;
        self.history = vec![[Complex::default(); LOOKAHEAD]; config.split_bands()];
    }

    /// Hybrid analysis of one stereo frame.
    ///
    /// `x` is the Annex 8.A.3 `Xinput` matrix: at least
    /// `NUM_QMF_SLOTS + LOOKAHEAD` slots of 64 QMF bands (the trailing
    /// 6 slots only need bands `0..split_bands` populated). Returns
    /// `NUM_QMF_SLOTS` slots of `nr_bands()` hybrid channels, and
    /// advances the cross-frame history.
    pub fn analyze(&mut self, x: &[[Complex; 64]]) -> Result<Vec<Vec<Complex>>> {
        if x.len() < NUM_QMF_SLOTS + LOOKAHEAD {
            return Err(Error::PsDataInvalid);
        }
        let nb = self.config.nr_bands();
        let split = self.config.split_bands();
        let mut out = vec![vec![Complex::default(); nb]; NUM_QMF_SLOTS];

        for p in 0..split {
            // Extended buffer: 6 history slots + the frame + look-ahead.
            let mut buf = [Complex::default(); LOOKAHEAD + NUM_QMF_SLOTS + LOOKAHEAD];
            buf[..LOOKAHEAD].copy_from_slice(&self.history[p]);
            for (j, slot) in x.iter().enumerate().take(NUM_QMF_SLOTS + LOOKAHEAD) {
                buf[LOOKAHEAD + j] = slot[p];
            }
            let q_cnt = self.config.q(p);
            let g = self.config.proto(p);
            let type_a = self.config.type_a(p);
            for q in 0..q_cnt {
                // G_q[m] for m = 0..13.
                let mut filt = [Complex::default(); PROTO_LEN];
                for (m, f) in filt.iter_mut().enumerate() {
                    let arg = if type_a {
                        2.0 * core::f64::consts::PI / q_cnt as f64
                            * (q as f64 + 0.5)
                            * (m as f64 - 6.0)
                    } else {
                        2.0 * core::f64::consts::PI * q as f64 / q_cnt as f64 * (m as f64 - 6.0)
                    };
                    let (s, c) = arg.sin_cos();
                    *f = if type_a {
                        Complex::new(g[m] * c, g[m] * s)
                    } else {
                        Complex::new(g[m] * c, 0.0)
                    };
                }
                for (n, row) in out.iter_mut().enumerate() {
                    // y[n] = Σ_m G[m]·x[n+6−m]; buf[j] = x[j−6].
                    let mut acc = Complex::default();
                    for (m, &f) in filt.iter().enumerate() {
                        acc += f * buf[n + 12 - m];
                    }
                    accumulate_channel(&self.config, p, q, acc, row);
                }
            }
            // Next frame's x[−6..0] are this frame's slots 26..32.
            for j in 0..LOOKAHEAD {
                self.history[p][j] = x[NUM_QMF_SLOTS - LOOKAHEAD + j][p];
            }
        }

        // Unsplit QMF bands pass through at zero delay.
        for (n, row) in out.iter_mut().enumerate() {
            for k in split..64 {
                row[hybrid_offset(&self.config) + k - split] = x[n][k];
            }
        }
        Ok(out)
    }
}

/// First hybrid channel index of the unsplit QMF region.
fn hybrid_offset(config: &HybridConfig) -> usize {
    match config {
        HybridConfig::Bands1020 => 10,
        HybridConfig::Bands34 => 32,
    }
}

/// Route split-band filter output `q` of QMF band `p` into its hybrid
/// channel (Figures 8.20 / 8.22), merging where the 10/20
/// configuration combines sub-subbands.
fn accumulate_channel(config: &HybridConfig, p: usize, q: usize, v: Complex, row: &mut [Complex]) {
    match config {
        HybridConfig::Bands1020 => match p {
            0 => {
                // s0=q6, s1=q7, s2=q0, s3=q1, s4=q2+q5, s5=q3+q4.
                let k = match q {
                    6 => 0,
                    7 => 1,
                    0 => 2,
                    1 => 3,
                    2 | 5 => 4,
                    _ => 5, // 3 | 4
                };
                row[k] += v;
            }
            1 => {
                // Spectrally inverted odd QMF band: s6=q1, s7=q0.
                row[if q == 0 { 7 } else { 6 }] += v;
            }
            _ => {
                // Band 2 in order: s8=q0, s9=q1.
                row[8 + q] += v;
            }
        },
        HybridConfig::Bands34 => {
            // Figure 8.22: filter order, bands packed consecutively.
            let base = [0usize, 12, 20, 24, 28][p];
            row[base + q] += v;
        }
    }
}

/// Hybrid synthesis (§8.6.4.7): sum each split QMF band's sub-subbands
/// back into the band; copy the unsplit region. `rows` are
/// `nr_bands()`-wide hybrid slots; returns 64-band QMF slots.
#[must_use]
pub fn synthesize(config: HybridConfig, rows: &[Vec<Complex>]) -> Vec<[Complex; 64]> {
    let split = config.split_bands();
    let off = hybrid_offset(&config);
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let mut slot = [Complex::default(); 64];
        // Per-band sub-subband spans in the hybrid row.
        let spans: &[(usize, usize)] = match config {
            HybridConfig::Bands1020 => &[(0, 6), (6, 8), (8, 10)],
            HybridConfig::Bands34 => &[(0, 12), (12, 20), (20, 24), (24, 28), (28, 32)],
        };
        for (p, &(lo, hi)) in spans.iter().enumerate() {
            for v in &row[lo..hi] {
                slot[p] += *v;
            }
        }
        for k in split..64 {
            slot[k] = row[off + k - split];
        }
        out.push(slot);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_from(f: impl Fn(usize, usize) -> Complex) -> Vec<[Complex; 64]> {
        (0..NUM_QMF_SLOTS + LOOKAHEAD)
            .map(|n| {
                let mut s = [Complex::default(); 64];
                for (k, cell) in s.iter_mut().enumerate() {
                    *cell = f(n, k);
                }
                s
            })
            .collect()
    }

    /// Deterministic pseudo-random complex signal.
    fn noise(seed: u64) -> impl Fn(usize, usize) -> Complex {
        move |n, k| {
            let mut h = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add((n * 64 + k) as u64);
            h ^= h >> 33;
            h = h.wrapping_mul(0xff51afd7ed558ccd);
            h ^= h >> 33;
            let re = (h & 0xFFFF) as f64 / 65535.0 - 0.5;
            let im = ((h >> 16) & 0xFFFF) as f64 / 65535.0 - 0.5;
            Complex::new(re, im)
        }
    }

    /// Analysis followed by synthesis reconstructs the input exactly
    /// (both configurations, across a frame boundary so the history
    /// path is exercised).
    #[test]
    fn perfect_reconstruction_both_configs() {
        for config in [HybridConfig::Bands1020, HybridConfig::Bands34] {
            let mut fb = PsHybrid::new(config);
            // Two consecutive frames of one continuous signal: frame f
            // covers absolute slots 32f .. 32f+38.
            for f in 0..3 {
                let sig = noise(7);
                let x = frame_from(|n, k| sig(32 * f + n, k));
                let hyb = fb.analyze(&x).unwrap();
                assert_eq!(hyb.len(), NUM_QMF_SLOTS);
                assert_eq!(hyb[0].len(), config.nr_bands());
                let back = synthesize(config, &hyb);
                // The split-band path reaches 6 slots into history,
                // which is zero for the first frame's first slots —
                // skip the warm-up region of frame 0.
                let start = if f == 0 { LOOKAHEAD } else { 0 };
                for n in start..NUM_QMF_SLOTS {
                    for k in 0..64 {
                        let d = back[n][k] - x[n][k];
                        assert!(
                            d.norm_sqr() < 1e-24,
                            "cfg {config:?} frame {f} slot {n} band {k}: {d:?}"
                        );
                    }
                }
            }
        }
    }

    /// A complex exponential at the centre of QMF-band-0 sub-subband
    /// `q = 0` (frequency π/8·(0+1/2) = π/16) concentrates in hybrid
    /// channel `s2` of the 10/20 configuration — pinning the Figure
    /// 8.20 reorder (positive low frequencies land on s2/s3, negative
    /// on s1/s0).
    #[test]
    fn band0_positive_low_frequency_lands_on_s2() {
        let mut fb = PsHybrid::new(HybridConfig::Bands1020);
        let omega = core::f64::consts::PI / 16.0;
        let x = frame_from(|n, k| {
            if k == 0 {
                let (s, c) = (omega * n as f64).sin_cos();
                Complex::new(c, s)
            } else {
                Complex::default()
            }
        });
        let hyb = fb.analyze(&x).unwrap();
        // Steady-state slot (history warm-up over).
        let row = &hyb[20];
        let energies: Vec<f64> = (0..10).map(|k| row[k].norm_sqr()).collect();
        let max_k = (0..10)
            .max_by(|&a, &b| energies[a].partial_cmp(&energies[b]).unwrap())
            .unwrap();
        assert_eq!(max_k, 2, "energies: {energies:?}");
    }

    /// The negative mirror (−π/16) lands on s1 (`q = 7`).
    #[test]
    fn band0_negative_low_frequency_lands_on_s1() {
        let mut fb = PsHybrid::new(HybridConfig::Bands1020);
        let omega = -core::f64::consts::PI / 16.0;
        let x = frame_from(|n, k| {
            if k == 0 {
                let (s, c) = (omega * n as f64).sin_cos();
                Complex::new(c, s)
            } else {
                Complex::default()
            }
        });
        let hyb = fb.analyze(&x).unwrap();
        let row = &hyb[20];
        let energies: Vec<f64> = (0..10).map(|k| row[k].norm_sqr()).collect();
        let max_k = (0..10)
            .max_by(|&a, &b| energies[a].partial_cmp(&energies[b]).unwrap())
            .unwrap();
        assert_eq!(max_k, 1, "energies: {energies:?}");
    }

    /// Unsplit bands pass through unchanged at zero delay.
    #[test]
    fn unsplit_bands_pass_through() {
        let mut fb = PsHybrid::new(HybridConfig::Bands1020);
        let sig = noise(11);
        let x = frame_from(&sig);
        let hyb = fb.analyze(&x).unwrap();
        for n in 0..NUM_QMF_SLOTS {
            for k in 3..64 {
                let d = hyb[n][10 + k - 3] - x[n][k];
                assert!(d.norm_sqr() < 1e-30);
            }
        }
    }

    /// Short input is rejected.
    #[test]
    fn short_input_rejected() {
        let mut fb = PsHybrid::new(HybridConfig::Bands34);
        let x = vec![[Complex::default(); 64]; NUM_QMF_SLOTS];
        assert!(fb.analyze(&x).is_err());
    }
}
