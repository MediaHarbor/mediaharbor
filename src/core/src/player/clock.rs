//! Playback position derived from frames written to the output device.
//!
//! Counting frames the device has actually consumed, minus its output latency,
//! gives a position tied to what the listener is hearing rather than to how far
//! the decoder has run ahead.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Shared, lock-free playback clock.
///
/// The audio callback only ever adds frames, so updates stay wait-free on the
/// realtime thread; readers see a monotonically advancing position between
/// seeks.
#[derive(Debug, Default)]
pub struct PlaybackClock {
    frames: AtomicU64,
    rate: AtomicU64,
    /// Position the current segment started at, in microseconds, so a seek does
    /// not have to reset the frame counter.
    base_us: AtomicU64,
    /// Device output latency in microseconds, subtracted from the reported
    /// position so it reflects audio leaving the speakers.
    latency_us: AtomicU64,
}

impl PlaybackClock {
    pub fn new(rate: u32) -> Arc<Self> {
        let clock = Self::default();
        clock.rate.store(u64::from(rate.max(1)), Ordering::Relaxed);
        Arc::new(clock)
    }

    pub fn set_latency(&self, latency: std::time::Duration) {
        self.latency_us
            .store(latency.as_micros() as u64, Ordering::Relaxed);
    }

    /// Called from the audio callback once frames have been handed to the device.
    pub fn advance(&self, frames: u64) {
        self.frames.fetch_add(frames, Ordering::Relaxed);
    }

    /// Re-anchor to `seconds` after a seek and restart the frame count.
    pub fn reset_to(&self, seconds: f64) {
        let us = (seconds.max(0.0) * 1_000_000.0) as u64;
        self.base_us.store(us, Ordering::Relaxed);
        self.frames.store(0, Ordering::Relaxed);
    }

    /// Current playback position in seconds.
    pub fn position(&self) -> f64 {
        let rate = self.rate.load(Ordering::Relaxed).max(1);
        let frames = self.frames.load(Ordering::Relaxed);
        let base_us = self.base_us.load(Ordering::Relaxed);
        let latency_us = self.latency_us.load(Ordering::Relaxed);

        let played_us = frames.saturating_mul(1_000_000) / rate;
        let total_us = base_us.saturating_add(played_us).saturating_sub(latency_us);
        total_us as f64 / 1_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn position_tracks_frames_written() {
        let clock = PlaybackClock::new(48_000);
        assert_eq!(clock.position(), 0.0);

        clock.advance(48_000);
        assert!((clock.position() - 1.0).abs() < 1e-6);

        clock.advance(24_000);
        assert!((clock.position() - 1.5).abs() < 1e-6);
    }

    #[test]
    fn seeking_rebases_without_losing_the_offset() {
        let clock = PlaybackClock::new(44_100);
        clock.advance(44_100);

        clock.reset_to(90.0);
        assert!((clock.position() - 90.0).abs() < 1e-6);

        clock.advance(22_050);
        assert!((clock.position() - 90.5).abs() < 1e-6);
    }

    #[test]
    fn output_latency_is_subtracted() {
        let clock = PlaybackClock::new(48_000);
        clock.set_latency(Duration::from_millis(100));
        clock.advance(48_000);
        assert!((clock.position() - 0.9).abs() < 1e-6);
    }

    #[test]
    fn position_never_goes_negative_at_startup() {
        let clock = PlaybackClock::new(48_000);
        clock.set_latency(Duration::from_millis(250));
        clock.advance(480);
        assert_eq!(clock.position(), 0.0);
    }
}
