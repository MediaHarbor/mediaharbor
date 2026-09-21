//! Rate and channel conversion between a decoded track and the open device.
//!
//! `AudioOutput::open` asks for the source's own format and only falls back to
//! the device default when that is unsupported, so on most systems this is a
//! pass-through. It is not one when the device refuses the source rate, when the
//! device default carries a different channel count, when HE-AAC corrects its
//! rate after the first frame, or when a crossfade pulls in a track at another
//! rate — Opus is always 48 kHz while most albums are 44.1 kHz. Handing those
//! samples straight to the device plays them at the wrong speed and starves the
//! ring, so they are converted here instead.

/// Half-length of the interpolation kernel, in input frames at unity ratio.
/// Widened on downsampling so the lower cutoff keeps the same number of zero
/// crossings inside the window.
const HALF_TAPS: usize = 24;

/// Kernel samples stored per input frame. The kernel is smooth at this scale,
/// so reading it with linear interpolation costs nothing audible.
const SUBPHASES: usize = 128;

/// Cutoff as a fraction of the lower of the two Nyquist frequencies. Backing
/// off keeps the window's transition band below Nyquist — the alternative is
/// aliasing folded back into the audible range.
const ROLLOFF: f64 = 0.92;

/// Converts interleaved f32 chunks to one fixed output format.
///
/// State is carried between chunks: the filter needs the frames either side of
/// each output position, so a resampler that restarted per chunk would put a
/// discontinuity at every packet boundary.
pub struct Resampler {
    out_rate: u32,
    out_channels: usize,
    in_rate: u32,
    in_channels: usize,
    /// Half of the symmetric windowed sinc, plus a trailing zero so reading the
    /// last entry can still interpolate towards its neighbour.
    kernel: Vec<f32>,
    /// Kernel support either side of an output position, in input frames.
    half_width: f64,
    /// Input frames consumed per output frame.
    step: f64,
    /// Input frames already mapped to `out_channels`, awaiting filtering.
    history: Vec<f32>,
    /// Position of the next output frame within `history`, in input frames.
    pos: f64,
    /// Kernel weights for the frame being produced, reused across channels.
    taps: Vec<f32>,
    out: Vec<f32>,
}

impl Resampler {
    pub fn new(out_rate: u32, out_channels: usize) -> Self {
        Self {
            out_rate: out_rate.max(1),
            out_channels: out_channels.max(1),
            in_rate: 0,
            in_channels: 0,
            kernel: Vec::new(),
            half_width: HALF_TAPS as f64,
            step: 1.0,
            history: Vec::new(),
            pos: 0.0,
            taps: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Convert `samples` to the output format.
    ///
    /// Returns the input untouched when no conversion is needed, so the common
    /// case — a device opened at exactly the source's format — costs nothing.
    pub fn process<'a>(&'a mut self, samples: &'a [f32], rate: u32, channels: usize) -> &'a [f32] {
        let rate = rate.max(1);
        let channels = channels.max(1);

        if rate == self.out_rate && channels == self.out_channels {
            return samples;
        }
        if rate != self.in_rate || channels != self.in_channels {
            self.configure(rate, channels);
        }

        self.out.clear();
        if rate == self.out_rate {
            map_channels(samples, channels, self.out_channels, &mut self.out);
            return &self.out;
        }

        map_channels(samples, channels, self.out_channels, &mut self.history);
        self.filter();
        &self.out
    }

    /// Drop the filter history, for a seek or a new source.
    ///
    /// Without this the frames either side of the seek point would be blended
    /// together, which is audible as a click at the new position.
    pub fn reset(&mut self) {
        self.history.clear();
        let prime = self.half_width.ceil() as usize;
        self.history.resize(prime * self.out_channels, 0.0);
        self.pos = prime as f64;
    }

    fn configure(&mut self, in_rate: u32, in_channels: usize) {
        self.in_rate = in_rate;
        self.in_channels = in_channels;
        self.step = f64::from(in_rate) / f64::from(self.out_rate);

        let band = (f64::from(self.out_rate) / f64::from(in_rate)).min(1.0);
        self.half_width = HALF_TAPS as f64 / band;
        build_kernel(band * ROLLOFF, self.half_width, &mut self.kernel);
        self.reset();
    }

    /// Emit every output frame whose kernel support has arrived, then discard
    /// the history that has fallen out of reach.
    fn filter(&mut self) {
        let Self {
            out_channels,
            kernel,
            half_width,
            step,
            history,
            pos,
            taps,
            out,
            ..
        } = self;
        let channels = *out_channels;
        let frames = history.len() / channels;
        let kernel_scale = if kernel.len() >= 2 {
            (kernel.len() - 2) as f64 / *half_width
        } else {
            0.0
        };

        while (*pos + *half_width).floor() < frames as f64 {
            let first = (*pos - *half_width).ceil().max(0.0) as usize;
            let last = ((*pos + *half_width).floor() as usize).min(frames - 1);

            taps.clear();
            let mut norm = 0.0f32;
            for k in first..=last {
                let weight = kernel_at(kernel, *half_width, kernel_scale, (*pos - k as f64).abs());
                norm += weight;
                taps.push(weight);
            }
            let gain = if norm.abs() > 1e-6 { 1.0 / norm } else { 1.0 };

            for c in 0..channels {
                let mut acc = 0.0f32;
                for (i, weight) in taps.iter().enumerate() {
                    acc += history[(first + i) * channels + c] * weight;
                }
                out.push(acc * gain);
            }
            *pos += *step;
        }

        let spent = (*pos - *half_width).floor().max(0.0) as usize;
        if spent > 0 {
            history.drain(..spent * channels);
            *pos -= spent as f64;
        }
    }
}

/// Sample the stored half-kernel at `u` input frames from the centre.
///
/// `kernel_scale` is `(kernel.len() - 2) / half_width`, which the caller hoists
/// out of the tap loop — it is fixed for the whole stream.
fn kernel_at(kernel: &[f32], half_width: f64, kernel_scale: f64, u: f64) -> f32 {
    if u >= half_width || kernel.len() < 2 {
        return 0.0;
    }
    let scaled = u * kernel_scale;
    let index = scaled as usize;
    let frac = (scaled - index as f64) as f32;
    kernel[index] + (kernel[index + 1] - kernel[index]) * frac
}

fn build_kernel(cutoff: f64, half_width: f64, table: &mut Vec<f32>) {
    let points = HALF_TAPS * SUBPHASES + 1;
    table.clear();
    table.reserve(points + 1);
    for i in 0..points {
        let u = i as f64 / (points - 1) as f64 * half_width;
        let window = blackman_harris(0.5 + u / (2.0 * half_width));
        table.push((cutoff * sinc(cutoff * u) * window) as f32);
    }
    table.push(0.0);
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        let pix = std::f64::consts::PI * x;
        pix.sin() / pix
    }
}

/// Blackman–Harris over `t` in `0.0..=1.0`. Its ~-92 dB sidelobes put the
/// stopband well under the noise floor of any source the player decodes.
fn blackman_harris(t: f64) -> f64 {
    let x = 2.0 * std::f64::consts::PI * t;
    0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()
}

/// Append `src` to `dst`, rearranged from `in_ch` to `out_ch` channels.
fn map_channels(src: &[f32], in_ch: usize, out_ch: usize, dst: &mut Vec<f32>) {
    if in_ch == out_ch {
        dst.extend_from_slice(src);
        return;
    }

    let frames = src.len() / in_ch;
    dst.reserve(frames * out_ch);

    if in_ch == 1 {
        for frame in src.iter().take(frames) {
            for c in 0..out_ch {
                dst.push(if c < 2 { *frame } else { 0.0 });
            }
        }
        return;
    }

    if out_ch == 1 {
        for f in 0..frames {
            let sum: f32 = src[f * in_ch..(f + 1) * in_ch].iter().sum();
            dst.push(sum / in_ch as f32);
        }
        return;
    }

    if out_ch > in_ch {
        for f in 0..frames {
            for c in 0..out_ch {
                dst.push(if c < in_ch { src[f * in_ch + c] } else { 0.0 });
            }
        }
        return;
    }

    for f in 0..frames {
        for c in 0..out_ch {
            let mut acc = 0.0;
            let mut k = c;
            while k < in_ch {
                acc += src[f * in_ch + k];
                k += out_ch;
            }
            dst.push(acc / ((in_ch - c).div_ceil(out_ch)) as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn tone(freq: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| (2.0 * PI * freq * n as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    /// Feed a signal in packet-sized chunks, as the decode thread does.
    fn run(resampler: &mut Resampler, input: &[f32], rate: u32, channels: usize) -> Vec<f32> {
        let mut out = Vec::new();
        for chunk in input.chunks(1024 * channels) {
            out.extend_from_slice(resampler.process(chunk, rate, channels));
        }
        out
    }

    fn rms(xs: &[f32]) -> f64 {
        if xs.is_empty() {
            return 0.0;
        }
        (xs.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
    }

    #[test]
    fn a_matching_format_is_handed_straight_through() {
        let mut r = Resampler::new(48_000, 2);
        let input = tone(440.0, 48_000, 512);
        let out = r.process(&input, 48_000, 2);
        assert_eq!(
            out,
            &input[..],
            "no conversion needed, so nothing may change"
        );
    }

    /// The whole point: a 44.1 kHz album on a device opened at 48 kHz must come
    /// back as the same tone, not a transposed one.
    #[test]
    fn a_tone_survives_44k_to_48k_intact() {
        let mut r = Resampler::new(48_000, 1);
        let out = run(&mut r, &tone(1_000.0, 44_100, 44_100), 44_100, 1);

        let ideal = tone(1_000.0, 48_000, out.len());
        let skip = 256;
        let (mut noise, mut signal) = (0.0f64, 0.0f64);
        for n in skip..out.len() {
            noise += (f64::from(out[n]) - f64::from(ideal[n])).powi(2);
            signal += f64::from(ideal[n]).powi(2);
        }
        let snr = 10.0 * (signal / noise).log10();
        assert!(snr > 60.0, "only {snr:.1} dB of signal-to-noise");
    }

    /// A chunk boundary is where a stateless resampler clicks: the seam lands
    /// mid-waveform. Comparing chunked output against one-shot output isolates
    /// exactly that.
    #[test]
    fn chunk_boundaries_are_seamless() {
        let input = tone(1_000.0, 44_100, 20_000);

        let mut whole = Resampler::new(48_000, 1);
        let one_shot = whole.process(&input, 44_100, 1).to_vec();

        let mut split = Resampler::new(48_000, 1);
        let mut chunked = Vec::new();
        for chunk in input.chunks(137) {
            chunked.extend_from_slice(split.process(chunk, 44_100, 1));
        }

        assert_eq!(chunked.len(), one_shot.len());
        let worst = chunked
            .iter()
            .zip(&one_shot)
            .fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
        assert!(
            worst < 1e-5,
            "seam error {worst:e} — the split output differs"
        );
    }

    #[test]
    fn output_length_follows_the_ratio() {
        let mut up = Resampler::new(48_000, 1);
        let out = run(&mut up, &tone(440.0, 44_100, 44_100), 44_100, 1);
        let expected = 48_000.0;
        assert!(
            (out.len() as f64 - expected).abs() < 64.0,
            "{} frames for one second at 48 kHz",
            out.len()
        );

        let mut down = Resampler::new(44_100, 1);
        let out = run(&mut down, &tone(440.0, 48_000, 48_000), 48_000, 1);
        assert!(
            (out.len() as f64 - 44_100.0).abs() < 64.0,
            "{} frames for one second at 44.1 kHz",
            out.len()
        );
    }

    /// Content above the new Nyquist has to be filtered out. Left in, it folds
    /// back into the audible range as a tone that was never in the recording.
    #[test]
    fn content_above_the_new_nyquist_is_removed_not_folded() {
        let mut r = Resampler::new(8_000, 1);
        let out = run(&mut r, &tone(18_000.0, 48_000, 48_000), 48_000, 1);

        let settled = &out[256..];
        assert!(
            rms(settled) < 0.01,
            "18 kHz survived downsampling at {:.4} rms — that is an alias",
            rms(settled)
        );
    }

    #[test]
    fn a_rate_change_mid_stream_is_adopted() {
        let mut r = Resampler::new(48_000, 1);
        let converted = r.process(&tone(1_000.0, 44_100, 4_096), 44_100, 1).len();
        assert!(converted > 4_096, "44.1 kHz must be stretched to 48 kHz");

        let native = tone(1_000.0, 48_000, 4_096);
        assert_eq!(
            r.process(&native, 48_000, 1).len(),
            native.len(),
            "a stream that corrects its rate must stop being resampled"
        );
    }

    #[test]
    fn mono_plays_out_of_both_speakers() {
        let mut r = Resampler::new(48_000, 2);
        let out = r.process(&[0.5, -0.25], 48_000, 1);
        assert_eq!(out, &[0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn stereo_folds_down_to_mono() {
        let mut r = Resampler::new(48_000, 1);
        let out = r.process(&[1.0, 0.0, 0.5, 0.5], 48_000, 2);
        assert_eq!(out, &[0.5, 0.5]);
    }

    #[test]
    fn a_stereo_source_on_a_surround_device_stays_in_front() {
        let mut r = Resampler::new(48_000, 6);
        let out = r.process(&[1.0, -1.0], 48_000, 2);
        assert_eq!(out, &[1.0, -1.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn surround_folds_down_without_losing_a_channel() {
        let mut r = Resampler::new(48_000, 2);
        let out = r.process(&[0.0, 0.0, 0.9, 0.0, 0.3, 0.6], 48_000, 6);
        assert!(out[0] > 0.0 && out[1] > 0.0, "content was dropped: {out:?}");
    }

    #[test]
    fn a_reset_drops_the_previous_position() {
        let mut r = Resampler::new(48_000, 1);
        r.process(&[1.0; 4_096], 44_100, 1);
        r.reset();

        let out = r.process(&[0.0; 4_096], 44_100, 1).to_vec();
        let worst = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(
            worst < 1e-6,
            "history from before the seek bled through: {worst}"
        );
    }

    #[test]
    fn silence_in_is_silence_out() {
        let mut r = Resampler::new(48_000, 2);
        let out = run(&mut r, &vec![0.0f32; 8_192], 44_100, 2);
        assert!(out.iter().all(|s| *s == 0.0));
        assert!(!out.is_empty());
    }
}
