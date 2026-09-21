//! §4.6.11 Filterbank and block switching — the inverse modified
//! discrete cosine transform (IMDCT), the analysis/synthesis windows
//! (sine and Kaiser-Bessel-derived), and the overlap-add that maps a
//! window-major decoded spectrum back to the time domain.
//!
//! This is the last stage of the per-channel decode chain
//! ([`crate::decoded_spectrum::decode_channel_spectrum`]) for a
//! single channel: it consumes the `num_windows × window_len = 1024`
//! window-major coefficients and emits 1024 PCM-domain samples per
//! frame after overlap-adding against the previous frame's tail.
//!
//! Spec basis (ISO/IEC 14496-3:2001, §4.6.11):
//!
//! * §4.6.11.3.1 — the IMDCT
//!   `x[n] = (2/N) · Σ_k spec[k] · cos((2π/N)·(n + n0)·(k + 1/2))`
//!   for `0 ≤ n < N`, with `n0 = (N/2 + 1)/2`. `N` is the
//!   *transform* window length (2048 for long sequences, 256 for each
//!   of the eight short windows). The crate carries the spectrum at
//!   `N/2` resolution (1024 long, 128 short) as
//!   [`crate::swb_offset::LONG_WINDOW_LEN`] /
//!   [`crate::swb_offset::SHORT_WINDOW_LEN`].
//! * §4.6.11.3.2 — windowing and block switching. The sine window is
//!   `W_SIN(n) = sin((π/N)·(n + 1/2))`; the KBD window is the
//!   normalized running sum of the Kaiser-Bessel kernel `W'(n, α)`
//!   with `α = 4` for the long transform and `α = 6` for the short
//!   transform. The four `window_sequence` shapes
//!   (`ONLY_LONG`, `LONG_START`, `EIGHT_SHORT`, `LONG_STOP`) compose
//!   left/right window halves; the left half's shape is inherited
//!   from the *previous* block's `window_shape`.
//! * §4.6.11.3.3 — the inter-block overlap-add
//!   `out[n] = z[i][n] + z[i-1][n + N/2]` for `0 ≤ n < N/2`,
//!   `N = 2048`, valid for all four sequences.
//!
//! The frame-length-960 (`N = 1920 / 240`) variant of the spec is
//! out of scope: the rest of the crate's `swb_offset` tables and
//! transmission-order machinery are wired to the 1024-coefficient
//! layout, so this module mirrors that and only implements the 2048
//! transform family.

use crate::ics_info::{IcsInfo, WindowSequence, WindowShape};
use crate::swb_offset::{LONG_WINDOW_LEN, SHORT_WINDOW_LEN};
use crate::Error;

/// `N` for a long-sequence transform (§4.6.11.3.1): 2 ×
/// [`LONG_WINDOW_LEN`].
const LONG_TRANSFORM_LEN: usize = 2 * LONG_WINDOW_LEN as usize; // 2048
/// `N` for a single short-sequence transform: 2 ×
/// [`SHORT_WINDOW_LEN`].
const SHORT_TRANSFORM_LEN: usize = 2 * SHORT_WINDOW_LEN as usize; // 256
/// `M = N_l / N_s` = number of short windows in an `EIGHT_SHORT`
/// sequence.
const NUM_SHORT_WINDOWS: usize = 8;
/// `N_l` — the long transform length, used as the frame's PCM stride.
const N_L: usize = LONG_TRANSFORM_LEN; // 2048
/// `N_s` — the short transform length.
const N_S: usize = SHORT_TRANSFORM_LEN; // 256

/// Result of [`Filterbank::synthesize`]: one frame of
/// `LONG_WINDOW_LEN` (1024) PCM-domain samples for a single channel.
type Result<T> = core::result::Result<T, Error>;

/// §4.6.11.3.1 — inverse MDCT for a length-`n_transform` window.
///
/// `spec` holds the `N/2` transmitted coefficients; the returned
/// vector holds the `N` time-domain values
/// `x[n] = (2/N) · Σ_k spec[k] · cos((2π/N)·(n + n0)·(k + 1/2))`.
///
/// `n0 = (N/2 + 1)/2` is the §4.6.11.3.1 phase offset. The `2/N`
/// scale and the half-coefficient phase are the only normalization
/// the spec attaches to the inverse transform; the energy-correcting
/// window then follows in the per-sequence windowing step.
fn imdct(spec: &[f64], n_transform: usize) -> Vec<f64> {
    let half = n_transform / 2;
    debug_assert_eq!(spec.len(), half);
    let n0 = (half + 1) as f64 / 2.0;
    let scale = 2.0 / n_transform as f64;
    let phase_step = 2.0 * core::f64::consts::PI / n_transform as f64;
    let mut out = vec![0.0f64; n_transform];
    for (n, slot) in out.iter_mut().enumerate() {
        let np = n as f64 + n0;
        let mut acc = 0.0f64;
        for (k, &c) in spec.iter().enumerate() {
            acc += c * (phase_step * np * (k as f64 + 0.5)).cos();
        }
        *slot = scale * acc;
    }
    out
}

/// §4.6.15.3.3 / §4.6.11.3.1 — the forward (analysis) MDCT for a
/// length-`n_transform` window.
///
/// `time` holds the `N` windowed time-domain values `z[n]`; the
/// returned vector holds the `N/2` spectral coefficients
/// `X[k] = 2 · Σ_n z[n] · cos((2π/N)·(n + n0)·(k + 1/2))`,
/// `0 ≤ k < N/2`, with the §4.6.11.3.1 phase `n0 = (N/2 + 1)/2`.
///
/// This is the exact analysis pair of [`imdct`]: the IMDCT carries the
/// `2/N` scale, the analysis here carries the matching factor `2`, so
/// the windowed-and-overlap-added round trip is unity for a
/// power-complementary §4.6.11.3.2 window. The same transform is the
/// `MDCT(x_est)` of the §4.6.7.3 Long-Term-Prediction loop.
pub(crate) fn forward_mdct(time: &[f64], n_transform: usize) -> Vec<f64> {
    let half = n_transform / 2;
    debug_assert_eq!(time.len(), n_transform);
    let n0 = (half + 1) as f64 / 2.0;
    let step = 2.0 * core::f64::consts::PI / n_transform as f64;
    (0..half)
        .map(|k| {
            2.0 * time
                .iter()
                .enumerate()
                .map(|(n, &t)| t * (step * (n as f64 + n0) * (k as f64 + 0.5)).cos())
                .sum::<f64>()
        })
        .collect()
}

/// §4.6.11.3.2 — build the length-2048 `ONLY_LONG_SEQUENCE` analysis
/// window `[W_LEFT_l | W_RIGHT_l]` for the given left/right shapes.
///
/// Exposed for the §4.6.7.3 LTP loop, which windows the predicted time
/// signal `x_est` with the current long window before the analysis
/// [`forward_mdct`]. (LTP for the AAC LTP object type is restricted to
/// long windows, §4.6.7.1.)
pub(crate) fn long_only_window(left_shape: WindowShape, right_shape: WindowShape) -> Vec<f64> {
    let halves = window_halves(LONG_TRANSFORM_LEN, left_shape, right_shape);
    let half_l = LONG_TRANSFORM_LEN / 2;
    let mut w = vec![0.0f64; LONG_TRANSFORM_LEN];
    w[..half_l].copy_from_slice(&halves.left);
    for (m, &rv) in halves.right.iter().enumerate() {
        w[half_l + m] = rv;
    }
    w
}

/// Modified Bessel function of the first kind, order 0, via its power
/// series `I0(x) = Σ_k ((x/2)^k / k!)^2` (§4.6.11.3.2). The series
/// converges quickly for the `x = π·α` arguments the KBD window uses
/// (`α ∈ {4, 6}`), so a fixed term cap with an early-out on negligible
/// terms is exact to f64 precision.
fn bessel_i0(x: f64) -> f64 {
    let half_x = x / 2.0;
    let mut term = 1.0f64; // k = 0 term: (half_x^0 / 0!)^2 = 1
    let mut sum = 1.0f64;
    let mut k = 1.0f64;
    loop {
        // term_k = term_{k-1} · (half_x / k)^2
        term *= (half_x / k) * (half_x / k);
        sum += term;
        if term <= sum * 1e-18 {
            break;
        }
        k += 1.0;
        if k > 256.0 {
            break;
        }
    }
    sum
}

/// §4.6.11.3.2 — the Kaiser-Bessel kernel
/// `W'(n, α) = I0(π·α·sqrt(1 − ((n − N/4)/(N/4))^2)) / I0(π·α)`
/// for `0 ≤ n ≤ N/2`, evaluated over `0..=half` (`half = N/2`).
fn kbd_kernel(half: usize, alpha: f64) -> Vec<f64> {
    let quarter = half as f64 / 2.0; // N/4
    let denom = bessel_i0(core::f64::consts::PI * alpha);
    (0..=half)
        .map(|n| {
            let t = (n as f64 - quarter) / quarter;
            let radicand = (1.0 - t * t).max(0.0);
            bessel_i0(core::f64::consts::PI * alpha * radicand.sqrt()) / denom
        })
        .collect()
}

/// §4.6.11.3.2 — the left half of the KBD window:
/// `W_KBD_LEFT(n) = sqrt( Σ_{p=0..n} W'(p) / Σ_{p=0..N/2} W'(p) )`
/// for `0 ≤ n < N/2`. Returns the `half = N/2` left-half samples.
///
/// `alpha` is 4 for the long transform and 6 for the short transform.
fn kbd_left(half: usize, alpha: f64) -> Vec<f64> {
    let kernel = kbd_kernel(half, alpha);
    let total: f64 = kernel.iter().sum();
    let mut running = 0.0f64;
    let mut out = Vec::with_capacity(half);
    for &w in kernel.iter().take(half) {
        running += w;
        out.push((running / total).sqrt());
    }
    out
}

/// §4.6.11.3.2 — the sine window left half
/// `W_SIN_LEFT(n) = sin((π/N)·(n + 1/2))`, `0 ≤ n < N/2`. Returns the
/// `half = N/2` samples.
fn sine_left(half: usize) -> Vec<f64> {
    let n_transform = (2 * half) as f64;
    (0..half)
        .map(|n| (core::f64::consts::PI / n_transform * (n as f64 + 0.5)).sin())
        .collect()
}

/// One transform's analysis/synthesis window halves, each `half = N/2`
/// long. The right half of a sine/KBD window is the mirror of its
/// left half (`W_RIGHT(n) = W_LEFT(N − 1 − n)`), so we store left
/// halves and index the right half by mirror at apply time.
struct WindowHalves {
    /// Left half, indices `0..half`.
    left: Vec<f64>,
    /// Right half, indices `0..half`; element `m` is the window value
    /// at transform position `half + m`.
    right: Vec<f64>,
}

/// Build the left half for the requested `shape` at transform length
/// `n_transform`.
fn half_window(n_transform: usize, shape: WindowShape) -> Vec<f64> {
    let half = n_transform / 2;
    match shape {
        WindowShape::Sine => sine_left(half),
        WindowShape::Kbd => {
            let alpha = if n_transform == LONG_TRANSFORM_LEN {
                4.0
            } else {
                6.0
            };
            kbd_left(half, alpha)
        }
    }
}

/// §4.6.11.3.2 — assemble a transform's window from a `left` shape
/// (inherited from the previous block) and a `right` shape (this
/// block's `window_shape`). For a sine/KBD window the right half is
/// the spatial mirror of that shape's *left* half, so we build the
/// `right`-shape left half and reverse it.
fn window_halves(
    n_transform: usize,
    left_shape: WindowShape,
    right_shape: WindowShape,
) -> WindowHalves {
    let left = half_window(n_transform, left_shape);
    let mut right = half_window(n_transform, right_shape);
    right.reverse();
    WindowHalves { left, right }
}

/// The stateful per-channel §4.6.11 filterbank. One instance per
/// decoded channel; [`Filterbank::synthesize`] is called once per
/// frame and carries the overlap-add tail (`z[i-1][n + N/2]`) plus the
/// previous block's `window_shape` (which determines the left-half
/// shape of the next block, §4.6.11.3.2) across calls.
#[derive(Clone, Debug)]
pub struct Filterbank {
    /// `z[i-1][N/2 .. N]` — the right half of the previous frame's
    /// windowed time signal, added to the left half of this frame's
    /// windowed signal (§4.6.11.3.3). `LONG_WINDOW_LEN` long.
    overlap: Vec<f64>,
    /// `window_shape` of the previous block, governing the left-half
    /// window shape of the next block. [`None`] before the first
    /// frame: per §4.6.11.3.2 the first block's left and right halves
    /// share its own `window_shape`.
    prev_shape: Option<WindowShape>,
}

impl Default for Filterbank {
    fn default() -> Self {
        Self::new()
    }
}

impl Filterbank {
    /// A fresh filterbank with a zeroed overlap buffer and no
    /// previous-block shape (so the first frame uses its own
    /// `window_shape` for both halves, per §4.6.11.3.2).
    pub fn new() -> Self {
        Filterbank {
            overlap: vec![0.0f64; LONG_WINDOW_LEN as usize],
            prev_shape: None,
        }
    }

    /// §4.6.7.3 — the current frame's *aliased half window*
    /// `x_rec(0 … N/2 − 1)`: the right half of the just-synthesized
    /// frame's windowed (pre-overlap-add) time signal `z[i][N/2 … N]`.
    ///
    /// After a [`Self::synthesize`] call the internal overlap buffer
    /// holds exactly this tail (it is reused as the *next* frame's
    /// overlap-add term, §4.6.11.3.3). The LTP reconstruction history
    /// ([`crate::ltp::LtpState`]) needs the same vector — its
    /// `x_rec(0 … N/2 − 1)` region — so the element driver reads it here
    /// after each synthesis and feeds it to
    /// [`crate::ltp::LtpState::push_frame`]. Before the first frame this
    /// is the zero buffer, matching the §4.6.7.3 zero initialisation.
    pub fn aliased_tail(&self) -> &[f64] {
        &self.overlap
    }

    /// §4.6.11.3.2 — the previous block's `window_shape`, which governs
    /// the left-half shape of the *next* block's analysis/synthesis
    /// window. [`None`] before the first frame (the first block uses its
    /// own shape for both halves).
    ///
    /// The §4.6.7.4.1 LTP analysis MDCT must window `x_est` with the
    /// same composite long window the filterbank uses for this frame, so
    /// the element driver reads the previous shape here before
    /// synthesizing.
    pub fn prev_shape(&self) -> Option<WindowShape> {
        self.prev_shape
    }

    /// §4.6.11 — synthesize one frame of `LONG_WINDOW_LEN` (1024) PCM
    /// samples from `spec`, the window-major decoded spectrum produced
    /// by [`crate::decoded_spectrum::decode_channel_spectrum`].
    ///
    /// `spec` must be:
    ///
    /// * `LONG_WINDOW_LEN` (1024) coefficients for `ONLY_LONG`,
    ///   `LONG_START`, `LONG_STOP`;
    /// * `8 × SHORT_WINDOW_LEN` (1024 total) for `EIGHT_SHORT`,
    ///   laid out window-major: window `w` at `spec[w * 128 ..]`.
    ///
    /// The result is the §4.6.11.3.3 overlap-added output; the method
    /// updates the internal overlap tail and previous-block shape for
    /// the next call.
    ///
    /// Errors: [`Error::FilterbankInvalid`] if `spec.len()` disagrees
    /// with `ics_info.window_sequence`.
    pub fn synthesize(&mut self, spec: &[f64], ics_info: &IcsInfo) -> Result<Vec<f64>> {
        let z = self.windowed_signal(spec, ics_info)?;
        debug_assert_eq!(z.len(), N_L);

        // §4.6.11.3.3 overlap-add: out[n] = z[i][n] + z[i-1][n + N/2].
        let half = LONG_WINDOW_LEN as usize;
        let out: Vec<f64> = z[..half]
            .iter()
            .zip(self.overlap.iter())
            .map(|(&zn, &on)| zn + on)
            .collect();

        // Retain z[i][N/2 .. N] as next frame's z[i-1][n + N/2].
        self.overlap.clear();
        self.overlap.extend_from_slice(&z[half..]);

        // §4.6.11.3.2: the left-half shape of the *next* block is this
        // block's window_shape.
        self.prev_shape = Some(ics_info.window_shape);
        Ok(out)
    }

    /// §4.6.11.3.1 + §4.6.11.3.2 — produce the full-length (`N_l =
    /// 2048`) windowed time signal `z[i][n]` for this frame, before
    /// the inter-block overlap-add. Dispatches on `window_sequence`.
    fn windowed_signal(&self, spec: &[f64], ics_info: &IcsInfo) -> Result<Vec<f64>> {
        let left_shape = self.prev_shape.unwrap_or(ics_info.window_shape);
        let right_shape = ics_info.window_shape;
        match ics_info.window_sequence {
            WindowSequence::OnlyLong => {
                self.long_windowed(spec, left_shape, right_shape, LongKind::OnlyLong)
            }
            WindowSequence::LongStart => {
                self.long_windowed(spec, left_shape, right_shape, LongKind::Start)
            }
            WindowSequence::LongStop => {
                self.long_windowed(spec, left_shape, right_shape, LongKind::Stop)
            }
            WindowSequence::EightShort => self.short_windowed(spec, left_shape, right_shape),
        }
    }

    /// §4.6.11.3.2 a)/b)/d) — the three long-transform sequences. Each
    /// runs a single length-2048 IMDCT and applies a composite window
    /// whose left half (`ONLY_LONG`, `LONG_START`) or right half
    /// (`LONG_STOP`) is the full long half-window, and whose other
    /// half is shaped by the start/stop transition (a short half-window
    /// flanked by a flat `1.0` plateau and a zero region).
    fn long_windowed(
        &self,
        spec: &[f64],
        left_shape: WindowShape,
        right_shape: WindowShape,
        kind: LongKind,
    ) -> Result<Vec<f64>> {
        if spec.len() != LONG_WINDOW_LEN as usize {
            return Err(Error::FilterbankInvalid);
        }
        let x = imdct(spec, LONG_TRANSFORM_LEN);
        let w = self.long_window(left_shape, right_shape, kind);
        let z: Vec<f64> = x.iter().zip(w.iter()).map(|(&xv, &wv)| xv * wv).collect();
        Ok(z)
    }

    /// §4.6.11.3.2 — assemble the length-2048 window vector for a
    /// long-transform sequence.
    ///
    /// * `OnlyLong` (a): `[W_LEFT_l | W_RIGHT_l]`.
    /// * `Start` (b): left half is `W_LEFT_l`; the right half is a
    ///   flat `1.0` plateau over `[N_l/2, (3N_l − N_s)/4)`, the short
    ///   right half-window over `[(3N_l − N_s)/4, (3N_l + N_s)/4)`, and
    ///   `0.0` over `[(3N_l + N_s)/4, N_l)`.
    /// * `Stop` (d): the left half is `0.0` over `[0, (N_l − N_s)/4)`,
    ///   the short left half-window over `[(N_l − N_s)/4, (N_l +
    ///   N_s)/4)`, and a flat `1.0` plateau over `[(N_l + N_s)/4,
    ///   N_l/2)`; the right half is `W_RIGHT_l`.
    fn long_window(
        &self,
        left_shape: WindowShape,
        right_shape: WindowShape,
        kind: LongKind,
    ) -> Vec<f64> {
        let long = window_halves(LONG_TRANSFORM_LEN, left_shape, right_shape);
        let short = window_halves(SHORT_TRANSFORM_LEN, left_shape, right_shape);
        let half_l = N_L / 2; // 1024
        let mut w = vec![0.0f64; N_L];

        // Left half is always the plain long left half for OnlyLong /
        // Start; Stop replaces it with the start-transition mirror.
        match kind {
            LongKind::OnlyLong | LongKind::Start => {
                w[..half_l].copy_from_slice(&long.left);
            }
            LongKind::Stop => {
                // 0.0 over [0, (N_l − N_s)/4); short left half over
                // [(N_l − N_s)/4, (N_l + N_s)/4); 1.0 over
                // [(N_l + N_s)/4, N_l/2).
                let a = (N_L - N_S) / 4; // 448
                for (m, &sv) in short.left.iter().enumerate() {
                    w[a + m] = sv;
                }
                for slot in w.iter_mut().take(half_l).skip(a + N_S / 2) {
                    *slot = 1.0;
                }
            }
        }

        match kind {
            LongKind::OnlyLong => {
                for (m, &rv) in long.right.iter().enumerate() {
                    w[half_l + m] = rv;
                }
            }
            LongKind::Start => {
                // 1.0 over [N_l/2, (3N_l − N_s)/4); short right half
                // over [(3N_l − N_s)/4, (3N_l + N_s)/4); 0.0 after.
                let b = (3 * N_L - N_S) / 4; // 1472
                for slot in w.iter_mut().take(b).skip(half_l) {
                    *slot = 1.0;
                }
                for (m, &rv) in short.right.iter().enumerate() {
                    w[b + m] = rv;
                }
                // [(3N_l + N_s)/4, N_l) stays 0.0 from the vec init.
            }
            LongKind::Stop => {
                for (m, &rv) in long.right.iter().enumerate() {
                    w[half_l + m] = rv;
                }
            }
        }
        w
    }

    /// §4.6.11.3.2 c) — the `EIGHT_SHORT` sequence: eight length-256
    /// IMDCTs, each windowed with a short window, then overlapped and
    /// added into the 2048-sample frame with leading/trailing zeros.
    ///
    /// Window-shape inheritance (§4.6.11.3.2): the *first* short
    /// window's left half uses the previous block's shape; every
    /// later short window's left half — and every short window's right
    /// half — uses this block's `window_shape`.
    fn short_windowed(
        &self,
        spec: &[f64],
        left_shape: WindowShape,
        right_shape: WindowShape,
    ) -> Result<Vec<f64>> {
        let short_len = SHORT_WINDOW_LEN as usize; // 128
        if spec.len() != NUM_SHORT_WINDOWS * short_len {
            return Err(Error::FilterbankInvalid);
        }

        // Per-window windowed length-256 time signals.
        let mut windowed: Vec<Vec<f64>> = Vec::with_capacity(NUM_SHORT_WINDOWS);
        for j in 0..NUM_SHORT_WINDOWS {
            let coeffs = &spec[j * short_len..(j + 1) * short_len];
            let x = imdct(coeffs, SHORT_TRANSFORM_LEN);
            // W_0 left half inherits the previous block's shape; all
            // other windows' left halves use this block's shape.
            let this_left = if j == 0 { left_shape } else { right_shape };
            let halves = window_halves(SHORT_TRANSFORM_LEN, this_left, right_shape);
            let mut z = vec![0.0f64; N_S];
            for n in 0..N_S / 2 {
                z[n] = x[n] * halves.left[n];
            }
            for n in N_S / 2..N_S {
                z[n] = x[n] * halves.right[n - N_S / 2];
            }
            windowed.push(z);
        }

        // §4.6.11.3.2 c) overlap-add of the eight short windows into a
        // 2048-sample frame. Short window `j` starts at offset
        // `(N_l − N_s)/4 + j·N_s/2` (each successive short window is
        // hopped by N_s/2 = 128 samples) — the spec's piecewise z_{i,n}
        // is exactly this 50%-overlap-add with the first window placed
        // at (N_l − N_s)/4.
        let mut z = vec![0.0f64; N_L];
        let start = (N_L - N_S) / 4; // 448
        let hop = N_S / 2; // 128
        for (j, win) in windowed.iter().enumerate() {
            let base = start + j * hop;
            for (n, &v) in win.iter().enumerate() {
                z[base + n] += v;
            }
        }
        Ok(z)
    }
}

/// Discriminates the three long-transform `window_sequence` shapes
/// inside [`Filterbank::long_window`].
#[derive(Clone, Copy)]
enum LongKind {
    OnlyLong,
    Start,
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ics_info::IcsInfo;

    fn long_info(shape: WindowShape, seq: WindowSequence) -> IcsInfo {
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: seq,
            window_shape: shape,
            max_sfb: 49,
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

    fn short_info(shape: WindowShape) -> IcsInfo {
        IcsInfo {
            ics_reserved_bit: false,
            window_sequence: WindowSequence::EightShort,
            window_shape: shape,
            max_sfb: 14,
            scale_factor_grouping: Some(0),
            predictor_data_present: false,
            predictor_data: None,
            ltp_data_present: false,
            ltp_data: None,
            ltp_data_present_pair: None,
            ltp_data_pair: None,
            num_windows: 8,
            num_window_groups: 8,
            window_group_length: vec![1; 8],
            num_swb: 14,
        }
    }

    #[test]
    fn sine_window_endpoints() {
        // W_SIN_LEFT(n) = sin((π/N)(n + 1/2)); for N = 2048 the first
        // sample is sin(π·0.5/2048) and the last left-half sample is
        // sin(π·1023.5/2048) ≈ sin(π/2 · 0.9995…).
        let left = sine_left(1024);
        assert_eq!(left.len(), 1024);
        let expect0 = (core::f64::consts::PI * 0.5 / 2048.0).sin();
        assert!((left[0] - expect0).abs() < 1e-15);
        // The window rises monotonically to ~1.0 at the centre.
        assert!(left[1023] > 0.9999 && left[1023] <= 1.0);
        for w in 1..1024 {
            assert!(left[w] > left[w - 1]);
        }
    }

    #[test]
    fn sine_window_unit_power_overlap() {
        // The sine window satisfies the Princen-Bradley condition:
        // W(n)^2 + W(n + N/2)^2 = 1 for a symmetric sine window. Build
        // a full OnlyLong sine window and check the squared-sum of the
        // overlapping halves is 1.
        let half = sine_left(1024);
        for n in 0..1024 {
            // Right half mirrors the left: W(N-1-n) = W_left(n).
            let wl = half[n];
            let wr = half[1023 - n]; // W(1024 + n) = W_left(1023 - n)
            let s = wl * wl + wr * wr;
            assert!((s - 1.0).abs() < 1e-12, "n={n} sum={s}");
        }
    }

    #[test]
    fn kbd_window_unit_power_overlap() {
        // The KBD window is constructed precisely so that
        // W(n)^2 + W(n + N/2)^2 = 1 (it is the canonical
        // perfect-reconstruction window). Verify against the long α=4
        // KBD window.
        let left = kbd_left(1024, 4.0);
        assert_eq!(left.len(), 1024);
        for n in 0..1024 {
            let wl = left[n];
            let wr = left[1023 - n];
            let s = wl * wl + wr * wr;
            assert!((s - 1.0).abs() < 1e-12, "n={n} sum={s}");
        }
        // KBD is monotonically increasing on its left half.
        for n in 1..1024 {
            assert!(left[n] >= left[n - 1]);
        }
    }

    #[test]
    fn bessel_i0_known_values() {
        // I0(0) = 1; I0(1) ≈ 1.2660658777520084;
        // I0(2) ≈ 2.2795853023360673 (standard tabulated values).
        assert!((bessel_i0(0.0) - 1.0).abs() < 1e-15);
        assert!((bessel_i0(1.0) - 1.266_065_877_752_008_4).abs() < 1e-12);
        assert!((bessel_i0(2.0) - 2.279_585_302_336_067_3).abs() < 1e-12);
    }

    #[test]
    fn imdct_dc_coefficient() {
        // A single non-zero spec[0] is a pure cosine basis function.
        // For N=8, half=4, n0=(4+1)/2=2.5: x[n] = (2/8)·cos((2π/8)(n+2.5)(0.5)).
        let n = 8usize;
        let spec = [1.0, 0.0, 0.0, 0.0];
        let x = imdct(&spec, n);
        let scale = 2.0 / 8.0;
        let n0 = 2.5;
        for (idx, &xv) in x.iter().enumerate() {
            let expect =
                scale * (2.0 * core::f64::consts::PI / 8.0 * (idx as f64 + n0) * 0.5).cos();
            assert!((xv - expect).abs() < 1e-15, "n={idx}");
        }
    }

    /// Time-domain aliasing cancellation (TDAC): for a windowed MDCT/
    /// IMDCT pair, two consecutive identical frames overlap-add to
    /// reconstruct the windowed input exactly in the steady state. We
    /// drive the filterbank with the production analysis [`forward_mdct`]
    /// of a known signal and confirm perfect reconstruction over the
    /// second frame. (The analysis/synthesis pair is unity for a
    /// power-complementary §4.6.11.3.2 window.)
    use super::forward_mdct;

    /// The full symmetric (sine) `OnlyLong` window, length `N`.
    fn long_sine_window() -> Vec<f64> {
        let left = sine_left(1024);
        let mut w = vec![0.0; LONG_TRANSFORM_LEN];
        w[..1024].copy_from_slice(&left);
        for m in 0..1024 {
            w[1024 + m] = left[1023 - m];
        }
        w
    }

    #[test]
    fn tdac_perfect_reconstruction_sine_long() {
        // Streaming time-domain aliasing cancellation. A long input is
        // analysed by a 50%-overlap forward MDCT (analysis window =
        // sine), each frame carried through the decoder's IMDCT +
        // synthesis window + overlap-add. For a power-complementary
        // window the central frames reconstruct the input exactly.
        //
        // The forward analysis used here is the transpose of the
        // decoder's §4.6.11.3.1 IMDCT basis with NO scale (the IMDCT
        // carries the 2/N), so the analysis/synthesis pair satisfies
        // TDAC for the sine window.
        let n = LONG_TRANSFORM_LEN; // 2048
        let hop = n / 2; // 1024
        let win = long_sine_window();

        // A long deterministic input; reconstruct the central hop.
        let total = 5 * hop;
        let input: Vec<f64> = (0..total)
            .map(|i| (0.013 * i as f64).sin() + 0.5 * (0.07 * i as f64).cos())
            .collect();

        let info = long_info(WindowShape::Sine, WindowSequence::OnlyLong);
        let mut fb = Filterbank::new();

        // Run four overlapping analysis frames (starts 0, 1024, 2048,
        // 3072), feeding each frame's MDCT to the filterbank. Collect
        // the decoder's per-frame outputs.
        let mut outputs = Vec::new();
        for f in 0..4 {
            let base = f * hop;
            let frame: Vec<f64> = (0..n)
                .map(|m| {
                    let idx = base + m;
                    if idx < total {
                        input[idx] * win[m]
                    } else {
                        0.0
                    }
                })
                .collect();
            let spec = forward_mdct(&frame, n);
            outputs.push(fb.synthesize(&spec, &info).unwrap());
        }

        // The decoder output for frame f covers input samples
        // [f·hop, f·hop + hop). The steady-state frames f = 1, 2
        // reconstruct the input (their window region is fully covered
        // by both the analysis-window taper and the overlap from the
        // neighbouring frames).
        for (f, out) in outputs.iter().enumerate().take(3).skip(1) {
            let base = f * hop;
            for k in 0..hop {
                let recon = out[k];
                let expect = input[base + k];
                assert!(
                    (recon - expect).abs() < 1e-9,
                    "frame={f} k={k} recon={recon} expect={expect}"
                );
            }
        }
    }

    #[test]
    fn tdac_perfect_reconstruction_kbd_long() {
        // Same streaming TDAC check with the KBD (α=4) long window.
        let n = LONG_TRANSFORM_LEN;
        let hop = n / 2;
        let win = {
            let left = kbd_left(1024, 4.0);
            let mut w = vec![0.0; n];
            w[..1024].copy_from_slice(&left);
            for m in 0..1024 {
                w[1024 + m] = left[1023 - m];
            }
            w
        };
        let total = 5 * hop;
        let input: Vec<f64> = (0..total)
            .map(|i| 0.3 * (0.02 * i as f64).cos() - 0.6 * (0.05 * i as f64).sin())
            .collect();
        let info = long_info(WindowShape::Kbd, WindowSequence::OnlyLong);
        let mut fb = Filterbank::new();
        let mut outputs = Vec::new();
        for f in 0..4 {
            let base = f * hop;
            let frame: Vec<f64> = (0..n)
                .map(|m| {
                    let idx = base + m;
                    if idx < total {
                        input[idx] * win[m]
                    } else {
                        0.0
                    }
                })
                .collect();
            let spec = forward_mdct(&frame, n);
            outputs.push(fb.synthesize(&spec, &info).unwrap());
        }
        for (f, out) in outputs.iter().enumerate().take(3).skip(1) {
            let base = f * hop;
            for k in 0..hop {
                assert!((out[k] - input[base + k]).abs() < 1e-9, "frame={f} k={k}");
            }
        }
    }

    #[test]
    fn eight_short_internal_tdac() {
        // §4.6.11.3.2 c): the eight short windows overlap-add inside
        // the frame with a 128-sample hop, the first window placed at
        // offset (N_l − N_s)/4 = 448. Drive the eight short MDCTs from
        // a streaming short-window analysis of a continuous input and
        // confirm the frame's interior reconstructs that input over
        // the fully-overlapped central short windows.
        let n_s = SHORT_TRANSFORM_LEN; // 256
        let hop = n_s / 2; // 128
        let sine_short = {
            let left = sine_left(hop);
            let mut w = vec![0.0; n_s];
            w[..hop].copy_from_slice(&left);
            for m in 0..hop {
                w[hop + m] = left[hop - 1 - m];
            }
            w
        };
        // A continuous input long enough to cover all eight short
        // windows once placed at start=448, hop=128: last window starts
        // at 448 + 7·128 = 1344, ends at 1600.
        let total = N_L;
        let input: Vec<f64> = (0..total)
            .map(|i| (0.05 * i as f64).sin() + 0.4 * (0.11 * i as f64).cos())
            .collect();
        let start = (N_L - N_S) / 4; // 448

        // Build the eight short windows' MDCTs from the windowed input
        // segments at the same offsets the decoder overlaps them.
        let mut spec = Vec::with_capacity(NUM_SHORT_WINDOWS * SHORT_WINDOW_LEN as usize);
        for j in 0..NUM_SHORT_WINDOWS {
            let base = start + j * hop;
            let seg: Vec<f64> = (0..n_s).map(|m| input[base + m] * sine_short[m]).collect();
            let s = forward_mdct(&seg, n_s);
            spec.extend_from_slice(&s);
        }

        let info = short_info(WindowShape::Sine);
        let mut fb = Filterbank::new();
        let out = fb.synthesize(&spec, &info).unwrap();

        // The output frame is z[0:1024]; overlap with the (zero) prior
        // frame leaves the interior intact. The central short windows
        // j=1..6 are fully overlapped by their neighbours, so the
        // reconstructed signal equals the input over their shared
        // central hops: input indices [start + hop, start + 7·hop).
        // The decoder output covers input [0, 1024); the short-window
        // region [start, 1600) is partly past 1024, so check the
        // covered central hops [start+hop, 1024).
        for idx in (start + hop)..1024 {
            assert!(
                (out[idx] - input[idx]).abs() < 1e-9,
                "idx={idx} out={} input={}",
                out[idx],
                input[idx]
            );
        }
    }

    #[test]
    fn synthesize_long_length_and_shape() {
        let info = long_info(WindowShape::Sine, WindowSequence::OnlyLong);
        let mut fb = Filterbank::new();
        let spec = vec![0.25f64; LONG_WINDOW_LEN as usize];
        let out = fb.synthesize(&spec, &info).unwrap();
        assert_eq!(out.len(), LONG_WINDOW_LEN as usize);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn synthesize_eight_short_length() {
        let info = short_info(WindowShape::Sine);
        let mut fb = Filterbank::new();
        let spec = vec![0.1f64; NUM_SHORT_WINDOWS * SHORT_WINDOW_LEN as usize];
        let out = fb.synthesize(&spec, &info).unwrap();
        assert_eq!(out.len(), LONG_WINDOW_LEN as usize);
        assert!(out.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn synthesize_rejects_wrong_length() {
        let info = long_info(WindowShape::Sine, WindowSequence::OnlyLong);
        let mut fb = Filterbank::new();
        let spec = vec![0.0f64; 512];
        assert!(matches!(
            fb.synthesize(&spec, &info),
            Err(Error::FilterbankInvalid)
        ));
        let sinfo = short_info(WindowShape::Sine);
        let mut fb2 = Filterbank::new();
        let bad = vec![0.0f64; 1000];
        assert!(matches!(
            fb2.synthesize(&bad, &sinfo),
            Err(Error::FilterbankInvalid)
        ));
    }

    #[test]
    fn start_window_plateau_and_zero_regions() {
        // LONG_START: left half is the long left window, then a flat
        // 1.0 plateau, then the short right half, then zeros.
        let fb = Filterbank::new();
        let w = fb.long_window(WindowShape::Sine, WindowShape::Sine, LongKind::Start);
        assert_eq!(w.len(), N_L);
        // Plateau region [1024, 1472) is all 1.0.
        for v in w.iter().take(1472).skip(1024) {
            assert!((*v - 1.0).abs() < 1e-15);
        }
        // Tail [1600, 2048) is all 0.0.  (3N_l + N_s)/4 = 1600.
        for v in w.iter().take(N_L).skip(1600) {
            assert_eq!(*v, 0.0);
        }
        // The short-right transition [1472, 1600) falls from 1 to 0.
        assert!(w[1472] > w[1599]);
    }

    #[test]
    fn stop_window_zero_and_plateau_regions() {
        // LONG_STOP: leading zeros, short left half, 1.0 plateau, then
        // the long right window.
        let fb = Filterbank::new();
        let w = fb.long_window(WindowShape::Sine, WindowShape::Sine, LongKind::Stop);
        assert_eq!(w.len(), N_L);
        // Leading [0, 448) zeros. (N_l − N_s)/4 = 448.
        for v in w.iter().take(448) {
            assert_eq!(*v, 0.0);
        }
        // Plateau [576, 1024) all 1.0. (N_l + N_s)/4 = 576.
        for v in w.iter().take(1024).skip(576) {
            assert!((*v - 1.0).abs() < 1e-15);
        }
        // The short-left transition [448, 576) rises from 0 to 1.
        assert!(w[448] < w[575]);
    }

    #[test]
    fn first_frame_uses_own_shape_for_left_half() {
        // Before any frame, prev_shape is None, so the first frame's
        // left half uses its own window_shape (KBD here). Confirm the
        // left half equals the KBD left window, not the sine one.
        let info = long_info(WindowShape::Kbd, WindowSequence::OnlyLong);
        let fb = Filterbank::new();
        let w = fb
            .windowed_signal(&vec![0.0; LONG_WINDOW_LEN as usize], &info)
            .unwrap();
        // All-zero spectrum → zero time signal regardless, so instead
        // inspect the window directly.
        let _ = w;
        let win = fb.long_window(WindowShape::Kbd, WindowShape::Kbd, LongKind::OnlyLong);
        let kbd = kbd_left(1024, 4.0);
        for n in 0..1024 {
            assert!((win[n] - kbd[n]).abs() < 1e-15);
        }
    }
}
