//! Optional execution and trusted-I/O durations are independent of storage call quotas.

use std::time::{Duration, Instant};

pub(super) const EPOCH_INTERVAL: Duration = Duration::from_millis(5);
const MAX_STORAGE_CALLS: u16 = 64;

pub(super) struct IoBudget {
    started: Instant,
    io_time: Duration,
    calls: u16,
    allowance: Option<Duration>,
    io_allowance: Option<Duration>,
}

impl Default for IoBudget {
    /// Initialize a context without granting any epoch extension.
    fn default() -> Self {
        Self {
            started: Instant::now(),
            io_time: Duration::ZERO,
            calls: 0,
            allowance: None,
            io_allowance: None,
        }
    }
}

impl IoBudget {
    /// A fresh guest export starts a new wall-time budget, never a fuel budget.
    #[cfg(test)]
    pub fn reset(&mut self, now: Instant) {
        self.reset_with_limits(now, Duration::from_millis(10), Duration::from_millis(250));
    }

    /// Reset independently selected durations; zero means no duration cutoff.
    pub fn reset_with_limits(&mut self, now: Instant, allowance: Duration, io_allowance: Duration) {
        self.started = now;
        self.io_time = Duration::ZERO;
        self.calls = 0;
        self.allowance = (!allowance.is_zero()).then_some(allowance);
        self.io_allowance = (!io_allowance.is_zero()).then_some(io_allowance);
    }

    /// Reject excessive host calls before they perform filesystem work.
    pub fn begin(&mut self, now: Instant) -> Option<Instant> {
        if self.calls >= MAX_STORAGE_CALLS
            || self.io_allowance.is_some_and(|limit| self.io_time >= limit)
        {
            return None;
        }
        self.calls += 1;
        Some(now)
    }

    /// Record only actual time spent inside a storage import.
    pub fn finish(&mut self, started: Instant, now: Instant) -> bool {
        self.io_time = self
            .io_time
            .saturating_add(now.saturating_duration_since(started));
        self.io_allowance.is_none_or(|limit| self.io_time <= limit)
    }

    /// Disabled execution timing continues at every real epoch check, without a huge relative deadline.
    /// Finite timing excludes measured trusted I/O but cannot bypass its independently configured quota.
    pub fn may_continue(&self, now: Instant) -> bool {
        let Some(allowance) = self.allowance else {
            return true;
        };
        self.calls > 0
            && self.io_allowance.is_none_or(|limit| self.io_time <= limit)
            && now
                .saturating_duration_since(self.started)
                .saturating_sub(self.io_time)
                < allowance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slow_confirmed_storage_leaves_time_to_handle_its_acknowledgement() {
        let start = Instant::now();
        let mut budget = IoBudget::default();
        budget.reset(start);
        let io = budget.begin(start + Duration::from_millis(2)).unwrap();
        assert!(budget.finish(io, start + Duration::from_millis(52)));
        assert!(budget.may_continue(start + Duration::from_millis(54)));
        assert!(!budget.may_continue(start + Duration::from_millis(60)));
    }

    #[test]
    fn normal_exports_and_cheap_getters_cannot_renew_a_deadline() {
        let start = Instant::now();
        let mut budget = IoBudget::default();
        budget.reset(start);
        assert!(!budget.may_continue(start));
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start));
        assert!(!budget.may_continue(start + Duration::from_millis(11)));
    }

    #[test]
    fn excessive_storage_work_stays_bounded_across_imports() {
        let start = Instant::now();
        let mut budget = IoBudget::default();
        budget.reset(start);
        for _ in 0..MAX_STORAGE_CALLS {
            let io = budget.begin(start).unwrap();
            assert!(budget.finish(io, start));
        }
        assert!(budget.begin(start).is_none());
        budget.reset(start);
        let io = budget.begin(start).unwrap();
        assert!(!budget.finish(io, start + Duration::from_millis(251)));
        assert!(budget.begin(start + Duration::from_millis(251)).is_none());
        assert!(!budget.may_continue(start + Duration::from_millis(251)));
    }

    #[test]
    fn initialization_credit_uses_its_allowance_without_enlarging_callback_time() {
        let start = Instant::now();
        let mut budget = IoBudget::default();
        budget.reset_with_limits(
            start,
            Duration::from_millis(1000),
            Duration::from_millis(250),
        );
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start + Duration::from_millis(50)));
        assert!(budget.may_continue(start + Duration::from_millis(900)));
        assert!(!budget.may_continue(start + Duration::from_millis(1050)));
        budget.reset(start);
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start));
        assert!(!budget.may_continue(start + Duration::from_millis(11)));
    }
    #[test]
    fn unlimited_durations_keep_storage_call_quotas_and_allow_long_confirmed_waits() {
        let start = Instant::now();
        let mut budget = IoBudget::default();
        budget.reset_with_limits(start, Duration::ZERO, Duration::ZERO);
        assert!(budget.may_continue(start + Duration::from_secs(3600)));
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start + Duration::from_secs(10)));
        for _ in 1..MAX_STORAGE_CALLS {
            let io = budget.begin(start).unwrap();
            assert!(budget.finish(io, start));
        }
        assert!(budget.begin(start).is_none());
        assert!(budget.may_continue(start + Duration::from_secs(3600)));
    }

    #[test]
    fn execution_and_io_durations_are_independent_and_reset_between_phases() {
        let start = Instant::now();
        let mut budget = IoBudget::default();
        budget.reset_with_limits(start, Duration::from_millis(10), Duration::ZERO);
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start + Duration::from_secs(10)));
        assert!(budget.may_continue(start + Duration::from_millis(10009)));
        assert!(!budget.may_continue(start + Duration::from_millis(10010)));
        budget.reset_with_limits(start, Duration::ZERO, Duration::from_millis(25));
        let io = budget.begin(start).unwrap();
        assert!(!budget.finish(io, start + Duration::from_millis(26)));
        assert!(budget.begin(start + Duration::from_millis(26)).is_none());
        // A handled host quota result does not turn into an unrelated execution deadline.
        assert!(budget.may_continue(start + Duration::from_secs(100)));
        budget.reset_with_limits(start, Duration::from_millis(10), Duration::ZERO);
        assert!(!budget.may_continue(start + Duration::from_millis(11)));
    }
}
