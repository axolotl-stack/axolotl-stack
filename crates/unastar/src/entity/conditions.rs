//! Conditions shared by the player and behaviour-pack living actor lanes.

use bevy_ecs::prelude::{Entity, World};
use jolyne::valentine::actor_flags;
use unastar_data::actor_components::body::ActorDamageCause;

use super::definition::flags::ActorFlags;

/// Whether the actor's current definition makes it immune to fire.
pub fn fire_immune(world: &World, actor: Entity) -> bool {
    world
        .get::<ActorFlags>(actor)
        .is_some_and(|flags| flags.get(actor_flags::FIRE_IMMUNE).unwrap_or(false))
}

/// Whether the actor's fire immunity rejects this damage cause.
pub fn rejects_fire_damage(world: &World, actor: Entity, cause: ActorDamageCause) -> bool {
    matches!(
        cause,
        ActorDamageCause::Fire
            | ActorDamageCause::FireTick
            | ActorDamageCause::Lava
            | ActorDamageCause::Magma
            | ActorDamageCause::Campfire
            | ActorDamageCause::SoulCampfire
    ) && fire_immune(world, actor)
}
