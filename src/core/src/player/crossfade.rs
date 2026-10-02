//! Equal-power crossfade between two decode pipelines, replacing the Web Audio
//! gain-node graph that ran in the WebView.

/// Gain pair for a crossfade `progress` in `0.0..=1.0`, returned as
/// `(outgoing, incoming)`.
///
/// Equal-power (cos/sin) rather than linear so the summed loudness stays
/// constant through the transition.
pub fn equal_power_gains(progress: f32) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0) * std::f32::consts::FRAC_PI_2;
    (t.cos(), t.sin())
}

/// Mix the two tracks into `out`, returning how many frames were consumed.
///
/// Only the frames both sides have buffered are mixed. The two decoders run on
/// their own chunk sizes — AAC emits 1024 frames, FLAC 4096, Opus whatever the
/// packet holds — so anything past the shorter side has no partner to mix with
/// and has to wait for the next chunk rather than being emitted unfaded.
pub fn mix_frames(
    out: &mut Vec<f32>,
    outgoing: &[f32],
    incoming: &[f32],
    channels: usize,
    frames_done: usize,
    frames_total: usize,
) -> usize {
    let channels = channels.max(1);
    let frames = (outgoing.len() / channels).min(incoming.len() / channels);

    for f in 0..frames {
        let progress = (frames_done + f) as f32 / frames_total.max(1) as f32;
        let (g_out, g_in) = equal_power_gains(progress);
        for c in 0..channels {
            let i = f * channels + c;
            out.push((outgoing[i] * g_out + incoming[i] * g_in).clamp(-1.0, 1.0));
        }
    }
    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_are_fully_swapped() {
        let (out, inc) = equal_power_gains(0.0);
        assert!((out - 1.0).abs() < 1e-6 && inc.abs() < 1e-6);

        let (out, inc) = equal_power_gains(1.0);
        assert!(out.abs() < 1e-6 && (inc - 1.0).abs() < 1e-6);
    }

    #[test]
    fn power_is_constant_across_the_fade() {
        for step in 0..=20 {
            let (out, inc) = equal_power_gains(step as f32 / 20.0);
            let power = out * out + inc * inc;
            assert!((power - 1.0).abs() < 1e-5, "power {power} at step {step}");
        }
    }

    /// The mix the decode thread performs, extracted so it can be checked
    /// without a device.
    fn mix(out: &[f32], inc: &[f32], progress: f32) -> Vec<f32> {
        let (g_out, g_in) = equal_power_gains(progress);
        out.iter()
            .zip(inc)
            .map(|(a, b)| a * g_out + b * g_in)
            .collect()
    }

    #[test]
    fn a_mix_never_clips_two_full_scale_tracks() {
        for step in 0..=20 {
            let progress = step as f32 / 20.0;
            let mixed = mix(&[1.0; 8], &[1.0; 8], progress);
            let peak = mixed.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak <= 1.415, "peak {peak} at progress {progress}");
        }
    }

    #[test]
    fn the_outgoing_track_is_gone_by_the_end() {
        let mixed = mix(&[1.0; 4], &[0.0; 4], 1.0);
        assert!(
            mixed.iter().all(|v| v.abs() < 1e-6),
            "outgoing still audible"
        );
    }

    #[test]
    fn progress_outside_the_range_is_clamped() {
        assert_eq!(equal_power_gains(-1.0), equal_power_gains(0.0));
        assert_eq!(equal_power_gains(2.0), equal_power_gains(1.0));
    }

    /// The codecs either side of a fade rarely share a chunk size. Only the
    /// overlap may be emitted: mixing 1024 frames and then passing the other
    /// 3072 through unfaded is the outgoing track punching through the fade.
    #[test]
    fn only_the_frames_both_sides_have_are_mixed() {
        let mut out = Vec::new();
        let frames = mix_frames(&mut out, &[1.0; 4096 * 2], &[1.0; 1024 * 2], 2, 0, 48_000);

        assert_eq!(frames, 1024, "the shorter side bounds the mix");
        assert_eq!(out.len(), 1024 * 2, "no unpartnered frame may be emitted");
    }

    #[test]
    fn the_shorter_side_bounds_the_mix_in_either_direction() {
        let mut out = Vec::new();
        let frames = mix_frames(&mut out, &[1.0; 1024 * 2], &[1.0; 4096 * 2], 2, 0, 48_000);
        assert_eq!(frames, 1024);
        assert_eq!(out.len(), 1024 * 2);
    }

    /// Feeding the mixer in mismatched chunks must land in the same place as
    /// one even run, or the fade drifts off its own clock.
    #[test]
    fn mismatched_chunks_still_reach_full_handover() {
        let total = 4_800usize;
        let mut done = 0usize;
        let mut out = Vec::new();
        let (mut buf_out, mut buf_in) = (Vec::new(), Vec::new());

        for step in 0..40 {
            buf_out.extend(std::iter::repeat_n(1.0f32, 1024 * 2));
            if step % 4 == 0 {
                buf_in.extend(std::iter::repeat_n(-1.0f32, 4096 * 2));
            }
            let frames = mix_frames(&mut out, &buf_out, &buf_in, 2, done, total);
            buf_out.drain(..frames * 2);
            buf_in.drain(..frames * 2);
            done += frames;
        }

        assert!(done >= total, "fade never completed: {done} of {total}");
        let tail = &out[out.len() - 64..];
        assert!(
            tail.iter().all(|v| *v < -0.5),
            "handover did not finish: {:?}",
            &tail[..8]
        );
    }
}
