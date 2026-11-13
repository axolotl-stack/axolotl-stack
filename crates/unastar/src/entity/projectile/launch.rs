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
    CLIMATE_VARIANT_PROPERTY, THROW_SOUND, THROW_SOUND_AHEAD, THROW_SOUND_EYE_DROP,
    THROWN_EGG_ITEMS,
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
pub fn thrown_by(item: &str) -> Option<(ActorType, Option<&'static str>)> {
    let actor = match item {
        "minecraft:snowball" => Some(ActorType::Snowball),
        "minecraft:ender_pearl" => Some(ActorType::EnderPearl),
        "minecraft:experience_bottle" => Some(ActorType::XpBottle),
        "minecraft:splash_potion" => Some(ActorType::SplashPotion),
        "minecraft:lingering_potion" => Some(ActorType::LingeringPotion),
        "minecraft:wind_charge" => Some(ActorType::WindChargeProjectile),
        _ => None,
    };
    if let Some(actor) = actor {
        return Some((actor, None));
    }
    THROWN_EGG_ITEMS
        .iter()
        .find(|(name, _)| *name == item)
        .map(|(_, climate)| (ActorType::Egg, Some(*climate)))
}

/// Spawn-time derived state of a projectile actor (it has no living body):
/// invisible until its second flight tick, and the physics flags, scale and
/// collision box in its actor data.
pub(crate) fn derive_projectile(world: &mut World, entity: Entity) {
    super::flight::set_invisible(world, entity, true);
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

/// Throw the held item. Measured on BDS 1.26.51 (capture of
/// `tools/bds-sweep/scripts/throwables.txt`): an item with a use cooldown
/// (`minecraft:cooldown`: the ender pearl 20 ticks, the wind charge 10)
/// starts it and is not thrown while it runs; the projectile carries a
/// potion's data value as its aux value.
fn throw_held(world: &mut World, player: Entity) {
    let Some(stack) = held_item(world, player).cloned() else {
        return;
    };
    let Some((actor, climate)) = thrown_by(stack.item_id()) else {
        return;
    };
    let mode = world.get::<GameMode>(player).copied().unwrap_or_default();
    if mode == GameMode::Spectator || world.get::<Health>(player).is_some_and(Health::is_dead) {
        return;
    }
    // An actor this server cannot spawn yet is not thrown, and the item is
    // kept.
    if world
        .get_resource::<crate::entity::mob::MobRegistry>()
        .is_some_and(|registry| registry.get(actor.name()).is_none())
    {
        warn!(
            actor = actor.name(),
            "Throwing an actor that cannot spawn yet"
        );
        return;
    }
    let cooldown = stack
        .item()
        .and_then(crate::item::cooldown::cooldown)
        .filter(|cooldown| !cooldown.attack);
    if let Some(cooldown) = &cooldown {
        if crate::item::cooldown::cooling(world, player, &cooldown.category) {
            return;
        }
        crate::item::cooldown::start(world, player, cooldown);
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
    let potion = matches!(actor, ActorType::SplashPotion | ActorType::LingeringPotion)
        .then_some(stack.damage);
    match launch(world, player, actor, climate) {
        Ok(projectile) => {
            if let Some(potion) = potion {
                set_potion(world, projectile, potion);
            }
        }
        Err(error) => warn!(%error, actor = actor.name(), "Throwing a projectile failed"),
    }
}

/// The potion a thrown potion carries: its aux value actor data.
fn set_potion(world: &mut World, projectile: Entity, potion: i16) {
    let mut entity = world.entity_mut(projectile);
    entity.insert(super::ProjectilePotion(potion));
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_short(ids::AUX_VALUE_DATA, potion);
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

/// How an item launches a projectile, beyond the definition's rules.
#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    /// Launch speed in blocks per tick instead of the definition's `power`.
    pub speed: Option<f64>,
    /// Spread: each axis of the unit launch direction gets a gaussian times
    /// `0.0172275 × inaccuracy` (public reference, unmeasured).
    pub inaccuracy: f64,
    /// A critical shot (fully drawn bow, crossbow).
    pub critical: bool,
    /// The item a player gets back from the stuck projectile.
    pub item: Option<crate::item::ItemStack>,
    /// Play the definition's throw sound (throwables); shooters play their
    /// own.
    pub throw_sound: bool,
}

impl Default for LaunchSpec {
    fn default() -> Self {
        Self {
            speed: None,
            inaccuracy: 0.0,
            critical: false,
            item: None,
            throw_sound: true,
        }
    }
}

/// A standard normal sample (Box-Muller).
fn gaussian(rng: &mut impl rand::Rng) -> f64 {
    let u: f64 = rng.r#gen::<f64>().max(f64::MIN_POSITIVE);
    let v: f64 = rng.r#gen();
    (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
}

/// Throw a new `actor` projectile from `thrower` (a player: its `Position`
/// is the eye), optionally setting its climate variant.
pub fn launch(
    world: &mut World,
    thrower: Entity,
    actor: ActorType,
    climate: Option<&str>,
) -> Result<Entity, LaunchError> {
    launch_with(world, thrower, actor, climate, LaunchSpec::default())
}

/// [`launch`] with an item's launch speed, spread, critical flag and
/// returned item.
pub fn launch_with(
    world: &mut World,
    thrower: Entity,
    actor: ActorType,
    climate: Option<&str>,
    spec: LaunchSpec,
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
    let mut direction = view_direction(look.yaw, pitch);
    direction /= direction.length().max(0.001);
    if spec.inaccuracy > 0.0 {
        let rng = &mut world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0;
        let spread = 0.0172275 * spec.inaccuracy;
        direction += DVec3::new(gaussian(rng), gaussian(rng), gaussian(rng)) * spread;
    }
    let velocity = direction * spec.speed.unwrap_or(rules.power);
    let (yaw, pitch) = heading(velocity);
    let mut entity = world.entity_mut(projectile);
    // A projectile's head yaw stays 0 (its BDS 1.26.51 `AddActor`).
    entity.insert((
        Position(feet),
        Velocity(velocity),
        Rotation::with_head_yaw(yaw, pitch, 0.0),
    ));
    if let Some(mut flight) = entity.get_mut::<ProjectileFlight>() {
        flight.owner = Some(thrower);
        flight.critical = spec.critical;
    }
    if let Some(item) = spec.item {
        entity.insert(super::ProjectileItem(item));
    }
    if spec.critical
        && let Some(mut flags) = entity.get_mut::<ActorFlags>()
    {
        flags
            .update(actor_flags::CRITICAL, true)
            .expect("catalogue flag index");
        let words = *flags.snapshot().words();
        if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
            metadata.set_long(ids::RESERVED_0, words[0] as i64);
        }
    }
    if spec.throw_sound {
        let (sin, cos) = f64::from(look.yaw).to_radians().sin_cos();
        let ahead = DVec3::new(-sin, 0.0, cos) * THROW_SOUND_AHEAD;
        play_level_sound(
            world,
            Some(thrower),
            eye - DVec3::Y * THROW_SOUND_EYE_DROP + ahead,
            THROW_SOUND,
            -1,
        );
    }
    // Its first viewers see it as launched, not as `spawn_mob` placed it.
    crate::entity::mob::net::network::record_spawn_state(world, projectile, Some(false));
    Ok(projectile)
}
