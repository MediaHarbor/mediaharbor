//! Long-Term Prediction (LTP) synthesis — ISO/IEC 14496-3 §4.6.7.
//!
//! LTP is a forward-adaptive, single-tap time-domain predictor that
//! reduces inter-frame redundancy for signals with a clear pitch.
//! Because the predictor coefficients are transmitted as side
//! information (`ltp_data()`, Table 4.55, parsed by
//! [`crate::ics_info::LtpData`]), the decoder applies the predictor
//! without the round-off sensitivity of the backward-adaptive MPEG-2
//! frequency-domain predictor (§4.6.6).
//!
//! ## Scope of this module
//!
//! This module implements the §4.6.7.3 **long-window** decoding
//! process, which is the only window family LTP supports for the AAC
//! LTP audio object type (§4.6.7.1 restricts LTP to long windows for
//! bitstream compatibility with MPEG-2 AAC). The three long sequences
//! (`ONLY_LONG_SEQUENCE`, `LONG_START_SEQUENCE`, `LONG_STOP_SEQUENCE`)
//! are handled; `EIGHT_SHORT_SEQUENCE` is a no-op here (LTP is disabled
//! and the per-window predictors are reset, §4.6.7.3 / the short-block
//! reset note).
//!
//! The decode steps, transcribed from the §4.6.7.3 pseudo code:
//!
//! ```text
//! x_est = predict();              // 1-tap time-domain prediction
//! X_est = MDCT(x_est);            // windowed analysis transform
//! for (sfb = 0; sfb < num_sfb; sfb++)
//!     if (ltp_data_present && ltp_long_used[sfb])
//!         X_rec = X_est + Y_rec;  // add predicted spectrum
//!     else
//!         X_rec = Y_rec;          // pass the transmitted spectrum
//! ```
//!
//! * `predict()` forms `x_est(i) = ltp_coef · x_rec(i − M − ltp_lag)`,
//!   `i = 0 … N−1`, with `M = 0` for every non-LD AOT (`M = N/2` for ER
//!   AAC LD, which is out of scope here). `x_rec` is the per-channel
//!   reconstruction history (see [`LtpState`]).
//! * `MDCT(x_est)` windows `x_est` with the current frame's §4.6.11
//!   long window and applies the §4.6.15.3.3 analysis transform
//!   ([`crate::filterbank::forward_mdct`]).
//! * `Y_rec` is the decoded (de-interleaved, inverse-quantised)
//!   spectrum; `X_est + Y_rec` replaces it on the sfb that carry
//!   `ltp_long_used == 1`.
//!
//! Per §4.6.7.4.1 (Figure 4.30) the LTP add precedes TNS synthesis in
//! the decode chain, so the spectrum passed in / out here is the
//! pre-TNS reconstructed spectrum.

use crate::filterbank::{forward_mdct, long_only_window};
use crate::ics_info::{IcsInfo, LtpData, WindowSequence, WindowShape};
use crate::swb_offset::{long_window_offsets, LONG_WINDOW_LEN};
use crate::Error;

type Result<T> = core::result::Result<T, Error>;

/// The long transform length `N = 2 · 1024 = 2048` (§4.6.11.3.1).
const LONG_TRANSFORM_LEN: usize = 2 * LONG_WINDOW_LEN as usize;

/// Table 4.98 — the 8-entry LTP coefficient codebook. `ltp_coef`
/// (3 bits) indexes this table; the value is the single-tap predictor
/// gain applied in [`LtpState::predict_long`].
pub const LTP_COEF: [f64; 8] = [
    0.570829, 0.696616, 0.813004, 0.911304, 0.984900, 1.067894, 1.194601, 1.369533,
];

/// Map a 3-bit `ltp_coef` index to its Table 4.98 gain.
///
/// Errors: [`Error::LtpInvalid`] if `index > 7`.
pub fn ltp_coefficient(index: u8) -> Result<f64> {
    LTP_COEF
        .get(index as usize)
        .copied()
        .ok_or(Error::LtpInvalid)
}

/// Per-channel LTP reconstruction-history buffer (§4.6.7.3).
///
/// The predictor reads `x_rec(i − M − ltp_lag)`; the buffer therefore
/// has to retain enough past output to cover the maximum lag
/// (`ltp_lag ≤ 2047`) plus the current transform window. The layout,
/// per §4.6.7.3:
///
/// * `x_rec(0 … N/2 − 1)` — the last aliased half window from the
///   current frame's IMDCT (the pre-overlap-add windowed tail);
/// * `x_rec(N/2 … N − 1)` — always all zeros;
/// * `x_rec(i < 0)` — the previous fully reconstructed time-domain
///   output of the decoder.
///
/// [`Self::history`] stores the `i < 0` region in chronological order
/// (oldest first), so `x_rec(j)` for `j < 0` is
/// `history[history.len() + j]`. [`Self::aliased_tail`] stores
/// `x_rec(0 … N/2 − 1)`. At the start of decoding the whole buffer is
/// zero, matching the §4.6.7.3 initialisation.
#[derive(Clone, Debug, Default)]
pub struct LtpState {
    /// Previously reconstructed decoder output (the `i < 0` region),
    /// oldest sample first. Capped at [`Self::history_cap`] samples.
    history: Vec<f64>,
    /// `x_rec(0 … N/2 − 1)` — the current frame's aliased IMDCT half
    /// window, `LONG_WINDOW_LEN` (1024) samples. Empty before the first
    /// frame (treated as zeros).
    aliased_tail: Vec<f64>,
}

impl LtpState {
    /// Maximum 11-bit `ltp_lag` (§4.6.7.2), used to size the history
    /// buffer so the deepest possible prediction still has data.
    const MAX_LAG: usize = 2047;

    /// A fresh, all-zero LTP state (§4.6.7.3 initialisation).
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of past-output samples to retain. The predictor needs
    /// `ltp_lag` samples before index 0, and the deepest window read is
    /// `N − 1`, so `MAX_LAG + N` past samples always suffice.
    fn history_cap() -> usize {
        Self::MAX_LAG + LONG_TRANSFORM_LEN
    }

    /// Read `x_rec(j)` for any integer index `j` per the §4.6.7.3
    /// buffer arrangement. Out-of-range indices (deeper than the
    /// retained history, or `j ≥ N`) read as zero, matching the
    /// zero-initialised buffer.
    fn x_rec(&self, j: isize) -> f64 {
        let half = LONG_WINDOW_LEN as isize; // N/2
        if j < 0 {
            // Previous fully reconstructed output, chronological.
            let idx = self.history.len() as isize + j;
            if idx < 0 {
                0.0
            } else {
                self.history[idx as usize]
            }
        } else if j < half {
            // Aliased IMDCT half window.
            self.aliased_tail.get(j as usize).copied().unwrap_or(0.0)
        } else {
            // x_rec(N/2 … N−1) is always zero.
            0.0
        }
    }

    /// §4.6.7.3 `predict()` — form the predicted time-domain signal
    /// `x_est(i) = ltp_coef · x_rec(i − M − ltp_lag)`, `i = 0 … N−1`,
    /// with `M = 0` (non-LD).
    fn predict_long(&self, lag: u16, coef: f64) -> Vec<f64> {
        let lag = lag as isize;
        (0..LONG_TRANSFORM_LEN as isize)
            .map(|i| coef * self.x_rec(i - lag))
            .collect()
    }

    /// Update the history after a frame is fully reconstructed.
    ///
    /// * `output` — this frame's `LONG_WINDOW_LEN` (1024) PCM samples,
    ///   i.e. the §4.6.11.3.3 overlap-added output, which become the
    ///   `i < 0` region for subsequent frames.
    /// * `aliased_tail` — this frame's `x_rec(0 … N/2 − 1)`, the
    ///   pre-overlap-add windowed IMDCT tail of length
    ///   `LONG_WINDOW_LEN`.
    ///
    /// Call once per frame, after synthesis, regardless of whether LTP
    /// was active, so the predictor history stays continuous.
    pub fn push_frame(&mut self, output: &[f64], aliased_tail: &[f64]) {
        self.history.extend_from_slice(output);
        let cap = Self::history_cap();
        if self.history.len() > cap {
            let excess = self.history.len() - cap;
            self.history.drain(0..excess);
        }
        self.aliased_tail.clear();
        self.aliased_tail.extend_from_slice(aliased_tail);
    }

    /// §4.6.7.3 — apply long-window LTP to one channel's reconstructed
    /// spectrum in place.
    ///
    /// * `spec` — the `LONG_WINDOW_LEN` (1024) decoded coefficients
    ///   `Y_rec`, modified to `X_rec` on the predicted bands.
    /// * `ics_info` — provides `window_sequence`, `window_shape` and
    ///   `max_sfb`; LTP only acts on the three long sequences.
    /// * `ltp` — the parsed §4.6.7.2 side info for this channel.
    /// * `prev_shape` — the previous block's `window_shape`, governing
    ///   the left half of this block's analysis window
    ///   (§4.6.11.3.2). `None` before the first frame, in which case
    ///   the block's own shape is used for both halves.
    /// * `fs_index` — the sampling-frequency index, selecting the
    ///   §4.5.4 long-window scalefactor-band offsets.
    ///
    /// When LTP is inactive (short sequence, or `ltp.long_used` all
    /// false) the spectrum is left untouched. Errors:
    /// [`Error::LtpInvalid`] for an out-of-range `ltp_coef`, a missing
    /// `ltp_lag`, or a spectrum length that is not `LONG_WINDOW_LEN`;
    /// the [`Error`] surfaced by [`long_window_offsets`] for a bad
    /// `fs_index`.
    pub fn apply_long(
        &self,
        spec: &mut [f64],
        ics_info: &IcsInfo,
        ltp: &LtpData,
        prev_shape: Option<WindowShape>,
        fs_index: u8,
    ) -> Result<()> {
        self.apply_long_with_analysis(spec, ics_info, ltp, prev_shape, fs_index, |_| Ok(()))
    }

    /// §4.6.7.3 + §4.6.7.4.1 — the LTP long-window add with the
    /// Figure 4.30 **TNS analysis filter** inserted between
    /// `X_est = MDCT(x_est)` and the per-sfb `X_rec = X_est + Y_rec`.
    ///
    /// When TNS is active on the channel, the transmitted residual
    /// `Y_rec` carried in `spec` lives in the noise-shaped (pre-TNS-
    /// synthesis) domain. The LTP-predicted spectrum `X_est` is a clean
    /// MDCT, so it has to be pushed through the same all-zero TNS
    /// analysis filter before it can be added like-for-like. `analyze`
    /// applies that filter in place to the freshly transformed `X_est`
    /// (length `LONG_WINDOW_LEN`); pass a no-op closure when the channel
    /// carries no TNS (which is what [`Self::apply_long`] does).
    ///
    /// The subsequent §4.6.9 TNS *synthesis* pass over the combined
    /// `X_rec` (run by the element driver after this add) undoes the
    /// analysis on the LTP contribution while shaping the residual,
    /// per the §4.6.7.4.1 inverse-filter relationship.
    ///
    /// All other semantics match [`Self::apply_long`].
    pub fn apply_long_with_analysis<F>(
        &self,
        spec: &mut [f64],
        ics_info: &IcsInfo,
        ltp: &LtpData,
        prev_shape: Option<WindowShape>,
        fs_index: u8,
        analyze: F,
    ) -> Result<()>
    where
        F: FnOnce(&mut [f64]) -> Result<()>,
    {
        // §4.6.7.3 / short-block note: prediction is disabled for
        // EIGHT_SHORT_SEQUENCE in the long-window LTP path.
        if ics_info.window_sequence == WindowSequence::EightShort {
            return Ok(());
        }
        if spec.len() != LONG_WINDOW_LEN as usize {
            return Err(Error::LtpInvalid);
        }
        // No bands flagged → nothing to add.
        if !ltp.long_used.iter().any(|&u| u) {
            return Ok(());
        }

        let coef = ltp_coefficient(ltp.coef)?;
        let lag = ltp.lag.ok_or(Error::LtpInvalid)?;

        // predict() → MDCT(x_est).
        let x_est = self.predict_long(lag, coef);
        let left_shape = prev_shape.unwrap_or(ics_info.window_shape);
        let window = long_only_window(left_shape, ics_info.window_shape);
        let z: Vec<f64> = x_est
            .iter()
            .zip(window.iter())
            .map(|(&x, &w)| x * w)
            .collect();
        let mut x_est_spec = forward_mdct(&z, LONG_TRANSFORM_LEN);

        // §4.6.7.4.1 / Figure 4.30: TNS analysis filter on X_est.
        analyze(&mut x_est_spec)?;

        // Per-sfb: X_rec = X_est + Y_rec where ltp_long_used[sfb].
        let offsets = long_window_offsets(fs_index)?;
        let num_sfb = ics_info.max_sfb as usize;
        for sfb in 0..num_sfb {
            if !ltp.long_used.get(sfb).copied().unwrap_or(false) {
                continue;
            }
            let start = offsets[sfb] as usize;
            let end = offsets[sfb + 1] as usize;
            for c in start..end.min(spec.len()) {
                spec[c] += x_est_spec[c];
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::WindowSequence;

    fn long_info(max_sfb: u8) -> IcsInfo {
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::OnlyLong,
            window_shape: WindowShape::Sine,
            max_sfb,
            scale_factor_grouping: None,
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: true,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 1,
            num_window_groups: 1,
            window_group_length: vec![1],
            num_swb: 49,
        }
    }

    fn ltp_with(coef: u8, lag: u16, long_used: Vec<bool>) -> LtpData {
        LtpData {
            lag_update: None,
            lag: Some(lag),
            coef,
            long_used,
        }
    }

    #[test]
    fn table_4_98_coefficients() {
        // Table 4.98 endpoints and a mid value.
        assert_eq!(ltp_coefficient(0).unwrap(), 0.570829);
        assert_eq!(ltp_coefficient(4).unwrap(), 0.984900);
        assert_eq!(ltp_coefficient(7).unwrap(), 1.369533);
        assert!(ltp_coefficient(8).is_err());
    }

    #[test]
    fn short_sequence_is_noop() {
        let st = LtpState::new();
        let mut info = long_info(40);
        info.window_sequence = WindowSequence::EightShort;
        let ltp = ltp_with(0, 100, vec![true; 40]);
        let mut spec = vec![1.0f64; LONG_WINDOW_LEN as usize];
        st.apply_long(&mut spec, &info, &ltp, None, 3).unwrap();
        assert!(spec.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn no_bands_flagged_is_noop() {
        let mut st = LtpState::new();
        // Seed history so a predictor would otherwise fire.
        let out = vec![0.5f64; LONG_WINDOW_LEN as usize];
        let tail = vec![0.25f64; LONG_WINDOW_LEN as usize];
        st.push_frame(&out, &tail);
        let info = long_info(40);
        let ltp = ltp_with(0, 100, vec![false; 40]);
        let mut spec = vec![1.0f64; LONG_WINDOW_LEN as usize];
        st.apply_long(&mut spec, &info, &ltp, None, 3).unwrap();
        assert!(spec.iter().all(|&v| v == 1.0));
    }

    #[test]
    fn zero_history_predicts_zero() {
        // §4.6.7.3 initialisation: x_rec all zero ⇒ x_est all zero ⇒
        // X_est all zero ⇒ spectrum unchanged even with bands flagged.
        let st = LtpState::new();
        let info = long_info(40);
        let ltp = ltp_with(7, 50, vec![true; 40]);
        let mut spec = vec![2.0f64; LONG_WINDOW_LEN as usize];
        st.apply_long(&mut spec, &info, &ltp, None, 3).unwrap();
        for &v in &spec {
            assert!((v - 2.0).abs() < 1e-12, "got {v}");
        }
    }

    #[test]
    fn predict_long_applies_lag_and_gain() {
        // Drive the predictor from a known history. With lag L and the
        // i<0 region holding a DC level d, x_est(i) = coef·d for all i
        // whose source index i−L < 0 (i.e. i < L). Verify a handful of
        // sample values directly via the private predictor.
        let mut st = LtpState::new();
        let d = 1.0f64;
        st.history = vec![d; LtpState::history_cap()];
        let coef = ltp_coefficient(2).unwrap(); // 0.813004
        let lag = 64u16;
        let x_est = st.predict_long(lag, coef);
        // i=0: source index −64 (in history) ⇒ coef·d.
        assert!((x_est[0] - coef * d).abs() < 1e-12);
        // i=63: source −1 ⇒ coef·d.
        assert!((x_est[63] - coef * d).abs() < 1e-12);
        // i=64: source 0 ⇒ aliased_tail (empty) ⇒ 0.
        assert!(x_est[64].abs() < 1e-12);
    }

    #[test]
    fn x_rec_regions_are_distinct() {
        let mut st = LtpState::new();
        st.history = vec![3.0; 10];
        st.aliased_tail = vec![7.0; LONG_WINDOW_LEN as usize];
        // i<0 region: most-recent past = 3.0.
        assert_eq!(st.x_rec(-1), 3.0);
        // Beyond retained history reads zero.
        assert_eq!(st.x_rec(-100), 0.0);
        // 0..N/2 is the aliased tail.
        assert_eq!(st.x_rec(0), 7.0);
        assert_eq!(st.x_rec(LONG_WINDOW_LEN as isize - 1), 7.0);
        // N/2..N is always zero.
        assert_eq!(st.x_rec(LONG_WINDOW_LEN as isize), 0.0);
    }

    #[test]
    fn push_frame_caps_history() {
        let mut st = LtpState::new();
        for _ in 0..4 {
            let out = vec![1.0f64; LONG_WINDOW_LEN as usize];
            let tail = vec![0.0f64; LONG_WINDOW_LEN as usize];
            st.push_frame(&out, &tail);
        }
        assert!(st.history.len() <= LtpState::history_cap());
    }

    #[test]
    fn nonzero_history_modifies_flagged_bands_only() {
        // With nonzero history, a flagged sfb gains X_est energy while
        // an unflagged sfb is untouched. fs_index 3 (48 kHz) long
        // offsets: sfb 0 = [0,4), so flag sfb 0 only and check bins
        // 0..4 changed but a high bin is unchanged.
        let mut st = LtpState::new();
        st.history = vec![1.0; LtpState::history_cap()];
        st.aliased_tail = vec![0.5; LONG_WINDOW_LEN as usize];
        let mut used = vec![false; 40];
        used[0] = true;
        let info = long_info(40);
        let ltp = ltp_with(5, 30, used);
        let baseline = vec![0.0f64; LONG_WINDOW_LEN as usize];
        let mut spec = baseline.clone();
        st.apply_long(&mut spec, &info, &ltp, None, 3).unwrap();
        let offsets = long_window_offsets(3).unwrap();
        let sfb0_end = offsets[1] as usize;
        let changed = (0..sfb0_end).any(|c| (spec[c] - baseline[c]).abs() > 1e-9);
        assert!(changed, "flagged sfb 0 should change");
        // A bin well above sfb 0 must be unchanged.
        assert!((spec[sfb0_end + 50] - baseline[sfb0_end + 50]).abs() < 1e-12);
    }
}
