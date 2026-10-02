//! SBR limiter frequency band table — ISO/IEC 14496-3 §4.6.18.3.2.3 /
//! Figure 4.41.
//!
//! `fTableLim` partitions the SBR range into the bands over which the
//! §4.6.18.7.5 gain limiter averages: either exactly one band
//! (`bs_limiter_bands == 0`) or approximately 1.2 / 2 / 3 bands per
//! octave. The table is a subset of the union of `fTableLow` and the
//! §4.6.18.6 patch borders; the Figure 4.41 walk merges neighbours
//! closer than `0.49 / limBands` octaves, always preferring to keep a
//! patch border over an envelope border (both being patch borders
//! keeps both).
//!
//! ## Provenance
//!
//! The construction is the Figure 4.41 flowchart of the staged spec,
//! with the `limiterBandsPerOctave = {1.2, 2, 3}` selector. No part of
//! this implementation is derived from any external decoder.

use crate::sbr_freq_bands::HiLoTables;
use crate::{Error, Result};

/// §4.6.18.3.2.3 / Figure 4.41 — build `fTableLim`.
///
/// * `bands` — the derived frequency tables (`fTableLow`, `k_x`, `m`).
/// * `patch_borders` — the §4.6.18.6 patch borders
///   ([`crate::sbr_hf_gen::Patches::borders`], starting at `k_x`).
/// * `bs_limiter_bands` — the 2-bit header field (`0..=3`).
///
/// Returns the border vector `fTableLim(0..=NL)`.
pub fn limiter_table(
    bands: &HiLoTables,
    patch_borders: &[i32],
    bs_limiter_bands: u8,
) -> Result<Vec<i32>> {
    let f_low = &bands.f_table_low;
    if f_low.len() < 2 || bs_limiter_bands > 3 {
        return Err(Error::SbrFreqBandInvalid);
    }

    // bs_limiter_bands == 0: one band over the whole SBR range.
    if bs_limiter_bands == 0 {
        return Ok(vec![f_low[0], f_low[f_low.len() - 1]]);
    }

    // limiterBandsPerOctave = {1.2, 2, 3}.
    let lim_bands = [1.2f64, 2.0, 3.0][usize::from(bs_limiter_bands - 1)];

    // limTable = fTableLow ∪ interior patch borders, sorted.
    let num_patches = patch_borders.len().saturating_sub(1);
    let mut lim_table: Vec<i32> = f_low.clone();
    if num_patches > 1 {
        lim_table.extend_from_slice(&patch_borders[1..num_patches]);
    }
    lim_table.sort_unstable();

    // nrLim = NLow + numPatches - 1 (the last index of limTable).
    let mut k = 1usize;
    while k < lim_table.len() {
        if lim_table[k] < 1 || lim_table[k - 1] < 1 {
            return Err(Error::SbrFreqBandInvalid);
        }
        let n_octaves = (f64::from(lim_table[k]) / f64::from(lim_table[k - 1])).log2();
        if n_octaves * lim_bands < 0.49 {
            if lim_table[k] == lim_table[k - 1] {
                // Duplicate border: drop one copy.
                lim_table.remove(k);
            } else if !patch_borders.contains(&lim_table[k]) {
                // The upper border is droppable (an envelope border).
                lim_table.remove(k);
            } else if !patch_borders.contains(&lim_table[k - 1]) {
                // The upper border is a patch border; drop the lower
                // envelope border instead.
                lim_table.remove(k - 1);
            } else {
                // Both are patch borders: keep both.
                k += 1;
            }
        } else {
            k += 1;
        }
    }

    Ok(lim_table)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bands(f_low: Vec<i32>) -> HiLoTables {
        let k_x = f_low[0];
        let m = f_low[f_low.len() - 1] - k_x;
        HiLoTables {
            f_table_high: f_low.clone(),
            f_table_low: f_low,
            f_table_noise: vec![k_x, k_x + m],
            m,
            k_x,
        }
    }

    /// bs_limiter_bands == 0 → exactly one band over the SBR range.
    #[test]
    fn zero_limiter_bands_is_one_band() {
        let b = bands(vec![8, 12, 16, 20, 24]);
        let t = limiter_table(&b, &[8, 16, 24], 0).unwrap();
        assert_eq!(t, vec![8, 24]);
    }

    /// A single patch adds no interior borders: wide envelope bands
    /// pass through untouched.
    #[test]
    fn single_patch_keeps_envelope_borders() {
        let b = bands(vec![8, 12, 16, 20, 24]);
        let t = limiter_table(&b, &[8, 24], 3).unwrap();
        assert_eq!(t, vec![8, 12, 16, 20, 24]);
    }

    /// A patch border duplicating an envelope border collapses to one
    /// entry.
    #[test]
    fn duplicate_border_removed() {
        let b = bands(vec![8, 12, 16, 20, 24]);
        // Interior patch border at 16 duplicates fLow's 16.
        let t = limiter_table(&b, &[8, 16, 24], 3).unwrap();
        assert_eq!(t, vec![8, 12, 16, 20, 24]);
    }

    /// A close pair drops the envelope border and keeps the patch
    /// border.
    #[test]
    fn close_pair_keeps_patch_border() {
        // fLow has 15 next to the interior patch border 16:
        // log2(16/15)·3 ≈ 0.28 < 0.49 → merge, dropping 15.
        let b = bands(vec![8, 12, 15, 20, 24]);
        let t = limiter_table(&b, &[8, 16, 24], 3).unwrap();
        assert!(t.contains(&16) && !t.contains(&15), "{t:?}");
        // Borders stay sorted, spanning the SBR range.
        assert_eq!(t.first(), Some(&8));
        assert_eq!(t.last(), Some(&24));
        assert!(t.windows(2).all(|w| w[0] < w[1]));
    }

    /// A close envelope pair (no patch border involved) drops the
    /// upper border.
    #[test]
    fn close_envelope_pair_drops_upper() {
        // 20 and 21 are ~0.07 octaves apart → merged; neither is a
        // patch border so the upper (21) goes.
        let b = bands(vec![8, 14, 20, 21, 28]);
        let t = limiter_table(&b, &[8, 28], 2).unwrap();
        assert_eq!(t, vec![8, 14, 20, 28]);
    }

    /// The coarsest per-octave setting (1.2) merges more bands than
    /// the finest (3).
    #[test]
    fn coarser_setting_merges_more() {
        let b = bands(vec![8, 9, 10, 12, 14, 17, 20, 24]);
        let pb = [8, 24];
        let t1 = limiter_table(&b, &pb, 1).unwrap();
        let t3 = limiter_table(&b, &pb, 3).unwrap();
        assert!(t1.len() <= t3.len(), "{t1:?} vs {t3:?}");
        for t in [&t1, &t3] {
            assert_eq!(t.first(), Some(&8));
            assert_eq!(t.last(), Some(&24));
            assert!(t.windows(2).all(|w| w[0] < w[1]));
        }
    }
}
