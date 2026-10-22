//! Player-count accounting shared by a listener and its connections.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::sync::Notify;

struct Inner {
    count: AtomicUsize,
    max: usize,
    changed: Notify,
}

/// The listener's player counter (gophertunnel `ListenerGroup`).
#[derive(Clone)]
pub struct PlayerSlots {
    inner: Arc<Inner>,
}

impl PlayerSlots {
    /// `max == 0` means unlimited.
    pub fn new(max: usize) -> Self {
        Self {
            inner: Arc::new(Inner {
                count: AtomicUsize::new(0),
                max,
                changed: Notify::new(),
            }),
        }
    }

    pub fn count(&self) -> usize {
        self.inner.count.load(Ordering::Acquire)
    }

    pub fn max(&self) -> usize {
        self.inner.max
    }

    /// Takes a slot, or `None` when the server is full.
    pub fn try_acquire(&self) -> Option<PlayerSlot> {
        let max = self.inner.max;
        self.inner
            .count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (max == 0 || count < max).then_some(count + 1)
            })
            .ok()?;
        self.inner.changed.notify_waiters();
        Some(PlayerSlot {
            inner: self.inner.clone(),
        })
    }

    /// Resolves the next time a player joins or leaves.
    pub async fn changed(&self) {
        self.inner.changed.notified().await;
    }
}

/// A held player slot; released on drop.
pub struct PlayerSlot {
    inner: Arc<Inner>,
}

impl Drop for PlayerSlot {
    fn drop(&mut self) {
        self.inner.count.fetch_sub(1, Ordering::AcqRel);
        self.inner.changed.notify_waiters();
    }
}

/// A connection's relationship to the listener's player count.
#[derive(Default)]
pub(crate) enum Seat {
    /// Not counted (connections built without a listener).
    #[default]
    Uncounted,
    /// Admission pending: a slot is taken when the login is accepted.
    #[cfg(feature = "server")]
    Pool(PlayerSlots),
    /// Counted until the connection is dropped (the slot releases on drop).
    #[cfg(feature = "server")]
    Held { _slot: PlayerSlot },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_cap_and_release() {
        let slots = PlayerSlots::new(2);
        let a = slots.try_acquire().unwrap();
        let _b = slots.try_acquire().unwrap();
        assert!(slots.try_acquire().is_none());
        drop(a);
        assert_eq!(slots.count(), 1);
        assert!(slots.try_acquire().is_some());
        let unlimited = PlayerSlots::new(0);
        let held: Vec<_> = (0..100).filter_map(|_| unlimited.try_acquire()).collect();
        assert_eq!(held.len(), 100);
    }
}
