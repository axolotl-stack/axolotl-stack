use std::collections::VecDeque;

use crate::protocol::types::Sequence24;

/// Tracks inbound reliable sequence numbers and filters duplicates.
pub struct ReliableTracker {
    base: Sequence24,
    window: VecDeque<bool>,
    max_window: usize,
}

impl ReliableTracker {
    pub fn new(max_window: usize) -> Self {
        Self {
            base: Sequence24::new(0),
            window: VecDeque::new(),
            max_window,
        }
    }

    /// Returns true if this reliable index is new and should be processed.
    /// Returns false for duplicates or indexes too far ahead.
    pub fn see(&mut self, ridx: Sequence24) -> bool {
        if ridx == self.base {
            self.base = self.base.next();
            self.advance_base();
            return true;
        }

        let dist = self.base.distance_to(ridx);
        if dist == 0 || dist as usize > self.max_window {
            return false;
        }

        let offset = dist as usize - 1;
        if self.window.len() <= offset {
            self.window.resize(offset + 1, false);
        }

        if self.window[offset] {
            return false;
        }
        self.window[offset] = true;
        true
    }

    /// Checks if a reliable index has already been seen/processed without updating the state.
    /// Returns true if it has been seen (duplicate).
    pub fn has_seen(&self, ridx: Sequence24) -> bool {
        if ridx == self.base {
            return false; // Expecting base, so we haven't seen it.
        }

        let dist = self.base.distance_to(ridx);
        if dist == 0 {
            return true; // Behind base, so seen.
        }
        if dist as usize > self.max_window {
            return false; // Too far ahead, treat as not seen (or invalid, but not duplicate).
        }

        let offset = dist as usize - 1;
        if offset < self.window.len() {
            return self.window[offset];
        }

        false
    }

    fn advance_base(&mut self) {
        while let Some(true) = self.window.front().copied() {
            self.window.pop_front();
            self.base = self.base.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_in_order_and_advances() {
        let mut t = ReliableTracker::new(16);
        assert!(t.see(Sequence24::new(0)));
        assert!(t.see(Sequence24::new(1)));
    }

    #[test]
    fn drops_duplicates() {
        let mut t = ReliableTracker::new(16);
        assert!(t.see(Sequence24::new(0)));
        assert!(!t.see(Sequence24::new(0)));
    }

    #[test]
    fn handles_gap_then_fill() {
        let mut t = ReliableTracker::new(16);
        assert!(t.see(Sequence24::new(1))); // gap over base=0
        assert!(t.see(Sequence24::new(0))); // filling gap advances base
    }

    #[test]
    fn rejects_too_far_ahead() {
        let mut t = ReliableTracker::new(2);
        assert!(!t.see(Sequence24::new(5)));
    }
}
