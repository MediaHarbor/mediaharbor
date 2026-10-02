//! IPQF — the SSR inverse polyphase quadrature filter (ISO/IEC
//! 14496-3 §4.6.12.3.4).
//!
//! The IPQF is the final stage of the SSR (AOT 3) gain-control tool: it
//! recombines the four per-band gain-controlled sample streams `V_B`
//! (produced by [`crate::gain_control::GainBandState::window_overlap`])
//! into a single full-rate PCM time signal `AS(n)`, cancelling the
//! aliasing the encoder's PQF analysis introduced.
//!
//! ## Synthesis filter (§4.6.12.3.4)
//!
//! The four bands are interpolated 4× (one band sample every fourth
//! output sample) and cosine-modulated through a length-96 prototype
//! filter:
//!
//! ```text
//! Ṽ_B(j) = V_B(k)  if j == 4k,  else 0                  (4× upsample)
//!
//! Q_B(j) = Q(j) · cos( (2B+1)(2j−3)π / 16 ),  0 ≤ j ≤ 95
//!
//! AS(n) = Σ_{B=0}^{3} Σ_{j=0}^{95} Q_B(j) · Ṽ_B(n − j)
//! ```
//!
//! The length-96 prototype `Q(j)` is symmetric: `Q(0..=47)` are the
//! Table 4.110 values; `Q(48..=95)` mirror them as `Q(j) = Q(95 − j)`.
//!
//! Because the `Ṽ_B` interpolation places a band sample only at the
//! multiples of four, the inner sum over `j` touches band `B`'s history
//! at the strided positions `j ≡ n (mod 4)`. The synthesizer is run as
//! a streaming polyphase bank: it keeps a 96-tap (24 band-sample) ring
//! of recent `V_B` history per band so each call produces the next
//! block of `AS(n)` from the new band samples and the carried tail.
//!
//! ## Provenance
//!
//! The prototype coefficients are Table 4.110 of ISO/IEC 14496-3:2001
//! (a numeric data table) staged under `docs/audio/aac/`; the
//! modulation and upsampling equations are the §4.6.12.3.4 normative
//! formulas. No external SSR / PQF implementation was consulted.

use core::f64::consts::PI;

/// The number of IPQF bands (§4.6.12.1): four uniform frequency bands.
pub const NUM_BANDS: usize = 4;

/// The prototype filter length (§4.6.12.3.4): `Q(0..=95)`.
pub const PROTO_LEN: usize = 96;

/// `Q(0)..=Q(47)` — the first half of the §4.6.12.3.4 prototype filter,
/// ISO/IEC 14496-3 Table 4.110. The second half `Q(48)..=Q(95)` is the
/// mirror `Q(j) = Q(95 − j)` (see [`prototype`]).
///
/// The literals are the f64-exact shortest round-trip forms of the
/// Table 4.110 decimal values (the table prints ~17 significant
/// digits, more than an `f64` can distinguish; these are the canonical
/// shortest forms that decode to the identical bit pattern).
pub const Q_HALF: [f64; 48] = [
    9.765529100757551e-5,
    1.3809589379038567e-4,
    9.840074925662353e-5,
    -8.667154478233572e-5,
    -4.6217998911921346e-4,
    -1.0211814095158174e-3,
    -1.6772149340010668e-3,
    -2.253333895141108e-3,
    -2.4987888343213967e-3,
    -2.139081596676188e-3,
    -9.559539745459777e-4,
    1.1172111530118943e-3,
    3.909130912734858e-3,
    6.963570342011867e-3,
    9.559544215947834e-3,
    1.081576654002136e-2,
    9.87705149917153e-3,
    6.156256729132736e-3,
    -4.179394606362971e-4,
    -9.212874309770764e-3,
    -1.883077587336902e-2,
    -2.7226498457701823e-2,
    -3.2022840857588906e-2,
    -3.099633252775461e-2,
    -2.2656858741499447e-2,
    -6.803111385896335e-3,
    1.5085400948280744e-2,
    3.975099338827274e-2,
    6.244536362943674e-2,
    7.762232774872133e-2,
    7.996833849613293e-2,
    6.561549306847558e-2,
    3.331365830088269e-2,
    -1.4691563058190206e-2,
    -7.230789047533415e-2,
    -1.2993222541703875e-1,
    -1.7551641029040532e-1,
    -1.9626543957670528e-1,
    -1.807333067021503e-1,
    -1.2097653136035738e-1,
    -1.4377370758549035e-2,
    1.3522730742860303e-1,
    3.1737852699301633e-1,
    5.159002179848223e-1,
    7.108002037976138e-1,
    8.80906324884448e-1,
    1.0068321641150089e0,
    1.0737914947736096e0,
];

/// The full length-96 prototype filter `Q(0..=95)` (§4.6.12.3.4): the
/// [`Q_HALF`] first half plus its mirror `Q(j) = Q(95 − j)`.
#[must_use]
pub fn prototype() -> [f64; PROTO_LEN] {
    let mut q = [0.0f64; PROTO_LEN];
    q[..48].copy_from_slice(&Q_HALF);
    for j in 48..PROTO_LEN {
        q[j] = Q_HALF[95 - j];
    }
    q
}

/// The §4.6.12.3.4 synthesis-filter coefficient
/// `Q_B(j) = Q(j) · cos((2B+1)(2j−3)π/16)` for band `b`, tap `j`.
#[must_use]
fn synthesis_coef(q: &[f64; PROTO_LEN], b: usize, j: usize) -> f64 {
    let angle = (2.0 * b as f64 + 1.0) * (2.0 * j as f64 - 3.0) * PI / 16.0;
    q[j] * angle.cos()
}

/// Streaming IPQF synthesizer: holds the per-band `V_B` history needed
/// to evaluate the length-96 `AS(n)` convolution across frame
/// boundaries.
///
/// The interpolation `Ṽ_B(j) = V_B(j/4)` means tap `j` of the
/// convolution reads band sample `(n − j)/4` for `j ≡ n (mod 4)`. The
/// prototype spans `j ∈ 0..=95`, so the bank needs the last
/// `ceil(96/4) = 24` band samples per band; a 24-deep ring per band is
/// retained between [`Ipqf::synthesize`] calls.
#[derive(Debug, Clone)]
pub struct Ipqf {
    /// The precomputed `Q_B(j)` matrix, `[band][tap]`.
    coefs: [[f64; PROTO_LEN]; NUM_BANDS],
    /// Per-band history of the most recent band samples, newest last.
    /// Holds at least [`HISTORY`] entries once primed.
    history: [Vec<f64>; NUM_BANDS],
}

/// The number of past band samples the length-96 prototype reaches:
/// `ceil(PROTO_LEN / NUM_BANDS) = 24`.
const HISTORY: usize = PROTO_LEN.div_ceil(NUM_BANDS);

impl Default for Ipqf {
    fn default() -> Self {
        Self::new()
    }
}

impl Ipqf {
    /// A fresh synthesizer with the prototype-derived coefficients and
    /// zero-initialised history (the spec's implicit pre-stream
    /// silence).
    #[must_use]
    pub fn new() -> Self {
        let q = prototype();
        let mut coefs = [[0.0f64; PROTO_LEN]; NUM_BANDS];
        for (b, band) in coefs.iter_mut().enumerate() {
            for (j, slot) in band.iter_mut().enumerate() {
                *slot = synthesis_coef(&q, b, j);
            }
        }
        let history = core::array::from_fn(|_| vec![0.0f64; HISTORY]);
        Ipqf { coefs, history }
    }

    /// Synthesize `AS(n)` for `len` band-sample steps from the four
    /// per-band input streams `bands[B]`.
    ///
    /// Each `bands[B]` supplies the next `len` band samples `V_B`. The
    /// returned vector holds `NUM_BANDS · len` output samples — the
    /// IPQF interpolates each band sample to four full-rate positions,
    /// so `len` band steps produce `4·len` PCM samples.
    ///
    /// The convolution `AS(n) = Σ_B Σ_j Q_B(j)·Ṽ_B(n − j)` is evaluated
    /// at every output position `n`, reading band `B`'s history at the
    /// strided positions; the per-band history rings advance one band
    /// sample per step.
    ///
    /// # Panics
    ///
    /// Panics if any `bands[B]` has fewer than `len` samples.
    #[must_use]
    pub fn synthesize(&mut self, bands: &[&[f64]; NUM_BANDS], len: usize) -> Vec<f64> {
        let mut out = Vec::with_capacity(NUM_BANDS * len);
        for step in 0..len {
            // Push the new band sample of each band onto its ring.
            for (hist, band) in self.history.iter_mut().zip(bands.iter()) {
                hist.push(band[step]);
            }
            // For this band step we emit NUM_BANDS output samples
            // n = 4·step + p, p = 0..NUM_BANDS. Output position n reads
            // Ṽ_B(n − j): non-zero only when (n − j) ≡ 0 (mod 4), i.e.
            // the band sample index is (n − j)/4. The newest band sample
            // sits at history end (index L−1) and is the p-aligned
            // phase, so tap j = 4·t + p reads the t-th most recent
            // band sample.
            for p in 0..NUM_BANDS {
                let mut acc = 0.0f64;
                for (coefs, hist) in self.coefs.iter().zip(self.history.iter()) {
                    let l = hist.len();
                    let mut j = p;
                    while j < PROTO_LEN {
                        let t = j / NUM_BANDS; // how many band samples back
                        if t < l {
                            acc += coefs[j] * hist[l - 1 - t];
                        }
                        j += NUM_BANDS;
                    }
                }
                out.push(acc);
            }
            // Trim the rings to the needed depth to bound memory.
            for hist in &mut self.history {
                let l = hist.len();
                if l > HISTORY {
                    hist.drain(0..l - HISTORY);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prototype_is_symmetric() {
        let q = prototype();
        for j in 0..PROTO_LEN {
            assert!((q[j] - q[95 - j]).abs() < 1e-15, "Q({j}) != Q({})", 95 - j);
        }
        // Spot-check the documented endpoints.
        assert!((q[0] - 9.765529100757551e-5).abs() < 1e-18);
        assert!((q[47] - 1.0737914947736096e0).abs() < 1e-15);
        assert!((q[48] - 1.0737914947736096e0).abs() < 1e-15);
        assert!((q[95] - 9.765529100757551e-5).abs() < 1e-18);
    }

    #[test]
    fn silence_produces_silence() {
        let mut ipqf = Ipqf::new();
        let z = vec![0.0f64; 16];
        let bands: [&[f64]; NUM_BANDS] = [&z, &z, &z, &z];
        let out = ipqf.synthesize(&bands, 16);
        assert_eq!(out.len(), NUM_BANDS * 16);
        assert!(out.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn output_length_is_four_times_band_steps() {
        let mut ipqf = Ipqf::new();
        let s: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let bands: [&[f64]; NUM_BANDS] = [&s, &s, &s, &s];
        let out = ipqf.synthesize(&bands, 10);
        assert_eq!(out.len(), 40);
        assert!(out.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn synthesis_coef_first_band_zero_tap() {
        // Q_0(0) = Q(0)·cos((1)(−3)π/16).
        let q = prototype();
        let expect = q[0] * ((-3.0) * PI / 16.0).cos();
        assert!((synthesis_coef(&q, 0, 0) - expect).abs() < 1e-15);
    }

    #[test]
    fn impulse_response_matches_direct_convolution() {
        // Feed an impulse into band 0 and verify the streamed output
        // equals the direct §4.6.12.3.4 convolution for the first
        // several output samples: AS(n) = Σ_j Q_0(j)·Ṽ_0(n−j), with
        // Ṽ_0(0)=1 (impulse) and 0 elsewhere ⇒ AS(n) = Q_0(n).
        let q = prototype();
        let mut ipqf = Ipqf::new();
        let mut b0 = vec![0.0f64; 30];
        b0[0] = 1.0; // first band sample = 1 ⇒ Ṽ_0(0)=1.
        let z = vec![0.0f64; 30];
        let bands: [&[f64]; NUM_BANDS] = [&b0, &z, &z, &z];
        let out = ipqf.synthesize(&bands, 30);
        // AS(n) for n = 0..PROTO_LEN should equal Q_0(n).
        for (n, &got) in out.iter().take(PROTO_LEN).enumerate() {
            let expect = synthesis_coef(&q, 0, n);
            assert!(
                (got - expect).abs() < 1e-12,
                "AS({n}) = {got} != Q_0({n}) = {expect}"
            );
        }
    }
}
