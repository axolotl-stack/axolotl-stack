//! Player-count accounting shared by a listener and its connections.
//!
//! A listener owns one [`PlayerSlots`] sized by `max_players` (0 =
//! unlimited). Each connection carries a [`Seat`]: `Uncounted` without a
//! listener, `Pool` once accepted, and `Held` from the moment `authenticate`
//! receives `Login` (before verification, so a full server answers cheaply)
//! until the connection, whatever typestate or [`super::Failed`] holds it,
//! is dropped. Every change wakes the listener to republish its status.

#[cfg(feature = "server")]
use std::sync::Arc;
#[cfg(feature = "server")]
use std::sync::atomic::{AtomicUsize, Ordering};

#[cfg(feature = "server")]
use tokio::sync::Notify;

#[cfg(feature = "server")]
struct Inner {
    count: AtomicUsize,
    max: usize,
    changed: Notify,
}

/// The listener's player counter (gophertunnel `ListenerGroup`).
#[cfg(feature = "server")]
#[derive(Clone)]
pub struct PlayerSlots {
    inner: Arc<Inner>,
}

#[cfg(feature = "server")]
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
        self.inner.changed.notify_one();
        Some(PlayerSlot {
            inner: self.inner.clone(),
        })
    }

    /// Resolves after a player joined or left since the last call (a change
    /// is remembered while nobody waits). Meant for one consumer.
    pub async fn changed(&self) {
        self.inner.changed.notified().await;
    }
}

/// A held player slot; released on drop.
#[cfg(feature = "server")]
pub struct PlayerSlot {
    inner: Arc<Inner>,
}

#[cfg(feature = "server")]
impl Drop for PlayerSlot {
    fn drop(&mut self) {
        self.inner.count.fetch_sub(1, Ordering::AcqRel);
        self.inner.changed.notify_one();
    }
}

/// A connection's relationship to the listener's player count.
#[derive(Default)]
pub(crate) enum Seat {
    /// Not counted (connections built without a listener).
    #[default]
    Uncounted,
    /// Admission pending: a slot is taken when `Login` arrives, before the
    /// login is verified (a full server answers before any verification work).
    #[cfg(feature = "server")]
    Pool(PlayerSlots),
    /// Counted until the connection is dropped (the slot releases on drop).
    #[cfg(feature = "server")]
    Held { _slot: PlayerSlot },
}

#[cfg(all(test, feature = "server"))]
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
