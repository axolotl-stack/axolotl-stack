//! Throwing throwable items.

use bevy_ecs::prelude::*;

/// The projectile a throwable item throws, if it is one.
pub fn thrown(item: &str) -> Option<unastar_data::actor_identifiers::ActorType> {
    crate::entity::projectile::thrown_by(item).map(|(actor, _)| actor)
}

/// Throw the held item (the projectile system takes it from the hand).
pub fn throw(world: &mut World, player: Entity) {
    world
        .get_resource_or_init::<crate::entity::projectile::ThrowQueue>()
        .0
        .push(player);
}
