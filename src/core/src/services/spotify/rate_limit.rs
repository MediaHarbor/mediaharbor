use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::errors::{MhError, MhResult};
use crate::services::common::ids::now_secs;

#[derive(Clone, Debug)]
pub struct LimiterConfig {
    pub capacity: f64,
    pub refill_per_sec: f64,
    pub refill_floor: f64,
    pub aimd_shrink: f64,
    pub aimd_grow_factor: f64,
    pub aimd_grow_after_secs: u64,
}

impl LimiterConfig {
    pub fn shipped_default() -> Self {
        Self {
            capacity: 12.0,
            refill_per_sec: 1.0 / 3.0,
            refill_floor: 1.0 / 60.0,
            aimd_shrink: 0.5,
            aimd_grow_factor: 1.5,
            aimd_grow_after_secs: 300,
        }
    }
}

impl Default for LimiterConfig {
    fn default() -> Self {
        Self::shipped_default()
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LearnedState {
    pub refill_per_sec: f64,
    pub saved_at_unix: i64,
    pub last_429_age_secs: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct PaceEvent {
    pub waiting_secs: f64,
    pub reason: &'static str,
}

struct Inner {
    capacity: f64,
    tokens: f64,
    refill_per_sec: f64,
    refill_floor: f64,
    refill_ceiling: f64,
    last_refill: Instant,
    last_429: Option<Instant>,
    last_grow: Instant,
    aimd_shrink: f64,
    aimd_grow_factor: f64,
    aimd_grow_after: Duration,
    dirty: bool,
}

impl Inner {
    fn refill(&mut self, now: Instant) {
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed > 0.0 {
            self.tokens = (self.tokens + elapsed * self.refill_per_sec).min(self.capacity);
            self.last_refill = now;
        }
    }

    fn maybe_grow(&mut self, now: Instant) {
        if self.last_429.is_some()
            && self.refill_per_sec < self.refill_ceiling
            && now.duration_since(self.last_grow) >= self.aimd_grow_after
        {
            self.refill_per_sec =
                (self.refill_per_sec * self.aimd_grow_factor).min(self.refill_ceiling);
            self.last_grow = now;
            self.dirty = true;
        }
    }
}

type Notifier = Box<dyn Fn(PaceEvent) + Send + Sync>;

pub struct LicenseRateLimiter {
    inner: Mutex<Inner>,
    notify: Mutex<Option<Notifier>>,
}

impl LicenseRateLimiter {
    pub fn from_persisted(cfg: LimiterConfig, learned: Option<LearnedState>) -> Self {
        let now = Instant::now();
        let ceiling = cfg.refill_per_sec;

        let (refill_per_sec, last_429) = match learned {
            Some(state) => {
                let rate = state.refill_per_sec.clamp(cfg.refill_floor, ceiling);
                let last = state
                    .last_429_age_secs
                    .map(|age| now.checked_sub(Duration::from_secs(age)).unwrap_or(now));
                (rate, last)
            }
            None => (cfg.refill_per_sec, None),
        };

        let inner = Inner {
            capacity: cfg.capacity,
            tokens: cfg.capacity,
            refill_per_sec,
            refill_floor: cfg.refill_floor,
            refill_ceiling: ceiling,
            last_refill: now,
            last_429,
            last_grow: now,
            aimd_shrink: cfg.aimd_shrink,
            aimd_grow_factor: cfg.aimd_grow_factor,
            aimd_grow_after: Duration::from_secs(cfg.aimd_grow_after_secs),
            dirty: false,
        };

        Self {
            inner: Mutex::new(inner),
            notify: Mutex::new(None),
        }
    }

    pub fn new() -> Self {
        Self::from_persisted(LimiterConfig::shipped_default(), None)
    }

    pub fn set_notifier(&self, f: Box<dyn Fn(PaceEvent) + Send + Sync>) {
        if let Ok(mut guard) = self.notify.lock() {
            *guard = Some(f);
        }
    }

    fn fire_notify(&self, ev: PaceEvent) {
        if let Ok(guard) = self.notify.lock() {
            if let Some(cb) = guard.as_ref() {
                cb(ev);
            }
        }
    }

    pub async fn acquire(&self, cancel: Option<&AtomicBool>) -> MhResult<()> {
        loop {
            if let Some(flag) = cancel {
                if flag.load(Ordering::Relaxed) {
                    return Err(MhError::Cancelled);
                }
            }

            let wait = {
                let mut inner = match self.inner.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                let now = Instant::now();
                inner.refill(now);
                inner.maybe_grow(now);

                if inner.tokens >= 1.0 {
                    inner.tokens -= 1.0;
                    return Ok(());
                }
                let deficit = 1.0 - inner.tokens;
                (deficit / inner.refill_per_sec.max(f64::MIN_POSITIVE)).max(0.0)
            };

            self.fire_notify(PaceEvent {
                waiting_secs: wait,
                reason: "rate-limit",
            });

            let mut remaining = Duration::from_secs_f64(wait);
            let chunk = Duration::from_millis(500);
            while !remaining.is_zero() {
                if let Some(flag) = cancel {
                    if flag.load(Ordering::Relaxed) {
                        return Err(MhError::Cancelled);
                    }
                }
                let step = remaining.min(chunk);
                tokio::time::sleep(step).await;
                remaining = remaining.checked_sub(step).unwrap_or(Duration::ZERO);
            }
        }
    }

    pub fn on_rate_limited(&self, retry_after_secs: u64) -> u64 {
        {
            let mut inner = match self.inner.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            inner.tokens = 0.0;
            let shrunk = inner.refill_per_sec * inner.aimd_shrink;
            inner.refill_per_sec = shrunk.max(inner.refill_floor);
            inner.last_429 = Some(Instant::now());
            inner.last_grow = Instant::now();
            inner.dirty = true;
        }
        retry_after_secs.max(1)
    }

    pub fn take_if_dirty(&self) -> Option<LearnedState> {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if !inner.dirty {
            return None;
        }
        inner.dirty = false;

        let now = Instant::now();
        let saved_at_unix = now_secs() as i64;
        let last_429_age_secs = inner.last_429.map(|t| now.duration_since(t).as_secs());

        Some(LearnedState {
            refill_per_sec: inner.refill_per_sec,
            saved_at_unix,
            last_429_age_secs,
        })
    }

    #[cfg(test)]
    pub(crate) fn refill_per_sec(&self) -> f64 {
        match self.inner.lock() {
            Ok(g) => g.refill_per_sec,
            Err(p) => p.into_inner().refill_per_sec,
        }
    }

    #[cfg(test)]
    fn tokens(&self) -> f64 {
        match self.inner.lock() {
            Ok(g) => g.tokens,
            Err(p) => p.into_inner().tokens,
        }
    }

    #[cfg(test)]
    fn grow_at(&self, now: Instant) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        inner.maybe_grow(now);
    }

    #[cfg(test)]
    fn arm_recovery(&self) {
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let past = Instant::now() - (inner.aimd_grow_after + Duration::from_secs(1));
        inner.last_429 = Some(past);
        inner.last_grow = past;
    }
}

impl Default for LicenseRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn cfg() -> LimiterConfig {
        LimiterConfig {
            capacity: 3.0,
            refill_per_sec: 1.0,
            refill_floor: 0.125,
            aimd_shrink: 0.5,
            aimd_grow_factor: 2.0,
            aimd_grow_after_secs: 10,
        }
    }

    #[tokio::test]
    async fn full_bucket_bursts_to_capacity() {
        let lim = LicenseRateLimiter::from_persisted(cfg(), None);
        for _ in 0..3 {
            lim.acquire(None).await.expect("burst within capacity");
        }
    }

    #[tokio::test]
    async fn bucket_empties_after_capacity() {
        let lim = LicenseRateLimiter::from_persisted(cfg(), None);
        for _ in 0..3 {
            lim.acquire(None).await.unwrap();
        }
        assert!(lim.tokens() < 1.0);
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            lim.acquire(Some(&cancel)).await,
            Err(MhError::Cancelled)
        ));
    }

    #[test]
    fn on_rate_limited_shrinks_and_clamps_to_floor() {
        let lim = LicenseRateLimiter::from_persisted(cfg(), None);
        assert_eq!(lim.on_rate_limited(7), 7);
        assert!((lim.refill_per_sec() - 0.5).abs() < 1e-9);
        for _ in 0..10 {
            lim.on_rate_limited(0);
        }
        assert!((lim.refill_per_sec() - 0.125).abs() < 1e-9);
        assert_eq!(lim.on_rate_limited(0), 1);
    }

    #[test]
    fn grow_recovers_toward_ceiling() {
        let lim = LicenseRateLimiter::from_persisted(cfg(), None);
        lim.on_rate_limited(1);
        lim.arm_recovery();
        let base = Instant::now();
        for i in 1..=5u64 {
            lim.grow_at(base + Duration::from_secs(20 * i));
        }
        assert!((lim.refill_per_sec() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn take_if_dirty_then_clean() {
        let lim = LicenseRateLimiter::from_persisted(cfg(), None);
        assert!(lim.take_if_dirty().is_none());
        lim.on_rate_limited(5);
        let state = lim.take_if_dirty().expect("dirty after 429");
        assert!((state.refill_per_sec - 0.5).abs() < 1e-9);
        assert!(state.last_429_age_secs.is_some());
        assert!(lim.take_if_dirty().is_none());
    }

    #[test]
    fn from_persisted_clamps_learned_rate() {
        let learned = LearnedState {
            refill_per_sec: 99.0,
            saved_at_unix: 0,
            last_429_age_secs: Some(120),
        };
        let lim = LicenseRateLimiter::from_persisted(cfg(), Some(learned));
        assert!((lim.refill_per_sec() - 1.0).abs() < 1e-9);
    }
}
