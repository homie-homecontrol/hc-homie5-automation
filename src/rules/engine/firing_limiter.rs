use std::{
    collections::HashMap,
    hash::Hash,
    time::{Duration, Instant},
};

/// Length of one rate-limit window.
pub const FIRING_WINDOW: Duration = Duration::from_secs(1);

/// Default number of firings allowed per rule and [`FIRING_WINDOW`].
pub const DEFAULT_MAX_FIRINGS_PER_SEC: u32 = 20;

/// Outcome of [`FiringLimiter::check`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiringDecision {
    Allowed,
    /// The firing exceeds the limit and is skipped. `first_in_window` is set for the first
    /// skipped firing of a window, so callers warn once per window.
    Limited {
        first_in_window: bool,
    },
}

#[derive(Debug)]
struct Window {
    start: Instant,
    count: u32,
    warned: bool,
}

/// Fixed-window rate limiter keyed per rule. Guards against rules that trigger themselves.
#[derive(Debug)]
pub struct FiringLimiter<K> {
    max_per_window: u32,
    windows: HashMap<K, Window>,
}

impl<K: Hash + Eq> FiringLimiter<K> {
    /// `max_per_window == 0` disables limiting.
    pub fn new(max_per_window: u32) -> Self {
        Self {
            max_per_window,
            windows: HashMap::new(),
        }
    }

    pub fn check(&mut self, key: K, now: Instant) -> FiringDecision {
        if self.max_per_window == 0 {
            return FiringDecision::Allowed;
        }
        let window = self.windows.entry(key).or_insert(Window {
            start: now,
            count: 0,
            warned: false,
        });
        if now.saturating_duration_since(window.start) >= FIRING_WINDOW {
            *window = Window {
                start: now,
                count: 0,
                warned: false,
            };
        }
        if window.count < self.max_per_window {
            window.count += 1;
            return FiringDecision::Allowed;
        }
        let first_in_window = !window.warned;
        window.warned = true;
        FiringDecision::Limited { first_in_window }
    }

    /// Drops the state of `key`, e.g. when its rule is removed.
    pub fn remove(&mut self, key: &K) {
        self.windows.remove(key);
    }
}

impl<K: Hash + Eq> Default for FiringLimiter<K> {
    fn default() -> Self {
        Self::new(DEFAULT_MAX_FIRINGS_PER_SEC)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITED_FIRST: FiringDecision = FiringDecision::Limited { first_in_window: true };
    const LIMITED: FiringDecision = FiringDecision::Limited { first_in_window: false };

    #[test]
    fn allows_up_to_the_limit_then_skips_with_one_warning() {
        let mut limiter = FiringLimiter::new(3);
        let t0 = Instant::now();
        for _ in 0..3 {
            assert_eq!(limiter.check("r", t0), FiringDecision::Allowed);
        }
        assert_eq!(limiter.check("r", t0), LIMITED_FIRST);
        assert_eq!(limiter.check("r", t0 + Duration::from_millis(500)), LIMITED);
    }

    #[test]
    fn new_window_resets_count_and_warning() {
        let mut limiter = FiringLimiter::new(1);
        let t0 = Instant::now();
        assert_eq!(limiter.check("r", t0), FiringDecision::Allowed);
        assert_eq!(limiter.check("r", t0), LIMITED_FIRST);
        let t1 = t0 + FIRING_WINDOW;
        assert_eq!(limiter.check("r", t1), FiringDecision::Allowed);
        assert_eq!(limiter.check("r", t1), LIMITED_FIRST);
    }

    #[test]
    fn rules_are_limited_independently() {
        let mut limiter = FiringLimiter::new(1);
        let t0 = Instant::now();
        assert_eq!(limiter.check("a", t0), FiringDecision::Allowed);
        assert_eq!(limiter.check("a", t0), LIMITED_FIRST);
        assert_eq!(limiter.check("b", t0), FiringDecision::Allowed);
    }

    #[test]
    fn zero_disables_limiting() {
        let mut limiter = FiringLimiter::new(0);
        let t0 = Instant::now();
        for _ in 0..1000 {
            assert_eq!(limiter.check("r", t0), FiringDecision::Allowed);
        }
        assert!(limiter.windows.is_empty());
    }

    #[test]
    fn remove_drops_state() {
        let mut limiter = FiringLimiter::new(1);
        let t0 = Instant::now();
        limiter.check("r", t0);
        limiter.remove(&"r");
        assert!(limiter.windows.is_empty());
        assert_eq!(limiter.check("r", t0), FiringDecision::Allowed);
    }
}
