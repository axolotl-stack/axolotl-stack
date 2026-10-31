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
    #[error(
        "particle type {0:?} is not implemented; only iconcrack and snowballpoof have measured ids"
    )]
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
    /// Teleport the owner to a collision-free impact destination.
    pub teleport_owner: bool,
    /// Extinguish water-bottle targets before ordinary potion effects.
    pub douse_fire: bool,
    /// Authored aux override; -1 uses the projectile's captured contents.
    pub thrown_potion: Option<i32>,
    pub cloud: Option<CloudRules>,
    /// `on_hit.stick_in_ground`: stays where it hit a block, shaking for
    /// this many ticks.
    pub stick_in_ground: Option<u32>,
    /// `should_bounce` `if_no_damage_dealt`: an actor hit that deals no
    /// damage sends it back.
    pub bounce_without_damage: bool,
    /// `stop_on_hurt`: it drops away from an actor it hurt.
    pub stop_on_hurt: bool,
}

/// Authored area-effect-cloud lifetime and per-recipient reuse parameters.
#[derive(Clone, Debug)]
pub struct CloudRules {
    pub duration_ticks: u32,
    pub radius: f64,
    pub radius_on_use: f64,
    pub reapplication_delay: u32,
    pub affect_owner: bool,
    /// -1 uses the projectile's captured potion contents.
    pub potion: i32,
}

/// `on_hit.impact_damage`.
#[derive(Debug, Clone)]
pub struct ImpactDamage {
    pub min: f32,
    pub max: f32,
    pub knockback: bool,
    pub destroy_on_hit: bool,
    pub destroy_requires_damage: bool,
    /// Without damage the hit does not name the attacker.
    pub last_hurt_requires_damage: bool,
    /// Damage per block-per-tick of speed instead of `min..=max` (arrows).
    pub power_multiplier: f32,
    /// Round speed damage up before the critical bonus.
    pub ceil_pre_critical: bool,
    /// Only this actor is damaged (e.g. snowballs hurt blazes); others are
    /// hit without damage.
    pub filter: Option<Arc<str>>,
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
    // `eye_height` launches from the thrower's eye, which is where every
    // launch starts (only players throw on this server; their position is
    // the eye).
    reject(
        !matches!(
            b.anchor,
            ProjectileAnchor::Origin | ProjectileAnchor::EyeHeight
        ),
        "anchor",
    )?;
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
    reject(
        !matches!(
            b.should_bounce,
            ShouldBounce::No | ShouldBounce::IfNoDamageDealt
        ),
        "should_bounce",
    )?;
    // `uncertainty_*` is the spread of mob shots; the items players shoot
    // with (bow, crossbow, trident) set their own spread at launch.
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
        teleport_owner: false,
        douse_fire: false,
        thrown_potion: None,
        cloud: None,
        stick_in_ground: None,
        bounce_without_damage: b.should_bounce == ShouldBounce::IfNoDamageDealt,
        stop_on_hurt: b.stop_on_hurt,
    };
    let Some(on_hit) = b.on_hit else {
        return Ok(rules);
    };
    for (present, name) in [
        (on_hit.catch_fire.is_some(), "catch_fire"),
        (on_hit.definition_event.is_some(), "definition_event"),
        (on_hit.freeze_on_hit.is_some(), "freeze_on_hit"),
        (on_hit.grant_xp.is_some(), "grant_xp"),
        (on_hit.hurt_owner.is_some(), "hurt_owner"),
        (on_hit.ignite.is_some(), "ignite"),
        (on_hit.mob_effect.is_some(), "mob_effect"),
        (on_hit.wind_burst_on_hit.is_some(), "wind_burst_on_hit"),
    ] {
        if present {
            return Err(ProjectileError::OnHit(name));
        }
    }
    rules.teleport_owner = on_hit.teleport_owner.is_some();
    rules.douse_fire = on_hit.douse_fire.is_some();
    rules.thrown_potion = on_hit.thrown_potion_effect.map(|value| value.effect);
    rules.cloud = on_hit.spawn_aoe_cloud.map(compile_cloud).transpose()?;
    rules.impact_damage = on_hit.impact_damage.map(compile_impact).transpose()?;
    rules.particle = on_hit
        .particle_on_hit
        .map(|particle| compile_particle(particle, items))
        .transpose()?;
    rules.remove_on_hit = on_hit.remove_on_hit.is_some();
    // Plain arrows carry no potion, so `arrow_effect` has nothing to apply;
    // bows only shoot plain arrows (tipped arrows are not implemented).
    rules.stick_in_ground = on_hit
        .stick_in_ground
        .map(|stick| (stick.shake_time * 20.0).round().max(0.0) as u32);
    rules.spawn_chance = on_hit.spawn_chance.map(compile_spawn_chance).transpose()?;
    Ok(rules)
}

/// Compile finite, bounded cloud values without silently accepting unsupported particles.
fn compile_cloud(
    value: body::SpawnAoECloudSubcomponentDefinition,
) -> Result<CloudRules, ProjectileError> {
    reject(
        value.particle != ParticleType::Mobspellambient,
        "cloud particle",
    )?;
    let seconds = match value.duration {
        body::EffectDuration::Int(value) => f64::from(value),
        body::EffectDuration::String(value) => value
            .parse::<f64>()
            .map_err(|_| ProjectileError::Unsupported("cloud duration"))?,
    };
    reject(
        !seconds.is_finite() || seconds < 0.0 || seconds > f64::from(i32::MAX) / 20.0,
        "cloud duration",
    )?;
    reject(
        !value.radius.is_finite() || value.radius < 0.0 || value.radius > 128.0,
        "cloud radius",
    )?;
    reject(
        !value.radius_on_use.is_finite() || value.radius_on_use.abs() > 128.0,
        "cloud radius_on_use",
    )?;
    reject(value.reapplication_delay < 0, "cloud reapplication_delay")?;
    Ok(CloudRules {
        duration_ticks: (seconds * 20.0).round() as u32,
        radius: f64::from(value.radius),
        radius_on_use: if value.radius_on_use == -1.0 {
            0.0
        } else {
            f64::from(value.radius_on_use)
        },
        reapplication_delay: value.reapplication_delay as u32,
        affect_owner: value.affect_owner,
        potion: value.potion,
    })
}

fn compile_impact(
    b: body::ImpactDamageSubcomponentDefinition,
) -> Result<ImpactDamage, ProjectileError> {
    // Shield blocking and critical hits do not exist for this server's
    // targets, so `apply_knockback_to_blocking_targets` and the critical
    // damage clamps never apply.
    reject(b.catch_fire, "impact_damage catch_fire")?;
    reject(b.channeling, "impact_damage channeling")?;
    // `difficulty_randomization` varies the damage of mob shots by
    // difficulty; only players shoot on this server, so it never applies.

    Ok(ImpactDamage {
        min: b.damage.min,
        max: b.damage.max.max(b.damage.min),
        knockback: b.knockback,
        destroy_on_hit: b.destroy_on_hit,
        destroy_requires_damage: b.destroy_on_hit_requires_damage,
        last_hurt_requires_damage: b.set_last_hurt_requires_damage,
        power_multiplier: b.power_multiplier,
        ceil_pre_critical: b.ceil_pre_critical_damage,
        filter: (!b.filter.is_empty()).then(|| {
            let name = b.filter.as_str();
            let name = if name.contains(':') {
                name.to_owned()
            } else {
                format!("minecraft:{name}")
            };
            Arc::from(name.as_str())
        }),
    })
}

fn compile_particle(
    b: body::ParticleOnHitSubcomponentDefinition,
    items: &ItemRegistry,
) -> Result<ParticleOnHit, ProjectileError> {
    let particle = match b.particle_type {
        ParticleType::Iconcrack => ICONCRACK_PARTICLE,
        ParticleType::Snowballpoof => crate::entity::mob::constants::SNOWBALLPOOF_PARTICLE,
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
/// `spawn_chance` targets that are available must define their `on_spawn` events.
/// An unavailable optional actor fails only that runtime spawn attempt.
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
    let Some(target) = get(name) else {
        return Ok(());
    };
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
