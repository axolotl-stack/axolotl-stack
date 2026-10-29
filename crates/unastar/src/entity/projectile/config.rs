//! Compiling `minecraft:projectile` into [`ProjectileRules`].

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use thiserror::Error;
use unastar_data::actor_components::body::{
    self, FilterSubject, ParticleType, ProjectileAnchor, ShouldBounce,
};
use unastar_data::actor_components::{ActorComponent, decode};
use unastar_data::actor_identifiers::ActorType;
use unastar_rules::filter::Filter;

use crate::entity::definition::{ComponentError, ComponentKindsBuilder, DefinitionError};
use crate::entity::mob::MobArchetype;
use crate::entity::mob::components::common::{FilterError, filter as parse_filter, insert};
use crate::entity::mob::constants::ICONCRACK_PARTICLE;
use crate::registry::ItemRegistry;

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
    Filter(#[from] FilterError),
    #[error("spawn_chance spawns {0}, which is not a spawnable archetype")]
    SpawnDefinition(&'static str),
    #[error("spawn_chance on_spawn event {0} is not defined by {1}")]
    SpawnEvent(Arc<str>, &'static str),
    /// The body does not decode (the generated decoder's report).
    #[error("body: {0}")]
    Decode(String),
}

impl ComponentError for ProjectileError {}

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

/// Register the `minecraft:projectile` component kind.
pub fn register_component_kinds(
    builder: &mut ComponentKindsBuilder,
    items: &Arc<ItemRegistry>,
) -> Result<(), DefinitionError> {
    let items = Arc::clone(items);
    insert(builder, move |b: body::Projectile| {
        compile(b, &items).map(|rules| ProjectileConfig(Arc::new(rules)))
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
            let filter = parse_filter(filter)?;
            Ok::<_, ProjectileError>((item.network_id(), filter))
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
                filter: trigger.filters.as_ref().map(parse_filter).transpose()?,
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
pub(crate) fn check_spawns(
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
