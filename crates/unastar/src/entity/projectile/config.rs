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
    /// Authoritative actor reference used by aimed launches.
    pub anchor: ProjectileAnchor,
    /// Gaussian spread for definition-owned shots before difficulty adjustment.
    pub uncertainty_base: f64,
    pub uncertainty_multiplier: f64,
    /// Projectiles may be redirected by accepted attacks after this delay.
    pub reflect_on_hurt: bool,
    pub reflect_immunity_ticks: u64,
    pub owner_immunity_ticks: u64,
    /// Entity ignition duration used by impact and legacy catch-fire rules.
    pub fire_ticks: u64,
    /// Optional block ignition, with mob-griefing policy owned by the host.
    pub block_fire: Option<BlockFire>,
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
    /// `on_hit.stick_in_ground`: stays where it hit a block, shaking for
    /// this many ticks.
    pub stick_in_ground: Option<u32>,
    /// `should_bounce` `if_no_damage_dealt`: an actor hit that deals no
    /// damage sends it back.
    pub bounce_without_damage: bool,
    /// `stop_on_hurt`: it drops away from an actor it hurt.
    pub stop_on_hurt: bool,
}

/// Block ignition configured by `on_hit.catch_fire`.
#[derive(Debug, Clone, Copy)]
pub struct BlockFire {
    pub affected_by_griefing: bool,
    pub entity_fire_ticks: u64,
}

/// `on_hit.impact_damage`.
#[derive(Debug, Clone)]
pub struct ImpactDamage {
    pub min: f32,
    pub max: f32,
    pub knockback: bool,
    pub catch_fire: bool,
    /// Non-default randomization remains unsupported for actor-owned shots.
    pub difficulty_randomization: body::ImpactDamageDifficultyRandomization,
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
    let ticks = |seconds: f32| -> Result<u64, ProjectileError> {
        if !seconds.is_finite() || !(0.0..=86_400.0).contains(&seconds) {
            return Err(ProjectileError::Unsupported("invalid duration"));
        }
        Ok((f64::from(seconds) * 20.0).ceil() as u64)
    };
    for value in [
        b.power,
        b.gravity,
        b.inertia,
        b.liquid_inertia,
        b.uncertainty_base,
        b.uncertainty_multiplier,
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(ProjectileError::Unsupported("invalid flight parameter"));
        }
    }
    reject(b.catch_fire, "legacy catch_fire")?;
    reject(b.crit_particle_on_hurt, "crit_particle_on_hurt")?;
    reject(b.destroy_on_hurt, "destroy_on_hurt")?;
    reject(b.hit_nearest_passenger, "hit_nearest_passenger")?;
    reject(b.hit_water, "hit_water")?;
    reject(b.homing, "homing")?;
    reject(b.is_dangerous, "is_dangerous")?;
    reject(b.lightning, "lightning")?;
    reject(b.potion_effect != -1, "potion_effect")?;
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
    if !offset.is_finite() || !b.angle_offset.is_finite() {
        return Err(ProjectileError::Unsupported("invalid launch offset"));
    }
    let sound = |name: String| (!name.is_empty()).then(|| Arc::from(name.as_str()));
    let mut rules = ProjectileRules {
        power: f64::from(b.power),
        anchor: b.anchor,
        uncertainty_base: f64::from(b.uncertainty_base),
        uncertainty_multiplier: f64::from(b.uncertainty_multiplier),
        reflect_on_hurt: b.reflect_on_hurt,
        reflect_immunity_ticks: ticks(b.reflect_immunity)?,
        // The pinned component schema predates this authored field; retain the
        // native five-tick launch default until that schema exposes overrides.
        owner_immunity_ticks: 5,
        fire_ticks: ticks(b.on_fire_time)?,
        block_fire: None,
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
        stick_in_ground: None,
        bounce_without_damage: b.should_bounce == ShouldBounce::IfNoDamageDealt,
        stop_on_hurt: b.stop_on_hurt,
    };
    let Some(on_hit) = b.on_hit else {
        return Ok(rules);
    };
    for (present, name) in [
        (on_hit.definition_event.is_some(), "definition_event"),
        (on_hit.douse_fire.is_some(), "douse_fire"),
        (on_hit.freeze_on_hit.is_some(), "freeze_on_hit"),
        (on_hit.grant_xp.is_some(), "grant_xp"),
        (on_hit.hurt_owner.is_some(), "hurt_owner"),
        (on_hit.ignite.is_some(), "ignite"),
        (on_hit.mob_effect.is_some(), "mob_effect"),
        (on_hit.spawn_aoe_cloud.is_some(), "spawn_aoe_cloud"),
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
    rules.block_fire = on_hit
        .catch_fire
        .map(|fire| {
            Ok::<_, ProjectileError>(BlockFire {
                affected_by_griefing: fire.fire_affected_by_griefing,
                entity_fire_ticks: if fire.on_fire_time == 0.0 {
                    rules.fire_ticks
                } else {
                    ticks(fire.on_fire_time)?
                },
            })
        })
        .transpose()?;
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

fn compile_impact(
    b: body::ImpactDamageSubcomponentDefinition,
) -> Result<ImpactDamage, ProjectileError> {
    // Shield blocking and critical hits do not exist for this server's
    // targets, so `apply_knockback_to_blocking_targets` and the critical
    // damage clamps never apply.
    reject(b.channeling, "impact_damage channeling")?;
    if [b.damage.min, b.damage.max, b.power_multiplier]
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(ProjectileError::Unsupported("invalid impact damage"));
    }

    Ok(ImpactDamage {
        min: b.damage.min,
        max: b.damage.max.max(b.damage.min),
        knockback: b.knockback,
        catch_fire: b.catch_fire,
        difficulty_randomization: b.difficulty_randomization,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::resources::TickCounter;
    use crate::entity::components::{Position, Rotation, Velocity};
    use crate::entity::projectile::{ProjectileFlight, reflect};
    use crate::server::game::types::ItemRegistryResource;
    use crate::server::game::{GameServer, ServerConfig};

    /// Decode the actual authored projectile body before applying a test override.
    fn body() -> body::Projectile {
        decode(
            unastar_data::actor_definitions::get("minecraft:small_fireball")
                .unwrap()
                .components
                .get("minecraft:projectile")
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn reflection_duration_boundary_uses_a_compiled_authored_delay() {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let world = server.ecs.world_mut();
        let mut source = body();
        source.reflect_immunity = 0.5;
        let rules = compile(source, &world.resource::<ItemRegistryResource>().0).unwrap();
        assert_eq!(rules.reflect_immunity_ticks, 10);
        let target = world
            .spawn((
                ProjectileConfig(Arc::new(rules)),
                ProjectileFlight {
                    owner: None,
                    launched_tick: 0,
                    critical: false,
                },
                Position(DVec3::ZERO),
                Velocity(DVec3::Z),
            ))
            .id();
        let attacker = world.spawn(Rotation::default()).id();
        world.resource_mut::<TickCounter>().current = 9;
        assert!(!reflect::from_attack(world, target, Some(attacker)));
        assert_eq!(world.get::<ProjectileFlight>(target).unwrap().owner, None);
        world.resource_mut::<TickCounter>().current = 10;
        assert!(reflect::from_attack(world, target, Some(attacker)));
        assert_eq!(
            world.get::<ProjectileFlight>(target).unwrap().owner,
            Some(attacker)
        );
    }

    #[test]
    fn malformed_flight_impact_and_duration_values_are_rejected_by_the_factory() {
        let server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..Default::default()
        });
        let items = &server.ecs.world().resource::<ItemRegistryResource>().0;
        for value in [f32::NAN, f32::INFINITY, -1.0] {
            let mut source = body();
            source.power = value;
            assert!(compile(source, items).is_err());
            let mut source = body();
            source.reflect_immunity = value;
            assert!(compile(source, items).is_err());
            let mut source = body();
            source
                .on_hit
                .as_mut()
                .unwrap()
                .impact_damage
                .as_mut()
                .unwrap()
                .damage
                .min = value;
            assert!(compile(source, items).is_err());
        }
    }
}
