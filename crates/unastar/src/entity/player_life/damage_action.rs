//! Process-local correlation for an eligible native player damage invocation.

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Allocate once before proposal callbacks; exhausted identities reject rather than wrap.
pub(super) fn next() -> Option<u64> {
    allocate(&NEXT)
}

/// Shared allocation prevents reuse across worlds, nested hits and runtime instances.
fn allocate(counter: &AtomicU64) -> Option<u64> {
    counter
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1).filter(|_| value != 0)
        })
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_is_nonzero_monotone_and_exhaustion_never_wraps() {
        let counter = AtomicU64::new(1);
        assert_eq!(allocate(&counter), Some(1));
        assert_eq!(allocate(&counter), Some(2));
        counter.store(u64::MAX - 1, Ordering::Relaxed);
        assert_eq!(allocate(&counter), Some(u64::MAX - 1));
        assert_eq!(allocate(&counter), None);
        assert_eq!(allocate(&counter), None);
        assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
        assert_eq!(allocate(&AtomicU64::new(0)), None);
    }
}
