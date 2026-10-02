//! MPEG-2 frequency-domain prediction — ISO/IEC 14496-3 §4.6.6
//! (carried over from ISO/IEC 13818-7).
//!
//! Frequency-domain prediction is the backward-adaptive intra-channel
//! predictor of the AAC **Main** object type. It exploits the
//! auto-correlation between the spectral components of consecutive
//! frames: for every MDCT line up to the §4.6.6.2 `PRED_SFB_MAX` limit
//! there is one second-order, backward-adaptive lattice predictor. The
//! predictor coefficients are derived from previously reconstructed
//! values on both encoder and decoder, so no coefficients are
//! transmitted — only the per-frame / per-sfb on/off side information
//! ([`crate::ics_info::PredictorData`], Table 4.6) controls whether the
//! reconstructed prediction error or the reconstructed spectral value is
//! carried.
//!
//! ## Scope of this module
//!
//! Prediction is only ever applied on the three long window sequences
//! (`ONLY_LONG_SEQUENCE`, `LONG_START_SEQUENCE`, `LONG_STOP_SEQUENCE`);
//! an `EIGHT_SHORT_SEQUENCE` disables prediction and resets every
//! predictor (§4.6.6.3.2.1 / §4.6.6.3.3). This module implements:
//!
//! * the §4.6.6.3.2.1 lattice `predict()` (estimate + LMS adaptation);
//! * the §4.6.6.3.2.3 `flt_round_inf()` 16-bit-float rounding used on
//!   every stored state variable and on the predicted value;
//! * the §4.6.6.3.2.1 per-frame reconstruction loop
//!   `x_rec = x_est + y_rec` on the predicted bands;
//! * the §4.6.6.3.3 predictor reset (cyclic group reset + short-block
//!   reset-all), with the 30 reset groups of Table 4.97.
//!
//! The decode steps, transcribed from the §4.6.6.3.2.1 pseudo code:
//!
//! ```text
//! if (ONLY_LONG || LONG_START || LONG_STOP) {
//!     for (sfb = 0; sfb < PRED_SFB_MAX; sfb++) {
//!         for (c = swb[sfb]; c < swb[sfb+1]; c++) {
//!             x_est[c] = predict();                 // lattice estimate
//!             if (predictor_data_present && prediction_used[sfb])
//!                 x_rec[c] = x_est[c] + y_rec[c];
//!             else
//!                 x_rec[c] = y_rec[c];
//!         }
//!     }
//! } else {
//!     reset_all_predictors();
//! }
//! ```
//!
//! Each per-coefficient predictor is run **every** frame (whether or not
//! its band is active) so its coefficients keep tracking the signal
//! statistics (§4.6.6.3.2.1, "all the predictors are run all the time").
//! The post-processing reset of the signalled group then follows
//! (§4.6.6.3.3, "after the normal predictor processing ... has been
//! carried out").
//!
//! Per §4.6.6 the predicted value `x_est` is rounded to a 16-bit float
//! before use, and the six saved state variables `r0, r1, COR1, COR2,
//! VAR1, VAR2` are stored as truncated 16-bit floats; both are realised
//! with [`flt_round_inf`].

use crate::ics_info::{IcsInfo, PredictorData, WindowSequence, PRED_SFB_MAX};
use crate::swb_offset::long_window_offsets;
use crate::Error;

type Result<T> = core::result::Result<T, Error>;

/// §4.6.6.3.2.1 LMS adaptation time constant `α = 0.90625`.
pub const ALPHA: f32 = 0.90625;

/// §4.6.6.3.2.1 attenuation factor `a = 0.953125`.
pub const A: f32 = 0.953125;

/// §4.6.6.3.2.1 attenuation factor `b = 0.953125`.
pub const B: f32 = 0.953125;

/// Number of cyclic reset groups (Table 4.97). Predictor `i` belongs to
/// reset group `(i mod 30) + 1` (the group numbers are 1-based and the
/// values `0` and `31` are reserved, §4.6.6.3.3).
pub const NUM_RESET_GROUPS: usize = 30;

/// §4.6.6.3.2.3 — round a single-precision float toward infinity to a
/// 16-bit float (a 7-bit mantissa: the 16 most-significant bits of the
/// IEEE-754 storage word).
///
/// This is the bit-exact transcription of the spec `flt_round_inf()`
/// pseudo code: the low 16 bits of the mantissa are discarded, and if
/// the most-significant discarded bit (`0x00008000`) was set, half an
/// lsb of the retained representation is added so the result rounds
/// toward (away from zero) infinity rather than truncating. The
/// add/subtract dance reproduces the spec's "add 1 lsb and elided one"
/// trick using only float arithmetic on the exponent/sign field.
pub fn flt_round_inf(pf: f32) -> f32 {
    let bits = pf.to_bits();
    // Most-significant discarded mantissa bit.
    let flg = bits & 0x0000_8000;
    // Truncate to the 16 msb (clears the low 16 mantissa bits).
    let truncated = bits & 0xffff_0000;
    let mut result = f32::from_bits(truncated);
    if flg != 0 {
        // Build "1 lsb" of the 16-bit representation from the retained
        // exponent + sign, then add it (carrying the elided leading
        // one) and subtract the elided one again — exactly the spec's
        // round-half-toward-infinity sequence.
        let exp_sign = truncated & 0xff80_0000;
        let one_lsb = exp_sign | 0x0001_0000;
        result += f32::from_bits(one_lsb);
        result -= f32::from_bits(exp_sign);
    }
    result
}

/// State of one second-order backward-adaptive lattice predictor
/// (§4.6.6.3.2.1), i.e. one MDCT line.
///
/// The six saved variables of §4.6.6.3.2.2 (`r0, r1, COR1, COR2, VAR1,
/// VAR2`) are stored as 16-bit-truncated floats. [`Self::new`] applies
/// the §4.6.6.3.3 initialisation `r0 = r1 = 0, COR1 = COR2 = 0,
/// VAR1 = VAR2 = 1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Predictor {
    /// `r_q,0(n-1)` — first basic element's delayed register.
    r0: f32,
    /// `r_q,1(n-1)` — second basic element's delayed register.
    r1: f32,
    /// `COR1(n-1)` — first element's running correlation estimate.
    cor1: f32,
    /// `COR2(n-1)` — second element's running correlation estimate.
    cor2: f32,
    /// `VAR1(n-1)` — first element's running variance estimate.
    var1: f32,
    /// `VAR2(n-1)` — second element's running variance estimate.
    var2: f32,
}

impl Default for Predictor {
    fn default() -> Self {
        Self::new()
    }
}

impl Predictor {
    /// §4.6.6.3.3 predictor initialisation: `r0 = r1 = 0,
    /// COR1 = COR2 = 0, VAR1 = VAR2 = 1`.
    pub fn new() -> Self {
        Self {
            r0: 0.0,
            r1: 0.0,
            cor1: 0.0,
            cor2: 0.0,
            var1: 1.0,
            var2: 1.0,
        }
    }

    /// §4.6.6.3.3 reset — re-initialise to the start-of-decoding state.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// `k_m(n) = COR_m(n-1) / VAR_m(n-1)` for `m = 1, 2`
    /// (§4.6.6.3.2.1). `VAR_m` is initialised to `1` and only ever grows
    /// by non-negative variance contributions, so it never reaches `0`.
    fn coefficients(&self) -> (f32, f32) {
        (self.cor1 / self.var1, self.cor2 / self.var2)
    }

    /// §4.6.6.3.2.1 `predict()` — form the estimate `x_est(n)` from the
    /// current state, **without** advancing it.
    ///
    /// The two cascaded basic elements compute
    /// `x_est,m(n) = b · k_m(n) · r_q,m-1(n-1)` and
    /// `x_est(n) = x_est,1(n) + x_est,2(n)`, where `r_q,0(n-1) = r0` and
    /// `r_q,1(n-1) = r1`. The result is rounded to a 16-bit float per
    /// §4.6.6.3.2.2 before use.
    pub fn predict(&self) -> f32 {
        let (k1, k2) = self.coefficients();
        let x_est1 = B * k1 * self.r0;
        let x_est2 = B * k2 * self.r1;
        flt_round_inf(x_est1 + x_est2)
    }

    /// §4.6.6.3.2.1 — advance the predictor by one frame given the
    /// reconstructed spectral value `x_rec(n)` of this line, updating the
    /// LMS correlation / variance estimates and the lattice registers.
    ///
    /// This realises the §4.6.6.3.2.1 recursion:
    ///
    /// ```text
    /// e_q,0(n)   = r_q,0(n)   = x_rec(n)                    (for adaptation)
    /// x_est,1(n) = b·k1(n)·r_q,0(n-1)
    /// e_q,1(n)   = e_q,0(n) − x_est,1(n)
    /// r_q,1(n)   = a·(r_q,0(n-1) − b·k1(n)·e_q,0(n))
    /// x_est,2(n) = b·k2(n)·r_q,1(n-1)
    /// COR_m(n)   = α·COR_m(n-1) + r_q,m-1(n-1)·e_q,m-1(n)
    /// VAR_m(n)   = α·VAR_m(n-1) + 0.5·(r_q,m-1²(n-1) + e_q,m-1²(n))
    /// r_q,0(n)   = a·x_rec(n)
    /// ```
    ///
    /// Every stored variable is rounded to a 16-bit float (§4.6.6.3.2.2).
    pub fn update(&mut self, x_rec: f32) {
        // Only element 1's coefficient enters the lattice register
        // update / second-element error; k2 only affects the estimate
        // (computed in `predict`).
        let k1 = self.cor1 / self.var1;

        // Element 1: e_q,0(n) = r_q,0(n) = x_rec(n).
        let e0 = x_rec;
        let r0_prev = self.r0;
        let r1_prev = self.r1;

        // x_est,1(n) = b·k1·r_q,0(n-1); e_q,1(n) = e_q,0(n) − x_est,1(n).
        let x_est1 = B * k1 * r0_prev;
        let e1 = e0 - x_est1;

        // Adapt element 1: COR1, VAR1 use r_q,0(n-1) and e_q,0(n).
        let cor1 = ALPHA * self.cor1 + r0_prev * e0;
        let var1 = ALPHA * self.var1 + 0.5 * (r0_prev * r0_prev + e0 * e0);

        // Adapt element 2: COR2, VAR2 use r_q,1(n-1) and e_q,1(n).
        let cor2 = ALPHA * self.cor2 + r1_prev * e1;
        let var2 = ALPHA * self.var2 + 0.5 * (r1_prev * r1_prev + e1 * e1);

        // New lattice registers.
        // r_q,1(n) = a·(r_q,0(n-1) − b·k1(n)·e_q,0(n)).
        let r1_new = A * (r0_prev - B * k1 * e0);
        // r_q,0(n) = a·x_rec(n).
        let r0_new = A * x_rec;

        self.r0 = flt_round_inf(r0_new);
        self.r1 = flt_round_inf(r1_new);
        self.cor1 = flt_round_inf(cor1);
        self.cor2 = flt_round_inf(cor2);
        self.var1 = flt_round_inf(var1);
        self.var2 = flt_round_inf(var2);
    }
}

/// A per-channel bank of §4.6.6 frequency-domain predictors, one for
/// every MDCT line up to the §4.6.6.2 `PRED_SFB_MAX` coefficient limit.
///
/// The bank lives for the whole channel decode (across frames), carrying
/// the backward-adaptive state. Construct one per channel with
/// [`PredictorBank::new`] and call [`PredictorBank::apply_long`] each
/// frame (§4.6.6.3.2.1) — including frames where prediction is off, so
/// the LMS coefficients keep adapting.
#[derive(Clone, Debug)]
pub struct PredictorBank {
    /// One predictor per coefficient index `0 .. num_predictors`.
    predictors: Vec<Predictor>,
}

impl PredictorBank {
    /// Build a fresh bank for the `fs_index` sampling-frequency index.
    ///
    /// The number of predictors is `swb_offset_long_window[fs_index]
    /// [PRED_SFB_MAX[fs_index]]`, i.e. the first MDCT line **above** the
    /// last predictable scalefactor band (§4.6.6.2 / Table 4.96). All
    /// predictors start in the §4.6.6.3.3 initial state.
    ///
    /// Errors: the [`Error`] from [`long_window_offsets`] for a bad
    /// `fs_index`, or [`Error::PredictorInvalid`] if the long-window
    /// offset table is too short to cover `PRED_SFB_MAX`.
    pub fn new(fs_index: u8) -> Result<Self> {
        let offsets = long_window_offsets(fs_index)?;
        let pred_sfb_max = PRED_SFB_MAX[fs_index as usize] as usize;
        let num_predictors = offsets
            .get(pred_sfb_max)
            .copied()
            .ok_or(Error::PredictorInvalid)? as usize;
        Ok(Self {
            predictors: vec![Predictor::new(); num_predictors],
        })
    }

    /// Number of per-line predictors in the bank.
    pub fn len(&self) -> usize {
        self.predictors.len()
    }

    /// Whether the bank carries no predictors.
    pub fn is_empty(&self) -> bool {
        self.predictors.is_empty()
    }

    /// §4.6.6.3.3 — reset every predictor in the bank (the
    /// `reset_all_predictors()` path taken on a short block).
    pub fn reset_all(&mut self) {
        for p in &mut self.predictors {
            p.reset();
        }
    }

    /// §4.6.6.3.3 — reset the predictors of one cyclic reset group.
    ///
    /// `group` is the 1-based `predictor_reset_group_number` (Table 4.97,
    /// valid range `1 ..= 30`). Predictor `i` belongs to group
    /// `(i mod 30) + 1`, so the members of group `g` are the lines
    /// `g-1, g-1+30, g-1+60, …`.
    ///
    /// Errors: [`Error::PredictorInvalid`] if `group` is `0` or `> 30`
    /// (the reserved values of §4.6.6.3.3).
    pub fn reset_group(&mut self, group: u8) -> Result<()> {
        if group == 0 || group as usize > NUM_RESET_GROUPS {
            return Err(Error::PredictorInvalid);
        }
        let start = (group - 1) as usize;
        let mut idx = start;
        while idx < self.predictors.len() {
            self.predictors[idx].reset();
            idx += NUM_RESET_GROUPS;
        }
        Ok(())
    }

    /// §4.6.6.3.2.1 — apply frequency-domain prediction to one channel's
    /// reconstructed long-window spectrum in place, then advance and (if
    /// signalled) reset the predictor bank.
    ///
    /// * `spec` — the decoded coefficients `y_rec` (the reconstructed
    ///   quantised prediction error or spectral value), modified to
    ///   `x_rec` on the predicted bands. Length must be at least the
    ///   bank's predictor count.
    /// * `ics_info` — provides `window_sequence` (prediction only acts on
    ///   the three long sequences; a short sequence resets the whole bank
    ///   and leaves the spectrum untouched) and `max_sfb` (bands at or
    ///   above `max_sfb` carry `prediction_used = 0`).
    /// * `pred` — the parsed §4.6.6.3.1 `predictor_data()` side info, or
    ///   `None` when `predictor_data_present == 0` (prediction off this
    ///   frame, but the bank is still run to keep adapting).
    /// * `fs_index` — selects the §4.5.4 long-window scalefactor-band
    ///   offsets.
    ///
    /// Returns `true` if prediction modified the spectrum, `false`
    /// otherwise (short block, or no active band).
    ///
    /// Errors: the [`Error`] from [`long_window_offsets`] for a bad
    /// `fs_index`; [`Error::PredictorInvalid`] if `spec` is shorter than
    /// the predictor bank or the reset-group number is reserved.
    pub fn apply_long(
        &mut self,
        spec: &mut [f64],
        ics_info: &IcsInfo,
        pred: Option<&PredictorData>,
        fs_index: u8,
    ) -> Result<bool> {
        // Short block: disable prediction and reset every predictor
        // (§4.6.6.3.2.1 else-branch / §4.6.6.3.3).
        if ics_info.window_sequence == WindowSequence::EightShort {
            self.reset_all();
            return Ok(false);
        }

        if spec.len() < self.predictors.len() {
            return Err(Error::PredictorInvalid);
        }

        let offsets = long_window_offsets(fs_index)?;
        let pred_sfb_max = PRED_SFB_MAX[fs_index as usize] as usize;
        let max_sfb = ics_info.max_sfb as usize;

        let mut modified = false;
        let num_predictors = self.predictors.len();
        // §4.6.6.3.2.1 — run every predictor every frame; only the
        // reconstruction differs by `prediction_used[sfb]`.
        for sfb in 0..pred_sfb_max {
            let fc = offsets[sfb] as usize;
            let lc = (offsets[sfb + 1] as usize).min(num_predictors);
            if fc >= lc {
                continue;
            }
            // A band at/above max_sfb has prediction_used = 0 (the bits
            // are not transmitted, §4.6.6.2).
            let active = sfb < max_sfb
                && pred.is_some_and(|p| p.prediction_used.get(sfb).copied().unwrap_or(false));
            for (p, y) in self.predictors[fc..lc]
                .iter_mut()
                .zip(spec[fc..lc].iter_mut())
            {
                let x_est = p.predict();
                let y_rec = *y as f32;
                let x_rec = if active {
                    modified = true;
                    // x_rec = x_est + y_rec, truncated to 16-bit float
                    // before being stored / fed back (§4.6.6.3.2.2).
                    flt_round_inf(x_est + y_rec)
                } else {
                    y_rec
                };
                *y = x_rec as f64;
                p.update(x_rec);
            }
        }

        // §4.6.6.3.3 — the signalled group reset is applied *after* the
        // normal per-frame processing.
        if let Some(p) = pred {
            if p.reset {
                if let Some(group) = p.reset_group_number {
                    self.reset_group(group)?;
                }
            }
        }

        Ok(modified)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::{WindowSequence, WindowShape};

    /// Build a minimal long-window `IcsInfo` for predictor tests.
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
            num_swb: 49,
        }
    }

    #[test]
    fn flt_round_inf_clears_low_16_bits_when_no_rounding() {
        // A value whose low 16 mantissa bits are already zero is a
        // fixed point of the rounding.
        let v = 1.5_f32; // 0x3FC00000, low 16 bits zero.
        assert_eq!(flt_round_inf(v).to_bits() & 0x0000_ffff, 0);
        assert_eq!(flt_round_inf(v), v);
    }

    #[test]
    fn flt_round_inf_result_always_has_zero_low_bits() {
        for &v in &[0.0_f32, 1.0, -1.0, 3.5_f32, -2.6_f32, 1e-8, 1e8, 0.953125] {
            let r = flt_round_inf(v);
            assert_eq!(
                r.to_bits() & 0x0000_ffff,
                0,
                "flt_round_inf({v}) left low mantissa bits set"
            );
        }
    }

    #[test]
    fn flt_round_inf_rounds_toward_infinity() {
        // Construct a positive value with the round bit (0x8000) set and
        // a larger magnitude in the remaining discarded bits: the result
        // must be >= the truncation.
        let bits = 1.0_f32.to_bits() | 0x0000_8001;
        let v = f32::from_bits(bits);
        let truncated = f32::from_bits(bits & 0xffff_0000);
        let r = flt_round_inf(v);
        assert!(r > truncated, "expected round-up: {r} vs trunc {truncated}");
        assert_eq!(r.to_bits() & 0x0000_ffff, 0);
    }

    #[test]
    fn fresh_predictor_predicts_zero() {
        // With r0 = r1 = 0, the estimate is 0 regardless of COR/VAR.
        let p = Predictor::new();
        assert_eq!(p.predict(), 0.0);
    }

    #[test]
    fn predictor_initial_state_matches_spec() {
        let p = Predictor::new();
        assert_eq!(p.r0, 0.0);
        assert_eq!(p.r1, 0.0);
        assert_eq!(p.cor1, 0.0);
        assert_eq!(p.cor2, 0.0);
        assert_eq!(p.var1, 1.0);
        assert_eq!(p.var2, 1.0);
    }

    #[test]
    fn update_then_reset_returns_to_initial() {
        let mut p = Predictor::new();
        for _ in 0..16 {
            p.update(0.7);
        }
        assert_ne!(p, Predictor::new());
        p.reset();
        assert_eq!(p, Predictor::new());
    }

    #[test]
    fn update_advances_lattice_register() {
        // After feeding x_rec, r0 should become flt_round_inf(a·x_rec).
        let mut p = Predictor::new();
        let x = 2.0_f32;
        p.update(x);
        assert_eq!(p.r0, flt_round_inf(A * x));
    }

    #[test]
    fn bank_size_covers_pred_sfb_max() {
        // fs_index 4 (44100 Hz): PRED_SFB_MAX = 40, swb[40] = 672.
        let bank = PredictorBank::new(4).unwrap();
        assert_eq!(bank.len(), 672);
        assert!(!bank.is_empty());
    }

    #[test]
    fn bank_size_24khz() {
        // fs_index 6 (24000 Hz): PRED_SFB_MAX = 41, swb[41] = 652.
        let bank = PredictorBank::new(6).unwrap();
        assert_eq!(bank.len(), 652);
    }

    #[test]
    fn reset_group_rejects_reserved_numbers() {
        let mut bank = PredictorBank::new(4).unwrap();
        assert!(matches!(bank.reset_group(0), Err(Error::PredictorInvalid)));
        assert!(matches!(bank.reset_group(31), Err(Error::PredictorInvalid)));
        assert!(bank.reset_group(1).is_ok());
        assert!(bank.reset_group(30).is_ok());
    }

    #[test]
    fn reset_group_only_touches_its_members() {
        let mut bank = PredictorBank::new(4).unwrap();
        // Dirty every predictor.
        for p in &mut bank.predictors {
            p.update(0.5);
        }
        let before: Vec<Predictor> = bank.predictors.clone();
        bank.reset_group(1).unwrap();
        // Group 1 members are lines 0, 30, 60, … — those must be fresh,
        // every other line unchanged.
        for (i, p) in bank.predictors.iter().enumerate() {
            if i % NUM_RESET_GROUPS == 0 {
                assert_eq!(*p, Predictor::new(), "line {i} should be reset");
            } else {
                assert_eq!(*p, before[i], "line {i} should be untouched");
            }
        }
    }

    #[test]
    fn short_block_resets_and_leaves_spectrum_untouched() {
        let mut bank = PredictorBank::new(4).unwrap();
        for p in &mut bank.predictors {
            p.update(0.3);
        }
        let mut ics = long_ics(40);
        ics.window_sequence = WindowSequence::EightShort;
        let mut spec = vec![1.0_f64; 1024];
        let original = spec.clone();
        let modified = bank.apply_long(&mut spec, &ics, None, 4).unwrap();
        assert!(!modified);
        assert_eq!(spec, original);
        // Every predictor is back to the initial state.
        for p in &bank.predictors {
            assert_eq!(*p, Predictor::new());
        }
    }

    #[test]
    fn prediction_off_leaves_spectrum_but_advances_state() {
        // predictor_data_present == 0: spectrum untouched, but predictors
        // still run (so they adapt). With a fresh bank, x_est = 0 so the
        // spectrum is unchanged either way; verify state advanced.
        let mut bank = PredictorBank::new(4).unwrap();
        let ics = long_ics(40);
        let mut spec = vec![2.0_f64; 1024];
        let original = spec.clone();
        let modified = bank.apply_long(&mut spec, &ics, None, 4).unwrap();
        assert!(!modified);
        assert_eq!(spec, original, "prediction-off must not alter the spectrum");
        // Predictors over the active range advanced (r0 = a·x_rec).
        assert_ne!(bank.predictors[0], Predictor::new());
    }

    #[test]
    fn active_band_modifies_spectrum_on_second_frame() {
        // Frame 1 primes the lattice; frame 2 produces a non-zero
        // estimate that is added on the active band.
        let mut bank = PredictorBank::new(4).unwrap();
        let mut ics = long_ics(40);
        ics.predictor_data_present = true;
        let pred = PredictorData {
            reset: false,
            reset_group_number: None,
            // Enable prediction on sfb 0 only.
            prediction_used: {
                let mut v = vec![false; 40];
                v[0] = true;
                v
            },
        };
        // Prime the lattice over several frames so the LMS correlation
        // and the delayed register r0 build up (a single frame leaves
        // COR1 = r0_prev·e0 = 0 because r0_prev starts at zero).
        for _ in 0..6 {
            let mut spec = vec![0.0_f64; 1024];
            for (c, s) in spec.iter_mut().enumerate().take(8) {
                *s = (c as f64) + 1.0;
            }
            bank.apply_long(&mut spec, &ics, Some(&pred), 4).unwrap();
        }
        // Next frame: an active band should now add a non-zero estimate.
        let mut spec2 = vec![1.0_f64; 1024];
        let y_rec = spec2.clone();
        let modified = bank.apply_long(&mut spec2, &ics, Some(&pred), 4).unwrap();
        assert!(modified);
        // At least one coefficient in sfb 0 changed from its y_rec.
        let band0_changed = (0..4).any(|c| spec2[c] != y_rec[c]);
        assert!(band0_changed, "active band 0 spectrum did not change");
    }

    #[test]
    fn reset_after_processing_clears_signalled_group() {
        let mut bank = PredictorBank::new(4).unwrap();
        let mut ics = long_ics(40);
        ics.predictor_data_present = true;
        let pred = PredictorData {
            reset: true,
            reset_group_number: Some(1),
            prediction_used: vec![true; 40],
        };
        let mut spec = vec![3.0_f64; 1024];
        bank.apply_long(&mut spec, &ics, Some(&pred), 4).unwrap();
        // Group 1 lines were reset *after* processing, so they are fresh.
        assert_eq!(bank.predictors[0], Predictor::new());
        assert_eq!(bank.predictors[NUM_RESET_GROUPS], Predictor::new());
        // A non-group-1 line still carries adapted state.
        assert_ne!(bank.predictors[1], Predictor::new());
    }

    #[test]
    fn spec_shorter_than_bank_is_rejected() {
        let mut bank = PredictorBank::new(4).unwrap();
        let ics = long_ics(40);
        let mut spec = vec![0.0_f64; 100];
        assert!(matches!(
            bank.apply_long(&mut spec, &ics, None, 4),
            Err(Error::PredictorInvalid)
        ));
    }

    #[test]
    fn bad_fs_index_propagates_error() {
        assert!(PredictorBank::new(13).is_err());
    }
}
