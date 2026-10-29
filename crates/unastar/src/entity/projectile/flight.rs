//! Projectile flight: one tick of movement, collision and hit resolution.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::DVec3;
use jolyne::valentine::{actor_data_i_ds as ids, actor_flags};
use unastar_data::actor_identifiers::ActorType;

use super::hit::on_hit;
use super::launch::heading;
use super::{ProjectileConfig, ProjectileFlight, ProjectileRules};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{
    GameMode, Health, PhysicsBody, Player, Position, Projectile, Rotation, Velocity,
};
use crate::entity::definition::components::RawCollisionBox;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::metadata::EntityMetadata;
use crate::entity::mob::MobKind;
use crate::entity::mob::constants::{
    PROJECTILE_HIT_INFLATE_DELAY, PROJECTILE_HIT_INFLATE_MAX, PROJECTILE_HIT_INFLATE_STEP,
    PROJECTILE_ROTATION_SMOOTHING, PROJECTILE_VISIBLE_TICK,
};
use crate::entity::mob::state::Dead;
use crate::entity::systems::physics::{collect_block_boxes, living_liquid};
use crate::registry::block::LiquidKind;
use crate::registry::collision::Aabb;
use crate::server::game::player_box;

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
pub(super) enum Target {
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
