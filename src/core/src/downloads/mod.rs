pub mod dedup;
pub mod native_common;
pub mod yt_dlp;

use std::sync::atomic::AtomicU64;

#[derive(Debug, Default)]
pub struct ByteProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

/// Progress across a multi-item download: where the batch as a whole is, and what
/// item it is on. Every downloader — the Python CLIs, the native Widevine engines
/// and the streaming pipeline — reports through this one shape.
#[derive(Debug, Clone, Default)]
pub struct BatchProgress {
    pub completed: u32,
    pub total: u32,
    pub current_track: String,
    pub percent: f32,
    pub speed: Option<f64>,
    pub eta: Option<f64>,
    /// Set once the service reveals what it really served, so the download row can
    /// stop showing the tier that was merely requested.
    pub quality: Option<String>,
}

impl BatchProgress {
    /// Progress after item `i` of `total` has settled. Percent is derived from
    /// the index so the batch bar advances even when an item is skipped.
    pub fn item_settled(i: usize, total: usize, current_track: String, completed: u32) -> Self {
        Self {
            percent: ((i + 1) as f32 / total as f32) * 100.0,
            current_track,
            completed,
            total: total as u32,
            ..Default::default()
        }
    }

    /// Progress as item `i` of `total` begins. The percent is the batch position
    /// *before* this item's own work, so the bar never runs ahead of it.
    pub fn item_started(i: usize, total: usize, current_track: String, completed: u32) -> Self {
        Self {
            percent: (i as f32 / total as f32) * 100.0,
            current_track,
            completed,
            total: total as u32,
            ..Default::default()
        }
    }

    /// Progress at an explicit percent, for the points inside one item's work that
    /// land on a known ceiling rather than on an item boundary.
    pub fn at(percent: f32, current_track: String, completed: u32, total: u32) -> Self {
        Self {
            percent,
            current_track,
            completed,
            total,
            ..Default::default()
        }
    }
}

/// Transfer rate over the last tick rather than over the whole run.
///
/// A cumulative `bytes / elapsed` counts every second spent on things that move no
/// bytes — license waits, cover fetches, tagging, ffmpeg — so on anything but a single
/// uninterrupted stream it decays towards zero and reports single-digit B/s.
pub struct SpeedMeter {
    last_bytes: u64,
    last_at: Option<std::time::Instant>,
    ema: Option<f64>,
}

impl Default for SpeedMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl SpeedMeter {
    /// Weight of the newest sample. Ticks arrive every 300 ms, so this settles in
    /// roughly a second while still absorbing one slow chunk.
    const ALPHA: f64 = 0.35;

    pub fn new() -> Self {
        Self {
            last_bytes: 0,
            last_at: None,
            ema: None,
        }
    }

    /// Feeds the running byte total and returns the current rate, or `None` until two
    /// samples have been seen.
    pub fn sample(&mut self, done: u64) -> Option<f64> {
        self.sample_at(done, std::time::Instant::now())
    }

    fn sample_at(&mut self, done: u64, now: std::time::Instant) -> Option<f64> {
        let Some(prev_at) = self.last_at.replace(now) else {
            self.last_bytes = done;
            return None;
        };
        let secs = now.duration_since(prev_at).as_secs_f64();
        let delta = done.saturating_sub(self.last_bytes);
        self.last_bytes = done;
        if secs <= 0.0 {
            return self.ema;
        }
        let instant = delta as f64 / secs;
        self.ema = Some(match self.ema {
            Some(prev) => prev + Self::ALPHA * (instant - prev),
            None => instant,
        });
        self.ema
    }
}

pub fn format_speed(bytes_per_sec: f64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    if bytes_per_sec >= MB {
        format!("{:.1} MB/s", bytes_per_sec / MB)
    } else if bytes_per_sec >= KB {
        format!("{:.0} KB/s", bytes_per_sec / KB)
    } else {
        format!("{:.0} B/s", bytes_per_sec)
    }
}

pub fn format_eta(secs: f64) -> String {
    let total = secs.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_units() {
        assert_eq!(format_speed(512.0), "512 B/s");
        assert_eq!(format_speed(2048.0), "2 KB/s");
        assert_eq!(format_speed(3.0 * 1024.0 * 1024.0), "3.0 MB/s");
    }

    #[test]
    fn speed_meter_reports_the_recent_rate_not_the_lifetime_average() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut m = SpeedMeter::new();
        assert_eq!(m.sample_at(0, t0), None);
        let mb = 1024.0 * 1024.0;
        let r1 = m
            .sample_at(1024 * 1024, t0 + Duration::from_secs(1))
            .unwrap();
        assert!((r1 - mb).abs() < 1.0, "got {r1}");

        let mut idle = 0.0;
        for i in 2..=11 {
            idle = m
                .sample_at(1024 * 1024, t0 + Duration::from_secs(i))
                .unwrap();
        }
        assert!(idle < r1);

        let mut resumed = idle;
        for i in 1..=10 {
            resumed = m
                .sample_at(
                    1024 * 1024 + i * 1024 * 1024,
                    t0 + Duration::from_secs(11 + i),
                )
                .unwrap();
        }
        assert!(resumed > mb * 0.8, "got {resumed}");
    }

    #[test]
    fn speed_meter_survives_a_counter_reset() {
        use std::time::{Duration, Instant};
        let t0 = Instant::now();
        let mut m = SpeedMeter::new();
        m.sample_at(5_000_000, t0);
        let r = m.sample_at(1_000, t0 + Duration::from_secs(1)).unwrap();
        assert!(r >= 0.0);
    }

    #[test]
    fn eta_formatting() {
        assert_eq!(format_eta(0.0), "0:00");
        assert_eq!(format_eta(9.0), "0:09");
        assert_eq!(format_eta(75.0), "1:15");
        assert_eq!(format_eta(3661.0), "1:01:01");
    }
}
