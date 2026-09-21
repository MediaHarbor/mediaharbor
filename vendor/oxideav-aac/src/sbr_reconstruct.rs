//! SBR envelope / noise-floor DPCM reconstruction — ISO/IEC 14496-3
//! §4.6.18.3.5.
//!
//! [`crate::sbr_envelope`] yields the **raw** transmitted values
//! `bs_data_env` / `bs_data_noise`, which are delta-coded (the spec's
//! `E_Delta(k,l)`). This module inverts the §4.6.18.3.5 delta coding to
//! recover the quantized scalefactors `E_Q(k,l)` (and the noise-floor
//! `Q(k,l)`).
//!
//! The spec defines `E_Delta` in terms of `E_Q`; inverting:
//!
//! * **frequency direction** (`bs_df_env(l) == 0`):
//!   - `E_Q(0,l)   = bs_data_env(0,l) / δ`
//!   - `E_Q(k,l)   = E_Q(k-1,l) + bs_data_env(k,l) / δ`, `k ≥ 1`
//! * **time direction** (`bs_df_env(l) == 1`):
//!   - `E_Q(k,l)   = g_E(k,l) + bs_data_env(k,l) / δ`
//!
//! where `δ = 0.5` for the second channel of a coupled pair (so the
//! transmitted balance values carry a factor of 2 — i.e. they must be
//! even, per §4.6.18.3.6) and `δ = 1` otherwise. In the integer
//! quantized domain the divide-by-δ is a multiply-by-`1/δ` (× 2 for the
//! coupled second channel); the transmitted values are even there, so
//! the result stays integral.
//!
//! `g_E(k,l)` is the "previous envelope, same band" reference for a
//! time delta:
//!
//! * for `l ≥ 1` it is `E_Q(k, l-1)` of the *current* frame,
//! * for `l == 0` it is `E'_Q(k, L'_E − 1)` — the last envelope of the
//!   *previous* frame.
//!
//! When the frequency resolution of the reference envelope differs from
//! the current envelope (`r(l) ≠ g(l)`), the band index must be
//! re-mapped between the high- and low-resolution band tables via the
//! `i(k)` relation:
//!
//! * `r(l) = 1, g(l) = 0` (current high, ref low): for current
//!   high-band `k`, the reference low-band `i` satisfies
//!   `fTableLow(i) ≤ fTableHigh(k) < fTableLow(i+1)`.
//! * `r(l) = 0, g(l) = 1` (current low, ref high): for current
//!   low-band `k`, the reference high-band `i` satisfies
//!   `fTableHigh(i) = fTableLow(k)`.
//!
//! Noise floors follow the identical scheme over `NQ` bands, except a
//! noise floor is always at the (single) noise-band resolution, so no
//! resolution remap is ever needed.

use crate::sbr_envelope::{SbrEnvelopeData, SbrNoiseData};
use crate::sbr_freq_bands::HiLoTables;
use crate::sbr_grid::{SbrDtdf, SbrGrid};
use crate::{Error, Result};

/// Reconstructed quantized envelope scalefactors `E_Q(k,l)` for one
/// channel: one band vector per envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeScalefactors {
    /// `E_Q[l][k]` — the quantized envelope scalefactor for envelope
    /// `l`, band `k`.
    pub eq: Vec<Vec<i32>>,
    /// Per-envelope frequency-resolution flag `r(l)` (copied from the
    /// grid) — needed by the next frame for a cross-frame time delta.
    pub freq_res: Vec<bool>,
}

/// Reconstructed quantized noise-floor scalefactors `Q(k,l)` for one
/// channel: one `NQ`-band vector per noise floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoiseScalefactors {
    /// `Q[l][k]` — the quantized noise-floor scalefactor for noise
    /// floor `l`, band `k`.
    pub q: Vec<Vec<i32>>,
}

/// The `1/δ` integer multiplier: `2` for the coupled second channel,
/// `1` otherwise.
#[inline]
fn inv_delta(coupling: bool, ch: bool) -> i32 {
    if coupling && ch {
        2
    } else {
        1
    }
}

/// `i(k)` for `r(l) = 1, g(l) = 0` (current high-res band `k` → ref
/// low-res band): the largest `i` with `fTableLow(i) ≤ fTableHigh(k)`.
fn high_to_low(bands: &HiLoTables, k: usize) -> usize {
    let target = bands.f_table_high[k];
    let mut i = 0usize;
    while i + 1 < bands.f_table_low.len() && bands.f_table_low[i + 1] <= target {
        i += 1;
    }
    i
}

/// `i(k)` for `r(l) = 0, g(l) = 1` (current low-res band `k` → ref
/// high-res band): the `i` with `fTableHigh(i) = fTableLow(k)`.
fn low_to_high(bands: &HiLoTables, k: usize) -> usize {
    let target = bands.f_table_low[k];
    bands
        .f_table_high
        .iter()
        .position(|&v| v == target)
        .unwrap_or(0)
}

/// Map a reference-envelope band array `prev` (at resolution
/// `prev_high`) onto the current envelope's band `k` (at resolution
/// `cur_high`), per the §4.6.18.3.5 `i(k)` relation.
fn ref_band(bands: &HiLoTables, prev: &[i32], cur_high: bool, prev_high: bool, k: usize) -> i32 {
    let idx = if cur_high == prev_high {
        k
    } else if cur_high {
        // r=1, g=0
        high_to_low(bands, k)
    } else {
        // r=0, g=1
        low_to_high(bands, k)
    };
    prev.get(idx).copied().unwrap_or(0)
}

impl EnvelopeScalefactors {
    /// Reconstruct `E_Q(k,l)` from the raw `bs_data_env`.
    ///
    /// `prev` is the previous frame's reconstructed envelopes (its last
    /// envelope is `g_E` for an `l == 0` time delta); pass `None` for
    /// the first frame after a reset (in which case a time-coded first
    /// envelope is treated as if the reference were all-zero, which the
    /// §4.6.18.3.5 reset rule forbids on the wire anyway).
    pub fn reconstruct(
        env: &SbrEnvelopeData,
        grid: &SbrGrid,
        dtdf: &SbrDtdf,
        bands: &HiLoTables,
        coupling: bool,
        ch: bool,
        prev: Option<&EnvelopeScalefactors>,
    ) -> Result<Self> {
        let inv = inv_delta(coupling, ch);
        let mut eq: Vec<Vec<i32>> = Vec::with_capacity(grid.num_env);

        for l in 0..grid.num_env {
            let cur_high = grid.freq_res[l];
            let n = if cur_high {
                bands.n_high()
            } else {
                bands.n_low()
            };
            let raw = &env.data[l];
            if raw.len() != n {
                return Err(Error::SbrGridInvalid);
            }
            let mut row = vec![0i32; n];

            if !dtdf.df_env[l] {
                // Frequency direction.
                row[0] = raw[0] * inv;
                for k in 1..n {
                    row[k] = row[k - 1] + raw[k] * inv;
                }
            } else {
                // Time direction: reference is the previous envelope of
                // this frame (l-1), or the last envelope of the previous
                // frame for l == 0.
                let (prev_row, prev_high): (Vec<i32>, bool) = if l >= 1 {
                    (eq[l - 1].clone(), grid.freq_res[l - 1])
                } else if let Some(p) = prev {
                    let last = p.eq.len().saturating_sub(1);
                    (
                        p.eq.get(last).cloned().unwrap_or_default(),
                        *p.freq_res.get(last).unwrap_or(&cur_high),
                    )
                } else {
                    (vec![0i32; n], cur_high)
                };
                for k in 0..n {
                    let g = ref_band(bands, &prev_row, cur_high, prev_high, k);
                    row[k] = g + raw[k] * inv;
                }
            }
            eq.push(row);
        }

        Ok(EnvelopeScalefactors {
            eq,
            freq_res: grid.freq_res.clone(),
        })
    }
}

impl NoiseScalefactors {
    /// Reconstruct `Q(k,l)` from the raw `bs_data_noise` over `NQ`
    /// bands. Noise floors share one resolution, so there is no
    /// `i(k)` remap.
    pub fn reconstruct(
        noise: &SbrNoiseData,
        grid: &SbrGrid,
        dtdf: &SbrDtdf,
        num_noise_bands: usize,
        coupling: bool,
        ch: bool,
        prev: Option<&NoiseScalefactors>,
    ) -> Result<Self> {
        let inv = inv_delta(coupling, ch);
        let mut q: Vec<Vec<i32>> = Vec::with_capacity(grid.num_noise);

        for l in 0..grid.num_noise {
            let raw = &noise.data[l];
            if raw.len() != num_noise_bands {
                return Err(Error::SbrGridInvalid);
            }
            let mut row = vec![0i32; num_noise_bands];
            if !dtdf.df_noise[l] {
                row[0] = raw[0] * inv;
                for k in 1..num_noise_bands {
                    row[k] = row[k - 1] + raw[k] * inv;
                }
            } else {
                let prev_row: Vec<i32> = if l >= 1 {
                    q[l - 1].clone()
                } else if let Some(p) = prev {
                    p.q.last()
                        .cloned()
                        .unwrap_or_else(|| vec![0i32; num_noise_bands])
                } else {
                    vec![0i32; num_noise_bands]
                };
                for k in 0..num_noise_bands {
                    let g = prev_row.get(k).copied().unwrap_or(0);
                    row[k] = g + raw[k] * inv;
                }
            }
            q.push(row);
        }

        Ok(NoiseScalefactors { q })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sbr_freq_bands::{k0, k2, master_table, HiLoTables};
    use crate::sbr_grid::FrameClass;

    fn bands_44100() -> HiLoTables {
        let k0v = k0(88_200, 5).unwrap();
        let k2v = k2(88_200, 5, k0v).unwrap();
        let fm = master_table(k0v, k2v, 0, false).unwrap();
        HiLoTables::derive(&fm, 1, 2).unwrap()
    }

    fn single_env_grid(high: bool) -> (SbrGrid, SbrDtdf) {
        (
            SbrGrid {
                frame_class: FrameClass::FixFix,
                num_env: 1,
                num_noise: 1,
                freq_res: vec![high],
                var_bord_0: 0,
                var_bord_1: 0,
                rel_bord_0: vec![],
                rel_bord_1: vec![],
                pointer: 0,
                amp_res_override: false,
            },
            SbrDtdf {
                df_env: vec![false],
                df_noise: vec![false],
            },
        )
    }

    #[test]
    fn freq_direction_accumulates() {
        let bands = bands_44100();
        let (grid, dtdf) = single_env_grid(true);
        let n = bands.n_high();
        // raw = [10, 1, 2, -1, ...] → cumulative sums.
        let mut raw = vec![10i32];
        for k in 1..n {
            raw.push(if k % 2 == 0 { 2 } else { -1 });
        }
        let env = SbrEnvelopeData {
            data: vec![raw.clone()],
        };
        let rec = EnvelopeScalefactors::reconstruct(&env, &grid, &dtdf, &bands, false, false, None)
            .unwrap();
        // Expected: cumulative sum.
        let mut acc = 10;
        assert_eq!(rec.eq[0][0], 10);
        for (k, &delta) in raw.iter().enumerate().skip(1) {
            acc += delta;
            assert_eq!(rec.eq[0][k], acc);
        }
    }

    #[test]
    fn coupled_second_channel_doubles_delta() {
        let bands = bands_44100();
        let (grid, dtdf) = single_env_grid(true);
        let n = bands.n_high();
        let mut raw = vec![4i32];
        raw.extend(std::iter::repeat_n(2, n - 1));
        let env = SbrEnvelopeData { data: vec![raw] };
        // coupling && ch → inv_delta = 2.
        let rec = EnvelopeScalefactors::reconstruct(&env, &grid, &dtdf, &bands, true, true, None)
            .unwrap();
        assert_eq!(rec.eq[0][0], 8); // 4 * 2
        assert_eq!(rec.eq[0][1], 12); // 8 + 2*2
    }

    #[test]
    fn time_direction_uses_prev_envelope_in_frame() {
        let bands = bands_44100();
        let n = bands.n_high();
        // Two high-res envelopes: env0 freq-coded, env1 time-coded.
        let grid = SbrGrid {
            frame_class: FrameClass::FixVar,
            num_env: 2,
            num_noise: 2,
            freq_res: vec![true, true],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: false,
        };
        let dtdf = SbrDtdf {
            df_env: vec![false, true], // env1 is time-coded
            df_noise: vec![false, false],
        };
        let mut raw0 = vec![20i32]; // env0 start = 20, flat thereafter
        raw0.extend(std::iter::repeat_n(0, n - 1));
        let raw1 = vec![1i32; n]; // env1 = env0 + 1 per band
        let env = SbrEnvelopeData {
            data: vec![raw0, raw1],
        };
        let rec = EnvelopeScalefactors::reconstruct(&env, &grid, &dtdf, &bands, false, false, None)
            .unwrap();
        for k in 0..n {
            assert_eq!(rec.eq[0][k], 20);
            assert_eq!(rec.eq[1][k], 21); // 20 + 1
        }
    }

    #[test]
    fn time_direction_cross_frame() {
        let bands = bands_44100();
        let n = bands.n_high();
        let (grid, _) = single_env_grid(true);
        // Previous frame: a single high-res envelope all = 30.
        let prev = EnvelopeScalefactors {
            eq: vec![vec![30i32; n]],
            freq_res: vec![true],
        };
        // Current frame: single time-coded envelope, deltas all +2.
        let dtdf = SbrDtdf {
            df_env: vec![true],
            df_noise: vec![false],
        };
        let env = SbrEnvelopeData {
            data: vec![vec![2i32; n]],
        };
        let rec = EnvelopeScalefactors::reconstruct(
            &env,
            &grid,
            &dtdf,
            &bands,
            false,
            false,
            Some(&prev),
        )
        .unwrap();
        for k in 0..n {
            assert_eq!(rec.eq[0][k], 32); // 30 + 2
        }
    }

    #[test]
    fn resolution_remap_high_to_low_is_monotone() {
        // high_to_low must be non-decreasing and in-range for every
        // high-res band.
        let bands = bands_44100();
        let mut prev = 0usize;
        for k in 0..=bands.n_high() {
            let i = high_to_low(&bands, k);
            assert!(i < bands.f_table_low.len());
            assert!(i >= prev);
            prev = i;
        }
    }

    #[test]
    fn noise_reconstruct_accumulates() {
        let bands = bands_44100();
        let nq = bands.n_q();
        let (grid, dtdf) = single_env_grid(true);
        let raw = (0..nq)
            .map(|k| if k == 0 { 5 } else { 1 })
            .collect::<Vec<_>>();
        let noise = SbrNoiseData { data: vec![raw] };
        let rec =
            NoiseScalefactors::reconstruct(&noise, &grid, &dtdf, nq, false, false, None).unwrap();
        let mut acc = 5;
        assert_eq!(rec.q[0][0], 5);
        for k in 1..nq {
            acc += 1;
            assert_eq!(rec.q[0][k], acc);
        }
    }
}
