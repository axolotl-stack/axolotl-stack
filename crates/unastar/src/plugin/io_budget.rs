//! Bounded trusted storage time does not consume a guest's epoch allowance.

use std::time::{Duration, Instant};

pub(super) const EPOCH_INTERVAL: Duration = Duration::from_millis(5);
const MAX_IO_TIME: Duration = Duration::from_millis(250);
const MAX_STORAGE_CALLS: u16 = 64;

pub(super) struct IoBudget {
    started: Instant,
    io_time: Duration,
    calls: u16,
    allowance: Duration,
}

impl Default for IoBudget {
    /// Initialize a context without granting any epoch extension.
    fn default() -> Self {
        Self {
            started: Instant::now(),
            io_time: Duration::ZERO,
            calls: 0,
            allowance: Duration::from_millis(10),
        }
    }
}

impl IoBudget {
    /// A fresh guest export starts a new wall-time budget, never a fuel budget.
    #[cfg(test)]
    pub fn reset(&mut self, now: Instant) {
        self.reset_with_allowance(now, Duration::from_millis(10));
    }

    /// Startup and simulation use their own bounded allowance without changing I/O credit.
    pub fn reset_with_allowance(&mut self, now: Instant, allowance: Duration) {
        self.started = now;
        self.io_time = Duration::ZERO;
        self.calls = 0;
        self.allowance = allowance;
    }

    /// Reject excessive host calls before they perform filesystem work.
    pub fn begin(&mut self, now: Instant) -> Option<Instant> {
        if self.calls >= MAX_STORAGE_CALLS || self.io_time >= MAX_IO_TIME {
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
        self.io_time <= MAX_IO_TIME
    }

    /// Ordinary exports still trap at their original epoch deadline. An export
    /// that used storage may continue only while its non-I/O time stays bounded.
    pub fn may_continue(&self, now: Instant) -> bool {
        self.calls > 0
            && self.io_time <= MAX_IO_TIME
            && now
                .saturating_duration_since(self.started)
                .saturating_sub(self.io_time)
                < self.allowance
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
        budget.reset_with_allowance(start, Duration::from_millis(1000));
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start + Duration::from_millis(50)));
        assert!(budget.may_continue(start + Duration::from_millis(900)));
        assert!(!budget.may_continue(start + Duration::from_millis(1050)));
        budget.reset(start);
        let io = budget.begin(start).unwrap();
        assert!(budget.finish(io, start));
        assert!(!budget.may_continue(start + Duration::from_millis(11)));
    }
}
