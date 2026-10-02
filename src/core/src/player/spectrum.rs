//! FFT over output frames, producing normalised bins for the frontend
//! visualizers.
//!
//! Replaces the Web Audio `AnalyserNode` the WebView provided. Bins are shaped
//! to match what the visualizers already expect: byte-ranged magnitudes with
//! exponential smoothing, mapped quadratically across the lower spectrum.

use std::sync::Arc;

use realfft::{RealFftPlanner, RealToComplex};

/// Window size. 512 real samples yields 256 usable bins, matching the
/// `fftSize` the WebView analyser used.
const FFT_SIZE: usize = 512;

/// How many bars the visualizers draw.
pub const BAR_COUNT: usize = 64;

/// Matches the analyser's `smoothingTimeConstant`, so bars decay at the same
/// rate they did under Web Audio.
const SMOOTHING: f32 = 0.8;

pub struct SpectrumAnalyser {
    fft: Arc<dyn RealToComplex<f32>>,
    /// Mono mixdown of the most recent frames, kept as a sliding window.
    window: Vec<f32>,
    scratch_in: Vec<f32>,
    scratch_out: Vec<realfft::num_complex::Complex<f32>>,
    /// The transform's own working buffer. `process` allocates one of these on
    /// every call; owning it keeps `bars()` allocation-free per frame.
    scratch: Vec<realfft::num_complex::Complex<f32>>,
    /// Previous bar values, for exponential smoothing.
    smoothed: Vec<f32>,
    hann: Vec<f32>,
}

impl SpectrumAnalyser {
    pub fn new() -> Self {
        let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
        let scratch_in = fft.make_input_vec();
        let scratch_out = fft.make_output_vec();
        let scratch = fft.make_scratch_vec();
        let hann = (0..FFT_SIZE)
            .map(|i| {
                let t = i as f32 / (FFT_SIZE - 1) as f32;
                0.5 - 0.5 * (t * std::f32::consts::TAU).cos()
            })
            .collect();

        Self {
            fft,
            window: vec![0.0; FFT_SIZE],
            scratch_in,
            scratch_out,
            scratch,
            smoothed: vec![0.0; BAR_COUNT],
            hann,
        }
    }

    /// Feed interleaved samples; keeps only the most recent window.
    pub fn push(&mut self, samples: &[f32], channels: usize) {
        let channels = channels.max(1);
        let frames = samples.len() / channels;
        if frames == 0 {
            return;
        }

        let take = frames.min(FFT_SIZE);
        let skip = frames - take;

        self.window.copy_within(take.., 0);
        let base = FFT_SIZE - take;
        for f in 0..take {
            let start = (skip + f) * channels;
            let sum: f32 = samples[start..start + channels].iter().sum();
            self.window[base + f] = sum / channels as f32;
        }
    }

    /// Current spectrum as `BAR_COUNT` values in `0..=255`.
    ///
    /// The byte range and quadratic bin mapping mirror what the visualizers
    /// received from `getByteFrequencyData`, so their drawing code is unchanged.
    pub fn bars(&mut self) -> Vec<u8> {
        self.scratch_in.copy_from_slice(&self.window);
        for (s, w) in self.scratch_in.iter_mut().zip(&self.hann) {
            *s *= *w;
        }

        if self
            .fft
            .process_with_scratch(
                &mut self.scratch_in,
                &mut self.scratch_out,
                &mut self.scratch,
            )
            .is_err()
        {
            return vec![0; BAR_COUNT];
        }

        let usable = (self.scratch_out.len() as f32 * 0.85) as usize;
        let mut out = Vec::with_capacity(BAR_COUNT);

        for bar in 0..BAR_COUNT {
            let t0 = (bar as f32 / BAR_COUNT as f32).powi(2);
            let t1 = ((bar + 1) as f32 / BAR_COUNT as f32).powi(2);
            let lo = (t0 * usable as f32) as usize;
            let hi = ((t1 * usable as f32) as usize).max(lo + 1).min(usable);

            let peak = self.scratch_out[lo..hi]
                .iter()
                .map(|c| c.norm())
                .fold(0.0f32, f32::max);

            let db = 20.0 * (peak / (FFT_SIZE as f32 / 4.0)).max(1e-6).log10();
            let level = ((db + 70.0) / 70.0).clamp(0.0, 1.0);

            let prev = self.smoothed[bar];
            let value = prev * SMOOTHING + level * (1.0 - SMOOTHING);
            self.smoothed[bar] = value;
            out.push((value * 255.0).clamp(0.0, 255.0) as u8);
        }

        out
    }
}

impl Default for SpectrumAnalyser {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f32, rate: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|i| {
                let v = (i as f32 / rate * freq * std::f32::consts::TAU).sin();
                [v, v]
            })
            .collect()
    }

    #[test]
    fn silence_produces_no_energy() {
        let mut a = SpectrumAnalyser::new();
        a.push(&vec![0.0; FFT_SIZE * 2], 2);
        assert!(a.bars().iter().all(|b| *b == 0));
    }

    #[test]
    fn a_tone_puts_energy_in_the_low_bars() {
        let mut a = SpectrumAnalyser::new();
        for _ in 0..40 {
            a.push(&tone(440.0, 48_000.0, FFT_SIZE), 2);
            a.bars();
        }
        let bars = a.bars();

        let low: u32 = bars[..16].iter().map(|b| u32::from(*b)).sum();
        let high: u32 = bars[48..].iter().map(|b| u32::from(*b)).sum();
        assert!(low > high, "440 Hz should sit low: low={low} high={high}");
    }

    #[test]
    fn output_is_always_the_expected_width() {
        let mut a = SpectrumAnalyser::new();
        a.push(&tone(1_000.0, 48_000.0, 64), 2);
        assert_eq!(a.bars().len(), BAR_COUNT);
    }

    #[test]
    fn mono_input_is_accepted() {
        let mut a = SpectrumAnalyser::new();
        a.push(&vec![0.5; FFT_SIZE], 1);
        assert_eq!(a.bars().len(), BAR_COUNT);
    }

    #[test]
    fn a_short_push_keeps_earlier_audio() {
        let mut a = SpectrumAnalyser::new();
        for _ in 0..40 {
            a.push(&tone(440.0, 48_000.0, 32), 2);
            a.bars();
        }
        assert!(a.bars().iter().any(|b| *b > 0), "window lost its history");
    }
}
