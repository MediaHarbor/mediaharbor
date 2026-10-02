//! SSR per-channel gain-control + IPQF back-end driver (ISO/IEC
//! 14496-3 §4.6.12).
//!
//! [`SsrGainControl`] composes the four-band gain-control state
//! ([`crate::gain_control::GainBandState`]) and the IPQF synthesizer
//! ([`crate::ipqf::Ipqf`]) into one persistent per-channel pipeline.
//! Per frame it consumes the four per-band IMDCT outputs `U_{W,B}` plus
//! the `gain_control_data()` side info and returns the reconstructed
//! PCM time signal `AS(n)`:
//!
//! ```text
//! for each PQF band B in 0..4:
//!     V_B = GainBandState[B].window_overlap(ladder[B], U_B, seq)   §4.6.12.3.3
//! AS = IPQF.synthesize([V_0, V_1, V_2, V_3])                       §4.6.12.3.4
//! ```
//!
//! ## What the caller supplies
//!
//! This driver runs the §4.6.12.3.3–4 *back half* of the SSR tool: the
//! per-band gain windowing/overlap and the IPQF synthesis. The caller
//! is responsible for the §4.6.12.1 *front half* — splitting the
//! transmitted spectrum into the four PQF-band coefficient columns,
//! the even-band spectral reversal, and the per-band 256-coefficient
//! (long) / 32-coefficient (short) IMDCTs that produce the
//! non-overlapped `U_{W,B}` this driver consumes. The exact
//! spectrum-to-band arrangement is not spelled out in the staged §4.6.12
//! text and is tracked as a docs gap; see the crate README.
//!
//! ## Provenance
//!
//! Composes the §4.6.12.3.1–4 stages implemented in
//! [`crate::gain_control`] and [`crate::ipqf`]; no new tables. No
//! external SSR implementation was consulted.

use crate::gain_control::{band_record, GainBandState};
use crate::gain_control_data::GainControlData;
use crate::ics_info::WindowSequence;
use crate::ipqf::{Ipqf, NUM_BANDS};

/// One channel's persistent SSR gain-control + IPQF state: the four
/// per-band [`GainBandState`] carries plus the streaming [`Ipqf`].
#[derive(Debug, Clone)]
pub struct SsrGainControl {
    /// Per-PQF-band gain-control cross-frame state (`PFMD` / `PT`).
    bands: [GainBandState; NUM_BANDS],
    /// The streaming IPQF synthesizer (cross-frame band history).
    ipqf: Ipqf,
}

impl Default for SsrGainControl {
    fn default() -> Self {
        Self::new()
    }
}

impl SsrGainControl {
    /// A fresh per-channel SSR pipeline with the §4.6.12 spec initial
    /// state (`PFMD ≡ 1.0`, `PT ≡ 0.0`, zero IPQF history).
    #[must_use]
    pub fn new() -> Self {
        SsrGainControl {
            bands: core::array::from_fn(|_| GainBandState::new()),
            ipqf: Ipqf::new(),
        }
    }

    /// Reconstruct one frame of PCM `AS(n)` from the four per-band IMDCT
    /// outputs and the frame's gain-control side info.
    ///
    /// * `u` — the four non-overlapped per-band IMDCT outputs
    ///   `U_{W,B}`. `u[B]` is the band-`B` column: a single 512-sample
    ///   window for the long sequences, or eight 64-sample windows
    ///   concatenated for `EIGHT_SHORT_SEQUENCE`.
    /// * `gcd` — the decoded `gain_control_data()` (`None` ⇒ no gain
    ///   control active this frame, every band runs `T = U`).
    /// * `seq` — the frame's `window_sequence`.
    ///
    /// Returns `NUM_BANDS · |V_B|` PCM samples (`4 · 256 = 1024` for the
    /// steady `ONLY_LONG` / `EIGHT_SHORT` case).
    #[must_use]
    pub fn decode_frame(
        &mut self,
        u: &[Vec<f64>; NUM_BANDS],
        gcd: Option<&GainControlData>,
        seq: WindowSequence,
    ) -> Vec<f64> {
        // §4.6.12.3.3 — per-band gain windowing + overlap → V_B.
        let mut v: [Vec<f64>; NUM_BANDS] = core::array::from_fn(|_| Vec::new());
        for (b, slot) in v.iter_mut().enumerate() {
            // Spec band index is 1..=3 for gain-controlled bands; PQF
            // band 0 never carries a ladder (§4.6.12.3.3 `B == 0`).
            let ladder = gcd.and_then(|g| band_record(g, b));
            *slot = self.bands[b].window_overlap(ladder, &u[b], seq);
        }

        // §4.6.12.3.4 — IPQF synthesis. All four V_B share the same
        // per-frame length by construction.
        let len = v[0].len();
        debug_assert!(v.iter().all(|vb| vb.len() == len));
        let band_refs: [&[f64]; NUM_BANDS] = core::array::from_fn(|b| v[b].as_slice());
        self.ipqf.synthesize(&band_refs, len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four bands of constant-zero U give silence out.
    #[test]
    fn zero_bands_give_silence() {
        let mut ssr = SsrGainControl::new();
        let u: [Vec<f64>; NUM_BANDS] = core::array::from_fn(|_| vec![0.0f64; 512]);
        let pcm = ssr.decode_frame(&u, None, WindowSequence::OnlyLong);
        assert_eq!(pcm.len(), 1024);
        assert!(pcm.iter().all(|&x| x == 0.0));
    }

    /// A steady ONLY_LONG stream produces 1024 PCM samples per frame and
    /// the pipeline is finite + deterministic.
    #[test]
    fn only_long_frame_is_1024_pcm() {
        let mut ssr = SsrGainControl::new();
        let u: [Vec<f64>; NUM_BANDS] =
            core::array::from_fn(|b| (0..512).map(|j| ((b * 512 + j) as f64) * 1e-3).collect());
        let pcm0 = ssr.decode_frame(&u, None, WindowSequence::OnlyLong);
        assert_eq!(pcm0.len(), 1024);
        assert!(pcm0.iter().all(|x| x.is_finite()));
        // A second identical frame also yields 1024 and threads state.
        let pcm1 = ssr.decode_frame(&u, None, WindowSequence::OnlyLong);
        assert_eq!(pcm1.len(), 1024);
        // The first and second frames differ (the overlap tail carries).
        assert!(pcm0 != pcm1);
    }

    /// Gain control with `max_band == 0` (the bare 2-bit field, no
    /// ladders) is the identity: same PCM as `None`.
    #[test]
    fn max_band_zero_matches_no_gain() {
        let u: [Vec<f64>; NUM_BANDS] = core::array::from_fn(|b| {
            (0..512)
                .map(|j| ((b + 1) as f64 * (j as f64 + 1.0)).sin())
                .collect()
        });
        let gcd = GainControlData {
            max_band: 0,
            bands: Vec::new(),
        };
        let mut a = SsrGainControl::new();
        let mut b = SsrGainControl::new();
        let pa = a.decode_frame(&u, Some(&gcd), WindowSequence::OnlyLong);
        let pb = b.decode_frame(&u, None, WindowSequence::OnlyLong);
        assert_eq!(pa.len(), pb.len());
        for (x, y) in pa.iter().zip(pb.iter()) {
            assert!((x - y).abs() < 1e-12);
        }
    }

    /// EIGHT_SHORT bands (eight 64-sample windows each) also reconstruct
    /// 1024 PCM samples per frame.
    #[test]
    fn eight_short_frame_is_1024_pcm() {
        let mut ssr = SsrGainControl::new();
        let u: [Vec<f64>; NUM_BANDS] =
            core::array::from_fn(|_| (0..512).map(|j| (j as f64 * 0.01).cos()).collect());
        let pcm = ssr.decode_frame(&u, None, WindowSequence::EightShort);
        assert_eq!(pcm.len(), 1024);
        assert!(pcm.iter().all(|x| x.is_finite()));
    }
}
