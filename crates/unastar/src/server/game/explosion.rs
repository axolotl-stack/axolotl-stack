//! Explosions: which blocks an explosion breaks and how hard it hits the
//! actors around it, as BDS 1.26.51 does (measured with the TNT sweeps in
//! `crates/unastar-data/data/measured/explosions/`, `tools/bds-sweep/tnt.py`;
//! `explosion/tests.rs` compares by distribution and per hit).
//!
//! - **Blocks.** One ray leaves the centre through each cell on the outer
//!   shell of a 16 x 16 x 16 grid (1352 rays). A ray starts with
//!   `power x (0.7 + 0.6 U)` and walks in 0.3-block steps; in every block
//!   that is not air it loses `(resistance + 0.3) x 0.3` and breaks the
//!   block while it still has strength; every step costs 0.225 more. A TNT
//!   explosion drops every block it breaks (by hand, without the correct
//!   tool: cobblestone drops, glass does not).
//! - **Actors.** An actor's distance is from the centre to its feet, less
//!   half its width; within twice the power its impact is
//!   `(1 - distance / (2 x power)) x exposure`, the share of points on its
//!   box that see the centre, and it takes
//!   `(impact^2 + impact) / 2 x 7 x 2 x power + 1` damage: a mob fully
//!   behind a wall still takes 1.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;

use super::block_drops::{pop_resource, popped_drops};
use crate::entity::components::{GameMode, PhysicsBody, Player, Position, Velocity};
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::sensing::can_see;
use crate::entity::mob::combat::{Hit, hurt_mob};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::{PLAYER_BOX_HEIGHT, PLAYER_BOX_WIDTH};
use crate::entity::mob::state::Dead;
use crate::entity::systems::BlockSource;
use crate::entity::{EntityRandom, PLAYER_EYE_HEIGHT};
use crate::registry::collision::Aabb;

mod viewers;

#[cfg(test)]
mod tests;

/// Rays per edge of the ray grid.
const GRID: usize = 16;
/// A ray's starting strength is `power x (BASE + SPREAD x U)`.
const RAY_BASE: f32 = 0.7;
const RAY_SPREAD: f32 = 0.6;
/// Length of one ray step, in blocks.
const STEP: f32 = 0.3;
/// Strength a ray loses on every step.
const STEP_COST: f32 = 0.225;
/// A block costs `(resistance + OFFSET) x FACTOR` per step inside it.
const RESISTANCE_OFFSET: f32 = 0.3;
const RESISTANCE_FACTOR: f32 = 0.3;
/// Damage of an impact of 1 per block of radius, before the constant 1.
const DAMAGE_FACTOR: f64 = 7.0;

/// The directions of the rays: from the centre through every cell on the
/// grid's outer shell.
fn ray_directions() -> impl Iterator<Item = glam::Vec3> {
    let last = (GRID - 1) as f32;
    (0..GRID).flat_map(move |i| {
        (0..GRID).flat_map(move |j| {
            (0..GRID).filter_map(move |k| {
                let shell = [i, j, k].iter().any(|&n| n == 0 || n == GRID - 1);
                shell.then(|| {
                    (glam::Vec3::new(i as f32, j as f32, k as f32) / last * 2.0 - glam::Vec3::ONE)
                        .normalize()
                })
            })
        })
    })
}

/// The blocks an explosion of `power` at `centre` breaks, in the order the
/// rays reach them (each once).
pub(crate) fn blasted_blocks(
    blocks: &impl BlockSource,
    centre: DVec3,
    power: f32,
    max_resistance: f32,
    rng: &mut impl Rng,
) -> Vec<IVec3> {
    let registry = blocks.registry();
    let air = registry.default_state_id_by_name("minecraft:air");
    let start = centre.as_vec3();
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for direction in ray_directions() {
        let mut strength = power * (RAY_BASE + RAY_SPREAD * rng.r#gen::<f32>());
        let mut point = start;
        while strength > 0.0 {
            let cell = point.floor().as_ivec3();
            let state = blocks.block_at(cell);
            if state != air {
                let resistance = state
                    .and_then(|state| registry.get_by_runtime_id(state))
                    .map_or(f32::MAX, |entry| entry.resistance)
                    .min(max_resistance);
                strength -= (resistance + RESISTANCE_OFFSET) * RESISTANCE_FACTOR;
                if strength > 0.0 && seen.insert(cell) {
                    out.push(cell);
                }
            }
            point += direction * STEP;
            strength -= STEP_COST;
        }
    }
    out
}

/// The share of sample points on `aabb` with a clear line to `centre`.
pub(crate) fn exposure(blocks: &impl BlockSource, centre: DVec3, aabb: &Aabb) -> f64 {
    let size = aabb.max - aabb.min;
    let step = DVec3::ONE / (size * 2.0 + DVec3::ONE);
    if step.x < 0.0 || step.y < 0.0 || step.z < 0.0 {
        return 0.0;
    }
    // Centre the sample grid on the box's horizontal faces.
    let offset_x = (1.0 - (1.0 / step.x).floor() * step.x) / 2.0;
    let offset_z = (1.0 - (1.0 / step.z).floor() * step.z) / 2.0;
    let (mut seen, mut total) = (0u32, 0u32);
    let mut x = 0.0;
    while x <= 1.0 {
        let mut y = 0.0;
        while y <= 1.0 {
            let mut z = 0.0;
            while z <= 1.0 {
                let point = DVec3::new(
                    aabb.min.x + size.x * x + offset_x,
                    aabb.min.y + size.y * y,
                    aabb.min.z + size.z * z + offset_z,
                );
                if can_see(blocks, point, centre) {
                    seen += 1;
                }
                total += 1;
                z += step.z;
            }
            y += step.y;
        }
        x += step.x;
    }
    if total == 0 {
        0.0
    } else {
        f64::from(seen) / f64::from(total)
    }
}

/// The impact on an actor `distance` blocks from an explosion of `power`
/// that sees `exposure` of it; 0 out of reach.
pub(crate) fn impact(distance: f64, power: f32, exposure: f64) -> f64 {
    let radius = 2.0 * f64::from(power);
    if distance > radius {
        return 0.0;
    }
    (1.0 - distance / radius) * exposure
}

/// The damage an actor takes from an explosion of `power` with `impact`.
pub(crate) fn damage(impact: f64, power: f32) -> f32 {
    let radius = 2.0 * f64::from(power);
    ((impact * impact + impact) / 2.0 * DAMAGE_FACTOR * radius + 1.0) as f32
}

/// Where an actor `height` tall with its feet at `feet` explodes: a
/// sixteenth of its height above its feet (BDS 1.26.51: the TNT blast
/// sweep's destroyed counts and the hit sweep's damage both fit this point
/// and not the feet or the middle of the actor).
pub(crate) fn blast_centre(feet: DVec3, height: f64) -> DVec3 {
    feet + DVec3::Y * (height / 16.0)
}

/// An actor's distance for its impact: from `centre` to its feet, less half
/// its width.
pub(crate) fn distance(centre: DVec3, feet: DVec3, width: f64) -> f64 {
    (centre.distance(feet) - width / 2.0).max(0.0)
}

/// One explosion in the live world.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Blast {
    pub centre: DVec3,
    pub power: f32,
    /// The actor that exploded (already removed): its explosion is an
    /// entity explosion.
    pub source: Option<bevy_ecs::entity::Entity>,
    pub breaks_blocks: bool,
    /// Cap on a block's explosion resistance.
    pub max_resistance: f32,
    pub damage_scaling: f32,
    pub knockback_scaling: f32,
    /// The level sound viewers hear.
    pub sound: std::sync::Arc<str>,
}

/// A target of the actor pass: where it stands and its box.
struct Target {
    entity: Entity,
    feet: DVec3,
    width: f64,
    aabb: Aabb,
    player: bool,
}

/// Players that take damage and living mobs whose feet are within `reach`
/// (plus their size) of `centre`.
fn targets(world: &mut World, centre: DVec3, reach: f64) -> Vec<Target> {
    let mut out = Vec::new();
    let mut players =
        world.query_filtered::<(Entity, &Position, Option<&GameMode>), With<Player>>();
    for (entity, position, mode) in players.iter(world) {
        if mode.is_some_and(|mode| !mode.allows_damage()) {
            continue;
        }
        let feet = position.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
        out.push(Target {
            entity,
            feet,
            width: PLAYER_BOX_WIDTH,
            aabb: Aabb::from_feet(feet, PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT),
            player: true,
        });
    }
    let mut mobs =
        world.query_filtered::<(Entity, &Position, &PhysicsBody), (With<MobKind>, Without<Dead>)>();
    for (entity, position, body) in mobs.iter(world) {
        out.push(Target {
            entity,
            feet: position.0,
            width: body.width,
            aabb: Aabb::from_feet(position.0, body.width, body.height),
            player: false,
        });
    }
    out.retain(|target| {
        target.feet.is_finite() && target.feet.distance(centre) <= reach + target.width
    });
    out
}

/// Run `blast`: hurt and push the actors around it, tell the viewers, then
/// break the blocks its rays reach (each drops what it drops by hand,
/// without the correct tool; TNT among them is primed).
pub(crate) fn explode(world: &mut World, blast: &Blast) {
    if blast.power <= 0.0 {
        return;
    }
    let blocks = if blast.breaks_blocks {
        let mut rng = world.get_resource_or_init::<EntityRandom>().0.clone();
        let blocks = blasted_blocks(
            &*world,
            blast.centre,
            blast.power,
            blast.max_resistance,
            &mut rng,
        );
        world.resource_mut::<EntityRandom>().0 = rng;
        blocks
    } else {
        Vec::new()
    };
    hurt_actors(world, blast);
    viewers::explosion(world, blast, &blocks);
    break_blocks(world, &blocks);
}

/// The damage an actor `distance` from the blast, seeing `seen` of it,
/// takes, and the strength of its push (the impact, scaled by the blast's
/// knockback scaling; the damage uses the unscaled impact).
fn hit_for(blast: &Blast, distance: f64, seen: f64) -> (Hit, f64) {
    let impact = impact(distance, blast.power, seen);
    let hit = Hit {
        damage: damage(impact, blast.power) * blast.damage_scaling,
        cause: if blast.source.is_some() {
            ActorDamageCause::EntityExplosion
        } else {
            ActorDamageCause::BlockExplosion
        },
        attacker: None,
        by_player: false,
        knockback_from: None,
        knockback_power: KnockbackPower::DEFAULT,
    };
    (hit, impact * f64::from(blast.knockback_scaling))
}

/// Damage and push every actor within reach.
fn hurt_actors(world: &mut World, blast: &Blast) {
    let reach = 2.0 * f64::from(blast.power);
    for target in targets(world, blast.centre, reach) {
        let distance = distance(blast.centre, target.feet, target.width);
        if distance > reach {
            continue;
        }
        let seen = exposure(&*world, blast.centre, &target.aabb);
        let (hit, impact) = hit_for(blast, distance, seen);
        if target.player {
            crate::entity::player_life::hurt::hurt_player(world, target.entity, &hit);
        } else {
            hurt_mob(world, target.entity, hit);
        }
        let away = target.feet - blast.centre;
        let push = if away.length() > 1e-4 {
            away.normalize() * impact
        } else {
            DVec3::ZERO
        };
        if let Some(mut velocity) = world.get_mut::<Velocity>(target.entity) {
            velocity.0 += push;
        }
        if target.player {
            viewers::motion(world, target.entity, push);
        }
    }
}

/// Break the blasted blocks: TNT is primed (with its short fuse), every
/// other block drops what it drops by hand and becomes air; then the block
/// updates of every change run.
fn break_blocks(world: &mut World, blocks: &[IVec3]) {
    let Some(air) = world.registry().default_state_id_by_name("minecraft:air") else {
        return;
    };
    let mut changed = Vec::new();
    for &pos in blocks {
        let Some(state) = world.block_at(pos).filter(|state| *state != air) else {
            continue;
        };
        let tnt = world
            .registry()
            .get_by_runtime_id(state)
            .is_some_and(|entry| entry.string_id == "minecraft:tnt");
        crate::server::game::blocks::write_block(world, pos, air, false);
        if tnt {
            prime_tnt(world, pos, Some(FROM_EXPLOSION));
        } else {
            for stack in popped_drops(world, state) {
                pop_resource(world, pos, stack);
            }
        }
        changed.push(pos);
    }
    for pos in changed {
        crate::server::game::block_update::world_changed(world, pos, false);
    }
}

/// The primed TNT actor.
const PRIMED_TNT: &str = "minecraft:tnt";
/// The event that gives a TNT primed by an explosion its short fuse.
const FROM_EXPLOSION: &str = "from_explosion";
/// A newly primed TNT moves off at this horizontal speed in a random
/// direction, and up at [`PRIME_LIFT`] (BDS 1.26.51, `tnt_prime.json`:
/// |(vx, vz)| = 0.02 and vy = 0.2 at the first read).
pub(crate) const PRIME_SPEED: f64 = 0.02;
pub(crate) const PRIME_LIFT: f64 = 0.2;

/// Spawn a primed TNT at the bottom centre of `pos` (the TNT block there is
/// already gone), moving off in a random direction; `event` replaces the
/// spawn event. TNT lit by power plays the fuse sound there first.
pub(crate) fn prime_tnt(world: &mut World, pos: IVec3, event: Option<&str>) -> Option<Entity> {
    use crate::entity::mob::{MobRegistry, SpawnCause, spawn_mob};
    let feet = DVec3::new(
        f64::from(pos.x) + 0.5,
        f64::from(pos.y),
        f64::from(pos.z) + 0.5,
    );
    let handle = event.and_then(|name| {
        world
            .get_resource::<MobRegistry>()
            .and_then(|registry| registry.get(PRIMED_TNT))
            .and_then(|archetype| archetype.events.event(name))
    });
    let cause = if handle.is_some() {
        SpawnCause::Loaded
    } else {
        SpawnCause::Spawned
    };
    if handle.is_none() {
        viewers::fuse(world, feet);
    }
    let tnt = match spawn_mob(world, PRIMED_TNT, feet, 0.0, cause) {
        Ok(tnt) => tnt,
        Err(error) => {
            tracing::warn!(%error, "primed TNT could not spawn");
            return None;
        }
    };
    if let Some(handle) = handle {
        if let Err(error) = crate::entity::mob::events::fire_event(world, tnt, &handle, None) {
            tracing::warn!(?error, "primed TNT event failed");
        }
        crate::entity::definition::apply_pending_definitions(world);
    }
    let angle = world
        .get_resource_or_init::<EntityRandom>()
        .0
        .r#gen::<f64>()
        * std::f64::consts::TAU;
    let motion = DVec3::new(
        -angle.sin() * PRIME_SPEED,
        PRIME_LIFT,
        -angle.cos() * PRIME_SPEED,
    );
    if let Some(mut velocity) = world.get_mut::<Velocity>(tnt) {
        velocity.0 = motion;
    }
    Some(tnt)
}
