//! The `on_hit` effects of a projectile hit.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;
use tracing::warn;
use unastar_data::actor_components::body::ActorDamageCause;

use super::config::{ImpactDamage, ParticleOnHit, ProjectileRules, SpawnChance};
use super::flight::Target;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Player, Rotation};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::mob::combat::{Hit, hurt_mob, knockback_power_of};
use crate::entity::mob::constants::{ICONCRACK_ITEM_SHIFT, PROJECTILE_PARTICLE_LEVEL_EVENT};
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::net::sound::{level_event, play_level_sound};
use crate::entity::mob::{MobKind, MobRegistry, SpawnCause, derive_entity, spawn_mob};
use crate::entity::systems::physics::collect_block_boxes;
use crate::registry::collision::Aabb;
use crate::server::game::types::ItemRegistryResource;

/// Run the `on_hit` effects of one hit at `at`; returns whether the
/// projectile is removed.
pub(super) fn on_hit(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    rules: &ProjectileRules,
    at: DVec3,
    target: Target,
) -> bool {
    if let Target::Actor(actor) = target
        && crate::entity::mob::teleport::projectile_escape(world, actor, projectile)
    {
        return false;
    }
    let mut removed = false;
    if let (Target::Actor(actor), Some(impact)) = (target, &rules.impact_damage) {
        let (destroyed, applied) = impact_damage(world, projectile, owner, impact, at, actor);
        removed |= destroyed;
        if !destroyed {
            rebound(world, projectile, rules, applied);
        }
    }
    if let (Target::Block, Some(shake)) = (target, rules.stick_in_ground) {
        stick(world, projectile, at, shake);
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

/// `impact_damage` on a hit actor: whether it destroys the projectile, and
/// the health removed.
///
/// Speed damage (`power_multiplier`, arrows) is the projectile's speed
/// times the multiplier, rounded up when `ceil_pre_critical_damage`, plus
/// for a critical shot a random bonus of up to half that plus one
/// (public reference; recorded as unmeasured in `bds-quirks.md`).
fn impact_damage(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    impact: &ImpactDamage,
    at: DVec3,
    actor: Entity,
) -> (bool, f32) {
    if world.get::<MobKind>(actor).is_none() && world.get::<Player>(actor).is_none() {
        return (false, 0.0);
    }
    let filtered_out = impact.filter.as_deref().is_some_and(|wanted| {
        world
            .get::<MobKind>(actor)
            .is_none_or(|kind| kind.0.identifier.as_ref() != wanted)
    });
    let damage = if filtered_out {
        0.0
    } else if impact.power_multiplier > 0.0 {
        let speed = world
            .get::<crate::entity::components::Velocity>(projectile)
            .map_or(0.0, |v| v.0.length() as f32);
        let mut damage = speed * impact.power_multiplier;
        if impact.ceil_pre_critical {
            damage = damage.ceil();
        }
        if world
            .get::<super::ProjectileFlight>(projectile)
            .is_some_and(|flight| flight.critical)
        {
            let bonus = (damage / 2.0) as i32 + 2;
            damage += world
                .get_resource_or_init::<EntityRandom>()
                .0
                .gen_range(0..bonus) as f32;
        }
        damage
    } else if impact.max > impact.min {
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
    // Players take hits through the player hurt path (game mode, armour,
    // death); mobs through the mob one.
    let applied = if world.get::<Player>(actor).is_some() {
        crate::entity::player_life::hurt_player(world, actor, &hit)
    } else {
        hurt_mob(world, actor, hit)
    };
    (
        impact.destroy_on_hit && (applied > 0.0 || !impact.destroy_requires_damage),
        applied,
    )
}

/// A projectile that hit an actor without being destroyed: one that dealt
/// no damage bounces back (`should_bounce`), one that hurt drops away
/// (`stop_on_hurt`, tridents).
fn rebound(world: &mut World, projectile: Entity, rules: &ProjectileRules, applied: f32) {
    let Some(mut velocity) = world.get_mut::<crate::entity::components::Velocity>(projectile)
    else {
        return;
    };
    if applied <= 0.0 && rules.bounce_without_damage {
        velocity.0 *= -0.1;
    } else if applied > 0.0 && rules.stop_on_hurt {
        velocity.0 *= DVec3::new(-0.01, -0.1, -0.01);
    }
}

/// `stick_in_ground`: stop at the hit point and shake for `shake` ticks
/// (`ActorEvent` 39 with the shake time, as BDS sends for arrows).
fn stick(world: &mut World, projectile: Entity, at: DVec3, shake: u32) {
    let now = world
        .resource::<crate::ecs::resources::TickCounter>()
        .current;
    world.entity_mut(projectile).insert((
        super::Stuck { since: now },
        crate::entity::components::Position(at),
        crate::entity::components::Velocity(DVec3::ZERO),
    ));
    crate::entity::mob::combat::send_actor_event(
        world,
        projectile,
        jolyne::valentine::types::EnumsActorEvent::Shake,
        i32::try_from(shake).unwrap_or(i32::MAX),
    );
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
                .map(|item| item.network_id())
        })
        .unwrap_or(0);
    // Only the item-crack particle names an item.
    let data = if particle.particle == crate::entity::mob::constants::ICONCRACK_PARTICLE {
        particle.particle | (item << ICONCRACK_ITEM_SHIFT)
    } else {
        particle.particle
    };
    for _ in 0..particle.count {
        level_event(world, at, PROJECTILE_PARTICLE_LEVEL_EVENT, data);
    }
}

/// How many actors one `spawn_chance` roll spawns.
pub(crate) fn spawn_count(spawn: &SpawnChance, rng: &mut impl Rng) -> u32 {
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
        derive_entity(world, spawned);
    }
}
