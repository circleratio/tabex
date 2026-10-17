use std::time::{Duration, Instant};

pub const AUTOSAVE_DELAY: Duration = Duration::from_secs(1);

/// Decides when a dirty workspace should be written. Time is passed in for testability.
#[derive(Debug)]
pub struct Debouncer {
    delay: Duration,
    last_change: Option<Instant>,
}

impl Debouncer {
    pub fn new(delay: Duration) -> Self {
        Debouncer {
            delay,
            last_change: None,
        }
    }

    pub fn mark(&mut self, now: Instant) {
        self.last_change = Some(now);
    }

    pub fn is_dirty(&self) -> bool {
        self.last_change.is_some()
    }

    /// True once `delay` has passed since the last change.
    pub fn due(&self, now: Instant) -> bool {
        self.last_change.is_some_and(|t| now.duration_since(t) >= self.delay)
    }

    /// Time left until `due`, if dirty.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        self.last_change
            .map(|t| self.delay.saturating_sub(now.duration_since(t)))
    }

    /// Call after a successful save. Returns whether anything was pending.
    pub fn clear(&mut self) -> bool {
        self.last_change.take().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_only_after_quiet_period() {
        let t0 = Instant::now();
        let ms = |n| t0 + Duration::from_millis(n);
        let mut d = Debouncer::new(AUTOSAVE_DELAY);
        assert!(!d.due(ms(5000)));
        d.mark(ms(0));
        assert!(!d.due(ms(999)));
        d.mark(ms(800));
        assert!(!d.due(ms(1500)));
        assert_eq!(d.remaining(ms(1500)), Some(Duration::from_millis(300)));
        assert!(d.due(ms(1800)));
        assert!(d.clear());
        assert!(!d.due(ms(9000)));
        assert!(!d.clear());
    }

    #[test]
    fn immediate_flush_uses_dirty_flag() {
        let mut d = Debouncer::new(AUTOSAVE_DELAY);
        assert!(!d.is_dirty());
        d.mark(Instant::now());
        assert!(d.is_dirty());
        assert!(d.clear());
    }
}
