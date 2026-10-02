//! Rational number used for time bases and frame rates.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, Div, Mul, Neg, Sub};

/// An exact fraction `num/den`.
///
/// # Equality vs. value comparison
///
/// The derived [`PartialEq`] / [`Eq`] / [`Hash`] are **structural**:
/// `1/2` and `2/4` compare *unequal* because their fields differ. This
/// is deliberate — it keeps `Hash` cheap and lets callers preserve the
/// exact on-wire fraction (e.g. a `30000/1001` frame rate must not be
/// silently folded into `30/1`). When you want to compare by *value*
/// — "do these two fractions denote the same number?" — use
/// [`Rational::equals_value`] or [`Rational::cmp_value`], which reduce
/// the comparison to an overflow-safe `i128` cross-product. Because the
/// derived `Eq` is structural and value-`cmp` is not, `Rational`
/// deliberately does **not** implement [`Ord`] / [`PartialOrd`] (a
/// value-based ordering would violate the `Ord`/`Eq` consistency
/// contract against the structural `Eq`).
///
/// # Overflow policy
///
/// All operations are total — nothing here panics, not even on
/// `i64::MIN` terms or zero denominators:
///
/// * The arithmetic operators (`+ - * /`) and [`reduced`](Self::reduced)
///   compute exactly in 128 bits, reduce to lowest terms, and — when the
///   reduced result still doesn't fit `i64` — return the **closest
///   representable approximation** (a saturated numerator for
///   out-of-range magnitudes, a rescaled `i64::MAX` denominator for
///   out-of-range precision) instead of silently wrapping.
/// * The `checked_add` / `checked_sub` / `checked_mul` / `checked_div`
///   variants return `None` in exactly the cases where the operators
///   would approximate, for callers that need to detect inexactness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    /// Numerator. Carries the sign after normalization; stored verbatim
    /// otherwise.
    pub num: i64,
    /// Denominator. Kept as constructed (may be negative or zero — see
    /// the type-level docs for how operations treat those).
    pub den: i64,
}

impl Rational {
    /// Construct the fraction `num/den`, stored verbatim (no reduction
    /// or sign normalization).
    pub const fn new(num: i64, den: i64) -> Self {
        Self { num, den }
    }

    /// The canonical zero: `0/1`.
    pub const fn zero() -> Self {
        Self { num: 0, den: 1 }
    }

    /// `true` when the numerator is zero (the denoted value is zero for
    /// any non-zero denominator).
    pub fn is_zero(&self) -> bool {
        self.num == 0
    }

    /// The denoted value as an `f64` (lossy for terms above 2^53;
    /// `±∞`/NaN for zero denominators).
    pub fn as_f64(&self) -> f64 {
        self.num as f64 / self.den as f64
    }

    /// Reduce the fraction to lowest terms. Sign is normalized onto the
    /// numerator.
    ///
    /// Computed in 128 bits so `i64::MIN` terms reduce correctly (e.g.
    /// `i64::MIN / i64::MIN` → `1/1`, `i64::MIN / -2` → `2^62 / 1`)
    /// instead of overflowing on negation. In the rare case where the
    /// reduced value still doesn't fit `i64` (e.g. `i64::MIN / -3`),
    /// the result is the closest representable approximation — see the
    /// type-level overflow policy.
    pub fn reduced(self) -> Self {
        reduce_i128(self.num as i128, self.den as i128)
    }

    /// Invert the fraction (num/den → den/num).
    pub fn invert(self) -> Self {
        Self {
            num: self.den,
            den: self.num,
        }
    }

    /// Compare two fractions by the *number they denote*, not by their
    /// stored fields. Uses a 128-bit cross-product so `30000/1001`
    /// and `30/1` order correctly without reducing or losing precision,
    /// and handles negative denominators by folding the sign onto the
    /// numerator first.
    ///
    /// A zero denominator is treated as a signed "infinity": `+n/0`
    /// sorts above every finite fraction, `-n/0` below every finite
    /// fraction, and `0/0` ties with a finite zero. All `+∞` compare
    /// equal to each other (likewise all `-∞`). This is a defensive
    /// total order, not a claim that such a fraction is meaningful.
    pub fn cmp_value(&self, other: &Self) -> Ordering {
        // Normalize sign onto the numerator so the cross-product
        // comparison is monotonic regardless of denominator sign. The
        // normalization happens in i128 so an `i64::MIN` term survives
        // negation.
        let (an, ad) = sign_normalized(self.num, self.den);
        let (bn, bd) = sign_normalized(other.num, other.den);
        if ad != 0 && bd != 0 {
            // Both finite: exact i128 cross-product (|terms| ≤ 2^63, so
            // each product is ≤ 2^126 and fits).
            return (an * bd).cmp(&(bn * ad));
        }
        // At least one side is zero-denominator. Map each value to an
        // "extended sign rank" on the line −∞ … 0 … +∞:
        //   +∞ (n>0, d=0) → +2,  −∞ (n<0, d=0) → −2,  0/0 → 0,
        //   finite > 0 → +1,  finite < 0 → −1,  finite == 0 → 0.
        // Comparing ranks yields a defensive total order: all +∞ tie,
        // all −∞ tie, ±∞ bracket every finite value, and 0/0 ties with
        // a finite zero. (Two finite operands never reach this branch;
        // they are compared exactly above.)
        fn rank(n: i128, d: i128) -> i128 {
            if d == 0 {
                n.signum() * 2
            } else {
                n.signum()
            }
        }
        rank(an, ad).cmp(&rank(bn, bd))
    }

    /// Whether two fractions denote the same number (value equality),
    /// e.g. `Rational::new(2, 4).equals_value(&Rational::new(1, 2))`.
    /// Contrast with `==`, which is structural (field-by-field).
    pub fn equals_value(&self, other: &Self) -> bool {
        self.cmp_value(other) == Ordering::Equal
    }

    /// The sign of the fraction: `-1`, `0`, or `1`. A zero denominator
    /// reports the sign of the numerator.
    pub fn signum(&self) -> i64 {
        let (n, _) = sign_normalized(self.num, self.den);
        n.signum() as i64
    }

    /// The absolute value of the fraction (sign stripped from both
    /// terms; a negative denominator is normalized onto the numerator
    /// first).
    ///
    /// An `i64::MIN` term (whose absolute value doesn't fit `i64`) is
    /// handled by reducing in 128 bits — `abs(i64::MIN / 2)` is exactly
    /// `2^62 / 1` — falling back to the closest representable
    /// approximation when reduction can't bring the term into range.
    /// All other inputs keep their fields verbatim (no reduction).
    pub fn abs(self) -> Self {
        let (n, d) = sign_normalized(self.num, self.den);
        let n = n.abs();
        if n <= i64::MAX as i128 && d <= i64::MAX as i128 {
            return Self {
                num: n as i64,
                den: d as i64,
            };
        }
        reduce_i128(n, d)
    }

    /// Exact addition: `Some(reduced sum)` when the reduced result fits
    /// `i64`, `None` otherwise (the `+` operator approximates instead).
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        let a = self.num as i128 * rhs.den as i128;
        let b = rhs.num as i128 * self.den as i128;
        let den = self.den as i128 * rhs.den as i128;
        reduce_exact_i128(a.checked_add(b)?, den)
    }

    /// Exact subtraction: `Some(reduced difference)` when the reduced
    /// result fits `i64`, `None` otherwise.
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        let a = self.num as i128 * rhs.den as i128;
        let b = rhs.num as i128 * self.den as i128;
        let den = self.den as i128 * rhs.den as i128;
        reduce_exact_i128(a.checked_sub(b)?, den)
    }

    /// Exact multiplication: `Some(reduced product)` when the reduced
    /// result fits `i64`, `None` otherwise.
    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        let num = self.num as i128 * rhs.num as i128;
        let den = self.den as i128 * rhs.den as i128;
        reduce_exact_i128(num, den)
    }

    /// Exact division: `Some(reduced quotient)` when the reduced result
    /// fits `i64`, `None` otherwise. Division by a zero-valued fraction
    /// yields a zero-denominator result (the same defensive "infinity"
    /// the operators produce), not `None`.
    pub fn checked_div(self, rhs: Self) -> Option<Self> {
        let num = self.num as i128 * rhs.den as i128;
        let den = self.den as i128 * rhs.num as i128;
        reduce_exact_i128(num, den)
    }
}

/// Move any sign on the denominator onto the numerator, leaving the
/// denominator non-negative. A zero denominator is left as-is. Widens
/// to `i128` so negating an `i64::MIN` term cannot overflow.
#[inline]
const fn sign_normalized(num: i64, den: i64) -> (i128, i128) {
    let (n, d) = (num as i128, den as i128);
    if d < 0 {
        (-n, -d)
    } else {
        (n, d)
    }
}

/// Reduce a 128-bit `num/den` pair exactly to lowest terms (sign on the
/// numerator) and narrow to `i64`. Returns `None` when either reduced
/// term is out of `i64` range.
fn reduce_exact_i128(mut num: i128, mut den: i128) -> Option<Rational> {
    if den < 0 {
        num = -num;
        den = -den;
    }
    let g = gcd_i128(num.unsigned_abs(), den.unsigned_abs()) as i128;
    if g > 1 {
        num /= g;
        den /= g;
    }
    Some(Rational {
        num: i64::try_from(num).ok()?,
        den: i64::try_from(den).ok()?,
    })
}

/// Reduce a 128-bit `num/den` pair to lowest terms (sign on the
/// numerator) and narrow back to `i64`. Used by the arithmetic
/// operators so intermediate products that overflow `i64` but reduce
/// back into range still yield the right answer. When even the reduced
/// form doesn't fit, returns the closest representable approximation
/// (never wraps):
///
/// * numerator out of range, denominator in range → numerator saturates
///   to `i64::MAX` / `i64::MIN` (magnitude overflow);
/// * denominator out of range, numerator in range → the fraction is
///   rescaled onto an `i64::MAX` denominator with a rounded numerator
///   (precision overflow — the value is tiny);
/// * both out of range → both terms are right-shifted (with rounding)
///   until the denominator fits, then the numerator saturates if it
///   still doesn't.
fn reduce_i128(mut num: i128, mut den: i128) -> Rational {
    if den < 0 {
        num = -num;
        den = -den;
    }
    let g = gcd_i128(num.unsigned_abs(), den.unsigned_abs()) as i128;
    if g > 1 {
        num /= g;
        den /= g;
    }
    if let (Ok(n), Ok(d)) = (i64::try_from(num), i64::try_from(den)) {
        return Rational { num: n, den: d };
    }
    approx_narrow(num, den)
}

/// Best-effort narrowing of an already-reduced, sign-normalized
/// (`den >= 0`) `i128` fraction that doesn't fit `i64`. See
/// [`reduce_i128`] for the three cases.
fn approx_narrow(num: i128, den: i128) -> Rational {
    fn sat(v: i128) -> i64 {
        if v > i64::MAX as i128 {
            i64::MAX
        } else if v < i64::MIN as i128 {
            i64::MIN
        } else {
            v as i64
        }
    }
    if den <= i64::MAX as i128 {
        // Magnitude overflow: only the numerator is out of range.
        return Rational {
            num: sat(num),
            den: den as i64,
        };
    }
    if num.unsigned_abs() <= i64::MAX as u128 {
        // Precision overflow: |value| < 1 with a too-fine denominator.
        // Rescale onto an i64::MAX denominator, rounding half away from
        // zero. (|num| ≤ 2^63 and den/2 < 2^126, so no i128 overflow.)
        let n = (num * i64::MAX as i128 + num.signum() * (den / 2)) / den;
        return Rational {
            num: n as i64,
            den: i64::MAX,
        };
    }
    // Both out of range: shift the denominator into range, apply the
    // same shift to the numerator (rounding half up on the magnitude),
    // and saturate whatever still doesn't fit.
    let dbits = 128 - den.leading_zeros();
    let k = dbits - 63;
    let half = 1u128 << (k - 1);
    let n_abs = (num.unsigned_abs() + half) >> k;
    let d = ((den as u128 + half) >> k).max(1);
    let n = if num < 0 {
        -(n_abs as i128)
    } else {
        n_abs as i128
    };
    Rational {
        num: sat(n),
        den: sat(d as i128),
    }
}

impl Add for Rational {
    type Output = Rational;
    /// Add two fractions, returning the result in lowest terms (or the
    /// closest representable approximation on overflow — see the
    /// type-level overflow policy).
    fn add(self, rhs: Self) -> Self {
        // The saturating add only differs from `+` when both cross
        // products are near ±2^126 (all four fields near ±2^63); the
        // result then approximates instead of panicking in debug.
        let a = self.num as i128 * rhs.den as i128;
        let b = rhs.num as i128 * self.den as i128;
        let den = self.den as i128 * rhs.den as i128;
        reduce_i128(a.saturating_add(b), den)
    }
}

impl Sub for Rational {
    type Output = Rational;
    /// Subtract `rhs` from `self`, returning the result in lowest terms
    /// (or the closest representable approximation on overflow).
    fn sub(self, rhs: Self) -> Self {
        let a = self.num as i128 * rhs.den as i128;
        let b = rhs.num as i128 * self.den as i128;
        let den = self.den as i128 * rhs.den as i128;
        reduce_i128(a.saturating_sub(b), den)
    }
}

impl Mul for Rational {
    type Output = Rational;
    /// Multiply two fractions, returning the result in lowest terms.
    fn mul(self, rhs: Self) -> Self {
        let num = self.num as i128 * rhs.num as i128;
        let den = self.den as i128 * rhs.den as i128;
        reduce_i128(num, den)
    }
}

impl Div for Rational {
    type Output = Rational;
    /// Divide `self` by `rhs` (multiply by the reciprocal), returning
    /// the result in lowest terms.
    fn div(self, rhs: Self) -> Self {
        let num = self.num as i128 * rhs.den as i128;
        let den = self.den as i128 * rhs.num as i128;
        reduce_i128(num, den)
    }
}

impl Neg for Rational {
    type Output = Rational;
    /// Negate the fraction (sign applied to the numerator). When the
    /// numerator is `i64::MIN` — whose negation doesn't fit `i64` — the
    /// sign is applied to the denominator instead, which denotes the
    /// same negated value; `-(i64::MIN / i64::MIN)` (value exactly `1`)
    /// returns `-1/1`, and `-(i64::MIN / 0)` (the defensive `-∞`)
    /// returns the saturated `+∞` `i64::MAX / 0`.
    fn neg(self) -> Self {
        if self.num != i64::MIN {
            Self {
                num: -self.num,
                den: self.den,
            }
        } else if self.den == 0 {
            Self {
                num: i64::MAX,
                den: 0,
            }
        } else if self.den != i64::MIN {
            Self {
                num: self.num,
                den: -self.den,
            }
        } else {
            Self { num: -1, den: 1 }
        }
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

fn gcd_i128(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduce() {
        assert_eq!(Rational::new(10, 20).reduced(), Rational::new(1, 2));
        assert_eq!(Rational::new(-6, 9).reduced(), Rational::new(-2, 3));
        assert_eq!(Rational::new(6, -9).reduced(), Rational::new(-2, 3));
    }

    #[test]
    fn invert() {
        assert_eq!(Rational::new(1, 2).invert(), Rational::new(2, 1));
    }

    #[test]
    fn cmp_value_orders_by_number_not_fields() {
        // Structurally unequal, value-equal.
        assert!(Rational::new(1, 2).equals_value(&Rational::new(2, 4)));
        assert_ne!(Rational::new(1, 2), Rational::new(2, 4));

        assert_eq!(
            Rational::new(1, 3).cmp_value(&Rational::new(1, 2)),
            std::cmp::Ordering::Less
        );
        assert_eq!(
            Rational::new(30_000, 1001).cmp_value(&Rational::new(30, 1)),
            std::cmp::Ordering::Less
        );
        // Negative denominator folds onto numerator for comparison.
        assert!(Rational::new(1, -2).equals_value(&Rational::new(-1, 2)));
        assert_eq!(
            Rational::new(-1, 2).cmp_value(&Rational::new(1, 2)),
            std::cmp::Ordering::Less
        );
    }

    #[test]
    fn cmp_value_zero_denominator_is_total() {
        let pos_inf = Rational::new(1, 0);
        let neg_inf = Rational::new(-1, 0);
        let finite = Rational::new(1_000_000, 1);
        assert_eq!(pos_inf.cmp_value(&finite), std::cmp::Ordering::Greater);
        assert_eq!(finite.cmp_value(&pos_inf), std::cmp::Ordering::Less);
        assert_eq!(neg_inf.cmp_value(&finite), std::cmp::Ordering::Less);
        assert_eq!(pos_inf.cmp_value(&neg_inf), std::cmp::Ordering::Greater);
        assert!(pos_inf.equals_value(&Rational::new(7, 0)));
    }

    #[test]
    fn signum_and_abs() {
        assert_eq!(Rational::new(3, 4).signum(), 1);
        assert_eq!(Rational::new(-3, 4).signum(), -1);
        assert_eq!(Rational::new(3, -4).signum(), -1);
        assert_eq!(Rational::new(0, 4).signum(), 0);
        assert_eq!(Rational::new(-3, 4).abs(), Rational::new(3, 4));
        assert_eq!(Rational::new(3, -4).abs(), Rational::new(3, 4));
    }

    #[test]
    fn arithmetic_reduces() {
        assert_eq!(
            Rational::new(1, 2) + Rational::new(1, 3),
            Rational::new(5, 6)
        );
        assert_eq!(
            Rational::new(1, 2) - Rational::new(1, 3),
            Rational::new(1, 6)
        );
        // 2/4 * 3/9 = 6/36 = 1/6
        assert_eq!(
            Rational::new(2, 4) * Rational::new(3, 9),
            Rational::new(1, 6)
        );
        // (1/2) / (3/4) = 4/6 = 2/3
        assert_eq!(
            Rational::new(1, 2) / Rational::new(3, 4),
            Rational::new(2, 3)
        );
        assert_eq!(-Rational::new(1, 2), Rational::new(-1, 2));
    }

    #[test]
    fn reduced_handles_i64_min_terms() {
        // Negative denominator with an i64::MIN numerator used to
        // overflow on negation; the i128 path reduces it correctly.
        assert_eq!(
            Rational::new(i64::MIN, i64::MIN).reduced(),
            Rational::new(1, 1)
        );
        assert_eq!(
            Rational::new(i64::MIN, -2).reduced(),
            Rational::new(1 << 62, 1)
        );
        assert_eq!(
            Rational::new(i64::MIN, 2).reduced(),
            Rational::new(-(1 << 62), 1)
        );
        assert_eq!(
            Rational::new(-2, i64::MIN).reduced(),
            Rational::new(1, 1 << 62)
        );
        // Coprime i64::MIN / -3: the exact reduction 2^63/3 doesn't fit,
        // so the numerator saturates (closest representable value).
        assert_eq!(
            Rational::new(i64::MIN, -3).reduced(),
            Rational::new(i64::MAX, 3)
        );
    }

    #[test]
    fn neg_handles_i64_min_numerator() {
        // Sign moves to the denominator when the numerator can't flip.
        let r = -Rational::new(i64::MIN, 5);
        assert_eq!(r, Rational::new(i64::MIN, -5));
        // Double negation restores the original value.
        assert!((-r).equals_value(&Rational::new(i64::MIN, 5)));
        // i64::MIN / i64::MIN denotes exactly 1 → its negation is -1.
        assert_eq!(-Rational::new(i64::MIN, i64::MIN), Rational::new(-1, 1));
        // Defensive -∞ negates to a saturated +∞.
        let inf = -Rational::new(i64::MIN, 0);
        assert_eq!(inf, Rational::new(i64::MAX, 0));
    }

    #[test]
    fn abs_signum_cmp_handle_i64_min_terms() {
        // abs of i64::MIN/2 reduces exactly to 2^62/1.
        assert_eq!(Rational::new(i64::MIN, 2).abs(), Rational::new(1 << 62, 1));
        // Coprime denominator: saturated approximation.
        assert_eq!(Rational::new(i64::MIN, 3).abs(), Rational::new(i64::MAX, 3));
        // i64::MIN *denominator* used to overflow in sign normalization.
        assert_eq!(Rational::new(3, i64::MIN).signum(), -1);
        assert_eq!(
            Rational::new(3, i64::MIN).cmp_value(&Rational::zero()),
            std::cmp::Ordering::Less
        );
        assert!(Rational::new(i64::MIN, i64::MIN).equals_value(&Rational::new(1, 1)));
    }

    #[test]
    fn checked_ops_exact_or_none() {
        // In-range results match the operators.
        assert_eq!(
            Rational::new(1, 2).checked_add(Rational::new(1, 3)),
            Some(Rational::new(5, 6))
        );
        assert_eq!(
            Rational::new(1, 2).checked_sub(Rational::new(1, 3)),
            Some(Rational::new(1, 6))
        );
        assert_eq!(
            Rational::new(2, 4).checked_mul(Rational::new(3, 9)),
            Some(Rational::new(1, 6))
        );
        assert_eq!(
            Rational::new(1, 2).checked_div(Rational::new(3, 4)),
            Some(Rational::new(2, 3))
        );
        // Results whose reduced form exceeds i64 report None.
        let max = Rational::new(i64::MAX, 1);
        assert_eq!(max.checked_add(Rational::new(1, 1)), None);
        assert_eq!(
            Rational::new(1 << 32, 1).checked_mul(Rational::new(1 << 32, 1)),
            None
        );
        assert_eq!(
            Rational::new(1, 1 << 32).checked_mul(Rational::new(1, 1 << 32)),
            None
        );
        // Division by zero-valued rhs is the defensive infinity, not None.
        assert_eq!(
            Rational::new(1, 2).checked_div(Rational::zero()),
            Some(Rational::new(1, 0))
        );
    }

    #[test]
    fn operators_approximate_instead_of_wrapping() {
        // Magnitude overflow: numerator saturates.
        let r = Rational::new(i64::MAX, 1) + Rational::new(1, 1);
        assert_eq!(r, Rational::new(i64::MAX, 1));
        let r = Rational::new(i64::MIN, 1) - Rational::new(1, 1);
        assert_eq!(r, Rational::new(i64::MIN, 1));
        // Precision overflow: denominator rescales onto i64::MAX with a
        // rounded numerator (value stays tiny, sign preserved).
        let tiny = Rational::new(1, i64::MAX) * Rational::new(1, 2);
        assert_eq!(tiny.den, i64::MAX);
        assert!(tiny.num == 0 || tiny.num == 1);
        let tiny_neg = Rational::new(-3, i64::MAX) * Rational::new(1, 2);
        assert_eq!(tiny_neg.den, i64::MAX);
        assert!(tiny_neg.num <= 0 && tiny_neg.num >= -2);
        // Large but reducible products still come out exact.
        let r = Rational::new(i64::MAX, 3) * Rational::new(3, i64::MAX);
        assert_eq!(r, Rational::new(1, 1));
    }

    #[test]
    fn arithmetic_uses_128bit_intermediates() {
        // Products that overflow i64 but reduce back into range must
        // still yield the correct reduced fraction.
        let big = Rational::new(i64::MAX / 2, 3);
        let r = big * Rational::new(3, 1);
        assert_eq!(r, Rational::new(i64::MAX / 2, 1));
        // num/den both large, equal → reduces to 1/1.
        let a = Rational::new(1_000_000_000, 1);
        let sum = a + a; // 2_000_000_000 / 1
        assert_eq!(sum, Rational::new(2_000_000_000, 1));
    }
}
