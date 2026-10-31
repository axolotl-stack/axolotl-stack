//! Owner teleportation at the actual projectile collision boundary.

use crate::entity::components::{GameMode, Health, Player, Position, Velocity};
use crate::entity::mob::combat::Hit;
use crate::entity::systems::physics::{BlockSource, collect_block_boxes};
use crate::server::game::item_use::{TeleportCause, teleport_checked_destination};
use bevy_ecs::prelude::*;
use glam::DVec3;
use unastar_data::actor_components::body::ActorDamageCause;

/// Require loaded, in-height terrain and a collision-free player destination.
pub(crate) fn safe_destination(world: &World, feet: DVec3) -> bool {
    let eye = feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
    let aabb = crate::server::game::player_box(eye);
    let max_y = crate::world::chunk::MIN_Y + i32::from(crate::world::chunk::SUBCHUNK_COUNT) * 16;
    if !feet.is_finite()
        || feet.x.abs() > f64::from(i32::MAX - 2)
        || feet.z.abs() > f64::from(i32::MAX - 2)
        || feet.y < f64::from(crate::world::chunk::MIN_Y)
        || aabb.max.y >= f64::from(max_y)
    {
        return false;
    }
    if world.block_at(feet.floor().as_ivec3()).is_none() {
        return false;
    }
    let mut boxes = Vec::new();
    collect_block_boxes(world, aabb, &mut boxes);
    !boxes.iter().any(|block| block.intersects(&aabb))
}

/// Teleport the same live owner, then apply the ordinary fall damage only on success.
pub(super) fn teleport_owner(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    collision: DVec3,
) {
    let Some(owner) = owner.filter(|owner| {
        world.get::<Player>(*owner).is_some()
            && world
                .get::<Health>(*owner)
                .is_some_and(|health| !health.is_dead())
            && world.get::<GameMode>(*owner) != Some(&GameMode::Spectator)
    }) else {
        return;
    };
    let direction = world
        .get::<Velocity>(projectile)
        .map_or(DVec3::ZERO, |v| v.0.normalize_or_zero());
    let Some(feet) = [0.0, 0.31, 0.61, 1.0]
        .into_iter()
        .map(|back| collision - direction * back)
        .find(|feet| safe_destination(world, *feet))
    else {
        return;
    };
    let old = world.get::<Position>(owner).map(|position| position.0);
    let identity = super::hooks::Identity::capture(world, owner);
    if let Some(destination) = teleport_checked_destination(
        world,
        owner,
        Some(projectile),
        feet,
        TeleportCause::Projectile,
        safe_destination,
    ) {
        if !identity.matches_account(world, owner) {
            return;
        }
        if let Some(old) = old {
            crate::entity::mob::net::sound::play_level_sound(
                world,
                Some(owner),
                old,
                "teleport",
                -1,
            );
        }
        crate::entity::mob::net::sound::level_event(world, destination, 2013, 0);
        crate::entity::player_life::hurt_player(
            world,
            owner,
            &Hit {
                damage: 5.0,
                cause: ActorDamageCause::Fall,
                attacker: None,
                by_player: false,
                knockback_from: None,
                knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
            },
        );
    }
}
