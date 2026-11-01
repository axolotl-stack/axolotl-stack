//! Runtime ownership for staged edits to container cells.

use bevy_ecs::prelude::*;
use glam::IVec3;
use std::collections::{BTreeMap, BTreeSet};

/// Native container access and writers pause for reserved cells. Ownership is
/// runtime-only: applications restore reservations before exposing pending edits.
/// Tokens identify the embedding application's operation, not a player permission.
#[derive(Resource, Default, Debug)]
pub struct ContainerMutationLocks {
    reservations: BTreeMap<[i32; 3], String>,
}

impl ContainerMutationLocks {
    /// Reserve every cell atomically. Reacquiring one's own cells is idempotent;
    /// conflicting owners and empty tokens leave all reservations unchanged.
    pub fn acquire(&mut self, owner: &str, cells: impl IntoIterator<Item = IVec3>) -> bool {
        let cells = cells
            .into_iter()
            .map(|cell| cell.to_array())
            .collect::<BTreeSet<_>>();
        if owner.is_empty()
            || cells.iter().any(|cell| {
                self.reservations
                    .get(cell)
                    .is_some_and(|held| held != owner)
            })
        {
            return false;
        }
        for cell in cells {
            self.reservations.insert(cell, owner.into());
        }
        true
    }

    /// Release only an entirely owned batch. This changes ownership alone; after
    /// authoritative block-actor edits use [`complete_container_mutations`] to
    /// invalidate decoded state and close obsolete windows as well.
    pub fn release(&mut self, owner: &str, cells: impl IntoIterator<Item = IVec3>) -> bool {
        let cells = cells
            .into_iter()
            .map(|cell| cell.to_array())
            .collect::<BTreeSet<_>>();
        if owner.is_empty()
            || cells
                .iter()
                .any(|cell| self.reservations.get(cell).is_none_or(|held| held != owner))
        {
            return false;
        }
        for cell in cells {
            self.reservations.remove(&cell);
        }
        true
    }

    /// Whether a particular cell is reserved, including cells not yet loaded.
    pub fn is_locked(&self, cell: IVec3) -> bool {
        self.reservations.contains_key(&cell.to_array())
    }

    /// The opaque owner of a reserved cell.
    pub fn owner(&self, cell: IVec3) -> Option<&str> {
        self.reservations.get(&cell.to_array()).map(String::as_str)
    }

    /// Whether the ordinary unreserved behavior applies everywhere.
    pub fn is_empty(&self) -> bool {
        self.reservations.is_empty()
    }
}

/// Release an exactly owned batch and reconcile native state after trusted edits.
/// The application finishes its persistence/acknowledgement before calling this.
/// Failure leaves ownership, windows and decoded state untouched. Direct trusted
/// chunk edits are application-owned; native writers never bypass reservations.
pub fn complete_container_mutations(
    world: &mut World,
    owner: &str,
    cells: impl IntoIterator<Item = IVec3>,
) -> bool {
    let cells = cells.into_iter().collect::<Vec<_>>();
    if !world
        .get_resource_mut::<ContainerMutationLocks>()
        .is_some_and(|mut locks| locks.release(owner, cells.iter().copied()))
    {
        return false;
    }
    super::containers::invalidate_after_mutation(world, &cells);
    true
}

/// Preflight bounded native command regions, including either container pair half.
pub(crate) fn area_locked(world: &World, min: IVec3, max: IVec3) -> bool {
    if world
        .get_resource::<ContainerMutationLocks>()
        .is_none_or(ContainerMutationLocks::is_empty)
    {
        return false;
    }
    (min.x..=max.x).any(|x| {
        (min.y..=max.y).any(|y| {
            (min.z..=max.z).any(|z| super::containers::mutation_locked(world, IVec3::new(x, y, z)))
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservations_are_atomic_idempotent_and_exactly_owned() {
        let mut locks = ContainerMutationLocks::default();
        let a = IVec3::new(1, 64, 2);
        let b = a + IVec3::X;
        assert!(!locks.acquire("", [a]));
        assert!(locks.acquire("first", [a, a]));
        assert!(locks.acquire("first", [a]));
        assert!(!locks.acquire("second", [b, a]));
        assert!(!locks.is_locked(b));
        assert!(!locks.release("first", [a, b]));
        assert_eq!(locks.owner(a), Some("first"));
        assert!(!locks.release("second", [a]));
        assert!(locks.release("first", [a]));
        assert!(locks.is_empty());
    }
}
