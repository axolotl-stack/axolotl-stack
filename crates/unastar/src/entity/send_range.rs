//! Which players an actor is sent to (BDS 1.26.51).
//!
//! A player sees an actor (gets its `AddActor`, `AddItemActor` or
//! `AddPlayer`, then its updates) while the chunk it stands in is within
//! [`SEND_RANGE_CHUNKS_SQ`] of the actor's chunk: `Δx² + Δz² ≤ 17` in chunk
//! offsets, so (4, 0) and (4, 1) are in and (5, 0) and (3, 3) are out. The
//! range is the same for players, mobs, armor stands, items, experience orbs,
//! arrows and minecarts, and does not change with the view distance or the
//! simulation distance (measured at 6 / 4, 10 / 10 and 16 / 12). It has no
//! hysteresis: an actor is added in the tick the viewer's chunk comes into
//! range and removed one tick after it leaves (see [`ViewerChanges`]).
//! `docs/quirks/bds.md`, "Entities / actor send range".

use bevy_ecs::prelude::*;
use glam::DVec3;

use crate::entity::components::ActorViewers;

/// Largest squared chunk offset (`Δx² + Δz²`) at which an actor is sent.
pub const SEND_RANGE_CHUNKS_SQ: i32 = 17;

/// The chunk (x, z) a position is in.
pub fn chunk_of(position: DVec3) -> (i32, i32) {
    (
        (position.x.floor() as i32) >> 4,
        (position.z.floor() as i32) >> 4,
    )
}

/// Whether a viewer standing in chunk `viewer` sees an actor in chunk
/// `actor`.
pub fn in_range(viewer: (i32, i32), actor: (i32, i32)) -> bool {
    let (dx, dz) = (viewer.0 - actor.0, viewer.1 - actor.1);
    dx * dx + dz * dz <= SEND_RANGE_CHUNKS_SQ
}

/// The players that may view actors this tick, with the chunk each stands
/// in.
#[derive(Debug, Default)]
pub struct Candidates(Vec<(Entity, (i32, i32))>);

impl Candidates {
    pub fn new(players: impl IntoIterator<Item = (Entity, DVec3)>) -> Self {
        Self(
            players
                .into_iter()
                .map(|(entity, position)| (entity, chunk_of(position)))
                .collect(),
        )
    }

    /// The candidates in range of an actor at `position`, except `itself`
    /// (a player is not its own viewer).
    pub fn in_range_of(&self, position: DVec3, itself: Option<Entity>) -> Vec<Entity> {
        let actor = chunk_of(position);
        self.0
            .iter()
            .filter(|(entity, chunk)| Some(*entity) != itself && in_range(*chunk, actor))
            .map(|(entity, _)| *entity)
            .collect()
    }

    fn contains(&self, entity: Entity) -> bool {
        self.0.iter().any(|(candidate, _)| *candidate == entity)
    }
}

/// Who gains and who loses an actor this tick.
#[derive(Debug, Default)]
pub struct ViewerChanges {
    /// In range, not yet a viewer: send the spawn packet.
    pub fresh: Vec<Entity>,
    /// A second tick out of range: send `RemoveActor`.
    pub gone: Vec<Entity>,
    /// No longer a player at all (it left the game): forgotten, no packet.
    pub left: Vec<Entity>,
}

impl ViewerChanges {
    /// The viewers this actor no longer has, gone or left.
    pub fn lost(&self) -> impl Iterator<Item = &Entity> {
        self.gone.iter().chain(&self.left)
    }
}

impl ViewerChanges {
    /// Compare `viewers` with the `desired` ones. A viewer out of range is
    /// kept for one tick (it is removed if it is still out of range in the
    /// next one); a viewer that is no longer a candidate at all (it left the
    /// game) is dropped at once without a packet. Updates `viewers`'
    /// out-of-range list and removes the gone viewers from it.
    pub fn update(viewers: &mut ActorViewers, desired: &[Entity], candidates: &Candidates) -> Self {
        let mut changes = Self::default();
        let mut leaving = std::mem::take(&mut viewers.leaving);
        viewers.viewers.retain(|viewer| {
            if desired.contains(viewer) {
                return true;
            }
            if !candidates.contains(*viewer) {
                changes.left.push(*viewer);
                return false;
            }
            if leaving.contains(viewer) {
                changes.gone.push(*viewer);
                false
            } else {
                viewers.leaving.push(*viewer);
                true
            }
        });
        leaving.clear();
        changes.fresh = desired
            .iter()
            .copied()
            .filter(|viewer| !viewers.viewers.contains(viewer))
            .collect();
        changes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The switches of the send-range sweeps: the observer walked away
    /// from markers in chunk (0, 0) along +x, along the diagonal and along -x.
    #[test]
    fn the_range_is_a_seventeen_square_chunk_circle() {
        for offset in [(4, 0), (4, 1), (-4, 1), (2, 3), (3, 2), (1, 4), (0, -4)] {
            assert!(in_range(offset, (0, 0)), "{offset:?} is in range");
        }
        for offset in [(5, 0), (5, 1), (-5, 0), (3, 3), (3, 4), (7, 0)] {
            assert!(!in_range(offset, (0, 0)), "{offset:?} is out of range");
        }
        // Chunks of negative coordinates: x -48.5 is chunk -4, -47.5 chunk -3.
        assert_eq!(chunk_of(DVec3::new(-48.5, -60.0, 8.5)), (-4, 0));
        assert_eq!(chunk_of(DVec3::new(-47.5, -60.0, 8.5)), (-3, 0));
    }

    #[test]
    fn viewers_leave_one_tick_after_they_go_out_of_range() {
        let mut world = World::new();
        let viewer = world.spawn_empty().id();
        let candidates = Candidates::new([(viewer, DVec3::new(80.5, 0.0, 8.5))]);
        let mut viewers = ActorViewers::default();
        let first = ViewerChanges::update(&mut viewers, &[viewer], &candidates);
        assert_eq!(first.fresh, vec![viewer]);
        viewers.viewers.push(viewer);
        let out = ViewerChanges::update(&mut viewers, &[], &candidates);
        assert!(out.gone.is_empty(), "kept for the tick it left");
        let later = ViewerChanges::update(&mut viewers, &[], &candidates);
        assert_eq!(later.gone, vec![viewer]);
        assert!(viewers.viewers.is_empty());
        // Back in range before the second tick: nothing is sent.
        viewers.viewers.push(viewer);
        let _ = ViewerChanges::update(&mut viewers, &[], &candidates);
        let back = ViewerChanges::update(&mut viewers, &[viewer], &candidates);
        assert!(back.gone.is_empty() && back.fresh.is_empty());
        assert!(viewers.leaving.is_empty());
    }
}
