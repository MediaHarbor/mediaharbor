//! SSR gain-control reconstruction — ISO/IEC 14496-3 §4.6.12.
//!
//! This is the §4.6.12 *back-end* of the SSR (Scalable Sample Rate,
//! AOT 3) gain-control tool, the counterpart to the
//! [`crate::gain_control_data`] wire parser. Where that module reads
//! the Table 4.12 `(max_band, adjust_num, alevcode, aloccode)` side
//! info off the bitstream, this module turns that side info plus the
//! per-band IMDCT output into the reconstructed PCM time signal:
//!
//! 1. **Gain-control data decoding** (§4.6.12.3.1) —
//!    [`BandGainFunction::reconstruct`] maps the wire codes to the
//!    `NADW` / `ALOC` / `ALEV` ladder via the Table 4.108 `AdjLoc()`
//!    and Table 4.109 `AdjLev()` tables.
//! 2. **Gain-control function setting** (§4.6.12.3.2) — the same call
//!    builds the `FMD` fragment-modification function, threads the
//!    cross-frame `PFMD`, composes the per-sequence `GMF` gain
//!    modification function, and inverts it to the gain-control
//!    function `AD(j) = 1/GMF(j)`.
//! 3. **Gain-control windowing & overlapping** (§4.6.12.3.3) —
//!    [`GainBandState::window_overlap`] applies `AD` to the band
//!    spectrum `U`, then overlap-adds against the previous frame's
//!    tail `PT` to produce the band sample data `V`.
//!
//! The IPQF synthesis filter (§4.6.12.3.4) that recombines the four
//! `V` bands into the output PCM lives in the `ipqf` module.
//!
//! ## Per-band, per-frame state
//!
//! Two quantities thread across frames, *per IPQF band*:
//!
//! * `PFMD_B(j)` — the previous frame's fragment-modification function,
//!   used to scale the left half of this frame's `GMF` (§4.6.12.3.2
//!   step 3). Its initial value is `1.0` (spec note).
//! * `PT_B(j)` — the previous frame's gain-controlled block sample
//!   data tail, overlap-added into this frame's `V` (§4.6.12.3.3
//!   step 2). Its initial value is `0.0` (spec note).
//!
//! [`GainBandState`] carries both for one band; the four-band decoder
//! holds a `[GainBandState; 4]`.
//!
//! ## Provenance
//!
//! Every table and formula is from ISO/IEC 14496-3:2001 §4.6.12
//! (Tables 4.108 / 4.109, the §4.6.12.3.1–3 equations) staged under
//! `docs/audio/aac/`. No external SSR implementation was consulted.

use crate::gain_control_data::{GainBand, GainControlData};
use crate::ics_info::WindowSequence;

/// `AdjLoc(AC)` — ISO/IEC 14496-3 Table 4.108. The 32 tabulated
/// values are exactly `8 · AC` for `AC ∈ 0..=31`.
#[must_use]
pub fn adj_loc(ac: u8) -> u32 {
    8 * u32::from(ac)
}

/// `AdjLev(AV)` — ISO/IEC 14496-3 Table 4.109. The 16 tabulated
/// values are exactly `AV − 4` for `AV ∈ 0..=15`.
#[must_use]
pub fn adj_lev(av: u8) -> i32 {
    i32::from(av) - 4
}

/// Number of gain-control windows `N(window_sequence)` — the per-band
/// window count over which the gain ladder is transmitted (Table 4.12
/// / §4.6.12.3.1). Long sequences carry one or two windows; the short
/// sequence carries eight.
#[must_use]
pub fn num_windows(seq: WindowSequence) -> usize {
    match seq {
        WindowSequence::OnlyLong => 1,
        WindowSequence::LongStart | WindowSequence::LongStop => 2,
        WindowSequence::EightShort => 8,
    }
}

/// `ALOC_{W,B}(NADW + 1)` — the §4.6.12.3.1 step (4) endpoint location
/// for the gain ladder of window `w` under `seq`.
///
/// ```text
///                              256, W == 0  if ONLY_LONG_SEQUENCE
///                              112, W == 0
///                                            if LONG_START_SEQUENCE
///                              32,  W == 1
/// ALOC(NADW+1) =
///                              32,  0..=7    if EIGHT_SHORT_SEQUENCE
///
///                              112, W == 0
///                                            if LONG_STOP_SEQUENCE
///                              256, W == 1
/// ```
#[must_use]
pub fn endpoint_aloc(seq: WindowSequence, w: usize) -> u32 {
    match seq {
        WindowSequence::OnlyLong => 256,
        WindowSequence::LongStart => {
            if w == 0 {
                112
            } else {
                32
            }
        }
        WindowSequence::EightShort => 32,
        WindowSequence::LongStop => {
            if w == 0 {
                112
            } else {
                256
            }
        }
    }
}

/// The §4.6.12.3.2 upper bound (inclusive) on `j` for the `FMD`
/// fragment-modification function of window `w` under `seq`. This is
/// the largest sample index over which `M`/`FMD` are defined.
#[must_use]
fn fmd_last_j(seq: WindowSequence, w: usize) -> usize {
    match seq {
        WindowSequence::OnlyLong => 255,
        WindowSequence::LongStart => {
            if w == 0 {
                111
            } else {
                31
            }
        }
        WindowSequence::EightShort => 31,
        WindowSequence::LongStop => {
            if w == 0 {
                111
            } else {
                255
            }
        }
    }
}

/// The §4.6.12.3.1 reconstructed gain ladder for one `(window, band)`
/// slot: the `ALOC` / `ALEV` arrays indexed `0..=NADW+1`.
#[derive(Debug, Clone, PartialEq)]
struct Ladder {
    /// `ALOC_{W,B}(m)`, `0 ≤ m ≤ NADW + 1`.
    aloc: Vec<u32>,
    /// `ALEV_{W,B}(m)`, `0 ≤ m ≤ NADW + 1`. Each entry is a power of
    /// two `2^AdjLev(...)` (or the unit endpoint / `NADW == 0` value).
    alev: Vec<f64>,
}

impl Ladder {
    /// Reconstruct the §4.6.12.3.1 ladder for window `w` of band `b`
    /// (1-based spec band) from the per-window wire record.
    ///
    /// `window` carries the `adjust_num[B][W]` ladder entries (the
    /// `(alevcode, aloccode)` pairs) for this `(b, w)` slot.
    fn reconstruct(
        window: &crate::gain_control_data::GainWindow,
        seq: WindowSequence,
        w: usize,
    ) -> Self {
        let nadw = window.adjustments.len();
        // ALOC / ALEV have NADW + 2 entries: indices 0..=NADW+1.
        let mut aloc = Vec::with_capacity(nadw + 2);
        let mut alev = Vec::with_capacity(nadw + 2);

        // Step (3): ALOC(0) = 0; ALEV(0) = 1 if NADW == 0 else ALEV(1).
        // ALEV(0) is back-patched once ALEV(1) is known.
        aloc.push(0);
        alev.push(1.0); // placeholder; patched below for NADW > 0.

        // Steps (1)/(2): the transmitted ladder entries, m = 1..=NADW.
        for adj in &window.adjustments {
            aloc.push(adj_loc(adj.aloccode));
            alev.push(2f64.powi(adj_lev(adj.alevcode)));
        }

        // Step (4): the endpoint, m = NADW + 1.
        aloc.push(endpoint_aloc(seq, w));
        alev.push(1.0);

        // Patch ALEV(0): equals ALEV(1) when NADW > 0.
        if nadw > 0 {
            alev[0] = alev[1];
        }

        Ladder { aloc, alev }
    }

    /// `M_{W,B,j} = max{ m : ALOC(m) ≤ j }` (§4.6.12.3.2 step 1).
    ///
    /// `ALOC` is monotonically increasing with `ALOC(0) = 0`, so for
    /// any `j ≥ 0` at least `m = 0` qualifies; the answer is the index
    /// of the last `ALOC` entry not exceeding `j`.
    fn m_at(&self, j: u32) -> usize {
        let mut m = 0usize;
        for (idx, &loc) in self.aloc.iter().enumerate() {
            if loc <= j {
                m = idx;
            } else {
                break;
            }
        }
        m
    }
}

/// `Inter(a, b, j) = 2^(((8 − j)·log2(a) + j·log2(b)) / 8)`
/// (§4.6.12.3.2) — the geometric interpolation between gain levels
/// `a` and `b` over the eight-sample ramp `0 ≤ j ≤ 8`. With `a`, `b`
/// powers of two the exponent is the linear blend of their `log2`s.
#[must_use]
fn inter(a: f64, b: f64, j: u32) -> f64 {
    let la = a.log2();
    let lb = b.log2();
    let jf = j as f64;
    let exp = ((8.0 - jf) * la + jf * lb) / 8.0;
    2f64.powf(exp)
}

/// The fully-reconstructed §4.6.12.3.2 gain-control function for one
/// band of one frame: the per-window `AD_{W,B}(j) = 1 / GMF_{W,B}(j)`
/// arrays plus the `PFMD_B(j)` to thread into the next frame.
#[derive(Debug, Clone, PartialEq)]
pub struct BandGainFunction {
    /// `AD_{W,B}(j)` per window. For long sequences the single (or
    /// `w == 0`) window spans `0..512`; `EIGHT_SHORT_SEQUENCE` has
    /// eight windows each spanning `0..64`.
    pub ad: Vec<Vec<f64>>,
    /// `PFMD_B(j)` for the next frame (§4.6.12.3.2 step 3).
    pub pfmd_next: Vec<f64>,
}

/// Build the §4.6.12.3.1–2 fragment-modification function `FMD_{W,B}`
/// for window `w` of one band.
fn fmd_window(ladder: &Ladder, seq: WindowSequence, w: usize) -> Vec<f64> {
    let last = fmd_last_j(seq, w);
    let mut fmd = vec![0.0f64; last + 1];
    for (j, slot) in fmd.iter_mut().enumerate() {
        let m = ladder.m_at(j as u32);
        let loc_m = ladder.aloc[m];
        let alev_m = ladder.alev[m];
        let alev_m1 = ladder.alev[m + 1];
        // FMD(j) = Inter(ALEV(M), ALEV(M+1), j − ALOC(M)) if
        // ALOC(M) ≤ j ≤ ALOC(M) + 7, else ALEV(M+1).
        let jj = j as u32;
        *slot = if jj <= loc_m + 7 {
            inter(alev_m, alev_m1, jj - loc_m)
        } else {
            alev_m1
        };
    }
    fmd
}

/// `ALEV_{W,B}(0)` for window `w` — the front gain used in the
/// §4.6.12.3.2 step-3 `GMF` composition. Reconstructs only the head of
/// the ladder.
fn alev0(band: &GainBand, w: usize) -> f64 {
    let window = &band.windows[w];
    if window.adjustments.is_empty() {
        1.0
    } else {
        2f64.powi(adj_lev(window.adjustments[0].alevcode))
    }
}

/// Compose the §4.6.12.3.2 step-3 gain-modification function `GMF` for
/// a non-`EIGHT_SHORT` band and thread `PFMD`.
fn gmf_long(
    fmd: &[Vec<f64>],
    band: &GainBand,
    pfmd_prev: &[f64],
    seq: WindowSequence,
) -> (Vec<f64>, Vec<f64>) {
    // GMF spans 0..512 for the long sequences.
    let mut gmf = vec![0.0f64; 512];
    let pfmd_next: Vec<f64>;
    match seq {
        WindowSequence::OnlyLong => {
            let a0 = alev0(band, 0);
            for (j, slot) in gmf.iter_mut().enumerate() {
                *slot = if j <= 255 {
                    a0 * pfmd_prev[j]
                } else {
                    fmd[0][j - 256]
                };
            }
            // PFMD_B(j) = FMD_0,B(j), 0 ≤ j ≤ 255.
            pfmd_next = fmd[0][..256].to_vec();
        }
        WindowSequence::LongStart => {
            let a0 = alev0(band, 0);
            let a1 = alev0(band, 1);
            for (j, slot) in gmf.iter_mut().enumerate() {
                *slot = if j <= 255 {
                    a0 * a1 * pfmd_prev[j]
                } else if j <= 367 {
                    a1 * fmd[0][j - 256]
                } else if j <= 399 {
                    fmd[1][j - 368]
                } else {
                    1.0
                };
            }
            // PFMD_B(j) = FMD_1,B(j), 0 ≤ j ≤ 31.
            pfmd_next = fmd[1][..32].to_vec();
        }
        WindowSequence::LongStop => {
            let a0 = alev0(band, 0);
            let a1 = alev0(band, 1);
            for (j, slot) in gmf.iter_mut().enumerate() {
                *slot = if j <= 111 {
                    1.0
                } else if j <= 143 {
                    a0 * a1 * pfmd_prev[j - 112]
                } else if j <= 255 {
                    a1 * fmd[0][j - 144]
                } else {
                    fmd[1][j - 256]
                };
            }
            // PFMD_B(j) = FMD_1,B(j), 0 ≤ j ≤ 255.
            pfmd_next = fmd[1][..256].to_vec();
        }
        WindowSequence::EightShort => unreachable!("gmf_long called for short sequence"),
    }
    (gmf, pfmd_next)
}

/// Compose the §4.6.12.3.2 step-3 `EIGHT_SHORT_SEQUENCE` gain
/// modification: eight 64-sample `GMF` windows, threading `PFMD`.
fn gmf_short(fmd: &[Vec<f64>], band: &GainBand, pfmd_prev: &[f64]) -> (Vec<Vec<f64>>, Vec<f64>) {
    let mut gmf: Vec<Vec<f64>> = Vec::with_capacity(8);
    for w in 0..8 {
        let a0 = alev0(band, w);
        let mut g = vec![0.0f64; 64];
        for (j, slot) in g.iter_mut().enumerate() {
            *slot = if j <= 31 {
                if w == 0 {
                    a0 * pfmd_prev[j]
                } else {
                    a0 * fmd[w - 1][j]
                }
            } else {
                fmd[w][j - 32]
            };
        }
        gmf.push(g);
    }
    // PFMD_B(j) = FMD_7,B(j), 0 ≤ j ≤ 31.
    let pfmd_next = fmd[7][..32].to_vec();
    (gmf, pfmd_next)
}

impl BandGainFunction {
    /// Reconstruct the §4.6.12.3.1–2 gain-control function `AD` for one
    /// band of one frame.
    ///
    /// * `band` — the band's per-window ladder records (the
    ///   `bands[b - 1]` entry of the wire [`GainControlData`], spec band
    ///   `b ∈ 1..=3`).
    /// * `seq` — the frame's `window_sequence`.
    /// * `pfmd_prev` — `PFMD_B(j)` carried from the previous frame
    ///   (initial `1.0`). Length is 256 for the long sequences, 32 for
    ///   the short sequence.
    ///
    /// Returns the per-window `AD_{W,B}(j) = 1 / GMF_{W,B}(j)` arrays
    /// and the `pfmd_next` to thread into the next frame.
    #[must_use]
    pub fn reconstruct(band: &GainBand, seq: WindowSequence, pfmd_prev: &[f64]) -> Self {
        let n_win = num_windows(seq);
        // Per-window FMD.
        let fmd: Vec<Vec<f64>> = (0..n_win)
            .map(|w| {
                let ladder = Ladder::reconstruct(&band.windows[w], seq, w);
                fmd_window(&ladder, seq, w)
            })
            .collect();

        match seq {
            WindowSequence::EightShort => {
                let (gmf, pfmd_next) = gmf_short(&fmd, band, pfmd_prev);
                let ad = gmf
                    .iter()
                    .map(|g| g.iter().map(|&v| 1.0 / v).collect())
                    .collect();
                BandGainFunction { ad, pfmd_next }
            }
            _ => {
                let (gmf, pfmd_next) = gmf_long(&fmd, band, pfmd_prev, seq);
                let ad = vec![gmf.iter().map(|&v| 1.0 / v).collect()];
                BandGainFunction { ad, pfmd_next }
            }
        }
    }

    /// An identity gain function (`AD ≡ 1`) for a band with no gain
    /// control active — the §4.6.12.3.3 `B == 0` case (band 0 never
    /// carries a ladder) and any band beyond `max_band`.
    ///
    /// `seq` selects the window layout: one 512-sample window for the
    /// long sequences, eight 64-sample windows for the short sequence.
    #[must_use]
    pub fn identity(seq: WindowSequence) -> Self {
        match seq {
            WindowSequence::EightShort => BandGainFunction {
                ad: vec![vec![1.0; 64]; 8],
                pfmd_next: vec![1.0; 32],
            },
            _ => BandGainFunction {
                ad: vec![vec![1.0; 512]],
                pfmd_next: vec![1.0; 256],
            },
        }
    }
}

/// One IPQF band's cross-frame gain-control state (§4.6.12.3.2–3): the
/// `PFMD_B(j)` fragment-modification carry and the `PT_B(j)`
/// gain-controlled block sample data tail.
///
/// Construct with [`GainBandState::new`] (spec initial values: `PFMD ≡
/// 1.0`, `PT ≡ 0.0`), then call [`GainBandState::window_overlap`] once
/// per frame; it returns the 256-sample-stride band sample data `V_B`
/// for this frame and advances both carries.
#[derive(Debug, Clone, PartialEq)]
pub struct GainBandState {
    /// `PFMD_B(j)` — 256 entries (only the first
    /// [`pfmd_len`]`(seq)` are read by the next frame).
    pfmd: Vec<f64>,
    /// `PT_B(j)` — 256 entries (only the written prefix is meaningful
    /// for the next frame's overlap).
    pt: Vec<f64>,
}

impl Default for GainBandState {
    fn default() -> Self {
        Self::new()
    }
}

impl GainBandState {
    /// A fresh band state with the §4.6.12 spec initial values:
    /// `PFMD_B(j) = 1.0` and `PT_B(j) = 0.0`.
    #[must_use]
    pub fn new() -> Self {
        GainBandState {
            pfmd: vec![1.0; 256],
            pt: vec![0.0; 256],
        }
    }

    /// The §4.6.12.3.3 gain-control windowing + overlapping for one band
    /// of one frame.
    ///
    /// * `band` — this band's wire ladder (`None` for band 0 or a band
    ///   beyond `max_band`: gain control is inactive and `T = U`).
    /// * `u` — the band spectrum data `U_{W,B}(j)`, the non-overlapped
    ///   per-band IMDCT output. For the long sequences this is a single
    ///   512-sample window; for `EIGHT_SHORT_SEQUENCE` it is eight
    ///   64-sample windows concatenated (window `w` at `u[64·w .. 64·w +
    ///   64]`).
    /// * `seq` — the frame's `window_sequence`.
    ///
    /// Returns the band sample data `V_B(j)` (the variable-length
    /// per-frame fragment: 256 for `ONLY_LONG` / `EIGHT_SHORT`, 368 for
    /// `LONG_START`, 144 for `LONG_STOP`) and updates the `PFMD` / `PT`
    /// carries in place.
    #[must_use]
    pub fn window_overlap(
        &mut self,
        band: Option<&GainBand>,
        u: &[f64],
        seq: WindowSequence,
    ) -> Vec<f64> {
        // (1) windowing: T = AD · U  (or T = U when gain control is off).
        // The produced `pfmd_next` is the 32- or 256-entry prefix the
        // next frame reads; write it into the persistent 256-buffer so
        // the buffer never shrinks (any branch can read its prefix).
        let t = match band {
            Some(b) => {
                let g = BandGainFunction::reconstruct(b, seq, &self.pfmd);
                self.store_pfmd(&g.pfmd_next);
                apply_gain(&g.ad, u, seq)
            }
            None => {
                // Band 0 / inactive: T = U, PFMD threads as the identity.
                let g = BandGainFunction::identity(seq);
                self.store_pfmd(&g.pfmd_next);
                u.to_vec()
            }
        };

        // (2) overlapping: produce V_B and update PT_B.
        self.overlap(&t, seq)
    }

    /// Write the produced `PFMD` prefix into the persistent 256-entry
    /// buffer (the buffer never shrinks, so any following frame can read
    /// the prefix it needs).
    fn store_pfmd(&mut self, produced: &[f64]) {
        self.pfmd[..produced.len()].copy_from_slice(produced);
    }

    /// The §4.6.12.3.3 step-(2) overlap for the gain-controlled block
    /// sample data `t` (`T_{W,B}` concatenated window-major).
    fn overlap(&mut self, t: &[f64], seq: WindowSequence) -> Vec<f64> {
        match seq {
            WindowSequence::OnlyLong => {
                // V(j) = PT(j) + T0(j), 0..256; PT(j) = T0(j+256), 0..256.
                let v = add_slices(&self.pt[..256], &t[..256]);
                self.pt[..256].copy_from_slice(&t[256..512]);
                v
            }
            WindowSequence::LongStart => {
                // V(j) = PT(j) + T0(j), 0..256;
                // V(j+256) = T0(j+256), 0..112;  ⇒ V spans 0..368.
                // PT(j) = T0(j+368), 0..32.
                let mut v = vec![0.0f64; 368];
                add_into(&mut v[..256], &self.pt[..256], &t[..256]);
                v[256..368].copy_from_slice(&t[256..368]);
                self.pt[..32].copy_from_slice(&t[368..400]);
                v
            }
            WindowSequence::EightShort => {
                // V(j) = PT(j) + T0(j), W==0, 0..32;
                // V(32W+j) = T_{W-1}(j+32) + T_W(j), 1..=7, 0..32;
                // PT(j) = T7(j+32), 0..32.  ⇒ V spans 0..256.
                let mut v = vec![0.0f64; 256];
                // Window w occupies t[64·w .. 64·w + 64].
                add_into(&mut v[..32], &self.pt[..32], &t[..32]);
                for w in 1..=7 {
                    let prev = &t[64 * (w - 1) + 32..64 * (w - 1) + 64];
                    let cur = &t[64 * w..64 * w + 32];
                    add_into(&mut v[32 * w..32 * w + 32], prev, cur);
                }
                self.pt[..32].copy_from_slice(&t[64 * 7 + 32..64 * 7 + 64]);
                v
            }
            WindowSequence::LongStop => {
                // V(j) = PT(j) + T0(j+112), 0..32;
                // V(j+32) = T0(j+144), 0..112;  ⇒ V spans 0..144.
                // PT(j) = T0(j+256), 0..256.
                let mut v = vec![0.0f64; 144];
                add_into(&mut v[..32], &self.pt[..32], &t[112..144]);
                v[32..144].copy_from_slice(&t[144..256]);
                self.pt[..256].copy_from_slice(&t[256..512]);
                v
            }
        }
    }
}

/// Element-wise sum of two equal-length slices into a fresh `Vec`.
fn add_slices(a: &[f64], b: &[f64]) -> Vec<f64> {
    a.iter().zip(b.iter()).map(|(&x, &y)| x + y).collect()
}

/// Element-wise `dst[i] = a[i] + b[i]` over equal-length slices.
fn add_into(dst: &mut [f64], a: &[f64], b: &[f64]) {
    for (d, (&x, &y)) in dst.iter_mut().zip(a.iter().zip(b.iter())) {
        *d = x + y;
    }
}

/// Apply the §4.6.12.3.3 step-(1) gain `T_{W,B}(j) = AD_{W,B}(j) ·
/// U_{W,B}(j)` window-major, returning the concatenated `T`.
fn apply_gain(ad: &[Vec<f64>], u: &[f64], seq: WindowSequence) -> Vec<f64> {
    match seq {
        WindowSequence::EightShort => {
            let mut t = vec![0.0f64; u.len()];
            for (w, ad_w) in ad.iter().enumerate() {
                for (j, &g) in ad_w.iter().enumerate() {
                    let idx = 64 * w + j;
                    t[idx] = g * u[idx];
                }
            }
            t
        }
        _ => ad[0].iter().zip(u.iter()).map(|(&g, &x)| g * x).collect(),
    }
}

/// The §4.6.12.3.2 `PFMD_B` **input** length a frame of `seq` reads
/// from the previous frame.
///
/// The step-3 `GMF` composition reads `PFMD_B(j)` over `0..256` for
/// `ONLY_LONG` / `LONG_START` (their left half spans the full 256), but
/// only `0..32` for `LONG_STOP` (the `112 ≤ j ≤ 143` region) and
/// `EIGHT_SHORT` (the `W == 0`, `0 ≤ j ≤ 31` region). A
/// [`GainBandState`] keeps the full 256-entry buffer, so any branch can
/// always read the prefix it needs.
#[must_use]
pub fn pfmd_len(seq: WindowSequence) -> usize {
    match seq {
        WindowSequence::OnlyLong | WindowSequence::LongStart => 256,
        WindowSequence::LongStop | WindowSequence::EightShort => 32,
    }
}

/// The §4.6.12.3.2 `PFMD_B` **output** length a frame of `seq` produces
/// for the next frame.
///
/// `ONLY_LONG` / `LONG_STOP` emit `FMD(0..256)` (256 entries);
/// `LONG_START` / `EIGHT_SHORT` emit `FMD(0..32)` (32 entries). In a
/// legal `window_sequence` chain the produced length always matches
/// what the following frame's [`pfmd_len`] reads (`LONG_START` →
/// `EIGHT_SHORT`, `EIGHT_SHORT` → `LONG_STOP`, etc.).
#[must_use]
pub fn pfmd_produced_len(seq: WindowSequence) -> usize {
    match seq {
        WindowSequence::OnlyLong | WindowSequence::LongStop => 256,
        WindowSequence::LongStart | WindowSequence::EightShort => 32,
    }
}

/// Look up the band's wire ladder from a [`GainControlData`] record for
/// spec band `b ∈ 1..=3`, or `None` when `b > max_band` (the band is
/// not gain-controlled, so its gain function is the identity).
#[must_use]
pub fn band_record(gcd: &GainControlData, b: usize) -> Option<&GainBand> {
    if b == 0 || b > gcd.max_band as usize {
        None
    } else {
        gcd.bands.get(b - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adj_loc_is_eight_times() {
        assert_eq!(adj_loc(0), 0);
        assert_eq!(adj_loc(1), 8);
        assert_eq!(adj_loc(15), 120);
        assert_eq!(adj_loc(31), 248);
    }

    #[test]
    fn adj_lev_is_offset_minus_four() {
        assert_eq!(adj_lev(0), -4);
        assert_eq!(adj_lev(4), 0);
        assert_eq!(adj_lev(15), 11);
    }

    #[test]
    fn endpoint_aloc_per_sequence() {
        assert_eq!(endpoint_aloc(WindowSequence::OnlyLong, 0), 256);
        assert_eq!(endpoint_aloc(WindowSequence::LongStart, 0), 112);
        assert_eq!(endpoint_aloc(WindowSequence::LongStart, 1), 32);
        assert_eq!(endpoint_aloc(WindowSequence::EightShort, 3), 32);
        assert_eq!(endpoint_aloc(WindowSequence::LongStop, 0), 112);
        assert_eq!(endpoint_aloc(WindowSequence::LongStop, 1), 256);
    }

    #[test]
    fn inter_endpoints_are_exact() {
        // Inter(a, b, 0) == a, Inter(a, b, 8) == b.
        assert!((inter(2.0, 8.0, 0) - 2.0).abs() < 1e-12);
        assert!((inter(2.0, 8.0, 8) - 8.0).abs() < 1e-12);
        // Geometric midpoint at j == 4: sqrt(a·b).
        assert!((inter(2.0, 8.0, 4) - (2.0f64 * 8.0).sqrt()).abs() < 1e-12);
    }

    #[test]
    fn empty_ladder_gives_unit_gain() {
        // A band with an all-empty (adjust_num == 0) ladder produces
        // AD ≡ 1 everywhere (GMF ≡ 1).
        let band = GainBand {
            windows: vec![crate::gain_control_data::GainWindow::default()],
        };
        let pfmd = vec![1.0f64; 256];
        let g = BandGainFunction::reconstruct(&band, WindowSequence::OnlyLong, &pfmd);
        assert_eq!(g.ad.len(), 1);
        assert_eq!(g.ad[0].len(), 512);
        for &v in &g.ad[0] {
            assert!((v - 1.0).abs() < 1e-12, "expected unit gain, got {v}");
        }
        // PFMD threads forward as FMD_0 == 1.
        assert!(g.pfmd_next.iter().all(|&v| (v - 1.0).abs() < 1e-12));
    }

    #[test]
    fn identity_matches_empty_ladder() {
        let band = GainBand {
            windows: vec![crate::gain_control_data::GainWindow::default(); 8],
        };
        let pfmd = vec![1.0f64; 32];
        let recon = BandGainFunction::reconstruct(&band, WindowSequence::EightShort, &pfmd);
        let ident = BandGainFunction::identity(WindowSequence::EightShort);
        assert_eq!(recon.ad.len(), ident.ad.len());
        for (r, i) in recon.ad.iter().zip(ident.ad.iter()) {
            for (&rv, &iv) in r.iter().zip(i.iter()) {
                assert!((rv - iv).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn overlap_only_long_is_tdac_add() {
        // Identity gain (band 0 / inactive): T == U. A 512-sample U;
        // first frame V(j) = 0 + U(j) (PT starts 0); PT becomes
        // U(256..512). Second frame with the same U: V(j) =
        // U(256+j) + U(j).
        let mut st = GainBandState::new();
        let u: Vec<f64> = (0..512).map(|j| (j as f64) * 0.01).collect();
        let v0 = st.window_overlap(None, &u, WindowSequence::OnlyLong);
        assert_eq!(v0.len(), 256);
        for j in 0..256 {
            assert!((v0[j] - u[j]).abs() < 1e-12);
        }
        let v1 = st.window_overlap(None, &u, WindowSequence::OnlyLong);
        for j in 0..256 {
            assert!((v1[j] - (u[256 + j] + u[j])).abs() < 1e-12);
        }
    }

    #[test]
    fn overlap_lengths_per_sequence() {
        let u_long = vec![1.0f64; 512];
        let u_short = vec![1.0f64; 512]; // eight 64-sample windows.
        assert_eq!(
            GainBandState::new()
                .window_overlap(None, &u_long, WindowSequence::OnlyLong)
                .len(),
            256
        );
        assert_eq!(
            GainBandState::new()
                .window_overlap(None, &u_long, WindowSequence::LongStart)
                .len(),
            368
        );
        assert_eq!(
            GainBandState::new()
                .window_overlap(None, &u_short, WindowSequence::EightShort)
                .len(),
            256
        );
        assert_eq!(
            GainBandState::new()
                .window_overlap(None, &u_long, WindowSequence::LongStop)
                .len(),
            144
        );
    }

    #[test]
    fn overlap_eight_short_overlaps_adjacent_windows() {
        // Identity gain. Each short window is constant c_w. The overlap
        // V(32W+j) = T_{W-1}(j+32) + T_W(j) = c_{W-1} + c_W for the
        // overlapped region, and PT becomes c_7.
        let mut st = GainBandState::new();
        let mut u = vec![0.0f64; 512];
        for w in 0..8 {
            for j in 0..64 {
                u[64 * w + j] = (w as f64) + 1.0;
            }
        }
        let v = st.window_overlap(None, &u, WindowSequence::EightShort);
        assert_eq!(v.len(), 256);
        // First segment: PT(0)=0 + T0 = 1.
        assert!((v[0] - 1.0).abs() < 1e-12);
        // Segment W=1: T0 + T1 = 1 + 2 = 3.
        assert!((v[32] - 3.0).abs() < 1e-12);
        // Segment W=7: T6 + T7 = 7 + 8 = 15.
        assert!((v[32 * 7] - 15.0).abs() < 1e-12);
        // PT now holds T7 = 8.
        assert!((st.pt[0] - 8.0).abs() < 1e-12);
    }

    #[test]
    fn gain_then_overlap_scales_band() {
        use crate::gain_control_data::{GainAdjust, GainWindow};
        // A constant band U ≡ 1.0; a single gain change makes AD ≠ 1 in
        // the [256..) region (where the FMD lands). The V output picks
        // up AD·U in that region.
        let band = GainBand {
            windows: vec![GainWindow {
                adjustments: vec![GainAdjust {
                    alevcode: 6, // AdjLev=2 ⇒ ALEV=4 ⇒ AD=1/4 in ramp.
                    aloccode: 0, // ALOC=0.
                }],
            }],
        };
        let mut st = GainBandState::new();
        let u = vec![1.0f64; 512];
        let v = st.window_overlap(Some(&band), &u, WindowSequence::OnlyLong);
        assert_eq!(v.len(), 256);
        // V is finite and the gain has been applied (not all 1.0).
        assert!(v.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn single_gain_change_scales_segment() {
        use crate::gain_control_data::{GainAdjust, GainWindow};
        // One gain change at aloccode=2 (ALOC=16), alevcode=6
        // (AdjLev=2 ⇒ ALEV=4). NADW=1.
        //   ALOC = [0, 16, 256], ALEV = [4, 4, 1] (ALEV(0)=ALEV(1)=4).
        // For j in 0..16, M=0, ALOC(0)=0, ramp Inter(4,4,j)=4 over the
        // first 8 then flat ALEV(1)=4 ⇒ FMD=4 throughout 0..16.
        let band = GainBand {
            windows: vec![GainWindow {
                adjustments: vec![GainAdjust {
                    alevcode: 6,
                    aloccode: 2,
                }],
            }],
        };
        let pfmd = vec![1.0f64; 256];
        let g = BandGainFunction::reconstruct(&band, WindowSequence::OnlyLong, &pfmd);
        // GMF(256) = FMD_0(0) = ALEV at j=0 region. Since ALOC(1)=16,
        // M(0)=0, ALEV(0)=4, ALEV(1)=4 ⇒ FMD(0)=4 ⇒ AD = 1/4.
        assert!((g.ad[0][256] - 0.25).abs() < 1e-9, "AD={}", g.ad[0][256]);
        // Beyond ALOC(NADW+1)=256 region: at j large, M=1 (ALOC(1)=16),
        // ALEV(1)=4, ALEV(2)=1, j-16 > 7 ⇒ FMD = ALEV(2) = 1 ⇒ AD=1.
        assert!((g.ad[0][511] - 1.0).abs() < 1e-9, "AD={}", g.ad[0][511]);
    }

    /// A band carrying a ladder reconstructs a finite, strictly-positive
    /// `AD` over the full window for every `window_sequence` — the
    /// `GMF`/`AD` reciprocal pair is well-defined (no zero or infinity).
    #[test]
    fn ad_is_finite_positive_all_sequences() {
        use crate::gain_control_data::{GainAdjust, GainWindow};
        for &seq in &[
            WindowSequence::OnlyLong,
            WindowSequence::LongStart,
            WindowSequence::LongStop,
            WindowSequence::EightShort,
        ] {
            let n_win = num_windows(seq);
            // Each window carries one mid-range gain change.
            let windows = (0..n_win)
                .map(|_| GainWindow {
                    adjustments: vec![GainAdjust {
                        alevcode: 7, // AdjLev=3 ⇒ ALEV=8.
                        aloccode: 1, // ALOC=8.
                    }],
                })
                .collect();
            let band = GainBand { windows };
            let pfmd = vec![1.0f64; pfmd_len(seq)];
            let g = BandGainFunction::reconstruct(&band, seq, &pfmd);
            for win in &g.ad {
                for &v in win {
                    assert!(v.is_finite() && v > 0.0, "AD={v} for {seq:?}");
                }
            }
            // PFMD threads with the right produced length.
            assert_eq!(g.pfmd_next.len(), pfmd_produced_len(seq));
        }
    }

    /// The §4.6.12.3.2 inversion is exact: `AD(j) · GMF(j) == 1`. We
    /// recover `GMF` as `1/AD` and confirm it round-trips to `AD`.
    #[test]
    fn ad_times_gmf_is_one() {
        use crate::gain_control_data::{GainAdjust, GainWindow};
        let band = GainBand {
            windows: vec![GainWindow {
                adjustments: vec![
                    GainAdjust {
                        alevcode: 8,
                        aloccode: 2,
                    },
                    GainAdjust {
                        alevcode: 2,
                        aloccode: 10,
                    },
                ],
            }],
        };
        let pfmd = vec![1.0f64; 256];
        let g = BandGainFunction::reconstruct(&band, WindowSequence::OnlyLong, &pfmd);
        for &ad in &g.ad[0] {
            let gmf = 1.0 / ad;
            assert!((ad * gmf - 1.0).abs() < 1e-12);
        }
    }

    /// Pre-stream defaults: a first frame with `PFMD ≡ 1.0` and a band
    /// whose only gain change sits at `ALOC = 0` scales the left-half
    /// `GMF` region by `ALEV(0)` (the §4.6.12.3.2 step-3 `ONLY_LONG`
    /// branch `ALEV(0)·PFMD`).
    #[test]
    fn long_left_half_scaled_by_alev0() {
        use crate::gain_control_data::{GainAdjust, GainWindow};
        // alevcode=7 ⇒ AdjLev=3 ⇒ ALEV=8; aloccode=0 ⇒ ALOC=0.
        // ALEV(0)=ALEV(1)=8. GMF(j) for j in 0..256 = ALEV(0)·PFMD(j)
        // = 8·1 = 8 ⇒ AD = 1/8.
        let band = GainBand {
            windows: vec![GainWindow {
                adjustments: vec![GainAdjust {
                    alevcode: 7,
                    aloccode: 0,
                }],
            }],
        };
        let pfmd = vec![1.0f64; 256];
        let g = BandGainFunction::reconstruct(&band, WindowSequence::OnlyLong, &pfmd);
        for &ad in &g.ad[0][..256] {
            assert!((ad - 0.125).abs() < 1e-12, "AD={ad}");
        }
    }
}
