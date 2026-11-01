//! Bounded trusted storage time does not consume a guest's epoch allowance.

use std::time::{Duration, Instant};

pub(super) const EPOCH_INTERVAL: Duration = Duration::from_millis(5);
pub(super) const INVOCATION_EPOCHS: u64 = 2;
const MAX_IO_TIME: Duration = Duration::from_millis(250);
const MAX_STORAGE_CALLS: u16 = 64;

pub(super) struct IoBudget {
    started: Instant,
    io_time: Duration,
    calls: u16,
}

impl Default for IoBudget {
    /// Initialize a context without granting any epoch extension.
    fn default() -> Self {
        Self {
            started: Instant::now(),
            io_time: Duration::ZERO,
            calls: 0,
        }
    }
}

impl IoBudget {
    /// A fresh guest export starts a new wall-time budget, never a fuel budget.
    pub fn reset(&mut self, now: Instant) {
        self.started = now;
        self.io_time = Duration::ZERO;
        self.calls = 0;
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
                < EPOCH_INTERVAL.saturating_mul(INVOCATION_EPOCHS as u32)
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
}
