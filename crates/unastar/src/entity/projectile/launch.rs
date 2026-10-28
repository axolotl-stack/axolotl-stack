//! Throwing projectiles ([`launch`]) and their spawn-time derived state.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use thiserror::Error;
use tracing::warn;
use unastar_data::actor_identifiers::ActorType;

use super::{ProjectileConfig, ProjectileFlight, ThrowQueue};
use crate::entity::components::{
    GameMode, Health, HeldSlot, MainInventory, Position, Rotation, Velocity,
};
use crate::entity::definition::components::{PhysicsConfig, RawCollisionBox, ScaleConfig};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue, PropertyError};
use crate::entity::mob::constants::{
    CLIMATE_VARIANT_PROPERTY, THROW_SOUND, THROW_SOUND_EYE_DROP, THROWN_EGG_ITEMS,
};
use crate::entity::mob::events::held_item;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::{MobSpawnError, SpawnCause, spawn_mob};
use crate::entity::systems::physics::move_aabb;
use crate::registry::collision::Aabb;
use crate::server::game::send_slot;

/// Why a throw produced no projectile.
#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("the thrower has no position or rotation")]
    NoThrowerPosition,
    #[error("spawning the projectile: {0}")]
    Spawn(#[source] MobSpawnError),
    #[error("{0} has no minecraft:projectile component")]
    NotProjectile(&'static str),
    #[error("climate variant: {0}")]
    Climate(#[from] PropertyError),
}

/// The projectile actor and its climate variant label an item throws when
/// used, if it is a throwable item.
pub fn thrown_by(item: &str) -> Option<(ActorType, &'static str)> {
    THROWN_EGG_ITEMS
        .iter()
        .find(|(name, _)| *name == item)
        .map(|(_, climate)| (ActorType::Egg, *climate))
}

/// Spawn-time derived state of a projectile actor (it has no living body):
/// the physics flags, scale and collision box in its actor data.
pub(crate) fn derive_projectile(world: &mut World, entity: Entity) {
    let physics = world.get::<PhysicsConfig>(entity).copied();
    let scale = world.get::<ScaleConfig>(entity).map_or(1.0, |s| s.value);
    let (width, height) = world
        .get::<RawCollisionBox>(entity)
        .map_or((0.0, 0.0), |b| (b.width.max(0.0), b.height.max(0.0)));
    let mut entity = world.entity_mut(entity);
    let Some(mut flags) = entity.get_mut::<ActorFlags>() else {
        return;
    };
    for (flag, on) in [
        (
            actor_flags::HAS_GRAVITY,
            physics.is_some_and(|p| p.has_gravity),
        ),
        (
            actor_flags::HAS_COLLISION,
            physics.is_some_and(|p| p.has_collision),
        ),
    ] {
        flags.update(flag, on).expect("catalogue flag index");
    }
    let words = *flags.snapshot().words();
    let Some(mut metadata) = entity.get_mut::<EntityMetadata>() else {
        return;
    };
    metadata.set_long(ids::RESERVED_0, words[0] as i64);
    metadata.set_long(ids::RESERVED_092, words[1] as i64);
    metadata.set_float(ids::RESERVED_038, scale);
    metadata.set_float(ids::RESERVED_053, width * scale);
    metadata.set_float(ids::RESERVED_054, height * scale);
}

/// System (exclusive, `PacketApplySet`): throw the held item of every
/// queued player. Outside creative one item is used up.
pub fn apply_throws(world: &mut World) {
    let Some(mut queue) = world.get_resource_mut::<ThrowQueue>() else {
        return;
    };
    let players = std::mem::take(&mut queue.0);
    for player in players {
        throw_held(world, player);
    }
}

fn throw_held(world: &mut World, player: Entity) {
    let Some((actor, climate)) = held_item(world, player).and_then(|s| thrown_by(&s.item_id))
    else {
        return;
    };
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    if mode == GameMode::Spectator || world.get::<Health>(player).is_some_and(Health::is_dead) {
        return;
    }
    if mode != GameMode::Creative
        && let Some(slot) = world.get::<HeldSlot>(player).map(|s| usize::from(s.0))
    {
        let used = world
            .get_mut::<MainInventory>(player)
            .and_then(|mut inventory| {
                let held = inventory.0.item_mut(slot)?;
                *held = held.grow(-1);
                Some(held.clone())
            });
        if let Some(stack) = used {
            send_slot(world, player, slot, &stack);
        }
    }
    if let Err(error) = launch(world, player, actor, Some(climate)) {
        warn!(%error, actor = actor.name(), "Throwing a projectile failed");
    }
}

/// The unit view vector of a rotation (degrees; Bedrock yaw/pitch).
fn view_direction(yaw: f32, pitch: f32) -> DVec3 {
    let (yaw, pitch) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    DVec3::new(
        -yaw.sin() * pitch.cos(),
        -pitch.sin(),
        yaw.cos() * pitch.cos(),
    )
}

/// The rotation a projectile faces when flying along `velocity`.
pub(super) fn heading(velocity: DVec3) -> (f32, f32) {
    let horizontal = velocity.x.hypot(velocity.z);
    (
        velocity.x.atan2(velocity.z).to_degrees() as f32,
        velocity.y.atan2(horizontal).to_degrees() as f32,
    )
}

/// Throw a new `actor` projectile from `thrower` (a player: its `Position`
/// is the eye), optionally setting its climate variant.
pub fn launch(
    world: &mut World,
    thrower: Entity,
    actor: ActorType,
    climate: Option<&str>,
) -> Result<Entity, LaunchError> {
    let (Some(eye), Some(look)) = (
        world.get::<Position>(thrower).map(|p| p.0),
        world.get::<Rotation>(thrower).copied(),
    ) else {
        return Err(LaunchError::NoThrowerPosition);
    };
    let projectile = spawn_mob(world, actor.name(), eye, look.yaw, SpawnCause::Spawned)
        .map_err(LaunchError::Spawn)?;
    let Some(rules) = world
        .get::<ProjectileConfig>(projectile)
        .map(|c| Arc::clone(&c.0))
    else {
        world.despawn(projectile);
        return Err(LaunchError::NotProjectile(actor.name()));
    };
    if let Some(climate) = climate
        && let Some(mut properties) = world.get_mut::<ActorProperties>(projectile)
        && let Err(error) =
            properties.set_named(CLIMATE_VARIANT_PROPERTY, NamedPropertyValue::Enum(climate))
    {
        world.despawn(projectile);
        return Err(LaunchError::Climate(error));
    }
    let mut feet = eye;
    if rules.offset != DVec3::ZERO {
        let (sin, cos) = f64::from(look.yaw).to_radians().sin_cos();
        let offset = DVec3::new(
            rules.offset.x * cos - rules.offset.z * sin,
            rules.offset.y,
            rules.offset.x * sin + rules.offset.z * cos,
        );
        let (width, height) = world
            .get::<RawCollisionBox>(projectile)
            .map_or((0.0, 0.0), |b| (f64::from(b.width), f64::from(b.height)));
        let moved = move_aabb(
            world,
            Aabb::from_feet(feet, width, height),
            offset,
            0.0,
            false,
        );
        feet = moved.aabb.feet();
    }
    let pitch = look.pitch + rules.angle_offset * f64::from(look.pitch).to_radians().cos() as f32;
    let direction = view_direction(look.yaw, pitch);
    let velocity = direction / direction.length().max(0.001) * rules.power;
    let (yaw, pitch) = heading(velocity);
    let mut entity = world.entity_mut(projectile);
    entity.insert((
        Position(feet),
        Velocity(velocity),
        Rotation::new(yaw, pitch),
    ));
    if let Some(mut flight) = entity.get_mut::<ProjectileFlight>() {
        flight.owner = Some(thrower);
    }
    play_level_sound(
        world,
        Some(thrower),
        eye - DVec3::Y * THROW_SOUND_EYE_DROP,
        THROW_SOUND,
        -1,
    );
    Ok(projectile)
}
