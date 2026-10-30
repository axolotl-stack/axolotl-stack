//! Explosions: which blocks an explosion destroys, the damage and knockback
//! actors around it take, and what viewers are sent. [`explode`] is the one
//! entry point (`minecraft:explode` fuses today; primed TNT later).
//!
//! The rules, in order (measured on BDS 1.26.51, see
//! `entity::mob::constants` for every value):
//!
//! 1. **Blocks.** When the explosion breaks blocks and its centre is not in
//!    water (unless `allow_underwater`), one ray leaves the centre through
//!    each cell on the outer shell of a 16 × 16 × 16 grid (x outermost, z
//!    innermost; 1352 rays). A ray starts with `power × (0.7 + U·0.6)` and
//!    walks in 0.3-block steps. In each block it passes: water in the
//!    secondary (waterlogging) layer stops it; air costs nothing (and is
//!    only remembered for fire); any other block costs
//!    `(resistance + 0.3) × 0.3`, where `resistance` is the block's
//!    explosion resistance capped at `max_resistance / 5`, and is destroyed
//!    while the ray still has strength. Every step then costs 0.225 more.
//! 2. **Actors.** Every living actor (spectators excepted) whose box meets
//!    the cube `floor(centre ± (2·power + 1))` is a target, measured from
//!    the source's eyes (the centre without a source) to its feet at its
//!    x/z (a mob's distance shortened by half its width). Within `2·power`
//!    its impact is `(1 − d / 2·power) × exposure × knockback_scaling`,
//!    exposure being the share of sample points on its box that see the
//!    centre. It takes `((impact² + impact) / 2 × 7 × 2·power + 1) ×
//!    damage_scaling` explosion damage (attacker knockback included), then
//!    is pushed along the direction from the centre to its eyes by
//!    `impact`; a player is sent its resulting motion.
//! 3. **Viewers** get `LevelEvent` ParticlesExplosion (data: the power).
//! 4. **Breaking.** Each remembered block position is sampled for the
//!    block-explosion particles with chance 1/8; every non-air one drops
//!    its items (whole-stack or per-item decay `1 / power`, see
//!    `server::game::explosion_drops`) and becomes air. The drops, merged by
//!    item, appear after the loop at the first block that produced them.
//! 5. **Viewers** get `LevelEventGeneric` ParticlesBlockExplosion with the
//!    sampled positions.
//! 6. **Fire.** With `causes_fire`, every remembered position that is air
//!    now catches fire with chance 1/3.
//! 7. **Viewers** get one `UpdateBlock` per changed block, then (once the
//!    caller removed the exploding actor and the network sync spawned the
//!    drops) the explosion sound ([`send_explosion_sounds`]), the wire order
//!    of the BDS 1.26.51 capture.
//!
//! Not modelled: TNT blocks are destroyed like any other block instead of
//! being primed (there is no primed TNT actor yet), shields do not halve
//! the impact, knockback resistance is 0 for every actor, item,
//! experience-orb and projectile actors are not affected, a hurt player is
//! not sent `SetLastHurtBy` (no actor-type id with its class flags is
//! known), and several changes in one sub-chunk go out as several
//! `UpdateBlock` rather than one `UpdateSubChunkBlocks`.

use std::collections::HashSet;

use bevy_ecs::prelude::*;
use bytes::BufMut;
use glam::{DVec3, IVec3, Vec3 as FVec3};
use jolyne::valentine::types::{ActorRuntimeId, BlockPos, PlayerInputTick, Vec3};
use jolyne::valentine::{
    LevelEventGenericPacket, LevelEventPacket, LevelSoundEventPacket, McpePacket,
    SetActorMotionPacket, UpdateBlockPacket,
};
use rand::Rng;
use unastar_data::actor_components::body::ActorDamageCause;
use valentine::bedrock::codec::NbtCompoundBody;

use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, PhysicsBody, Player, PlayerSession, Position, RuntimeEntityId, Velocity,
};
use crate::entity::item_drop::PLAYER_EYE_HEIGHT;
use crate::entity::mob::MobKind;
use crate::entity::mob::ai::sensing::can_see;
use crate::entity::mob::combat::{Hit, hurt_mob, knockback_power_of};
use crate::entity::mob::components::KnockbackPower;
use crate::entity::mob::constants::{
    EXPLOSION_DAMAGE_FACTOR, EXPLOSION_DAMAGE_SCALING_EPSILON, EXPLOSION_FIRE_CHANCE,
    EXPLOSION_GRID, EXPLOSION_KNOCKBACK_MIN_LENGTH, EXPLOSION_PARTICLE_CHANCE, EXPLOSION_RAY_BASE,
    EXPLOSION_RAY_SPREAD, EXPLOSION_RAY_STEP, EXPLOSION_RESISTANCE_FACTOR,
    EXPLOSION_RESISTANCE_OFFSET, EXPLOSION_RESISTANCE_SCALE, EXPLOSION_SAMPLE_Y_OFFSET,
    EXPLOSION_STEP_DECAY, LEVEL_EVENT_PARTICLES_BLOCK_EXPLOSION, LEVEL_EVENT_PARTICLES_EXPLOSION,
    MOB_EYES_ATTACH_RATIO, PLAYER_BOX_HEIGHT, PLAYER_BOX_WIDTH,
};
use crate::entity::mob::net::sound::send_to_viewers;
use crate::entity::mob::state::Dead;
use crate::entity::systems::BlockSource;
use crate::item::ItemStack;
use crate::registry::block::LiquidKind;
use crate::registry::collision::Aabb;
use crate::server::broadcast::{BLOCK_UPDATE_NEIGHBOURS, BLOCK_UPDATE_NETWORK};
use crate::server::game::types::ServerConfigResource;
use crate::world::ecs::{
    BlockChanged, ChunkData, ChunkManager, ChunkStateFlags, ChunkViewers, world_to_chunk_coords,
    world_to_local_coords,
};

/// One explosion.
#[derive(Debug, Clone, PartialEq)]
pub struct Explosion {
    /// Where it happens: the source's feet for a mob.
    pub centre: DVec3,
    /// Its radius in blocks.
    pub power: f32,
    /// The exploding actor, if any: it is the attacker of the damage, its
    /// eyes are where distances are measured from, and it is not hurt.
    pub source: Option<Entity>,
    pub breaks_blocks: bool,
    pub causes_fire: bool,
    /// Blocks and actors under water are affected too.
    pub allow_underwater: bool,
    /// Cap on block resistance, in the scale behaviour packs author.
    pub max_resistance: f32,
    pub damage_scaling: f32,
    pub knockback_scaling: f32,
    /// The level sound viewers hear.
    pub sound: std::sync::Arc<str>,
}

/// What an explosion did (for callers and tests).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ExplosionOutcome {
    /// Blocks it turned into air, in processing order.
    pub destroyed: Vec<IVec3>,
    /// Blocks it set on fire.
    pub ignited: Vec<IVec3>,
    /// Actors it reached, with the damage each was dealt (before the
    /// victim's own rules) and the impulse it received.
    pub hits: Vec<(Entity, f32, DVec3)>,
}

/// Explosion sounds waiting for the network sync: BDS sends a level sound
/// after the block updates, the removal of the exploding actor and its
/// drops, so [`send_explosion_sounds`] sends them once those went out.
#[derive(Resource, Debug, Default)]
pub struct PendingExplosionSounds(Vec<(DVec3, McpePacket)>);

/// Run `explosion` in `world`.
pub fn explode(world: &mut World, explosion: &Explosion) -> ExplosionOutcome {
    let mut outcome = ExplosionOutcome::default();
    if explosion.power <= 0.0 {
        return outcome;
    }
    let in_water = in_water(world, explosion);
    // Fire only spreads to the air the block rays passed through.
    let positions = if explosion.breaks_blocks && (!in_water || explosion.allow_underwater) {
        affected_blocks(world, explosion)
    } else {
        Vec::new()
    };
    hurt_actors(world, explosion, in_water, &mut outcome);
    world
        .get_resource_or_init::<PendingExplosionSounds>()
        .0
        .push((explosion.centre, sound_packet(explosion)));
    send_to_viewers(
        world,
        explosion.centre,
        &McpePacket::from(LevelEventPacket {
            event_id: LEVEL_EVENT_PARTICLES_EXPLOSION,
            position: wire(explosion.centre),
            data: explosion.power as i32,
        }),
    );
    let mut changes = Vec::new();
    let sampled = if explosion.breaks_blocks {
        break_blocks(world, explosion, &positions, &mut outcome, &mut changes)
    } else {
        Vec::new()
    };
    send_to_viewers(
        world,
        explosion.centre,
        &McpePacket::from(LevelEventGenericPacket {
            event_id: LEVEL_EVENT_PARTICLES_BLOCK_EXPLOSION,
            __ctd__: block_explosion_particles(explosion, &sampled),
        }),
    );
    if explosion.causes_fire {
        ignite(world, &positions, &mut outcome, &mut changes);
    }
    send_block_updates(world, &changes);
    outcome
}

/// System (`NetworkSendSet`, after the actor sync): send the sounds of this
/// tick's explosions.
pub fn send_explosion_sounds(world: &mut World) {
    let Some(mut pending) = world.get_resource_mut::<PendingExplosionSounds>() else {
        return;
    };
    if pending.0.is_empty() {
        return;
    }
    let sounds = std::mem::take(&mut pending.0);
    for (position, packet) in &sounds {
        send_to_viewers(world, *position, packet);
    }
}

/// Whether the explosion starts in water: the source stands in water, or
/// without a source the centre's block is water.
fn in_water(world: &World, explosion: &Explosion) -> bool {
    let point = explosion
        .source
        .and_then(|source| world.get::<Position>(source))
        .map_or(explosion.centre, |position| position.0);
    let state = world.block_at(point.floor().as_ivec3());
    state.is_some_and(|state| is_water(world, state))
}

fn is_water(world: &World, state: u32) -> bool {
    world
        .registry()
        .liquid_state(state)
        .is_some_and(|liquid| liquid.kind == LiquidKind::Water)
}

/// The block in the secondary (waterlogging) layer at `pos`.
fn secondary_block(world: &World, pos: IVec3) -> Option<u32> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let chunk = world.get::<ChunkData>(world.resource::<ChunkManager>().get_by_coords(cx, cz)?)?;
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    Some(chunk.inner.get_block_layer(x, y, z, 1))
}

/// The block positions the rays reach, in first-reached order, without
/// repeats. Air positions are included only for fire.
fn affected_blocks(world: &mut World, explosion: &Explosion) -> Vec<IVec3> {
    let registry = world.registry().clone();
    let air = registry
        .default_state_id_by_name("minecraft:air")
        .expect("air is a vanilla block");
    let centre = explosion.centre.as_vec3();
    let last = (EXPLOSION_GRID - 1) as f32;
    let resistance_cap = explosion.max_resistance / EXPLOSION_RESISTANCE_SCALE;
    let strengths: Vec<f32> = {
        let mut rng = world.resource_mut::<EntityRandom>();
        (0..EXPLOSION_GRID.pow(3) - (EXPLOSION_GRID - 2).pow(3))
            .map(|_| {
                explosion.power * (rng.0.r#gen::<f32>() * EXPLOSION_RAY_SPREAD + EXPLOSION_RAY_BASE)
            })
            .collect()
    };
    let mut strengths = strengths.into_iter();
    let mut seen = HashSet::new();
    let mut positions = Vec::new();
    for i in 0..EXPLOSION_GRID {
        for j in 0..EXPLOSION_GRID {
            for k in 0..EXPLOSION_GRID {
                let on_shell = [i, j, k].iter().any(|&n| n == 0 || n == EXPLOSION_GRID - 1);
                if !on_shell {
                    continue;
                }
                let mut strength = strengths.next().expect("one strength per shell ray");
                if strength <= 0.0 {
                    continue;
                }
                let direction = (FVec3::new(i as f32, j as f32, k as f32) / last * 2.0
                    - FVec3::ONE)
                    .normalize();
                let step = direction * EXPLOSION_RAY_STEP;
                let mut point = centre;
                loop {
                    let cell = point.floor().as_ivec3();
                    if !explosion.allow_underwater
                        && secondary_block(world, cell).is_some_and(|s| is_water(world, s))
                    {
                        strength = 0.0;
                    }
                    let state = world.block_at(cell).unwrap_or(air);
                    let passable =
                        state == air || (explosion.allow_underwater && is_water(world, state));
                    if passable {
                        if explosion.causes_fire && seen.insert(cell) {
                            positions.push(cell);
                        }
                    } else {
                        let mut resistance = registry
                            .get_by_runtime_id(state)
                            .map_or(f32::MAX, |block| block.resistance);
                        if explosion.source.is_some() {
                            resistance = resistance.min(resistance_cap);
                        }
                        strength -= (resistance + EXPLOSION_RESISTANCE_OFFSET)
                            * EXPLOSION_RESISTANCE_FACTOR;
                        if strength > 0.0 && seen.insert(cell) {
                            positions.push(cell);
                        }
                    }
                    point += step;
                    strength -= EXPLOSION_STEP_DECAY;
                    if strength <= 0.0 {
                        break;
                    }
                }
            }
        }
    }
    positions
}

/// A target of the actor pass.
struct Target {
    entity: Entity,
    /// Feet (block-space) position.
    feet: DVec3,
    aabb: Aabb,
    eye_y: f64,
    width: f64,
    player: bool,
}

/// Damage and push every living actor near the explosion.
fn hurt_actors(
    world: &mut World,
    explosion: &Explosion,
    in_water: bool,
    outcome: &mut ExplosionOutcome,
) {
    let radius = f64::from(explosion.power) * 2.0;
    let reach = DVec3::splat(radius + 1.0);
    let area = Aabb::new(
        (explosion.centre - reach).floor(),
        (explosion.centre + reach).floor(),
    );
    let measured_from = explosion
        .source
        .and_then(|source| actor_eye(world, source))
        .unwrap_or(explosion.centre);
    let targets = targets(world, explosion.source, &area);
    for target in targets {
        let foot = DVec3::new(target.feet.x, target.aabb.min.y, target.feet.z);
        let mut distance = foot.distance(measured_from);
        if !target.player {
            distance = (distance - 0.5 * target.width).max(0.0);
        }
        if distance / radius > 1.0 {
            continue;
        }
        let exposure = exposure(world, explosion.centre, &target.aabb);
        let impact = (1.0 - distance / radius) * exposure * f64::from(explosion.knockback_scaling);
        let mut damage = ((impact * impact + impact) * 0.5 * EXPLOSION_DAMAGE_FACTOR * radius + 1.0)
            as f32
            * explosion.damage_scaling;
        let mut base_motion = DVec3::ZERO;
        if explosion.damage_scaling.abs() > EXPLOSION_DAMAGE_SCALING_EPSILON {
            if in_water && !explosion.allow_underwater {
                damage = 0.0;
            }
            let hit = Hit {
                damage,
                cause: if explosion.source.is_some() {
                    ActorDamageCause::EntityExplosion
                } else {
                    ActorDamageCause::BlockExplosion
                },
                attacker: explosion.source,
                by_player: false,
                knockback_from: explosion
                    .source
                    .and_then(|source| world.get::<Position>(source))
                    .map(|position| position.0),
                knockback_power: explosion.source.map_or(KnockbackPower::DEFAULT, |source| {
                    knockback_power_of(world, source)
                }),
            };
            if target.player {
                let (_, knockback) = crate::entity::player_life::hurt::hurt_player_knocked(
                    world,
                    target.entity,
                    &hit,
                );
                base_motion = knockback.unwrap_or(DVec3::ZERO);
            } else {
                hurt_mob(world, target.entity, hit);
            }
        }
        let away = DVec3::new(
            target.feet.x - explosion.centre.x,
            target.eye_y - explosion.centre.y,
            target.feet.z - explosion.centre.z,
        );
        let length = away.length();
        let direction = if length >= EXPLOSION_KNOCKBACK_MIN_LENGTH {
            away / length
        } else {
            DVec3::ZERO
        };
        let impulse = direction * impact;
        if target.player {
            send_motion(world, target.entity, base_motion + impulse);
        } else if let Some(mut velocity) = world.get_mut::<Velocity>(target.entity) {
            velocity.0 += impulse;
        }
        outcome.hits.push((target.entity, damage, impulse));
    }
}

/// The eye position of an actor (a player's position is its eye).
fn actor_eye(world: &World, actor: Entity) -> Option<DVec3> {
    let position = world.get::<Position>(actor)?.0;
    if world.get::<Player>(actor).is_some() {
        return Some(position);
    }
    let height = world.get::<PhysicsBody>(actor)?.height;
    Some(position + DVec3::Y * height * MOB_EYES_ATTACH_RATIO)
}

/// Living players (not spectators) and mobs whose box meets `area`.
fn targets(world: &mut World, source: Option<Entity>, area: &Aabb) -> Vec<Target> {
    let mut out = Vec::new();
    let mut players =
        world.query_filtered::<(Entity, &Position, Option<&GameMode>), With<Player>>();
    for (entity, position, mode) in players.iter(world) {
        if Some(entity) == source || mode == Some(&GameMode::Spectator) {
            continue;
        }
        let feet = position.0 - DVec3::Y * PLAYER_EYE_HEIGHT;
        let aabb = Aabb::from_feet(feet, PLAYER_BOX_WIDTH, PLAYER_BOX_HEIGHT);
        if world.get::<Dead>(entity).is_none() && aabb.intersects(area) {
            out.push(Target {
                entity,
                feet,
                aabb,
                eye_y: position.0.y,
                width: PLAYER_BOX_WIDTH,
                player: true,
            });
        }
    }
    let mut mobs =
        world.query_filtered::<(Entity, &Position, &PhysicsBody), (With<MobKind>, Without<Dead>)>();
    for (entity, position, body) in mobs.iter(world) {
        if Some(entity) == source {
            continue;
        }
        let aabb = Aabb::from_feet(position.0, body.width, body.height);
        if aabb.intersects(area) {
            out.push(Target {
                entity,
                feet: position.0,
                aabb,
                eye_y: position.0.y + body.height * MOB_EYES_ATTACH_RATIO,
                width: body.width,
                player: false,
            });
        }
    }
    out
}

/// The share of sample points on `aabb` that see `centre` (1 when the
/// centre is inside the box).
pub fn exposure(world: &World, centre: DVec3, aabb: &Aabb) -> f64 {
    if aabb.min.cmple(centre).all() && centre.cmple(aabb.max).all() {
        return 1.0;
    }
    let size = aabb.max - aabb.min;
    let step = DVec3::ONE / (size * 2.0 + DVec3::ONE);
    if step.min_element() < 0.0 {
        return 0.0;
    }
    let x_offset = (1.0 - (1.0 / step.x).floor() * step.x) * 0.5;
    let z_offset = (1.0 - (1.0 / step.z).floor() * step.z) * 0.5;
    let (mut seen, mut total) = (0u32, 0u32);
    let mut a = 0.0;
    while a <= 1.0 {
        let mut b = 0.0;
        while b <= 1.0 {
            let mut c = 0.0;
            while c <= 1.0 {
                let sample = DVec3::new(
                    aabb.min.x + size.x * a + x_offset,
                    aabb.min.y + size.y * b + EXPLOSION_SAMPLE_Y_OFFSET,
                    aabb.min.z + size.z * c + z_offset,
                );
                total += 1;
                if can_see(world, sample, centre) {
                    seen += 1;
                }
                c += step.z;
            }
            b += step.y;
        }
        a += step.x;
    }
    if total == 0 {
        0.0
    } else {
        f64::from(seen) / f64::from(total)
    }
}

/// Send `player` its motion (the server does not simulate players).
fn send_motion(world: &World, player: Entity, motion: DVec3) {
    let (Some(session), Some(runtime_id)) = (
        world.get::<PlayerSession>(player),
        world.get::<RuntimeEntityId>(player),
    ) else {
        return;
    };
    let _ = session.send(McpePacket::from(SetActorMotionPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id.0 as u64,
        },
        motion: wire(motion),
        tick: PlayerInputTick { inputtick: 0 },
    }));
}

/// Destroy the non-air blocks among `positions`, drop their items and
/// return the positions sampled for the block-explosion particles.
fn break_blocks(
    world: &mut World,
    explosion: &Explosion,
    positions: &[IVec3],
    outcome: &mut ExplosionOutcome,
    changes: &mut Vec<(IVec3, u32)>,
) -> Vec<IVec3> {
    let air = world
        .registry()
        .default_state_id_by_name("minecraft:air")
        .expect("air is a vanilla block");
    let mut sampled = Vec::new();
    let mut drops: Vec<(IVec3, ItemStack)> = Vec::new();
    for &pos in positions {
        let chosen = world
            .resource_mut::<EntityRandom>()
            .0
            .gen_range(0..EXPLOSION_PARTICLE_CHANCE)
            == 0;
        if chosen {
            sampled.push(pos);
        }
        let Some(state) = world.block_at(pos).filter(|&state| state != air) else {
            continue;
        };
        for stack in crate::server::game::explosion_drops(world, state, explosion.power) {
            merge_drop(&mut drops, pos, stack);
        }
        set_block(world, pos, air, changes);
        outcome.destroyed.push(pos);
    }
    for (pos, stack) in drops {
        crate::server::game::pop_resource(world, pos, stack);
    }
    sampled
}

/// Merge `stack` into the drops so far; a new item keeps the position of
/// the block that first produced it.
fn merge_drop(drops: &mut Vec<(IVec3, ItemStack)>, pos: IVec3, mut stack: ItemStack) {
    for (_, existing) in drops.iter_mut() {
        if let Some((merged, leftover)) = existing.merge(&stack) {
            *existing = merged;
            stack = leftover;
            if stack.is_empty() {
                return;
            }
        }
    }
    drops.push((pos, stack));
}

/// Set fire to the remembered positions that are air.
fn ignite(
    world: &mut World,
    positions: &[IVec3],
    outcome: &mut ExplosionOutcome,
    changes: &mut Vec<(IVec3, u32)>,
) {
    let registry = world.registry().clone();
    let (Some(air), Some(fire)) = (
        registry.default_state_id_by_name("minecraft:air"),
        registry.default_state_id_by_name("minecraft:fire"),
    ) else {
        return;
    };
    for &pos in positions {
        if world.block_at(pos) != Some(air) {
            continue;
        }
        let roll = world
            .resource_mut::<EntityRandom>()
            .0
            .gen_range(0..EXPLOSION_FIRE_CHANCE);
        if roll == 0 {
            set_block(world, pos, fire, changes);
            outcome.ignited.push(pos);
        }
    }
}

/// Set the block at `pos` (observers run) and record its new state for
/// [`send_block_updates`].
fn set_block(world: &mut World, pos: IVec3, state: u32, changes: &mut Vec<(IVec3, u32)>) {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    let Some(chunk) = world.resource::<ChunkManager>().get_by_coords(cx, cz) else {
        return;
    };
    let (lx, ly, lz) = world_to_local_coords(pos.x, pos.y, pos.z);
    let Some(mut data) = world.get_mut::<ChunkData>(chunk) else {
        return;
    };
    let old = data.inner.get_block(lx, ly, lz);
    data.inner.set_block(lx, ly, lz, state);
    if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(chunk) {
        flags.mark_needs_rebroadcast();
    }
    world.trigger(BlockChanged {
        chunk_entity: chunk,
        block_pos: pos,
        old_block: old,
        new_block: state,
    });
    match changes.iter_mut().find(|(changed, _)| *changed == pos) {
        Some(change) => change.1 = state,
        None => changes.push((pos, state)),
    }
}

/// Send each changed block's final state to the viewers of its chunk.
fn send_block_updates(world: &World, changes: &[(IVec3, u32)]) {
    let ids = world
        .get_resource::<ServerConfigResource>()
        .map_or_else(Default::default, |config| config.0.block_network_ids);
    let chunks = world.resource::<ChunkManager>();
    for &(pos, state) in changes {
        let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
        let Some(viewers) = chunks
            .get_by_coords(cx, cz)
            .and_then(|chunk| world.get::<ChunkViewers>(chunk))
        else {
            continue;
        };
        let packet = McpePacket::from(UpdateBlockPacket {
            block_position: BlockPos {
                x: pos.x,
                y: pos.y,
                z: pos.z,
            },
            block_runtime_id: ids.to_wire(state),
            flags: BLOCK_UPDATE_NEIGHBOURS | BLOCK_UPDATE_NETWORK,
            layer: 0,
        });
        for viewer in viewers.iter() {
            if let Some(session) = world.get::<PlayerSession>(viewer) {
                let _ = session.send(packet.clone());
            }
        }
    }
}

fn sound_packet(explosion: &Explosion) -> McpePacket {
    McpePacket::from(LevelSoundEventPacket {
        sound_event: explosion.sound.as_ref().into(),
        position: wire(explosion.centre),
        data: -1,
        actor_identifier: "".into(),
        is_baby: false,
        is_global: false,
        actor_unique_id: -1,
        fire_at_position: None,
    })
}

/// The ParticlesBlockExplosion event body: the centre, the radius and the
/// sampled block corners, as a network-NBT compound body with sorted keys
/// (`originX/Y/Z`, `pos{i}x/y/z`, `radius`, `size`).
fn block_explosion_particles(explosion: &Explosion, sampled: &[IVec3]) -> NbtCompoundBody {
    const FLOAT: u8 = 5;
    const INT: u8 = 3;
    let mut fields: Vec<(String, u8, f32, i32)> = Vec::with_capacity(5 + sampled.len() * 3);
    let centre = explosion.centre.as_vec3();
    for (name, value) in [
        ("originX", centre.x),
        ("originY", centre.y),
        ("originZ", centre.z),
    ] {
        fields.push((name.to_owned(), FLOAT, value, 0));
    }
    for (index, pos) in sampled.iter().enumerate() {
        for (axis, value) in [("x", pos.x), ("y", pos.y), ("z", pos.z)] {
            fields.push((format!("pos{index}{axis}"), FLOAT, value as f32, 0));
        }
    }
    fields.push(("radius".to_owned(), FLOAT, explosion.power, 0));
    fields.push(("size".to_owned(), INT, 0.0, sampled.len() as i32));
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    let mut body = bytes::BytesMut::new();
    for (name, kind, float, int) in fields {
        body.put_u8(kind);
        put_var_u32(&mut body, name.len() as u32);
        body.put_slice(name.as_bytes());
        if kind == FLOAT {
            body.put_f32_le(float);
        } else {
            put_var_u32(&mut body, ((int << 1) ^ (int >> 31)) as u32);
        }
    }
    body.put_u8(0);
    NbtCompoundBody(body.freeze())
}

fn put_var_u32(out: &mut bytes::BytesMut, mut value: u32) {
    while value >= 0x80 {
        out.put_u8((value as u8) | 0x80);
        value >>= 7;
    }
    out.put_u8(value as u8);
}

fn wire(position: DVec3) -> Vec3 {
    Vec3 {
        x: position.x as f32,
        y: position.y as f32,
        z: position.z as f32,
    }
}
