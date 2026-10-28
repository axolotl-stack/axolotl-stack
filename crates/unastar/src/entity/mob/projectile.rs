//! `minecraft:projectile`: thrown actors (the egg).
//!
//! Launch ([`launch`]): the projectile actor is created at the thrower's eye,
//! moved by the definition's `offset` (x/z turned by the thrower's yaw,
//! stopped by blocks) and sent along the thrower's view direction (pitch
//! plus `angle_offset × cos(pitch)`) at `power` blocks per tick; the
//! thrower's own velocity is not added. Every projectile actor starts
//! invisible and shows from its second tick of flight.
//!
//! Flight ([`tick_projectiles`]): each tick the segment from the position to
//! position + velocity is tested against block collision boxes and against
//! the boxes of mobs and players, grown by `(ticks in flight - 2) × 0.05`
//! (at most 0.3). The thrower is never hit: only a bounced or reflected
//! projectile may hit its owner, and neither is implemented. Hits run
//! nearest first, only the first unless `multiple_targets`. Then the
//! projectile moves by its velocity without collision, its velocity is
//! scaled by `inertia` (`liquid_inertia` in water) and loses `gravity`, and
//! its rotation turns 20 % of the way to its heading.
//!
//! On a hit the `on_hit` effects run in alphabetical order:
//! - `impact_damage`: a hit mob takes the rolled damage (cause projectile,
//!   even 0), pushed away from the projectile by the projectile's knockback
//!   rules; players have no hurt path on this server and only stop the
//!   projectile.
//! - `particle_on_hit`: level events at the projectile.
//! - `remove_on_hit`.
//! - `spawn_chance`: one roll for the first group and, only when that
//!   succeeded, one for the second; the spawns appear at the projectile
//!   (raised out of a block it is inside, turned to its yaw) and get the
//!   `on_spawn` events whose filters pass, with the projectile as `other`.
//!
//! Then the hit sound plays and a removed projectile despawns.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use rand::Rng;
use thiserror::Error;
use tracing::warn;
use unastar_data::actor_components::body::{
    self, ActorDamageCause, FilterSubject, ParticleType, ProjectileAnchor, ShouldBounce,
};
use unastar_data::actor_components::{ActorComponent, decode};
use unastar_data::actor_identifiers::ActorType;
use unastar_rules::filter::Filter;

use super::combat::{Hit, hurt_mob, knockback_power_of};
use super::constants::{
    CLIMATE_VARIANT_PROPERTY, ICONCRACK_ITEM_SHIFT, ICONCRACK_PARTICLE,
    PROJECTILE_HIT_INFLATE_DELAY, PROJECTILE_HIT_INFLATE_MAX, PROJECTILE_HIT_INFLATE_STEP,
    PROJECTILE_PARTICLE_LEVEL_EVENT, PROJECTILE_ROTATION_SMOOTHING, PROJECTILE_VISIBLE_TICK,
    THROW_SOUND, THROW_SOUND_EYE_DROP, THROWN_EGG_ITEMS,
};
use super::events::{MobFacts, fire_event, held_item};
use super::sound::{level_event, play_level_sound};
use super::state::Dead;
use super::{MobArchetype, MobKind, MobRegistry, SpawnCause, spawn_mob};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, Health, HeldSlot, MainInventory, PhysicsBody, Player, Position, Projectile, Rotation,
    Velocity,
};
use crate::entity::definition::{ComponentKindsBuilder, apply_pending_definitions};
use crate::entity::definition_components::{PhysicsConfig, RawCollisionBox, ScaleConfig};
use crate::entity::flags::ActorFlags;
use crate::entity::metadata::EntityMetadata;
use crate::entity::properties::{ActorProperties, NamedPropertyValue};
use crate::entity::systems::physics::{collect_block_boxes, living_liquid, move_aabb};
use crate::registry::ItemRegistry;
use crate::registry::block::LiquidKind;
use crate::registry::collision::Aabb;
use crate::server::game::types::ItemRegistryResource;
use crate::server::game::{player_box, send_slot};

/// Why a `minecraft:projectile` body cannot be compiled.
#[derive(Debug, Error)]
pub enum ProjectileError {
    #[error("projectile {0} is not implemented")]
    Unsupported(&'static str),
    #[error("on_hit {0} is not implemented")]
    OnHit(&'static str),
    #[error("particle type {0:?} is not implemented; only iconcrack has a known id")]
    Particle(ParticleType),
    #[error("{0} is not a native actor type")]
    ActorType(String),
    #[error("particle item {0} is not a vanilla item")]
    Item(String),
    #[error("on_spawn event {0} targets another actor; only the spawned actor is implemented")]
    OnSpawnTarget(String),
    #[error("filter: {0}")]
    Filter(String),
    #[error("spawn_chance spawns {0}, which is not a spawnable archetype")]
    SpawnDefinition(&'static str),
    #[error("spawn_chance on_spawn event {0} is not defined by {1}")]
    SpawnEvent(Arc<str>, &'static str),
    #[error("body: {0}")]
    Decode(String),
}

/// Why a throw produced no projectile.
#[derive(Debug, Error)]
pub enum LaunchError {
    #[error("the thrower has no position or rotation")]
    NoThrowerPosition,
    #[error("spawning the projectile: {0}")]
    Spawn(String),
    #[error("{0} has no minecraft:projectile component")]
    NotProjectile(&'static str),
    #[error("climate variant: {0}")]
    Climate(#[from] crate::entity::properties::PropertyError),
}

/// `minecraft:projectile`.
#[derive(Component, Debug, Clone)]
pub struct ProjectileConfig(pub Arc<ProjectileRules>);

/// The compiled projectile behaviour.
#[derive(Debug)]
pub struct ProjectileRules {
    pub power: f64,
    pub gravity: f64,
    pub inertia: f64,
    pub liquid_inertia: f64,
    /// Degrees added to the launch pitch, scaled by `cos(pitch)`.
    pub angle_offset: f32,
    /// Spawn offset from the thrower's eye; x/z turn with the thrower's yaw.
    pub offset: DVec3,
    pub multiple_targets: bool,
    pub ignored: Box<[ActorType]>,
    pub hit_sound: Option<Arc<str>>,
    pub hit_ground_sound: Option<Arc<str>>,
    pub impact_damage: Option<ImpactDamage>,
    pub particle: Option<ParticleOnHit>,
    pub remove_on_hit: bool,
    pub spawn_chance: Option<SpawnChance>,
}

/// `on_hit.impact_damage`.
#[derive(Debug, Clone, Copy)]
pub struct ImpactDamage {
    pub min: f32,
    pub max: f32,
    pub knockback: bool,
    pub destroy_on_hit: bool,
    pub destroy_requires_damage: bool,
    /// Without damage the hit does not name the attacker.
    pub last_hurt_requires_damage: bool,
}

/// `on_hit.particle_on_hit`.
#[derive(Debug)]
pub struct ParticleOnHit {
    pub count: u32,
    pub on_entity_hit: bool,
    pub on_other_hit: bool,
    /// Wire particle id.
    pub particle: i32,
    /// Icon items (network id) by filter on the projectile; the first match
    /// wins, else the item named like the projectile actor.
    pub items: Box<[(i32, Filter)]>,
}

/// `on_hit.spawn_chance`.
#[derive(Debug)]
pub struct SpawnChance {
    pub first_chance: f32,
    pub first_count: u32,
    pub second_chance: f32,
    pub second_count: u32,
    pub baby: bool,
    pub definition: ActorType,
    pub on_spawn: Box<[OnSpawn]>,
}

/// One `spawn_chance.on_spawn` trigger, fired on each spawned actor.
#[derive(Debug)]
pub struct OnSpawn {
    pub event: Arc<str>,
    /// `self` is the spawned actor, `other` the projectile.
    pub filter: Option<Filter>,
}

/// Flight state of a projectile actor.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct ProjectileFlight {
    pub owner: Option<Entity>,
    /// The server tick of its first flight tick.
    pub launched_tick: u64,
}

/// Players that used a throwable item this tick (queued by the inventory
/// system, thrown by [`apply_throws`]).
#[derive(Resource, Default)]
pub struct ThrowQueue(pub Vec<Entity>);

pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
    items: &Arc<ItemRegistry>,
) -> Result<(), String> {
    let items = Arc::clone(items);
    super::config::insert(builder, move |b: body::Projectile| {
        compile(b, &items)
            .map(|rules| ProjectileConfig(Arc::new(rules)))
            .map_err(|e| e.to_string())
    })
}

fn reject(unsupported: bool, what: &'static str) -> Result<(), ProjectileError> {
    if unsupported {
        Err(ProjectileError::Unsupported(what))
    } else {
        Ok(())
    }
}

fn compile(b: body::Projectile, items: &ItemRegistry) -> Result<ProjectileRules, ProjectileError> {
    // Fields with no effect on this server's projectiles: `isolated_physics`
    // only removes mob travel, which projectiles never have; `particle`,
    // `on_fire_time`, `reflect_immunity`, `shoot_target` and `splash_range`
    // only matter to the features rejected below.
    reject(b.anchor != ProjectileAnchor::Origin, "anchor")?;
    reject(b.catch_fire, "catch_fire")?;
    reject(b.crit_particle_on_hurt, "crit_particle_on_hurt")?;
    reject(b.destroy_on_hurt, "destroy_on_hurt")?;
    reject(b.hit_nearest_passenger, "hit_nearest_passenger")?;
    reject(b.hit_water, "hit_water")?;
    reject(b.homing, "homing")?;
    reject(b.is_dangerous, "is_dangerous")?;
    reject(b.lightning, "lightning")?;
    reject(b.potion_effect != -1, "potion_effect")?;
    reject(b.reflect_on_hurt, "reflect_on_hurt")?;
    reject(!b.shoot_sound.is_empty(), "shoot_sound")?;
    reject(b.should_bounce != ShouldBounce::No, "should_bounce")?;
    reject(b.stop_on_hurt, "stop_on_hurt")?;
    reject(
        b.uncertainty_base != 0.0 || b.uncertainty_multiplier != 0.0,
        "uncertainty",
    )?;
    let ignored = b
        .ignored_entities
        .iter()
        .map(|name| {
            ActorType::from_name(name).ok_or_else(|| ProjectileError::ActorType(name.clone()))
        })
        .collect::<Result<Box<[_]>, _>>()?;
    let offset = match b.offset {
        body::Vec3::Array([x, y, z]) => DVec3::new(f64::from(x), f64::from(y), f64::from(z)),
        body::Vec3::Vec3Object(v) => DVec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z)),
    };
    let sound = |name: String| (!name.is_empty()).then(|| Arc::from(name.as_str()));
    let mut rules = ProjectileRules {
        power: f64::from(b.power),
        gravity: f64::from(b.gravity),
        inertia: f64::from(b.inertia),
        liquid_inertia: f64::from(b.liquid_inertia),
        angle_offset: b.angle_offset,
        offset,
        multiple_targets: b.multiple_targets,
        ignored,
        hit_sound: sound(b.hit_sound),
        hit_ground_sound: sound(b.hit_ground_sound),
        impact_damage: None,
        particle: None,
        remove_on_hit: false,
        spawn_chance: None,
    };
    let Some(on_hit) = b.on_hit else {
        return Ok(rules);
    };
    for (present, name) in [
        (on_hit.arrow_effect.is_some(), "arrow_effect"),
        (on_hit.catch_fire.is_some(), "catch_fire"),
        (on_hit.definition_event.is_some(), "definition_event"),
        (on_hit.douse_fire.is_some(), "douse_fire"),
        (on_hit.freeze_on_hit.is_some(), "freeze_on_hit"),
        (on_hit.grant_xp.is_some(), "grant_xp"),
        (on_hit.hurt_owner.is_some(), "hurt_owner"),
        (on_hit.ignite.is_some(), "ignite"),
        (on_hit.mob_effect.is_some(), "mob_effect"),
        (on_hit.spawn_aoe_cloud.is_some(), "spawn_aoe_cloud"),
        (on_hit.stick_in_ground.is_some(), "stick_in_ground"),
        (on_hit.teleport_owner.is_some(), "teleport_owner"),
        (
            on_hit.thrown_potion_effect.is_some(),
            "thrown_potion_effect",
        ),
        (on_hit.wind_burst_on_hit.is_some(), "wind_burst_on_hit"),
    ] {
        if present {
            return Err(ProjectileError::OnHit(name));
        }
    }
    rules.impact_damage = on_hit.impact_damage.map(compile_impact).transpose()?;
    rules.particle = on_hit
        .particle_on_hit
        .map(|particle| compile_particle(particle, items))
        .transpose()?;
    rules.remove_on_hit = on_hit.remove_on_hit.is_some();
    rules.spawn_chance = on_hit.spawn_chance.map(compile_spawn_chance).transpose()?;
    Ok(rules)
}

fn compile_impact(
    b: body::ImpactDamageSubcomponentDefinition,
) -> Result<ImpactDamage, ProjectileError> {
    // Shield blocking and critical hits do not exist for this server's
    // targets, so `apply_knockback_to_blocking_targets` and the critical
    // damage clamps never apply.
    reject(b.catch_fire, "impact_damage catch_fire")?;
    reject(
        b.ceil_pre_critical_damage,
        "impact_damage ceil_pre_critical_damage",
    )?;
    reject(b.channeling, "impact_damage channeling")?;
    reject(
        b.difficulty_randomization != body::ImpactDamageDifficultyRandomization::None,
        "impact_damage difficulty_randomization",
    )?;
    reject(!b.filter.is_empty(), "impact_damage filter")?;
    reject(b.power_multiplier != 0.0, "impact_damage power_multiplier")?;
    Ok(ImpactDamage {
        min: b.damage.min,
        max: b.damage.max.max(b.damage.min),
        knockback: b.knockback,
        destroy_on_hit: b.destroy_on_hit,
        destroy_requires_damage: b.destroy_on_hit_requires_damage,
        last_hurt_requires_damage: b.set_last_hurt_requires_damage,
    })
}

fn compile_particle(
    b: body::ParticleOnHitSubcomponentDefinition,
    items: &ItemRegistry,
) -> Result<ParticleOnHit, ProjectileError> {
    let particle = match b.particle_type {
        ParticleType::Iconcrack => ICONCRACK_PARTICLE,
        other => return Err(ProjectileError::Particle(other)),
    };
    let items = b
        .particle_item_name
        .unwrap_or_default()
        .iter()
        .map(|(name, filter)| {
            let identifier = if name.contains(':') {
                name.clone()
            } else {
                format!("minecraft:{name}")
            };
            let item = items
                .get_by_name(&identifier)
                .ok_or_else(|| ProjectileError::Item(identifier.clone()))?;
            let filter = super::config::filter(filter).map_err(ProjectileError::Filter)?;
            Ok((item.network_id, filter))
        })
        .collect::<Result<Box<[_]>, _>>()?;
    Ok(ParticleOnHit {
        count: b.num_particles.max(0) as u32,
        on_entity_hit: b.on_entity_hit,
        on_other_hit: b.on_other_hit,
        particle,
        items,
    })
}

fn compile_spawn_chance(
    b: body::SpawnChanceSubcomponentDefinition,
) -> Result<SpawnChance, ProjectileError> {
    let definition = ActorType::from_name(&b.spawn_definition)
        .ok_or_else(|| ProjectileError::ActorType(b.spawn_definition.clone()))?;
    let on_spawn = b
        .on_spawn
        .into_iter()
        .filter(|trigger| !trigger.event.is_empty())
        .map(|trigger| {
            if trigger.target != FilterSubject::SelfValue {
                return Err(ProjectileError::OnSpawnTarget(trigger.event));
            }
            Ok(OnSpawn {
                event: Arc::from(trigger.event.as_str()),
                filter: trigger
                    .filters
                    .as_ref()
                    .map(super::config::filter)
                    .transpose()
                    .map_err(ProjectileError::Filter)?,
            })
        })
        .collect::<Result<Box<[_]>, _>>()?;
    Ok(SpawnChance {
        first_chance: b.first_spawn_chance,
        first_count: b.first_spawn_count.max(0) as u32,
        second_chance: b.second_spawn_chance,
        second_count: b.second_spawn_count.max(0) as u32,
        baby: b.spawn_baby,
        definition,
        on_spawn,
    })
}

/// Startup check, once every archetype is compiled: what a projectile's
/// `spawn_chance` spawns must be spawnable and define its `on_spawn` events.
pub(super) fn check_spawns(
    archetype: &MobArchetype,
    items: &ItemRegistry,
    get: impl Fn(&str) -> Option<Arc<MobArchetype>>,
) -> Result<(), ProjectileError> {
    let Some(body) = archetype
        .source
        .components
        .get(ActorComponent::Projectile.name())
    else {
        return Ok(());
    };
    let rules = decode::<body::Projectile>(body)
        .map_err(ProjectileError::Decode)
        .and_then(|body| compile(body, items))?;
    let Some(spawn) = rules.spawn_chance else {
        return Ok(());
    };
    let name = spawn.definition.name();
    let target = get(name).ok_or(ProjectileError::SpawnDefinition(name))?;
    for trigger in spawn.on_spawn.iter() {
        if target.events.event(&trigger.event).is_none() {
            return Err(ProjectileError::SpawnEvent(
                Arc::clone(&trigger.event),
                name,
            ));
        }
    }
    Ok(())
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
pub(super) fn derive_projectile(world: &mut World, entity: Entity) {
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
fn heading(velocity: DVec3) -> (f32, f32) {
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

/// Where a projectile's flight segment meets `aabb`: the fraction of the
/// segment, 0 when it starts inside.
fn segment_entry(aabb: &Aabb, from: DVec3, delta: DVec3) -> Option<f64> {
    let (mut enter, mut exit) = (0.0f64, 1.0f64);
    for axis in 0..3 {
        let (start, step, min, max) = (from[axis], delta[axis], aabb.min[axis], aabb.max[axis]);
        if step.abs() < 1e-12 {
            if start < min || start > max {
                return None;
            }
            continue;
        }
        let (mut near, mut far) = ((min - start) / step, (max - start) / step);
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        enter = enter.max(near);
        exit = exit.min(far);
        if enter > exit {
            return None;
        }
    }
    Some(enter)
}

/// What a projectile hit.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Target {
    Block,
    Actor(Entity),
}

/// Reused per-tick buffers.
#[derive(Default)]
pub struct ProjectileScratch {
    due: Vec<(Entity, Arc<ProjectileRules>)>,
    boxes: Vec<Aabb>,
    hits: Vec<(f64, Target)>,
}

/// System (exclusive, `PhysicsSet`, after the living physics step): fly
/// every projectile one tick and resolve its hits.
#[allow(clippy::type_complexity)]
pub fn tick_projectiles(
    world: &mut World,
    projectiles: &mut QueryState<(Entity, &ProjectileConfig), With<Projectile>>,
    mobs: &mut QueryState<
        (Entity, &Position, &PhysicsBody, &MobKind),
        (Without<Dead>, Without<Projectile>),
    >,
    players: &mut QueryState<(Entity, &Position, Option<&GameMode>, Option<&Health>), With<Player>>,
    mut scratch: Local<ProjectileScratch>,
) {
    let ProjectileScratch { due, boxes, hits } = &mut *scratch;
    due.extend(
        projectiles
            .iter(world)
            .map(|(entity, config)| (entity, Arc::clone(&config.0))),
    );
    for (projectile, rules) in due.drain(..) {
        step(world, projectile, &rules, mobs, players, boxes, hits);
    }
}

#[allow(clippy::type_complexity)]
fn step(
    world: &mut World,
    projectile: Entity,
    rules: &ProjectileRules,
    mobs: &mut QueryState<
        (Entity, &Position, &PhysicsBody, &MobKind),
        (Without<Dead>, Without<Projectile>),
    >,
    players: &mut QueryState<(Entity, &Position, Option<&GameMode>, Option<&Health>), With<Player>>,
    boxes: &mut Vec<Aabb>,
    hits: &mut Vec<(f64, Target)>,
) {
    use crate::entity::systems::physics::BlockSource;
    let (Some(from), Some(velocity), Some(flight)) = (
        world.get::<Position>(projectile).map(|p| p.0),
        world.get::<Velocity>(projectile).map(|v| v.0),
        world.get::<ProjectileFlight>(projectile).copied(),
    ) else {
        return;
    };
    // Frozen until its chunk loads, like every physics body.
    if world.block_at(from.floor().as_ivec3()).is_none() {
        return;
    }
    let now = world.resource::<TickCounter>().current;
    let ticks = now.saturating_sub(flight.launched_tick) + 1;
    if ticks == PROJECTILE_VISIBLE_TICK {
        set_invisible(world, projectile, false);
    }

    hits.clear();
    let to = from + velocity;
    let swept = Aabb::new(from.min(to), from.max(to)).inflate(DVec3::splat(1e-7));
    boxes.clear();
    collect_block_boxes(world, swept, boxes);
    if let Some(t) = boxes
        .iter()
        .filter_map(|b| segment_entry(b, from, velocity))
        .min_by(f64::total_cmp)
    {
        hits.push((t, Target::Block));
    }
    let grow = ((ticks as f64 - PROJECTILE_HIT_INFLATE_DELAY) * PROJECTILE_HIT_INFLATE_STEP)
        .clamp(0.0, PROJECTILE_HIT_INFLATE_MAX);
    let candidate = |entity: Entity, actor: ActorType| {
        entity != projectile && Some(entity) != flight.owner && !rules.ignored.contains(&actor)
    };
    for (entity, position, body, kind) in mobs.iter(world) {
        if !candidate(entity, kind.0.actor_type) {
            continue;
        }
        let aabb = Aabb::from_feet(position.0, body.width, body.height).inflate(DVec3::splat(grow));
        if let Some(t) = segment_entry(&aabb, from, velocity) {
            hits.push((t, Target::Actor(entity)));
        }
    }
    for (entity, eye, mode, health) in players.iter(world) {
        if !candidate(entity, ActorType::Player)
            || mode == Some(&GameMode::Spectator)
            || health.is_some_and(Health::is_dead)
        {
            continue;
        }
        let aabb = player_box(eye.0).inflate(DVec3::splat(grow));
        if let Some(t) = segment_entry(&aabb, from, velocity) {
            hits.push((t, Target::Actor(entity)));
        }
    }
    // Fractions of one segment order like distances from its start.
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));

    let mut removed = false;
    for &(_, target) in hits.iter() {
        removed |= on_hit(world, projectile, flight.owner, rules, from, target);
        if removed || !rules.multiple_targets {
            break;
        }
    }
    if removed {
        world.despawn(projectile);
        return;
    }

    let (width, height) = world
        .get::<RawCollisionBox>(projectile)
        .map_or((0.0, 0.0), |b| (f64::from(b.width), f64::from(b.height)));
    let in_water =
        living_liquid(world, Aabb::from_feet(from, width, height)) == Some(LiquidKind::Water);
    let mut next = velocity
        * if in_water {
            rules.liquid_inertia
        } else {
            rules.inertia
        };
    next.y -= rules.gravity;
    let (yaw, pitch) = heading(velocity);
    let mut entity = world.entity_mut(projectile);
    entity.insert((Position(to), Velocity(next)));
    if let Some(mut rotation) = entity.get_mut::<Rotation>() {
        rotation.yaw = lerp_rotation(rotation.yaw, yaw);
        rotation.pitch = lerp_rotation(rotation.pitch, pitch);
        rotation.head_yaw = rotation.yaw;
    }
}

/// Turn `current` towards `target` (degrees) by the smoothing fraction, the
/// short way round.
fn lerp_rotation(current: f32, target: f32) -> f32 {
    let mut delta = (target - current).rem_euclid(360.0);
    if delta >= 180.0 {
        delta -= 360.0;
    }
    current + delta * PROJECTILE_ROTATION_SMOOTHING
}

fn set_invisible(world: &mut World, projectile: Entity, on: bool) {
    let mut entity = world.entity_mut(projectile);
    let Some(mut flags) = entity.get_mut::<ActorFlags>() else {
        return;
    };
    flags
        .update(actor_flags::INVISIBLE, on)
        .expect("catalogue flag index");
    let word = flags.snapshot().words()[0];
    if let Some(mut metadata) = entity.get_mut::<EntityMetadata>() {
        metadata.set_long(ids::RESERVED_0, word as i64);
    }
}

/// Run the `on_hit` effects of one hit at `at`; returns whether the
/// projectile is removed.
fn on_hit(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    rules: &ProjectileRules,
    at: DVec3,
    target: Target,
) -> bool {
    let mut removed = false;
    if let (Target::Actor(actor), Some(impact)) = (target, &rules.impact_damage) {
        removed |= impact_damage(world, projectile, owner, impact, at, actor);
    }
    if let Some(particle) = &rules.particle {
        emit_particles(world, projectile, particle, at, target);
    }
    removed |= rules.remove_on_hit;
    if let Some(spawn) = &rules.spawn_chance {
        spawn_on_hit(world, projectile, spawn, at);
    }
    let sound = match target {
        Target::Actor(actor) => rules
            .hit_sound
            .as_ref()
            .filter(|_| world.get::<GameMode>(actor) != Some(&GameMode::Creative)),
        Target::Block => rules.hit_ground_sound.as_ref().or(rules.hit_sound.as_ref()),
    };
    if let Some(sound) = sound {
        play_level_sound(world, Some(projectile), at, sound, -1);
    }
    removed
}

/// `impact_damage` on a hit actor; returns whether it destroys the
/// projectile.
fn impact_damage(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    impact: &ImpactDamage,
    at: DVec3,
    actor: Entity,
) -> bool {
    if world.get::<MobKind>(actor).is_none() {
        return false;
    }
    let damage = if impact.max > impact.min {
        world
            .get_resource_or_init::<EntityRandom>()
            .0
            .gen_range(impact.min..=impact.max)
    } else {
        impact.min
    };
    let owner = owner.filter(|owner| world.get_entity(*owner).is_ok());
    let names_attacker = damage > 0.0 || !impact.last_hurt_requires_damage;
    let attacker = owner.filter(|_| names_attacker);
    let hit = Hit {
        damage,
        cause: ActorDamageCause::Projectile,
        attacker,
        by_player: attacker.is_some_and(|owner| world.get::<Player>(owner).is_some()),
        knockback_from: impact.knockback.then_some(at),
        knockback_power: knockback_power_of(world, projectile),
    };
    let applied = hurt_mob(world, actor, hit);
    impact.destroy_on_hit && (applied > 0.0 || !impact.destroy_requires_damage)
}

fn emit_particles(
    world: &World,
    projectile: Entity,
    particle: &ParticleOnHit,
    at: DVec3,
    target: Target,
) {
    let wanted = match target {
        Target::Actor(_) => particle.on_entity_hit,
        Target::Block => particle.on_other_hit,
    };
    if !wanted || particle.count == 0 {
        return;
    }
    let facts = MobFacts {
        world,
        entity: projectile,
        other: None,
    };
    let item = particle
        .items
        .iter()
        .find(|(_, filter)| filter.evaluate(&facts))
        .map(|(item, _)| *item)
        .or_else(|| {
            let kind = world.get::<MobKind>(projectile)?;
            world
                .resource::<ItemRegistryResource>()
                .0
                .get_by_name(&kind.0.identifier)
                .map(|item| item.network_id)
        })
        .unwrap_or(0);
    let data = particle.particle | (item << ICONCRACK_ITEM_SHIFT);
    for _ in 0..particle.count {
        level_event(world, at, PROJECTILE_PARTICLE_LEVEL_EVENT, data);
    }
}

/// How many actors one `spawn_chance` roll spawns.
pub(super) fn spawn_count(spawn: &SpawnChance, rng: &mut impl Rng) -> u32 {
    if rng.r#gen::<f32>() > spawn.first_chance {
        return 0;
    }
    if rng.r#gen::<f32>() <= spawn.second_chance {
        spawn.second_count
    } else {
        spawn.first_count
    }
}

fn spawn_on_hit(world: &mut World, projectile: Entity, spawn: &SpawnChance, at: DVec3) {
    let count = spawn_count(spawn, &mut world.get_resource_or_init::<EntityRandom>().0);
    if count == 0 {
        return;
    }
    let mut feet = at;
    let mut boxes = Vec::new();
    collect_block_boxes(
        world,
        Aabb::new(at, at).inflate(DVec3::splat(1e-7)),
        &mut boxes,
    );
    if let Some(top) = boxes
        .iter()
        .filter(|b| (b.min.cmple(at) & at.cmplt(b.max)).all())
        .map(|b| b.max.y)
        .reduce(f64::max)
    {
        feet.y = top;
    }
    let yaw = world.get::<Rotation>(projectile).map_or(0.0, |r| r.yaw);
    let cause = if spawn.baby {
        SpawnCause::Born
    } else {
        SpawnCause::Spawned
    };
    let Some(archetype) = world
        .get_resource::<MobRegistry>()
        .and_then(|registry| registry.get(spawn.definition.name()))
        .map(Arc::clone)
    else {
        return;
    };
    for _ in 0..count {
        let spawned = match spawn_mob(world, &archetype.identifier, feet, yaw, cause) {
            Ok(spawned) => spawned,
            Err(error) => {
                warn!(%error, "spawn_chance could not spawn");
                return;
            }
        };
        for trigger in spawn.on_spawn.iter() {
            let passes = trigger.filter.as_ref().is_none_or(|filter| {
                filter.evaluate(&MobFacts {
                    world,
                    entity: spawned,
                    other: Some(projectile),
                })
            });
            let Some(event) = passes
                .then(|| archetype.events.event(&trigger.event))
                .flatten()
            else {
                continue;
            };
            if let Err(error) = fire_event(world, spawned, &event, Some(projectile)) {
                warn!(%error, event = %trigger.event, "spawn_chance on_spawn event failed");
            }
        }
        apply_pending_definitions(world);
        super::derive_entity(world, spawned);
    }
}
