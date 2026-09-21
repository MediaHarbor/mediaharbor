//! PS parameter-band maps — ISO/IEC 14496-3:2009 §8.6.4.6.1
//! (Tables 8.45 / 8.46 / 8.48 / 8.49).
//!
//! The stereo cues are defined per *stereo band* `b` (20 or 34 of
//! them), while the signal lives in 71 or 91 *hybrid channels* `k`.
//! [`parameter_map`] is `b(k)` — which stereo band governs each hybrid
//! channel — and [`conjugate_flags`] marks the negative-frequency
//! sub-subbands whose mixing coefficients apply conjugated
//! (the `*`-marked rows of Tables 8.48 / 8.49).
//!
//! [`map_10_to_20`], [`MAP_20_TO_34`] and [`MAP_34_TO_20`] convert
//! parameter vectors between band counts (§8.6.4.6.1): 10→20
//! duplicates every parameter; 20→34 and 34→20 follow Tables 8.45 and
//! 8.46, averaging in *ANSI-C integer arithmetic* on the index
//! representation (the same tables are reused with float arithmetic
//! for the `h`-coefficient hand-over when the stereo-band count
//! switches mid-stream).
//!
//! In the 34-band configuration `b(k)` is deliberately non-monotonic
//! over the split region: the short 13-tap sub-filters of QMF bands
//! 1–4 have pass-bands reaching into neighbouring QMF bands (e.g.
//! hybrid channel 14, the third sub-subband of QMF band 1, sits at
//! 5/8 of a QMF bandwidth — inside stereo band 4), exactly as the
//! Table 8.41 centre-frequency ladder describes.
//!
//! All truth from ISO/IEC 14496-3:2009 subpart 8 staged under
//! `docs/audio/aac/`.

use crate::ps_hybrid::HybridConfig;

/// Table 8.48 — `b(k)` for the 20-stereo-band configuration
/// (71 hybrid channels).
const B_K_20: [u8; 71] = [
    1, 0, 0, 1, 2, 3, 4, 5, 6, 7, // sub-QMF (k0/k1 conjugate)
    8, 9, 10, 11, 12, 13, // QMF 3..8
    14, 14, // 9-10
    15, 15, 15, // 11-13
    16, 16, 16, 16, // 14-17
    17, 17, 17, 17, 17, // 18-22
    18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, 18, // 23-34
    19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19, 19,
    19, 19, 19, 19, 19, // 35-63
];

/// Table 8.49 — `b(k)` for the 34-stereo-band configuration
/// (91 hybrid channels).
const B_K_34: [u8; 91] = [
    0, 1, 2, 3, 4, 5, 6, 6, 7, 2, 1, 0, // QMF band 0 (k9..k11 conjugate)
    10, 10, 4, 5, 6, 7, 8, 9, // QMF band 1
    10, 11, 12, 9, // QMF band 2
    14, 11, 12, 13, // QMF band 3
    14, 15, 16, 13, // QMF band 4
    16, // QMF 5
    17, // 6
    18, // 7
    19, // 8
    20, // 9
    21, // 10
    22, 22, // 11-12
    23, 23, // 13-14
    24, 24, // 15-16
    25, 25, // 17-18
    26, 26, // 19-20
    27, 27, 27, // 21-23
    28, 28, 28, // 24-26
    29, 29, 29, // 27-29
    30, 30, 30, // 30-32
    31, 31, 31, 31, // 33-36
    32, 32, 32, 32, // 37-40
    33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33, 33,
    33, // 41-63
];

/// `b(k)` — stereo band per hybrid channel (Tables 8.48 / 8.49).
#[must_use]
pub fn parameter_map(config: HybridConfig) -> &'static [u8] {
    match config {
        HybridConfig::Bands1020 => &B_K_20,
        HybridConfig::Bands34 => &B_K_34,
    }
}

/// The `*`-marked hybrid channels of Tables 8.48 / 8.49 — the
/// negative-frequency sub-subbands whose `h` coefficients apply
/// complex-conjugated when phase parameters are enabled.
#[must_use]
pub fn conjugate_flags(config: HybridConfig) -> &'static [usize] {
    match config {
        HybridConfig::Bands1020 => &[0, 1],
        HybridConfig::Bands34 => &[9, 10, 11],
    }
}

/// §8.6.4.6.1 — map a 10-band parameter vector to 20 bands by
/// duplication (Table 8.45: `20idx_k ← 10idx_{k/2}`).
#[must_use]
pub fn map_10_to_20(v: &[i32]) -> Vec<i32> {
    (0..20).map(|k| v[k / 2]).collect()
}

/// Table 8.45 — 20→34 source per 34-band entry: `Single(i)` copies
/// `idx_i`, `Avg(i, j)` takes `(idx_i + idx_j) / 2` (integer
/// arithmetic on indices).
#[derive(Debug, Clone, Copy)]
pub enum MapSrc {
    /// Copy one source band.
    Single(usize),
    /// Average two source bands.
    Avg(usize, usize),
    /// Average four source bands (only 34→20's `idx18`).
    Avg4(usize, usize, usize, usize),
    /// Weighted `(2·a + b)/3`.
    W21(usize, usize),
    /// Weighted `(a + 2·b)/3`.
    W12(usize, usize),
}

/// Table 8.45 — mapping from 20 to 34 parameters.
pub const MAP_20_TO_34: [MapSrc; 34] = [
    MapSrc::Single(0),
    MapSrc::Avg(0, 1),
    MapSrc::Single(1),
    MapSrc::Single(2),
    MapSrc::Avg(2, 3),
    MapSrc::Single(3),
    MapSrc::Single(4),
    MapSrc::Single(4),
    MapSrc::Single(5),
    MapSrc::Single(5),
    MapSrc::Single(6),
    MapSrc::Single(7),
    MapSrc::Single(8),
    MapSrc::Single(8),
    MapSrc::Single(9),
    MapSrc::Single(9),
    MapSrc::Single(10),
    MapSrc::Single(11),
    MapSrc::Single(12),
    MapSrc::Single(13),
    MapSrc::Single(14),
    MapSrc::Single(14),
    MapSrc::Single(15),
    MapSrc::Single(15),
    MapSrc::Single(16),
    MapSrc::Single(16),
    MapSrc::Single(17),
    MapSrc::Single(17),
    MapSrc::Single(18),
    MapSrc::Single(18),
    MapSrc::Single(18),
    MapSrc::Single(18),
    MapSrc::Single(19),
    MapSrc::Single(19),
];

/// Table 8.46 — mapping from 34 down to 20 parameters.
pub const MAP_34_TO_20: [MapSrc; 20] = [
    MapSrc::W21(0, 1),
    MapSrc::W12(1, 2),
    MapSrc::W21(3, 4),
    MapSrc::W12(4, 5),
    MapSrc::Avg(6, 7),
    MapSrc::Avg(8, 9),
    MapSrc::Single(10),
    MapSrc::Single(11),
    MapSrc::Avg(12, 13),
    MapSrc::Avg(14, 15),
    MapSrc::Single(16),
    MapSrc::Single(17),
    MapSrc::Single(18),
    MapSrc::Single(19),
    MapSrc::Avg(20, 21),
    MapSrc::Avg(22, 23),
    MapSrc::Avg(24, 25),
    MapSrc::Avg(26, 27),
    MapSrc::Avg4(28, 29, 30, 31),
    MapSrc::Avg(32, 33),
];

impl MapSrc {
    /// Apply to an integer index vector (ANSI-C truncating division).
    #[must_use]
    pub fn apply_i32(&self, v: &[i32]) -> i32 {
        match *self {
            MapSrc::Single(i) => v[i],
            MapSrc::Avg(i, j) => (v[i] + v[j]) / 2,
            MapSrc::Avg4(i, j, k, l) => (v[i] + v[j] + v[k] + v[l]) / 4,
            MapSrc::W21(i, j) => (2 * v[i] + v[j]) / 3,
            MapSrc::W12(i, j) => (v[i] + 2 * v[j]) / 3,
        }
    }

    /// Apply to a float vector (the `h`-coefficient hand-over on a
    /// stereo-band-count switch, §8.6.4.6.1).
    #[must_use]
    pub fn apply_f64(&self, v: &[f64]) -> f64 {
        match *self {
            MapSrc::Single(i) => v[i],
            MapSrc::Avg(i, j) => (v[i] + v[j]) / 2.0,
            MapSrc::Avg4(i, j, k, l) => (v[i] + v[j] + v[k] + v[l]) / 4.0,
            MapSrc::W21(i, j) => (2.0 * v[i] + v[j]) / 3.0,
            MapSrc::W12(i, j) => (v[i] + 2.0 * v[j]) / 3.0,
        }
    }
}

/// Map an index vector of `n` parameters (10, 20 or 34) to the target
/// stereo-band count (20 or 34), per §8.6.4.6.1: 10→20 duplication,
/// 20→34 via Table 8.45, 34→20 via Table 8.46, 10→34 via 20.
#[must_use]
pub fn map_indices(v: &[i32], target: usize) -> Vec<i32> {
    match (v.len(), target) {
        (n, t) if n == t => v.to_vec(),
        (10, 20) => map_10_to_20(v),
        (20, 34) => MAP_20_TO_34.iter().map(|m| m.apply_i32(v)).collect(),
        (10, 34) => {
            let v20 = map_10_to_20(v);
            MAP_20_TO_34.iter().map(|m| m.apply_i32(&v20)).collect()
        }
        (34, 20) => MAP_34_TO_20.iter().map(|m| m.apply_i32(v)).collect(),
        // Shorter vectors (IPD/OPD's nr_ipdopd_par = 5/11/17) are
        // handled by the caller; anything else passes through.
        _ => v.to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b_k_tables_are_consistent() {
        assert_eq!(B_K_20.len(), 71);
        assert_eq!(B_K_34.len(), 91);
        assert!(B_K_20.iter().all(|&b| b < 20));
        assert!(B_K_34.iter().all(|&b| b < 34));
        // Every stereo band is hit at least once.
        for b in 0..20u8 {
            assert!(B_K_20.contains(&b), "20-band {b} unused");
        }
        for b in 0..34u8 {
            assert!(B_K_34.contains(&b), "34-band {b} unused");
        }
        // The unsplit QMF region of the 20-band table: k=10..16 map
        // QMF bands 3..9 one-to-one (Table 8.48 rows 10..15 + 16-17).
        assert_eq!(&B_K_20[10..16], &[8, 9, 10, 11, 12, 13]);
        // Table 8.49 spot rows: the QMF band 1 sub-subbands reach into
        // stereo bands 4..10.
        assert_eq!(&B_K_34[12..20], &[10, 10, 4, 5, 6, 7, 8, 9]);
    }

    #[test]
    fn index_mapping_round_trips_shape() {
        let v10: Vec<i32> = (0..10).collect();
        let v20 = map_indices(&v10, 20);
        assert_eq!(
            v20,
            vec![0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9]
        );
        let v34 = map_indices(&v20, 34);
        assert_eq!(v34.len(), 34);
        // Table 8.45 first rows: idx0, (idx0+idx1)/2, idx1, idx2, ...
        assert_eq!(v34[0], 0);
        assert_eq!(v34[1], (v20[0] + v20[1]) / 2);
        assert_eq!(v34[2], v20[1]);
        let back = map_indices(&v34, 20);
        assert_eq!(back.len(), 20);
        // A constant vector survives every mapping exactly.
        let c34 = map_indices(&[5i32; 20], 34);
        assert_eq!(c34, vec![5i32; 34]);
        let c20 = map_indices(&[5i32; 34], 20);
        assert_eq!(c20, vec![5i32; 20]);
    }

    #[test]
    fn ansi_c_integer_average_truncates_toward_zero() {
        // (-3 + 2)/2 = -0 in C (truncation), not -1 (flooring).
        let v = vec![-3i32, 2];
        assert_eq!(MapSrc::Avg(0, 1).apply_i32(&v), 0);
        assert_eq!(MapSrc::W21(0, 1).apply_i32(&v), -1); // (-6+2)/3
    }
}
