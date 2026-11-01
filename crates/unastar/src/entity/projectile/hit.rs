//! The `on_hit` effects of a projectile hit.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use rand::Rng;
use tracing::warn;
use unastar_data::actor_components::body::{
    ActorDamageCause, ImpactDamageDifficultyRandomization as Randomization,
};

use super::config::{ImpactDamage, ParticleOnHit, ProjectileRules, SpawnChance};
use super::flight::Target;
use super::launch::gaussian;
use crate::entity::EntityRandom;
use crate::entity::components::{EffectType, Effects, GameMode, Health, Player, Rotation};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::mob::combat::{Hit, hurt_mob, knockback_power_of};
use crate::entity::mob::constants::{
    ARROW_EFFECT_DURATION_DIVISOR, ICONCRACK_ITEM_SHIFT, IMPACT_DAMAGE_GAUSSIAN,
    IMPACT_DAMAGE_PER_DIFFICULTY, PROJECTILE_PARTICLE_LEVEL_EVENT,
};
use crate::entity::mob::events::{MobFacts, fire_event};
use crate::entity::mob::net::sound::{level_event, play_level_sound};
use crate::entity::mob::{MobKind, MobRegistry, SpawnCause, derive_entity, spawn_mob};
use crate::entity::systems::physics::collect_block_boxes;
use crate::registry::collision::Aabb;
use crate::server::game::effects::PotionScale;
use crate::server::game::types::ItemRegistryResource;
use crate::world::rules::Difficulty;

/// Run the `on_hit` effects of one hit at `at`; returns whether the
/// projectile is removed. `at` is where the projectile was at the start of
/// the tick, `impact` where its path met the target.
pub(super) fn on_hit(
    world: &mut World,
    projectile: Entity,
    owner: Option<Entity>,
    rules: &ProjectileRules,
    at: DVec3,
    impact: DVec3,
    target: Target,
) -> bool {
    let mut removed = false;
    if let (Target::Actor(actor), true) = (target, rules.arrow_effect) {
        arrow_effect(world, projectile, actor);
    }
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
    if rules.teleport_owner
        && let Some(owner) = owner
    {
        teleport_owner(world, owner, impact);
    }
    let potion = world
        .get::<super::ProjectilePotion>(projectile)
        .map(|p| p.0);
    // Splash and lingering potions show a splash of the potion's colour.
    if let Some(aux) = potion
        && (rules.thrown_potion || rules.aoe_cloud.is_some())
    {
        level_event(
            world,
            impact,
            POTION_SPLASH_EVENT,
            crate::server::game::effects::potion_color(aux),
        );
    }
    if let Some(aux) = potion
        && (rules.thrown_potion || rules.douse_fire)
    {
        if rules.thrown_potion {
            splash(world, aux, impact, target);
        }
        if rules.douse_fire && aux == WATER_POTION {
            douse_fire(world, impact);
        }
    }
    if let Some(cloud) = &rules.aoe_cloud
        && let Some(potion) = cloud.potion.or(potion)
    {
        super::cloud::spawn_cloud(world, cloud, potion, owner, impact);
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

/// `arrow_effect` on a hit actor: the effects of the potion the projectile
/// carries ([`super::ProjectilePotion`], the potion aux), each at its
/// amplifier for [`ARROW_EFFECT_DURATION_DIVISOR`]th of its duration (at
/// least one tick). A player gets them through the player effect rules, a
/// mob unless its class or family is immune.
fn arrow_effect(world: &mut World, projectile: Entity, actor: Entity) {
    let Some(super::ProjectilePotion(aux)) = world.get(projectile).copied() else {
        return;
    };
    let Some(potion) = unastar_rules::potion::by_id(i32::from(aux)) else {
        return;
    };
    if world.get::<Health>(actor).is_none_or(Health::is_dead) {
        return;
    }
    for effect in potion.effects {
        let kind = EffectType(effect.effect);
        let ticks = (effect.duration_ticks / ARROW_EFFECT_DURATION_DIVISOR).max(1);
        if world.get::<Player>(actor).is_some() {
            crate::server::game::effects::add_effect(world, actor, kind, effect.amplifier, ticks);
        } else if world.get::<MobKind>(actor).is_some()
            && !crate::entity::mob::combat::melee::immune_to_effect(world, actor, kind)
        {
            world
                .entity_mut(actor)
                .entry::<Effects>()
                .or_default()
                .get_mut()
                .add(kind, effect.amplifier, ticks);
        }
    }
}

/// `impact_damage` on a hit actor: whether it destroys the projectile, and
/// the health removed.
///
/// Speed damage (`power_multiplier`, arrows) is the projectile's speed
/// times the multiplier, randomised by the difficulty per
/// `difficulty_randomization` (`multiplicative`: speed × (multiplier +
/// [`IMPACT_DAMAGE_PER_DIFFICULTY`] × difficulty +
/// [`IMPACT_DAMAGE_GAUSSIAN`] × gaussian); `additive`: those terms added
/// to the speed damage), rounded up when `ceil_pre_critical_damage`, plus
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
        let difficulty = world
            .get_resource::<Difficulty>()
            .copied()
            .unwrap_or_default()
            .wire() as f32;
        let randomised = |world: &mut World| {
            difficulty * IMPACT_DAMAGE_PER_DIFFICULTY
                + gaussian(&mut world.get_resource_or_init::<EntityRandom>().0) as f32
                    * IMPACT_DAMAGE_GAUSSIAN
        };
        let mut damage = match impact.difficulty_randomization {
            Randomization::None => speed * impact.power_multiplier,
            Randomization::Multiplicative => speed * (impact.power_multiplier + randomised(world)),
            Randomization::Additive => speed * impact.power_multiplier + randomised(world),
        };
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
    let damage = damage + impaling_bonus(world, projectile, actor);
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
    let factor = if applied <= 0.0 && rules.bounce_without_damage {
        DVec3::splat(-0.1)
    } else if applied > 0.0 && rules.stop_on_hurt {
        DVec3::new(-0.01, -0.1, -0.01)
    } else {
        return;
    };
    let mut entity = world.entity_mut(projectile);
    if let Some(mut velocity) = entity.get_mut::<crate::entity::components::Velocity>() {
        velocity.0 *= factor;
    }
    if applied > 0.0 && rules.stop_on_hurt {
        entity.insert(super::DealtDamage);
    }
}

/// Extra damage of an Impaling trident against an actor in water.
fn impaling_bonus(world: &World, projectile: Entity, actor: Entity) -> f32 {
    let Some(super::Impaling(level)) = world.get::<super::Impaling>(projectile).copied() else {
        return 0.0;
    };
    let Some(position) = world.get::<crate::entity::components::Position>(actor) else {
        return 0.0;
    };
    let feet = if world.get::<Player>(actor).is_some() {
        position.0 - DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0)
    } else {
        position.0
    };
    if crate::server::game::body_in_water(world, feet) {
        2.5 * f32::from(level)
    } else {
        0.0
    }
}

/// `teleport_owner` (ender pearls), as measured on BDS 1.26.51: the owning
/// player, if alive and not a spectator, moves to the hit point (teleport
/// particles there, the pearl sound at both ends) and takes 5 fall damage.
fn teleport_owner(world: &mut World, owner: Entity, at: DVec3) {
    if world.get::<Player>(owner).is_none()
        || world.get::<GameMode>(owner) == Some(&GameMode::Spectator)
        || world
            .get::<crate::entity::components::Health>(owner)
            .is_none_or(|h| h.is_dead())
    {
        return;
    }
    let Some(from) = world
        .get::<crate::entity::components::Position>(owner)
        .map(|p| p.0)
    else {
        return;
    };
    level_event(world, at, PEARL_TELEPORT_PARTICLES, 0);
    crate::server::game::item_use::teleport(
        world,
        owner,
        at,
        crate::server::game::item_use::TeleportCause::EnderPearl,
    );
    let to = at + DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0);
    level_event(world, from, PEARL_TELEPORT_SOUND, 0);
    level_event(world, to, PEARL_TELEPORT_SOUND, 0);
    hurt_mob(
        world,
        owner,
        Hit {
            damage: PEARL_FALL_DAMAGE,
            cause: ActorDamageCause::Fall,
            attacker: None,
            by_player: false,
            knockback_from: None,
            knockback_power: crate::entity::mob::components::KnockbackPower::DEFAULT,
        },
    );
}

/// Level events and damage of a pearl landing, measured on BDS 1.26.51
/// (client-diff `item-use-ranged`): teleport particles 2013, the pearl
/// teleport sound 1032, 5 fall damage.
const PEARL_TELEPORT_PARTICLES: i32 = 2013;
const PEARL_TELEPORT_SOUND: i32 = 1032;
const PEARL_FALL_DAMAGE: f32 = 5.0;

/// Level event of a potion splash; its data is the potion's ARGB colour
/// (BDS 1.26.51 capture).
const POTION_SPLASH_EVENT: i32 = 2002;

/// Aux of the water potion.
const WATER_POTION: i16 = 0;
/// Reach of a splash, blocks of horizontal distance.
const SPLASH_RANGE: f64 = 4.0;
/// Added to an actor's half-width when measuring a splash's distance.
const SPLASH_MARGIN: f64 = 0.15;
/// Vertical reach of a splash above and below the impact (public
/// reference).
const SPLASH_HEIGHT: f64 = 2.0;

/// Horizontal distance from `at` to a box of half-width `half` centred on
/// `centre`.
fn distance_to_square(at: DVec3, centre: DVec3, half: f64) -> f64 {
    let dx = ((at.x - centre.x).abs() - half).max(0.0);
    let dz = ((at.z - centre.z).abs() - half).max(0.0);
    dx.hypot(dz)
}

/// `thrown_potion_effect`, as measured on BDS 1.26.51 (client-diff
/// `item-use-splash-distance`): every living actor gets the potion scaled by
/// `1 - d / 4`, where `d` is the horizontal distance from the impact to a
/// square around the actor's feet of half-width (half its width + 0.15;
/// 0.45 for players), in any direction. The actor hit directly gets it in
/// full.
fn splash(world: &mut World, aux: i16, at: DVec3, target: Target) {
    let direct = match target {
        Target::Actor(actor) => Some(actor),
        Target::Block => None,
    };
    let mut targets: Vec<(Entity, f64)> = Vec::new();
    let mut living = world.query_filtered::<(
        Entity,
        &crate::entity::components::Position,
        &crate::entity::components::Health,
        Option<&Player>,
        Option<&crate::entity::definition::components::RawCollisionBox>,
    ), Or<(With<Player>, With<crate::entity::components::Living>)>>();
    for (entity, position, health, player, collision) in living.iter(world) {
        if health.is_dead() {
            continue;
        }
        let (feet, width, height) = if player.is_some() {
            (
                position.0 - DVec3::new(0.0, crate::entity::PLAYER_EYE_HEIGHT, 0.0),
                0.6,
                1.8,
            )
        } else {
            let (w, h) = collision.map_or((0.0, 0.0), |b| (b.width, b.height));
            (position.0, f64::from(w), f64::from(h))
        };
        let scale = if Some(entity) == direct {
            1.0
        } else {
            if at.y < feet.y - SPLASH_HEIGHT || at.y > feet.y + height + SPLASH_HEIGHT {
                continue;
            }
            let d = distance_to_square(at, feet, width / 2.0 + SPLASH_MARGIN);
            if d >= SPLASH_RANGE {
                continue;
            }
            1.0 - d / SPLASH_RANGE
        };
        targets.push((entity, scale));
    }
    for (entity, scale) in targets {
        crate::server::game::effects::apply_potion(
            world,
            entity,
            aux,
            PotionScale {
                duration: scale,
                instant: scale,
            },
        );
    }
}

/// `douse_fire`: a water splash puts out fire in the hit block and the
/// blocks beside it.
fn douse_fire(world: &mut World, at: DVec3) {
    let centre = at.floor().as_ivec3();
    let registry = world
        .resource::<crate::server::game::types::BlockRegistryResource>()
        .0
        .clone();
    let Some(air) = registry.default_state_id_by_name("minecraft:air") else {
        return;
    };
    for offset in [
        glam::IVec3::ZERO,
        glam::IVec3::X,
        glam::IVec3::NEG_X,
        glam::IVec3::Y,
        glam::IVec3::Z,
        glam::IVec3::NEG_Z,
    ] {
        let pos = centre + offset;
        let burning = crate::server::game::block_at(world, (pos.x, pos.y, pos.z))
            .and_then(|id| registry.get_by_runtime_id(id))
            .is_some_and(|b| {
                matches!(
                    b.string_id.as_str(),
                    "minecraft:fire" | "minecraft:soul_fire"
                )
            });
        if burning {
            crate::server::game::blocks::changed_by_actor(world, |world| {
                crate::server::game::set_block(world, pos, air, false);
            });
        }
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
    // A target without a runtime archetype (see `SPAWNS_WITHOUT_ARCHETYPE`)
    // is skipped before rolling.
    let Some(archetype) = world
        .get_resource::<MobRegistry>()
        .and_then(|registry| registry.get(spawn.definition.name()))
        .map(Arc::clone)
    else {
        return;
    };
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
