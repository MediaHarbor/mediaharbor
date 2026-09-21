//! SBR time / frequency grid derivation — ISO/IEC 14496-3 §4.6.18.3.3.
//!
//! Turns a parsed [`crate::sbr_grid::SbrGrid`] into the envelope and
//! noise-floor time border vectors `tE(l)` / `tQ(l)` (in SBR time
//! slots) plus the `lA` "transient envelope" index of Table 4.176:
//!
//! * `absBordLead` / `absBordTrail` — the leading / trailing SBR frame
//!   borders per frame class (`bs_var_bord_*` offsets for the variable
//!   sides).
//! * `nRelLead` / `nRelTrail` and the relative-border vectors —
//!   `NINT(numTimeSlots / LE)` uniform spacing for FIXFIX, the
//!   reconstructed `2·bs_rel_bord + 2` values for the variable sides.
//! * `tQ` — one or two noise floors, the two-floor split at
//!   `tE(middleBorder)` with `middleBorder` from Table 4.174.
//! * `lA` — Table 4.176 (`-1` when no transient envelope is
//!   signalled), consumed by the §4.6.18.7.5 gain calculation.
//!
//! ## Provenance
//!
//! Every branch below is from the §4.6.18.3.3 text and Tables 4.174 /
//! 4.176 of the staged spec. No part of this implementation is derived
//! from any external decoder.

use crate::sbr_grid::{FrameClass, SbrGrid};
use crate::{Error, Result};

/// The derived §4.6.18.3.3 time grid for one channel's SBR frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeGrid {
    /// `tE(0..=LE)` — envelope time borders in SBR time slots. The
    /// start border of segment `l` is inclusive, the stop border
    /// exclusive.
    pub t_e: Vec<i32>,
    /// `tQ(0..=LQ)` — noise-floor time borders (a subset of `t_e`).
    pub t_q: Vec<i32>,
    /// `lA` per Table 4.176: the envelope index where a newly started
    /// sinusoid begins (and where the §4.6.18.7.5 `δ(l)` noise gate
    /// opens); `-1` when none is signalled.
    pub l_a: i32,
}

/// Derive the §4.6.18.3.3 time grid from a parsed `sbr_grid()`.
///
/// `num_time_slots` is the §4.6.18.2.6 `numTimeSlots` (16 for the
/// 1024-sample core frame this crate decodes). Border vectors that are
/// not strictly increasing, or that leave the
/// `[0, num_time_slots + 8]` range, are rejected with
/// [`Error::SbrGridInvalid`] (a malformed variable-border grid).
pub fn derive_time_grid(grid: &SbrGrid, num_time_slots: i32) -> Result<TimeGrid> {
    let le = grid.num_env;
    if le == 0 {
        return Err(Error::SbrGridInvalid);
    }

    // Leading / trailing absolute borders.
    let abs_bord_lead = match grid.frame_class {
        FrameClass::FixFix | FrameClass::FixVar => 0,
        FrameClass::VarFix | FrameClass::VarVar => i32::from(grid.var_bord_0),
    };
    let abs_bord_trail = match grid.frame_class {
        FrameClass::FixFix | FrameClass::VarFix => num_time_slots,
        FrameClass::FixVar | FrameClass::VarVar => i32::from(grid.var_bord_1) + num_time_slots,
    };

    // Relative-border counts.
    let n_rel_lead = match grid.frame_class {
        FrameClass::FixFix => le - 1,
        FrameClass::FixVar => 0,
        FrameClass::VarFix | FrameClass::VarVar => grid.rel_bord_0.len(),
    };
    let n_rel_trail = match grid.frame_class {
        FrameClass::FixFix | FrameClass::VarFix => 0,
        FrameClass::FixVar | FrameClass::VarVar => grid.rel_bord_1.len(),
    };
    if n_rel_lead + n_rel_trail + 1 != le {
        return Err(Error::SbrGridInvalid);
    }

    // relBordLead(l): FIXFIX splits the frame uniformly with
    // NINT(numTimeSlots / LE); the variable classes carry
    // 2·bs_rel_bord_0 + 2.
    let rel_lead = |l: usize| -> i32 {
        match grid.frame_class {
            FrameClass::FixFix => nint_ratio(num_time_slots, le as i32),
            _ => 2 * i32::from(grid.rel_bord_0[l]) + 2,
        }
    };
    // relBordTrail(l): 2·bs_rel_bord_1 + 2.
    let rel_trail = |l: usize| -> i32 { 2 * i32::from(grid.rel_bord_1[l]) + 2 };

    // tE(l).
    let mut t_e = Vec::with_capacity(le + 1);
    for l in 0..=le {
        let border = if l == 0 {
            abs_bord_lead
        } else if l == le {
            abs_bord_trail
        } else if l <= n_rel_lead {
            let mut b = abs_bord_lead;
            for i in 0..l {
                b += rel_lead(i);
            }
            b
        } else {
            let mut b = abs_bord_trail;
            for i in 0..(le - l) {
                b -= rel_trail(i);
            }
            b
        };
        t_e.push(border);
    }

    // §4.6.18.3.3 border sanity: strictly increasing, within the
    // addressable slot range (the XLow / XHigh buffers extend
    // tHFGen = 8 slots past the frame).
    for w in t_e.windows(2) {
        if w[1] <= w[0] {
            return Err(Error::SbrGridInvalid);
        }
    }
    if t_e[0] < 0 || t_e[le] > num_time_slots + 8 {
        return Err(Error::SbrGridInvalid);
    }

    // tQ: one floor spans the frame; two floors split at
    // tE(middleBorder) (Table 4.174).
    let t_q = if le == 1 {
        vec![t_e[0], t_e[1]]
    } else {
        let middle = middle_border(grid.frame_class, grid.pointer, le)?;
        if middle == 0 || middle >= le {
            return Err(Error::SbrGridInvalid);
        }
        vec![t_e[0], t_e[middle], t_e[le]]
    };
    if grid.num_noise != t_q.len() - 1 {
        return Err(Error::SbrGridInvalid);
    }

    // lA (Table 4.176).
    let l_a = match grid.frame_class {
        FrameClass::FixFix => -1,
        FrameClass::FixVar | FrameClass::VarVar => {
            if grid.pointer == 0 {
                -1
            } else {
                le as i32 + 1 - grid.pointer as i32
            }
        }
        FrameClass::VarFix => {
            if grid.pointer > 1 {
                grid.pointer as i32 - 1
            } else {
                -1
            }
        }
    };

    Ok(TimeGrid { t_e, t_q, l_a })
}

/// Table 4.174 — the `middleBorder` envelope index that splits the two
/// noise floors.
fn middle_border(class: FrameClass, pointer: u32, le: usize) -> Result<usize> {
    let le_i = le as i32;
    let v = match class {
        FrameClass::FixFix => le_i / 2,
        FrameClass::VarFix => match pointer {
            0 => 1,
            1 => le_i - 1,
            _ => pointer as i32 - 1,
        },
        FrameClass::FixVar | FrameClass::VarVar => match pointer {
            0 | 1 => le_i - 1,
            _ => le_i + 1 - pointer as i32,
        },
    };
    if v < 0 {
        return Err(Error::SbrGridInvalid);
    }
    Ok(v as usize)
}

/// §1.3 `NINT()` of the ratio `num / den` (round half away from zero;
/// both operands are positive here).
#[inline]
fn nint_ratio(num: i32, den: i32) -> i32 {
    (2 * num + den) / (2 * den)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixfix(num_env: usize) -> SbrGrid {
        SbrGrid {
            frame_class: FrameClass::FixFix,
            num_env,
            num_noise: if num_env > 1 { 2 } else { 1 },
            freq_res: vec![true; num_env],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![],
            pointer: 0,
            amp_res_override: num_env == 1,
        }
    }

    /// FIXFIX splits the frame uniformly: LE ∈ {1, 2, 4} over 16 slots.
    #[test]
    fn fixfix_uniform_borders() {
        assert_eq!(derive_time_grid(&fixfix(1), 16).unwrap().t_e, vec![0, 16]);
        assert_eq!(
            derive_time_grid(&fixfix(2), 16).unwrap().t_e,
            vec![0, 8, 16]
        );
        assert_eq!(
            derive_time_grid(&fixfix(4), 16).unwrap().t_e,
            vec![0, 4, 8, 12, 16]
        );
    }

    /// FIXFIX noise floors: LE = 1 has one floor over the frame; LE > 1
    /// splits at tE(LE/2); lA is always -1.
    #[test]
    fn fixfix_noise_floors_and_la() {
        let g1 = derive_time_grid(&fixfix(1), 16).unwrap();
        assert_eq!(g1.t_q, vec![0, 16]);
        assert_eq!(g1.l_a, -1);
        let g4 = derive_time_grid(&fixfix(4), 16).unwrap();
        assert_eq!(g4.t_q, vec![0, 8, 16]);
        assert_eq!(g4.l_a, -1);
    }

    /// FIXVAR counts envelopes back from the variable trailing border.
    #[test]
    fn fixvar_borders_from_trail() {
        let grid = SbrGrid {
            frame_class: FrameClass::FixVar,
            num_env: 2,
            num_noise: 2,
            freq_res: vec![true; 2],
            var_bord_0: 0,
            var_bord_1: 3,
            rel_bord_0: vec![],
            rel_bord_1: vec![1], // reconstructed 2·1 + 2 = 4
            pointer: 0,
            amp_res_override: false,
        };
        let g = derive_time_grid(&grid, 16).unwrap();
        // absBordTrail = 3 + 16 = 19; tE(1) = 19 - 4 = 15.
        assert_eq!(g.t_e, vec![0, 15, 19]);
        // middleBorder (pointer = 0) = LE - 1 = 1.
        assert_eq!(g.t_q, vec![0, 15, 19]);
        assert_eq!(g.l_a, -1);
        // pointer = 1 → lA = LE + 1 - 1 = 2.
        let g = derive_time_grid(&SbrGrid { pointer: 1, ..grid }, 16).unwrap();
        assert_eq!(g.l_a, 2);
    }

    /// VARFIX counts envelopes forward from the variable leading
    /// border; lA fires only for pointer > 1.
    #[test]
    fn varfix_borders_from_lead() {
        let grid = SbrGrid {
            frame_class: FrameClass::VarFix,
            num_env: 2,
            num_noise: 2,
            freq_res: vec![false; 2],
            var_bord_0: 2,
            var_bord_1: 0,
            rel_bord_0: vec![0], // reconstructed 2
            rel_bord_1: vec![],
            pointer: 2,
            amp_res_override: false,
        };
        let g = derive_time_grid(&grid, 16).unwrap();
        assert_eq!(g.t_e, vec![2, 4, 16]);
        // middleBorder (pointer = 2) = pointer - 1 = 1.
        assert_eq!(g.t_q, vec![2, 4, 16]);
        // lA = pointer - 1 = 1.
        assert_eq!(g.l_a, 1);
        let g = derive_time_grid(&SbrGrid { pointer: 1, ..grid }, 16).unwrap();
        assert_eq!(g.l_a, -1);
    }

    /// VARVAR mixes both variable sides.
    #[test]
    fn varvar_mixed_borders() {
        let grid = SbrGrid {
            frame_class: FrameClass::VarVar,
            num_env: 3,
            num_noise: 2,
            freq_res: vec![true; 3],
            var_bord_0: 1,
            var_bord_1: 2,
            rel_bord_0: vec![2], // 6
            rel_bord_1: vec![3], // 8
            pointer: 0,
            amp_res_override: false,
        };
        let g = derive_time_grid(&grid, 16).unwrap();
        // lead: 1, 1+6 = 7; trail: 18, 18-8 = 10.
        assert_eq!(g.t_e, vec![1, 7, 10, 18]);
        // middleBorder (pointer = 0) = LE - 1 = 2 → tQ splits at 10.
        assert_eq!(g.t_q, vec![1, 10, 18]);
    }

    /// Non-monotonic borders are rejected.
    #[test]
    fn non_monotonic_borders_rejected() {
        let grid = SbrGrid {
            frame_class: FrameClass::FixVar,
            num_env: 2,
            num_noise: 2,
            freq_res: vec![true; 2],
            var_bord_0: 0,
            var_bord_1: 0,
            rel_bord_0: vec![],
            rel_bord_1: vec![3], // tE(1) = 16 - 8 = 8 … fine
            pointer: 0,
            amp_res_override: false,
        };
        assert!(derive_time_grid(&grid, 16).is_ok());
        let bad = SbrGrid {
            var_bord_1: 0,
            rel_bord_1: vec![3, 3, 3],
            num_env: 4,
            freq_res: vec![true; 4],
            ..grid
        };
        // tE = [0, 16-24, …] — not increasing.
        assert!(matches!(
            derive_time_grid(&bad, 16),
            Err(Error::SbrGridInvalid)
        ));
    }
}
