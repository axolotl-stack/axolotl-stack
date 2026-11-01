//! Native value bytes remain charged while accepted asynchronous work retains them.

use super::wit;
use std::ops::Deref;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const MAX_BYTES: usize = 16 * 1024 * 1024;

/// One component's byte allowance is shared with its outstanding owned work.
#[derive(Clone, Default)]
pub(in crate::plugin::manager) struct Budget(Arc<AtomicUsize>);

/// A reservation releases once, after the last owner of its value disappears.
pub(super) struct Reservation {
    budget: Budget,
    bytes: usize,
}

impl Reservation {
    /// Attach data only after the caller has acquired its byte allowance.
    pub(super) fn bind<T>(self, data: T) -> Arc<Value<T>> {
        Arc::new(Value {
            data,
            _reservation: self,
        })
    }

    /// Internal constructors verify their borrowed estimate against the materialized image.
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        let previous = self.budget.0.fetch_sub(self.bytes, Ordering::AcqRel);
        debug_assert!(previous >= self.bytes);
    }
}

/// Sharing a value also shares its lease; dropping a guest resource cannot uncharge a request.
pub(super) struct Value<T> {
    data: T,
    _reservation: Reservation,
}

impl<T> Deref for Value<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.data
    }
}

impl Budget {
    /// Reserve before cloning native buffers; dropping an unused reservation releases it.
    pub(super) fn reserve(&self, bytes: usize) -> Result<Reservation, wit::AccessError> {
        self.0
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= MAX_BYTES)
            })
            .map_err(|_| wit::AccessError::LimitExceeded)?;
        Ok(Reservation {
            budget: self.clone(),
            bytes,
        })
    }

    /// Preserve the existing owned-value contract for already constructed bounded data.
    pub(super) fn own<T>(&self, data: T, bytes: usize) -> Result<Arc<Value<T>>, wit::AccessError> {
        self.reserve(bytes)
            .map(|reservation| reservation.bind(data))
    }

    /// Observe retained bytes without acquiring or releasing a lease.
    #[cfg(test)]
    pub(in crate::plugin::manager) fn used(&self) -> usize {
        self.0.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepted_work_retains_its_charge_after_the_resource_owner_drops() {
        let budget = Budget::default();
        let resource = budget.own(vec![7; 32], MAX_BYTES).unwrap();
        let accepted_work = resource.clone();
        drop(resource);
        assert_eq!(budget.used(), MAX_BYTES);
        assert!(matches!(
            budget.own((), 1),
            Err(wit::AccessError::LimitExceeded)
        ));
        drop(accepted_work);
        assert_eq!(budget.used(), 0);
        assert!(budget.own((), MAX_BYTES).is_ok());
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn overflows_and_failed_reservations_leave_existing_owners_unchanged() {
        let budget = Budget::default();
        let first = budget.own((), 128).unwrap();
        assert!(matches!(
            budget.own((), usize::MAX),
            Err(wit::AccessError::LimitExceeded)
        ));
        assert_eq!(budget.used(), 128);
        assert!(matches!(
            budget.own((), MAX_BYTES),
            Err(wit::AccessError::LimitExceeded)
        ));
        drop(first);
        assert_eq!(budget.used(), 0);
    }
}

#[cfg(test)]
mod reservation_tests {
    use super::*;
    #[test]
    fn capacity_is_owned_before_materialization_and_follows_bound_values() {
        let budget = Budget::default();
        let reservation = budget.reserve(MAX_BYTES).unwrap();
        assert_eq!(budget.used(), MAX_BYTES);
        assert!(matches!(
            budget.reserve(1),
            Err(wit::AccessError::LimitExceeded)
        ));
        let value = reservation.bind(vec![1, 2, 3]);
        let retained = value.clone();
        drop(value);
        assert_eq!(budget.used(), MAX_BYTES);
        drop(retained);
        assert_eq!(budget.used(), 0);
        let unused = budget.reserve(64).unwrap();
        drop(unused);
        assert_eq!(budget.used(), 0);
    }
}
