//! Throwing projectiles ([`launch`]), mob shots ([`launch_at`]) and their
//! spawn-time derived state.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use thiserror::Error;
use tracing::warn;
use unastar_data::actor_identifiers::ActorType;

use super::{ProjectileConfig, ProjectileFlight, ThrowQueue};
use crate::entity::components::{
    GameMode, Health, HeldSlot, MainInventory, PhysicsBody, Player, Position, Rotation, Velocity,
};
use crate::entity::definition::components::{PhysicsConfig, RawCollisionBox, ScaleConfig};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue, PropertyError};
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::constants::{
    CLIMATE_VARIANT_PROPERTY, MOB_EYES_HEIGHT_RATIO, MOB_SHOT_MIN_HORIZONTAL,
    MOB_SHOT_OUT_OF_REACH_PITCH, MOB_SHOT_SPREAD, MOB_SHOT_STEP, PLAYER_BOX_HEIGHT, THROW_SOUND,
    THROW_SOUND_EYE_DROP, THROWN_EGG_ITEMS,
};
use crate::entity::mob::events::held_item;
use crate::entity::mob::net::sound::play_level_sound;
use crate::entity::mob::{MobSpawnError, SpawnCause, spawn_mob, spawn_mob_by};
use crate::entity::systems::physics::move_aabb;
use crate::registry::collision::Aabb;
use crate::server::game::send_slot;
use crate::world::rules::Difficulty;

/// A player's throw spreads each axis of its unit direction by a gaussian
/// times this times the item's inaccuracy (public reference, unmeasured).
const PLAYER_SHOT_SPREAD: f64 = 0.0172275;

/// Why a throw produced no projectile.
#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("the thrower has no position or rotation")]
    NoThrowerPosition,
    #[error("the target is gone or straight above or below the shooter")]
    NoTarget,
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
    match item {
        "minecraft:snowball" => return Some((ActorType::Snowball, None)),
        "minecraft:ender_pearl" => return Some((ActorType::EnderPearl, None)),
        "minecraft:splash_potion" => return Some((ActorType::SplashPotion, None)),
        "minecraft:lingering_potion" => return Some((ActorType::LingeringPotion, None)),
        _ => {}
    }
    THROWN_EGG_ITEMS
        .iter()
        .find(|(name, _)| *name == item)
        .map(|(_, climate)| (ActorType::Egg, Some(*climate)))
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
    let Some(held) = held_item(world, player) else {
        return;
    };
    let Some((actor, climate)) = thrown_by(held.item_id()) else {
        return;
    };
    // A thrown potion carries the held potion's type.
    let potion = matches!(actor, ActorType::SplashPotion | ActorType::LingeringPotion)
        .then_some(held.damage);
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
    match launch(world, player, actor, climate) {
        Ok(projectile) => {
            if let Some(potion) = potion {
                super::carry_potion(world, projectile, potion, false);
            }
        }
        Err(error) => warn!(%error, actor = actor.name(), "Throwing a projectile failed"),
    }
}

/// The unit view vector of a rotation (degrees; Bedrock yaw/pitch).
pub(crate) fn view_direction(yaw: f32, pitch: f32) -> DVec3 {
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
pub(super) fn gaussian(rng: &mut impl rand::Rng) -> f64 {
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
    let rules = projectile_rules(world, projectile, actor)?;
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
        let (width, height) = world
            .get::<RawCollisionBox>(projectile)
            .map_or((0.0, 0.0), |b| (f64::from(b.width), f64::from(b.height)));
        let moved = move_aabb(
            world,
            Aabb::from_feet(feet, width, height),
            turned(rules.offset, look.yaw),
            0.0,
            false,
        );
        feet = moved.aabb.feet();
    }
    let pitch = look.pitch + rules.angle_offset * f64::from(look.pitch).to_radians().cos() as f32;
    let direction = view_direction(look.yaw, pitch);
    let speed = spec.speed.unwrap_or(rules.power);
    let velocity = spread(
        world,
        direction,
        PLAYER_SHOT_SPREAD * spec.inaccuracy,
        speed,
    );
    release(world, projectile, thrower, feet, velocity, spec.critical);
    if let Some(item) = spec.item {
        world
            .entity_mut(projectile)
            .insert(super::ProjectileItem(item));
    }
    if spec.throw_sound {
        play_level_sound(
            world,
            Some(thrower),
            eye - DVec3::Y * THROW_SOUND_EYE_DROP,
            THROW_SOUND,
            -1,
        );
    }
    Ok(projectile)
}

/// How a mob shooter's projectile flies, beyond the projectile's rules.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MobShot {
    /// Launch speed instead of the projectile's `power`.
    pub power: Option<f64>,
    /// The shooter's `aux_val`: tipped-arrow data (the potion aux plus one)
    /// the projectile carries and replicates.
    pub potion: Option<i8>,
}

/// Shoot a new `actor` projectile from the mob `shooter` at `target`, as
/// `minecraft:shooter` does (matches BDS 1.26.51).
///
/// The projectile is created with the shooter as `other` of its
/// `minecraft:entity_spawned` (so an arrow takes its `mob_arrow` rules). It
/// starts at the shooter's eyes ([`MOB_EYES_HEIGHT_RATIO`]; its feet for an
/// `origin` anchor) plus the `offset` turned by the body yaw, moved
/// [`MOB_SHOT_STEP`] horizontally toward the target, and aims at the centre
/// of the target's box measured from before that step. With gravity the
/// pitch is the lower solution of the drag-free ballistic equation for the
/// launch speed ([`MOB_SHOT_OUT_OF_REACH_PITCH`] when out of reach). The
/// unit direction then gets a gaussian per axis times [`MOB_SHOT_SPREAD`]
/// times `max(0, uncertainty_base − difficulty × uncertainty_multiplier)`.
/// A mob's shot is never critical and leaves nothing to pick up.
pub fn launch_at(
    world: &mut World,
    shooter: Entity,
    actor: ActorType,
    target: Entity,
    shot: MobShot,
) -> Result<Entity, LaunchError> {
    let (Some(feet), Some(look), Some(height)) = (
        world.get::<Position>(shooter).map(|p| p.0),
        world.get::<Rotation>(shooter).copied(),
        world.get::<PhysicsBody>(shooter).map(|b| b.height),
    ) else {
        return Err(LaunchError::NoThrowerPosition);
    };
    let Some(aim) = target_centre(world, target) else {
        return Err(LaunchError::NoTarget);
    };
    let eyes = feet + DVec3::Y * height * MOB_EYES_HEIGHT_RATIO;
    let projectile =
        spawn_mob_by(world, actor.name(), eyes, look.yaw, shooter).map_err(LaunchError::Spawn)?;
    let rules = projectile_rules(world, projectile, actor)?;
    let origin = if rules.anchor_eye { eyes } else { feet };
    let start = origin + turned(rules.offset, look.yaw);
    let delta = aim - start;
    let horizontal = delta.x.hypot(delta.z);
    if horizontal < MOB_SHOT_MIN_HORIZONTAL {
        world.despawn(projectile);
        return Err(LaunchError::NoTarget);
    }
    let at = start + DVec3::new(delta.x, 0.0, delta.z) / horizontal * MOB_SHOT_STEP;
    let yaw = (delta.z.atan2(delta.x).to_degrees() - 90.0) as f32;
    let power = shot.power.unwrap_or(rules.power);
    let pitch = if rules.gravity != 0.0 {
        ballistic_pitch(power, rules.gravity, horizontal, delta.y)
    } else {
        -delta.y.atan2(horizontal).to_degrees() as f32
    };
    let difficulty = world
        .get_resource::<Difficulty>()
        .copied()
        .unwrap_or_default()
        .wire() as f32;
    let uncertainty = (rules.uncertainty_base - difficulty * rules.uncertainty_multiplier).max(0.0);
    let velocity = spread(
        world,
        view_direction(yaw, pitch),
        MOB_SHOT_SPREAD * f64::from(uncertainty),
        power,
    );
    release(world, projectile, shooter, at, velocity, false);
    if let Some(data) = shot.potion {
        let aux = i16::from(data) - unastar_rules::potion::TIPPED_ARROW_DATA_OFFSET;
        super::carry_potion(world, projectile, aux, true);
    }
    Ok(projectile)
}

/// The pitch (degrees, negative up) of the lower ballistic arc that reaches
/// a point `horizontal` away and `rise` up at `speed` under `gravity`.
fn ballistic_pitch(speed: f64, gravity: f64, horizontal: f64, rise: f64) -> f32 {
    let v2 = speed * speed;
    let discriminant = v2 * v2 - gravity * (2.0 * rise * v2 + horizontal * horizontal * gravity);
    if discriminant < 0.0 {
        return MOB_SHOT_OUT_OF_REACH_PITCH;
    }
    let root = discriminant.sqrt();
    let low = (v2 - root)
        .atan2(gravity * horizontal)
        .min((v2 + root).atan2(gravity * horizontal));
    -low.to_degrees() as f32
}

/// The centre of `target`'s box: a player's from its eye position, a mob's
/// from its feet.
fn target_centre(world: &World, target: Entity) -> Option<DVec3> {
    let position = world.get::<Position>(target)?.0;
    if world.get::<Player>(target).is_some() {
        let feet = position - DVec3::Y * PLAYER_EYE_HEIGHT;
        return Some(feet + DVec3::Y * PLAYER_BOX_HEIGHT / 2.0);
    }
    let height = world.get::<PhysicsBody>(target)?.height;
    Some(position + DVec3::Y * height / 2.0)
}

/// `offset` with x and z turned by `yaw` (degrees).
fn turned(offset: DVec3, yaw: f32) -> DVec3 {
    let (sin, cos) = f64::from(yaw).to_radians().sin_cos();
    DVec3::new(
        offset.x * cos - offset.z * sin,
        offset.y,
        offset.x * sin + offset.z * cos,
    )
}

/// The projectile's compiled rules; a definition without them is removed.
fn projectile_rules(
    world: &mut World,
    projectile: Entity,
    actor: ActorType,
) -> Result<Arc<super::ProjectileRules>, LaunchError> {
    match world
        .get::<ProjectileConfig>(projectile)
        .map(|c| Arc::clone(&c.0))
    {
        Some(rules) => Ok(rules),
        None => {
            world.despawn(projectile);
            Err(LaunchError::NotProjectile(actor.name()))
        }
    }
}

/// `direction` normalised, plus a gaussian per axis times `spread`, times
/// `speed`.
fn spread(world: &mut World, direction: DVec3, spread: f64, speed: f64) -> DVec3 {
    let mut direction = direction / direction.length().max(0.001);
    if spread > 0.0 {
        let rng = &mut world
            .get_resource_or_init::<crate::entity::EntityRandom>()
            .0;
        direction += DVec3::new(gaussian(rng), gaussian(rng), gaussian(rng)) * spread;
    }
    direction * speed
}

/// Send `projectile` off from `feet` at `velocity`, owned by `thrower`.
fn release(
    world: &mut World,
    projectile: Entity,
    thrower: Entity,
    feet: DVec3,
    velocity: DVec3,
    critical: bool,
) {
    let (yaw, pitch) = heading(velocity);
    let mut entity = world.entity_mut(projectile);
    entity.insert((
        Position(feet),
        Velocity(velocity),
        Rotation::new(yaw, pitch),
    ));
    if let Some(mut flight) = entity.get_mut::<ProjectileFlight>() {
        flight.owner = Some(thrower);
        flight.critical = critical;
    }
    if critical && let Some(mut flags) = entity.get_mut::<ActorFlags>() {
        flags
            .update(actor_flags::CRITICAL, true)
            .expect("catalogue flag index");
        let words = *flags.snapshot().words();
        if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
            metadata.set_long(ids::RESERVED_0, words[0] as i64);
        }
    }
}
