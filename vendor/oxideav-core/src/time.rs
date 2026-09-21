//! Time base and timestamp types.

use crate::rational::Rational;

/// A time base expressed as a rational number of seconds per tick.
///
/// A `TimeBase` of 1/48000 means each timestamp unit is 1/48000 second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimeBase(pub Rational);

impl TimeBase {
    /// Construct a time base of `num/den` seconds per tick.
    pub const fn new(num: i64, den: i64) -> Self {
        Self(Rational::new(num, den))
    }

    /// Construct a `TimeBase` representing `1/rate` seconds per tick —
    /// the canonical "sample-rate-style" base used by audio codecs
    /// (`1/48000` for 48 kHz PCM, `1/44100` for CD audio, `1/8000` for
    /// G.711) and by the common video bases (`1/90000` for MPEG-TS,
    /// `1/1000000` for microsecond PTS).
    ///
    /// Equivalent to `TimeBase::new(1, rate as i64)`, but reads more
    /// clearly at call sites and documents the inverse-of-rate
    /// convention so a reader doesn't have to mentally swap arguments.
    pub const fn from_rate(rate: u32) -> Self {
        Self(Rational::new(1, rate as i64))
    }

    /// `num` of the underlying [`Rational`]. Sugar over `tb.0.num` for
    /// callers that don't want to reach through the tuple-struct field.
    pub const fn num(&self) -> i64 {
        self.0.num
    }

    /// `den` of the underlying [`Rational`]. Sugar over `tb.0.den`.
    pub const fn den(&self) -> i64 {
        self.0.den
    }

    /// The underlying seconds-per-tick fraction.
    pub fn as_rational(&self) -> Rational {
        self.0
    }

    /// `true` when this time base is usable for rescaling — both terms
    /// non-zero. A zero denominator denotes "no defined time base" (the
    /// `1/0` placeholder some demuxers stamp on data-only streams);
    /// callers that want to skip rescaling on those streams can branch
    /// on `is_valid()` instead of re-doing the same `den != 0 && num != 0`
    /// check at every call site.
    pub const fn is_valid(&self) -> bool {
        self.0.num != 0 && self.0.den != 0
    }

    /// Convert a tick count in this time base to seconds.
    pub fn seconds_of(&self, ticks: i64) -> f64 {
        ticks as f64 * self.0.as_f64()
    }

    /// Convert a fractional-seconds count to the nearest tick count in
    /// this time base. The inverse of [`seconds_of`](Self::seconds_of):
    /// `seconds_of` goes
    /// `ticks → seconds`; `ticks_of` goes `seconds → ticks`. Useful
    /// for muxers and encoders that have a target wall-clock duration
    /// and need to land it on the stream's time base without hand-rolling
    /// the divide-and-round at every call site.
    ///
    /// Rounds half-away-from-zero (matches [`rescale`]). On an invalid
    /// time base (`is_valid() == false`) or when the result would exceed
    /// `i64` range, returns `0` — pick a defaulted timestamp rather than
    /// panicking, since callers are typically muxing best-effort output.
    pub fn ticks_of(&self, seconds: f64) -> i64 {
        // ticks = seconds / (num/den) = seconds * den / num
        if !self.is_valid() || !seconds.is_finite() {
            return 0;
        }
        let scaled = seconds * (self.0.den as f64) / (self.0.num as f64);
        if !scaled.is_finite() {
            return 0;
        }
        // Half-away-from-zero rounding, matching `rescale`.
        let rounded = if scaled >= 0.0 {
            (scaled + 0.5).floor()
        } else {
            (scaled - 0.5).ceil()
        };
        // Clamp to i64 range.
        if rounded >= i64::MAX as f64 {
            i64::MAX
        } else if rounded <= i64::MIN as f64 {
            i64::MIN
        } else {
            rounded as i64
        }
    }

    /// Rescale a timestamp from this time base to another.
    ///
    /// Saturates at the `i64` range boundaries and returns `0` on an
    /// undefined conversion (zero term in the factor) — see [`rescale`].
    pub fn rescale(&self, ts: i64, target: TimeBase) -> i64 {
        rescale(ts, self.0, target.0)
    }

    /// Rescale a timestamp from this time base to another with an
    /// explicit [`Rounding`] mode. See [`rescale_rnd`].
    pub fn rescale_rnd(&self, ts: i64, target: TimeBase, rounding: Rounding) -> i64 {
        rescale_rnd(ts, self.0, target.0, rounding)
    }

    /// Rescale a timestamp from this time base to another, reporting
    /// `None` instead of saturating or defaulting — see
    /// [`rescale_checked`].
    pub fn rescale_checked(&self, ts: i64, target: TimeBase) -> Option<i64> {
        rescale_checked(ts, self.0, target.0)
    }
}

/// Common time-base constants.
///
/// These are the rates that show up over and over across the workspace:
/// MPEG-TS / RTP video at 90 kHz, microsecond PTS (most demuxers'
/// "expose-everything" base), MKV at 1 ms, and the audio sample rates
/// the codec crates spend most of their lives at. Naming them once
/// removes the magic-numbers-at-call-sites that grep-fishing has to
/// distinguish from random integer literals.
impl TimeBase {
    /// 1/1 — one tick per second. The "no rescaling" identity base,
    /// useful for placeholders on streams without a defined cadence
    /// (e.g. one-shot SVG / image frames).
    pub const SECONDS: TimeBase = TimeBase::new(1, 1);

    /// 1/1000 — millisecond ticks (Matroska / WebM `Timecode` default).
    pub const MILLIS: TimeBase = TimeBase::new(1, 1_000);

    /// 1/1_000_000 — microsecond ticks (the base most demuxers expose
    /// to consumers when they want the finest sane resolution without
    /// going to nanoseconds).
    pub const MICROS: TimeBase = TimeBase::new(1, 1_000_000);

    /// 1/1_000_000_000 — nanosecond ticks.
    pub const NANOS: TimeBase = TimeBase::new(1, 1_000_000_000);

    /// 1/90000 — 90 kHz, the MPEG-TS / RTP video PTS clock.
    pub const MPEG_TS: TimeBase = TimeBase::new(1, 90_000);

    /// 1/48000 — 48 kHz audio sample-clock (Opus, AC-3, most modern
    /// AAC, DTS).
    pub const AUDIO_48K: TimeBase = TimeBase::new(1, 48_000);

    /// 1/44100 — 44.1 kHz audio sample-clock (CD audio, MP3 at 44.1,
    /// many FLAC streams).
    pub const AUDIO_44K1: TimeBase = TimeBase::new(1, 44_100);

    /// 1/8000 — 8 kHz audio sample-clock (G.711, G.722, G.729, AMR-NB).
    pub const AUDIO_8K: TimeBase = TimeBase::new(1, 8_000);
}

/// A timestamp in a particular time base.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Timestamp {
    /// Tick count in `base` units.
    pub value: i64,
    /// The time base the tick count is expressed in.
    pub base: TimeBase,
}

impl Timestamp {
    /// Construct a timestamp of `value` ticks in `base`.
    pub const fn new(value: i64, base: TimeBase) -> Self {
        Self { value, base }
    }

    /// Construct a timestamp at `seconds` in the given `base`, rounded
    /// to the nearest tick. Sugar over `Timestamp::new(base.ticks_of(s), base)`.
    pub fn from_seconds(seconds: f64, base: TimeBase) -> Self {
        Self::new(base.ticks_of(seconds), base)
    }

    /// The timestamp as fractional seconds (`value × base`).
    pub fn seconds(&self) -> f64 {
        self.base.seconds_of(self.value)
    }

    /// Rescale onto `target` with half-away-from-zero rounding; the
    /// saturating/defaulting semantics of [`rescale`].
    pub fn rescale(&self, target: TimeBase) -> Self {
        Self {
            value: self.base.rescale(self.value, target),
            base: target,
        }
    }

    /// Rescale onto `target` with an explicit [`Rounding`] mode. Muxers
    /// that must never stamp a DTS later than the true instant use
    /// [`Rounding::Floor`]; [`Rounding::NearestAway`] reproduces
    /// [`rescale`](Self::rescale).
    pub fn rescale_rnd(&self, target: TimeBase, rounding: Rounding) -> Self {
        Self {
            value: self.base.rescale_rnd(self.value, target, rounding),
            base: target,
        }
    }

    /// Rescale onto `target`, returning `None` when the conversion is
    /// undefined (zero term in the factor) or the result doesn't fit
    /// `i64` — instead of the defaulting/saturating [`rescale`](Self::rescale).
    pub fn checked_rescale(&self, target: TimeBase) -> Option<Self> {
        self.base
            .rescale_checked(self.value, target)
            .map(|value| Self {
                value,
                base: target,
            })
    }

    /// Advance the timestamp by `ticks` units in its own base. Returns
    /// `None` on `i64` overflow rather than wrapping silently — muxers
    /// that compute a packet-end timestamp at the edge of the
    /// representable range get a clean signal instead of a wrap.
    pub fn checked_add_ticks(&self, ticks: i64) -> Option<Self> {
        self.value.checked_add(ticks).map(|v| Self {
            value: v,
            base: self.base,
        })
    }

    /// Move the timestamp backwards by `ticks` units in its own base.
    /// Returns `None` on `i64` overflow.
    pub fn checked_sub_ticks(&self, ticks: i64) -> Option<Self> {
        self.value.checked_sub(ticks).map(|v| Self {
            value: v,
            base: self.base,
        })
    }

    /// Tick-difference `self - other` after rescaling `other` onto
    /// `self`'s base. Returns `None` when the subtraction would overflow
    /// `i64` (rare in practice but easy to surface cleanly).
    ///
    /// Use this to compute the duration between two `Timestamp`s that
    /// may have been produced by different sources (e.g. a packet from a
    /// container demuxer minus a packet from a different demuxer in a
    /// remux pipeline).
    pub fn checked_diff(&self, other: Timestamp) -> Option<i64> {
        let other_in_self_base = other.rescale(self.base).value;
        self.value.checked_sub(other_in_self_base)
    }
}

/// How a rescale operation rounds a result that falls between two
/// integer ticks of the target base.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rounding {
    /// Round to the nearest tick; a tie (exactly halfway) rounds away
    /// from zero (`+1.5 → +2`, `-1.5 → -2`). The default, and the mode
    /// the plain [`rescale`] uses.
    #[default]
    NearestAway,
    /// Round toward negative infinity. The right choice for DTS-like
    /// stamps that must never land *after* the true instant.
    Floor,
    /// Round toward positive infinity. The mirror of [`Rounding::Floor`]
    /// for end-of-range stamps that must never land *before* the true
    /// instant.
    Ceil,
    /// Round toward zero (truncate the fractional part).
    TowardZero,
}

/// Divide `|prod|` by `den` with the given rounding mode, in unsigned
/// magnitude space so no intermediate can overflow (`p ≤ 2^127`,
/// `d ≤ 2^126`, and every adjustment stays below `2^128`). `neg` is the
/// sign of the true quotient; sign-dependent modes (floor / ceil) use
/// it to pick the right direction.
fn div_round_abs(p: u128, d: u128, neg: bool, rounding: Rounding) -> u128 {
    let q = p / d;
    let r = p % d;
    // Whether the magnitude rounds up to q+1. Remainder-based so no
    // intermediate exceeds u128 (`r < d ≤ 2^126`, so `r * 2 < 2^127`).
    let bump = match rounding {
        // Ties (r*2 == d) round the magnitude up = away from zero.
        Rounding::NearestAway => r * 2 >= d,
        // Floor rounds a negative quotient's magnitude up, a positive
        // one down; Ceil is the mirror.
        Rounding::Floor => neg && r != 0,
        Rounding::Ceil => !neg && r != 0,
        Rounding::TowardZero => false,
    };
    if bump {
        // q + 1 can only wrap when q == u128::MAX (p = 2^128-1, d = 1);
        // the saturated value narrows to the same saturated i64 anyway.
        q.saturating_add(1)
    } else {
        q
    }
}

/// Narrow a sign+magnitude quotient to `i64`, saturating out-of-range
/// magnitudes.
fn sat_narrow(neg: bool, q_abs: u128) -> i64 {
    if neg {
        if q_abs >= 1u128 << 63 {
            i64::MIN
        } else {
            -(q_abs as i64)
        }
    } else if q_abs > i64::MAX as u128 {
        i64::MAX
    } else {
        q_abs as i64
    }
}

/// Narrow a sign+magnitude quotient to `i64`, reporting `None` for
/// out-of-range magnitudes.
fn checked_narrow(neg: bool, q_abs: u128) -> Option<i64> {
    if neg {
        if q_abs > 1u128 << 63 {
            None
        } else {
            Some((q_abs as i128).wrapping_neg() as i64)
        }
    } else if q_abs > i64::MAX as u128 {
        None
    } else {
        Some(q_abs as i64)
    }
}

/// Split the rescale factor into `(numerator, positive denominator)`,
/// folding the denominator's sign onto the numerator. `None` when the
/// denominator is zero (undefined conversion).
fn rescale_factor(from: Rational, to: Rational) -> Option<(i128, u128)> {
    // value * (from.num/from.den) / (to.num/to.den)
    //   = value * from.num * to.den / (from.den * to.num)
    let mut num = from.num as i128 * to.den as i128;
    let den = from.den as i128 * to.num as i128;
    if den == 0 {
        return None;
    }
    if den < 0 {
        // |num| ≤ 2^126, so the negation cannot overflow.
        num = -num;
    }
    Some((num, den.unsigned_abs()))
}

/// Rescale a value from one rational time base to another using 128-bit
/// intermediate arithmetic. Rounding is half-away-from-zero: a tie
/// rounds toward the larger magnitude (e.g. `+1.5 → +2`, `-1.5 → -2`).
///
/// Total — never panics or wraps: an undefined conversion factor
/// (`from.den * to.num == 0`) returns `0`, and a result outside `i64`
/// range **saturates** to `i64::MAX` / `i64::MIN`. Use
/// [`rescale_checked`] to detect those cases instead, or
/// [`rescale_rnd`] for a different rounding mode.
pub fn rescale(value: i64, from: Rational, to: Rational) -> i64 {
    rescale_rnd(value, from, to, Rounding::NearestAway)
}

/// [`rescale`] with an explicit [`Rounding`] mode. Same totality
/// guarantees: `0` on an undefined factor, saturation at the `i64`
/// boundaries.
pub fn rescale_rnd(value: i64, from: Rational, to: Rational, rounding: Rounding) -> i64 {
    let Some((num, den)) = rescale_factor(from, to) else {
        return 0;
    };
    let neg = (value < 0) != (num < 0);
    // |value| ≤ 2^63 and |num| ≤ 2^126, so the magnitude product can
    // reach ~2^189 with pathological bases; the true result is then far
    // outside i64 either way, so saturate by sign.
    let Some(p) = (value.unsigned_abs() as u128).checked_mul(num.unsigned_abs()) else {
        return if neg { i64::MIN } else { i64::MAX };
    };
    sat_narrow(neg && p != 0, div_round_abs(p, den, neg, rounding))
}

/// [`rescale`] that reports failure instead of papering over it:
/// returns `None` when the conversion factor is undefined
/// (`from.den * to.num == 0`) or the rounded result doesn't fit `i64`.
/// Rounding is half-away-from-zero, matching [`rescale`].
pub fn rescale_checked(value: i64, from: Rational, to: Rational) -> Option<i64> {
    let (num, den) = rescale_factor(from, to)?;
    let neg = (value < 0) != (num < 0);
    let p = (value.unsigned_abs() as u128).checked_mul(num.unsigned_abs())?;
    checked_narrow(
        neg && p != 0,
        div_round_abs(p, den, neg, Rounding::NearestAway),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescale_samples_to_pts() {
        // 48000 samples at 1/48000 base → 1 second at 1/1000 base = 1000 ticks
        assert_eq!(
            rescale(48000, Rational::new(1, 48000), Rational::new(1, 1000)),
            1000
        );
    }

    #[test]
    fn timestamp_seconds() {
        let ts = Timestamp::new(48000, TimeBase::new(1, 48000));
        assert!((ts.seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn rescale_rounds_half_away_from_zero() {
        // 1 tick at 1/2 s/tick → 1/1 base = 0.5 → ties up to 1.
        assert_eq!(rescale(1, Rational::new(1, 2), Rational::new(1, 1)), 1);
        // -1 tick at 1/2 s/tick → -0.5 → ties to -1 (away from zero).
        assert_eq!(rescale(-1, Rational::new(1, 2), Rational::new(1, 1)), -1);
        // 3 ticks at 1/2 → 1.5 → 2.
        assert_eq!(rescale(3, Rational::new(1, 2), Rational::new(1, 1)), 2);
        assert_eq!(rescale(-3, Rational::new(1, 2), Rational::new(1, 1)), -2);
    }

    #[test]
    fn rescale_saturates_instead_of_wrapping() {
        // i64::MAX seconds → milliseconds overflows ×1000; the result
        // used to wrap through `as i64`, now it saturates.
        assert_eq!(
            rescale(i64::MAX, Rational::new(1, 1), Rational::new(1, 1000)),
            i64::MAX
        );
        assert_eq!(
            rescale(i64::MIN, Rational::new(1, 1), Rational::new(1, 1000)),
            i64::MIN
        );
        // Sign flip through a negative factor saturates the other way.
        assert_eq!(
            rescale(i64::MAX, Rational::new(-1, 1), Rational::new(1, 1000)),
            i64::MIN
        );
        // Pathological factor whose 128-bit product overflows: still
        // saturates by sign instead of panicking.
        assert_eq!(
            rescale(
                i64::MAX,
                Rational::new(i64::MAX, 1),
                Rational::new(1, i64::MAX)
            ),
            i64::MAX
        );
        // Undefined factor (zero denominator term) stays 0.
        assert_eq!(rescale(5, Rational::new(1, 0), Rational::new(1, 1)), 0);
        assert_eq!(rescale(5, Rational::new(1, 1), Rational::new(0, 1)), 0);
    }

    #[test]
    fn rescale_negative_denominator_ties_away_from_zero() {
        // 3 ticks × (1/1) ÷ (-2/1) = -1.5 → -2 (half away from zero).
        // The old sign handling rounded these ties toward zero.
        assert_eq!(rescale(3, Rational::new(1, 1), Rational::new(-2, 1)), -2);
        assert_eq!(rescale(-3, Rational::new(1, 1), Rational::new(-2, 1)), 2);
        // Non-tie sanity through a negative source den.
        assert_eq!(rescale(4, Rational::new(1, -2), Rational::new(1, 1)), -2);
    }

    #[test]
    fn rescale_checked_reports_failure() {
        // In-range conversions match the plain rescale.
        assert_eq!(
            rescale_checked(48000, Rational::new(1, 48000), Rational::new(1, 1000)),
            Some(1000)
        );
        // Overflow → None (plain rescale saturates).
        assert_eq!(
            rescale_checked(i64::MAX, Rational::new(1, 1), Rational::new(1, 1000)),
            None
        );
        // Undefined factor → None (plain rescale returns 0).
        assert_eq!(
            rescale_checked(5, Rational::new(1, 0), Rational::new(1, 1)),
            None
        );
        assert_eq!(
            rescale_checked(5, Rational::new(1, 1), Rational::new(0, 1)),
            None
        );
        // Exactly i64::MIN is representable, one tick below is not.
        assert_eq!(
            rescale_checked(i64::MIN, Rational::new(1, 1), Rational::new(1, 1)),
            Some(i64::MIN)
        );
        assert_eq!(
            rescale_checked(i64::MIN, Rational::new(2, 1), Rational::new(1, 1)),
            None
        );
    }

    #[test]
    fn rescale_rnd_modes() {
        let from = Rational::new(1, 2);
        let to = Rational::new(1, 1);
        // 5 × (1/2) = 2.5
        assert_eq!(rescale_rnd(5, from, to, Rounding::NearestAway), 3);
        assert_eq!(rescale_rnd(5, from, to, Rounding::Floor), 2);
        assert_eq!(rescale_rnd(5, from, to, Rounding::Ceil), 3);
        assert_eq!(rescale_rnd(5, from, to, Rounding::TowardZero), 2);
        // -5 × (1/2) = -2.5
        assert_eq!(rescale_rnd(-5, from, to, Rounding::NearestAway), -3);
        assert_eq!(rescale_rnd(-5, from, to, Rounding::Floor), -3);
        assert_eq!(rescale_rnd(-5, from, to, Rounding::Ceil), -2);
        assert_eq!(rescale_rnd(-5, from, to, Rounding::TowardZero), -2);
        // Exact results are mode-independent.
        for mode in [
            Rounding::NearestAway,
            Rounding::Floor,
            Rounding::Ceil,
            Rounding::TowardZero,
        ] {
            assert_eq!(rescale_rnd(4, from, to, mode), 2);
        }
        // Default mode is NearestAway (matches plain rescale).
        assert_eq!(
            rescale_rnd(5, from, to, Rounding::default()),
            rescale(5, from, to)
        );
    }

    #[test]
    fn timestamp_rescale_rnd_and_checked() {
        // 1 tick at 1/3 s → milliseconds = 333.33…
        let ts = Timestamp::new(1, TimeBase::new(1, 3));
        assert_eq!(ts.rescale_rnd(TimeBase::MILLIS, Rounding::Floor).value, 333);
        assert_eq!(ts.rescale_rnd(TimeBase::MILLIS, Rounding::Ceil).value, 334);
        assert_eq!(
            ts.rescale_rnd(TimeBase::MILLIS, Rounding::Ceil).base,
            TimeBase::MILLIS
        );
        // checked_rescale mirrors rescale in range…
        let ok = Timestamp::new(48_000, TimeBase::AUDIO_48K)
            .checked_rescale(TimeBase::MILLIS)
            .unwrap();
        assert_eq!(ok.value, 1000);
        assert_eq!(ok.base, TimeBase::MILLIS);
        // …and reports None past it.
        let edge = Timestamp::new(i64::MAX, TimeBase::SECONDS);
        assert!(edge.checked_rescale(TimeBase::MILLIS).is_none());
        assert_eq!(edge.rescale(TimeBase::MILLIS).value, i64::MAX);
    }

    #[test]
    fn from_rate_matches_long_form() {
        assert_eq!(TimeBase::from_rate(48_000), TimeBase::new(1, 48_000));
        assert_eq!(TimeBase::from_rate(90_000), TimeBase::new(1, 90_000));
        assert_eq!(TimeBase::from_rate(1), TimeBase::new(1, 1));
    }

    #[test]
    fn num_den_accessors() {
        let tb = TimeBase::new(1, 90_000);
        assert_eq!(tb.num(), 1);
        assert_eq!(tb.den(), 90_000);
        // Const-context callable.
        const NUM: i64 = TimeBase::AUDIO_48K.num();
        const DEN: i64 = TimeBase::AUDIO_48K.den();
        assert_eq!(NUM, 1);
        assert_eq!(DEN, 48_000);
    }

    #[test]
    fn is_valid_rejects_zero_terms() {
        assert!(TimeBase::new(1, 1000).is_valid());
        // Den == 0: undefined rate.
        assert!(!TimeBase::new(1, 0).is_valid());
        // Num == 0: degenerate ratio (everything is zero seconds).
        assert!(!TimeBase::new(0, 1).is_valid());
    }

    #[test]
    fn ticks_of_is_inverse_of_seconds_of() {
        // 1 second on a 1/48000 base = 48000 ticks.
        assert_eq!(TimeBase::AUDIO_48K.ticks_of(1.0), 48_000);
        // 1 second on a 1/90000 base = 90000 ticks.
        assert_eq!(TimeBase::MPEG_TS.ticks_of(1.0), 90_000);
        // 0.5 second on 1/1000 base = 500 ticks.
        assert_eq!(TimeBase::MILLIS.ticks_of(0.5), 500);
        // Round-trip on integer multiples.
        let tb = TimeBase::AUDIO_44K1;
        assert_eq!(tb.ticks_of(tb.seconds_of(44_100)), 44_100);
    }

    #[test]
    fn ticks_of_rounds_half_away_from_zero() {
        // 0.5 tick on 1/1 base → 1 (positive ties up).
        assert_eq!(TimeBase::SECONDS.ticks_of(0.5), 1);
        // -0.5 tick on 1/1 base → -1 (negative ties down).
        assert_eq!(TimeBase::SECONDS.ticks_of(-0.5), -1);
        // 1.5 ticks → 2.
        assert_eq!(TimeBase::SECONDS.ticks_of(1.5), 2);
        // -1.5 ticks → -2.
        assert_eq!(TimeBase::SECONDS.ticks_of(-1.5), -2);
    }

    #[test]
    fn ticks_of_invalid_inputs() {
        // Invalid time base → 0.
        assert_eq!(TimeBase::new(1, 0).ticks_of(1.0), 0);
        assert_eq!(TimeBase::new(0, 1).ticks_of(1.0), 0);
        // Non-finite seconds → 0.
        assert_eq!(TimeBase::MILLIS.ticks_of(f64::NAN), 0);
        assert_eq!(TimeBase::MILLIS.ticks_of(f64::INFINITY), 0);
        assert_eq!(TimeBase::MILLIS.ticks_of(f64::NEG_INFINITY), 0);
    }

    #[test]
    fn common_constants_match_long_form() {
        assert_eq!(TimeBase::SECONDS, TimeBase::new(1, 1));
        assert_eq!(TimeBase::MILLIS, TimeBase::new(1, 1_000));
        assert_eq!(TimeBase::MICROS, TimeBase::new(1, 1_000_000));
        assert_eq!(TimeBase::NANOS, TimeBase::new(1, 1_000_000_000));
        assert_eq!(TimeBase::MPEG_TS, TimeBase::new(1, 90_000));
        assert_eq!(TimeBase::AUDIO_48K, TimeBase::new(1, 48_000));
        assert_eq!(TimeBase::AUDIO_44K1, TimeBase::new(1, 44_100));
        assert_eq!(TimeBase::AUDIO_8K, TimeBase::new(1, 8_000));
    }

    #[test]
    fn timestamp_from_seconds() {
        let ts = Timestamp::from_seconds(1.0, TimeBase::AUDIO_48K);
        assert_eq!(ts.value, 48_000);
        assert_eq!(ts.base, TimeBase::AUDIO_48K);
        // Round-trip.
        assert!((ts.seconds() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn checked_add_sub_ticks_round_trip() {
        let ts = Timestamp::new(100, TimeBase::MILLIS);
        assert_eq!(ts.checked_add_ticks(50).unwrap().value, 150);
        assert_eq!(ts.checked_sub_ticks(50).unwrap().value, 50);
        // Base unchanged through the arithmetic.
        assert_eq!(ts.checked_add_ticks(50).unwrap().base, TimeBase::MILLIS);
    }

    #[test]
    fn checked_add_ticks_detects_overflow() {
        let ts = Timestamp::new(i64::MAX - 5, TimeBase::SECONDS);
        assert!(ts.checked_add_ticks(10).is_none());
        // Boundary case: i64::MAX exactly is fine.
        let near_max = Timestamp::new(i64::MAX - 1, TimeBase::SECONDS);
        assert_eq!(near_max.checked_add_ticks(1).unwrap().value, i64::MAX);
    }

    #[test]
    fn checked_sub_ticks_detects_overflow() {
        let ts = Timestamp::new(i64::MIN + 5, TimeBase::SECONDS);
        assert!(ts.checked_sub_ticks(10).is_none());
    }

    #[test]
    fn checked_diff_rescales_other_onto_self_base() {
        // 1 second at 1/48000 minus 500ms at 1/1000 = 500ms = 24000 ticks at 48k.
        let a = Timestamp::new(48_000, TimeBase::AUDIO_48K); // 1.0s
        let b = Timestamp::new(500, TimeBase::MILLIS); // 0.5s
        assert_eq!(a.checked_diff(b), Some(24_000));
    }

    #[test]
    fn checked_diff_same_base() {
        let a = Timestamp::new(1000, TimeBase::MILLIS);
        let b = Timestamp::new(250, TimeBase::MILLIS);
        assert_eq!(a.checked_diff(b), Some(750));
        assert_eq!(b.checked_diff(a), Some(-750));
    }
}
