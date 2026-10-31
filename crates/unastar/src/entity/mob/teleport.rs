//! Loaded-world teleportation for living actors. Every accepted hop resolves
//! real support geometry and requires the complete actor box to be dry and free.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::types::{ActorUniqueId, Vec3};
use jolyne::valentine::{McpePacket, SpawnParticleEffectPacket};
use rand::Rng;

use super::components::teleport::{Teleport, TeleportConfig};
use super::events::MobFacts;
use super::net::network::MobNetState;
use super::net::sound::{play_level_sound, send_to_viewers};
use super::state::{AttackTarget, Dead};
use super::{MobKind, ai::MobBrain};
use crate::ecs::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{FallDistance, Health, OnGround, PhysicsBody, Position, Velocity};
use crate::entity::systems::physics::{
    BlockSource, collect_block_boxes, collides_with_blocks, living_liquid,
};
use crate::registry::collision::Aabb;
use crate::world::clock;
use crate::world::ecs::{ChunkData, ChunkManager, world_to_chunk_coords};

/// A finite work bound per attempted hop, independent of authored intervals.
const ATTEMPTS: usize = 32;

/// Find loaded solid support beneath a proposal and verify the entire actor
/// box against real collision and fluid geometry, including chunk boundaries.
fn destination(world: &World, actor: Entity, proposed: DVec3) -> Option<DVec3> {
    if !proposed.is_finite()
        || proposed.x.abs() > f64::from(i32::MAX - 32)
        || proposed.z.abs() > f64::from(i32::MAX - 32)
    {
        return None;
    }
    let body = world.get::<PhysicsBody>(actor)?;
    if !body.width.is_finite()
        || !body.height.is_finite()
        || body.width <= 0.0
        || body.height <= 0.0
    {
        return None;
    }
    let column = proposed.floor().as_ivec3();
    let (cx, cz) = world_to_chunk_coords(column.x, column.z);
    let chunk = world.get::<ChunkData>(
        world
            .get_resource::<ChunkManager>()?
            .get_by_coords(cx, cz)?,
    )?;
    let bounds = chunk.inner.block_bounds();
    let start = column.y.clamp(bounds.start, bounds.end - 1);
    let mut boxes = Vec::new();
    for y in (bounds.start..=start).rev() {
        let support = IVec3::new(column.x, y, column.z);
        let state = world.block_at(support)?;
        let entry = world.registry().get_by_runtime_id(state)?;
        if !entry.data.blocks_motion || entry.data.liquid {
            continue;
        }
        boxes.clear();
        collect_block_boxes(
            world,
            Aabb::new(support.as_dvec3(), support.as_dvec3() + DVec3::ONE),
            &mut boxes,
        );
        let top = boxes
            .iter()
            .filter(|box_| {
                box_.min.x <= proposed.x
                    && box_.max.x >= proposed.x
                    && box_.min.z <= proposed.z
                    && box_.max.z >= proposed.z
            })
            .map(|box_| box_.max.y)
            .fold(f64::NEG_INFINITY, f64::max);
        if !top.is_finite() {
            continue;
        }
        let feet = DVec3::new(proposed.x, top, proposed.z);
        let aabb = Aabb::from_feet(feet, body.width, body.height);
        if top < f64::from(bounds.start)
            || aabb.max.y > f64::from(bounds.end)
            || collides_with_blocks(world, aabb)
            || living_liquid(world, aabb).is_some()
        {
            return None;
        }
        return Some(feet);
    }
    None
}

/// Emit a built-in world-space portal effect without binding it to the actor
/// whose authoritative position is about to change.
fn particles(world: &World, position: DVec3) {
    let Some(dimension) = world
        .get_resource::<ChunkManager>()
        .and_then(|chunks| u8::try_from(chunks.dimension()).ok())
    else {
        return;
    };
    send_to_viewers(
        world,
        position,
        &McpePacket::from(SpawnParticleEffectPacket {
            dimension_id: dimension,
            actor_id: ActorUniqueId {
                actor_unique_id: -1,
            },
            position: Vec3 {
                x: position.x as f32,
                y: position.y as f32,
                z: position.z as f32,
            },
            effect_name: "minecraft:mob_portal".into(),
            molang_variables: None,
        }),
    );
}

/// Teleport a live mob onto loaded dry support. Failure leaves the actor,
/// its motion, navigation and viewers unchanged. Success resets falling and
/// navigation and forces the next native movement update to complete at once.
pub fn teleport_mob(world: &mut World, actor: Entity, proposed: DVec3) -> bool {
    if world.get::<MobKind>(actor).is_none()
        || world.get::<Dead>(actor).is_some()
        || world.get::<Health>(actor).is_none_or(Health::is_dead)
    {
        return false;
    }
    let Some(from) = world.get::<Position>(actor).map(|position| position.0) else {
        return false;
    };
    let Some(feet) = destination(world, actor, proposed) else {
        return false;
    };
    if from.distance_squared(feet) < 1.0e-8 {
        return false;
    }
    particles(world, from);
    let mut entity = world.entity_mut(actor);
    entity.insert((
        Position(feet),
        Velocity::default(),
        OnGround(true),
        FallDistance::default(),
    ));
    if let Some(mut brain) = entity.get_mut::<MobBrain>() {
        brain.teleported();
    }
    if let Some(mut net) = entity.get_mut::<MobNetState>() {
        net.teleport_pending = true;
    }
    particles(world, feet);
    play_level_sound(world, Some(actor), from, "teleport", -1);
    play_level_sound(world, Some(actor), feet, "teleport", -1);
    true
}

/// Draw candidate offsets using the configured search dimensions. Targeted
/// hops are clamped to their configured maximum displacement toward the target.
fn proposals(
    world: &mut World,
    origin: DVec3,
    config: &TeleportConfig,
    target: Option<DVec3>,
) -> Vec<DVec3> {
    let rng = &mut world.get_resource_or_init::<EntityRandom>().0;
    (0..ATTEMPTS)
        .map(|_| {
            let offset = DVec3::new(
                rng.gen_range(-config.cube.x * 0.5..=config.cube.x * 0.5),
                rng.gen_range(-config.cube.y * 0.5..=config.cube.y * 0.5),
                rng.gen_range(-config.cube.z * 0.5..=config.cube.z * 0.5),
            );
            if let Some(target) = target {
                let toward = (target - origin).normalize_or_zero() * config.target_distance;
                origin + (toward + offset).clamp_length_max(config.target_distance)
            } else {
                origin + offset
            }
        })
        .collect()
}

/// Re-arm the authored random interval after a completed or failed scheduled
/// attempt. Other reactive hops also begin a fresh interval when successful.
fn reschedule(world: &mut World, actor: Entity, config: &TeleportConfig) {
    let now = world.resource::<TickCounter>().current;
    let wait = config.wait_ticks(&mut world.get_resource_or_init::<EntityRandom>().0);
    if let Some(mut state) = world.get_mut::<Teleport>(actor) {
        state.next_random_tick = now.saturating_add(wait.max(1));
        state.earliest_random_tick = now.saturating_add(config.wait.0);
    }
}

/// Try a bounded set of current-rule candidates without loading new chunks.
fn attempt(world: &mut World, actor: Entity, target: Option<DVec3>) -> bool {
    let Some(config) = world
        .get::<Teleport>(actor)
        .map(|state| state.config.clone())
    else {
        return false;
    };
    let Some(origin) = world.get::<Position>(actor).map(|position| position.0) else {
        return false;
    };
    for proposal in proposals(world, origin, &config, target) {
        let Some(feet) = destination(world, actor, proposal) else {
            continue;
        };
        if target.is_some()
            && origin.distance_squared(feet)
                > config.target_distance * config.target_distance + 1.0e-8
        {
            continue;
        }
        if teleport_mob(world, actor, feet) {
            reschedule(world, actor, &config);
            return true;
        }
    }
    false
}

/// A successful projectile reaction means the projectile missed; callers must
/// skip all hit effects and let its ordinary flight continue.
pub fn projectile_escape(world: &mut World, actor: Entity, projectile: Entity) -> bool {
    if world
        .get::<crate::entity::components::Projectile>(projectile)
        .is_none()
    {
        return false;
    }
    let Some(config) = world
        .get::<Teleport>(actor)
        .map(|state| state.config.clone())
    else {
        return false;
    };
    if !config.projectile_reactive
        || config.projectile_filter.as_ref().is_some_and(|filter| {
            !filter.evaluate(&MobFacts {
                world,
                entity: actor,
                other: Some(projectile),
            })
        })
    {
        return false;
    }
    attempt(world, actor, None)
}

/// Native classes with water avoidance may escape after a committed water hit.
/// This reads the actual body volume; unrelated damage never invokes a hop.
pub(crate) fn escape_water(world: &mut World, actor: Entity) {
    if !world
        .get::<MobKind>(actor)
        .is_some_and(|kind| kind.0.native.water_escape)
    {
        return;
    }
    let (Some(position), Some(body)) = (
        world.get::<Position>(actor),
        world.get::<PhysicsBody>(actor),
    ) else {
        return;
    };
    if living_liquid(world, Aabb::from_feet(position.0, body.width, body.height))
        == Some(crate::registry::block::LiquidKind::Water)
    {
        attempt(world, actor, None);
    }
}

/// Run scheduled ambient hops and target-directed attempts before AI/physics.
pub fn tick_teleports(world: &mut World) {
    let now = world.resource::<TickCounter>().current;
    let actors: Vec<Entity> = world
        .query_filtered::<Entity, (With<MobKind>, With<Teleport>, Without<Dead>)>()
        .iter(world)
        .collect();
    for actor in actors {
        let Some(state) = world.get::<Teleport>(actor).cloned() else {
            continue;
        };
        let Some(origin) = world.get::<Position>(actor).map(|position| position.0) else {
            continue;
        };
        let target = world
            .get::<AttackTarget>(actor)
            .and_then(|target| {
                super::ai::combat_candidate(world, actor, target.0)
                    .then(|| world.get::<Position>(target.0).map(|position| position.0))
                    .flatten()
            })
            .filter(|position| {
                state.config.target_distance > 0.0
                    && origin.distance_squared(*position)
                        > state.config.target_distance * state.config.target_distance
            });
        if target.is_some()
            && world
                .get_resource_or_init::<EntityRandom>()
                .0
                .r#gen::<f32>()
                < state.config.target_chance
            && attempt(world, actor, target)
        {
            continue;
        }
        if !state.config.random {
            continue;
        }
        let chance = if clock::sky_brightness(world, origin.floor().as_ivec3()) >= 8 {
            state.config.light_chance
        } else {
            state.config.dark_chance
        };
        if (now >= state.next_random_tick
            || (now >= state.earliest_random_tick
                && world
                    .get_resource_or_init::<EntityRandom>()
                    .0
                    .r#gen::<f32>()
                    < chance))
            && !attempt(world, actor, None)
        {
            reschedule(world, actor, &state.config);
        }
    }
}
