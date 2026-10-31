//! Redirection of authored reflectable projectiles by real attacks.

use bevy_ecs::prelude::*;
use glam::DVec3;

use super::{ProjectileConfig, ProjectileFlight, Stuck};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{Position, Rotation, Velocity};

/// Reflect a projectile along the attacker's authoritative view direction.
/// Returns false for unsupported actors, missing sources and launch immunity.
pub(crate) fn from_attack(world: &mut World, target: Entity, attacker: Option<Entity>) -> bool {
    let Some(attacker) = attacker else {
        return false;
    };
    let Some(look) = world.get::<Rotation>(attacker) else {
        return false;
    };
    let direction = super::launch::view_direction(look.yaw, look.pitch);
    redirect(world, target, attacker, direction)
}

/// A projectile collision uses the incoming flight direction and attributes the
/// reflection to its surviving shooter, falling back to the projectile itself.
pub(super) fn from_projectile(world: &mut World, target: Entity, incoming: Entity) -> bool {
    let Some(direction) = world.get::<Velocity>(incoming).map(|velocity| velocity.0) else {
        return false;
    };
    let owner = world
        .get::<ProjectileFlight>(incoming)
        .and_then(|flight| flight.owner)
        .filter(|owner| world.get_entity(*owner).is_ok())
        .unwrap_or(incoming);
    redirect(world, target, owner, direction)
}

/// Commit a finite redirection without creating health or replaying a spawn.
fn redirect(world: &mut World, target: Entity, owner: Entity, direction: DVec3) -> bool {
    let Some(config) = world.get::<ProjectileConfig>(target) else {
        return false;
    };
    if !config.0.reflect_on_hurt || !direction.is_finite() || direction.length_squared() < 1e-12 {
        return false;
    }
    let Some(flight) = world.get::<ProjectileFlight>(target).copied() else {
        return false;
    };
    let now = world.resource::<TickCounter>().current;
    if now.saturating_sub(flight.launched_tick) < config.0.reflect_immunity_ticks
        || world.get::<Position>(target).is_none()
    {
        return false;
    }
    let speed = world
        .get::<Velocity>(target)
        .map_or(config.0.power, |velocity| velocity.0.length());
    if !speed.is_finite() || speed <= 0.0 {
        return false;
    }
    let velocity = direction.normalize() * speed;
    let (yaw, pitch) = super::launch::heading(velocity);
    let mut actor = world.entity_mut(target);
    actor.insert((
        Velocity(velocity),
        Rotation::new(yaw, pitch),
        ProjectileFlight {
            owner: Some(owner),
            launched_tick: now,
            critical: false,
        },
    ));
    actor.remove::<Stuck>();
    true
}
