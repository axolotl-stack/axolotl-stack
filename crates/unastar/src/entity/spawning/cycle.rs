//! The natural spawn cycle and the world-generation pass.
//!
//! Every tick each ticking chunk rolls for a spawn attempt. An attempt picks
//! a random column of the chunk, finds its top spawn block (the surface) and
//! every cave floor below it (underground), and runs one cluster at each:
//! draw a spawn entry from the biome's pool, check its conditions and the
//! spawn position, then spawn a herd within the population caps. A freshly
//! generated chunk additionally runs one surface cluster in a random column
//! the first tick it is ticking.
//!
//! Light is not tracked by the server, so the brightness at a block is
//! derived from the chunk column: at or above the column's heightmap (the
//! first air block above the highest non-air block) it is full sky light;
//! below, sky light falls by the light filtering of every block from the
//! heightmap down to and including the block. Block light is not modelled
//! (0), and the sky is not darkened by the time of day ([`SKY_DARKEN`]).
//! There is no weather, so `adjust_for_weather` never changes the
//! brightness, and no position is in a bubble column, so
//! `disallow_spawns_in_bubble` never rejects.

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;
use unastar_data::actor_identifiers::ActorType;
use unastar_data::blocks::{BlockData, StateValue};

use super::SurfaceMob;
use super::rules::{Category, Difficulty, Herd, NaturalSpawnRules, SpawnEntry};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, PhysicsBody, Player, Position};
use crate::entity::mob::constants::{
    DO_MOB_SPAWNING_DEFAULT, DO_MOB_SPAWNING_RULE, MATERIAL_ICE, MATERIAL_LAVA, MATERIAL_LEAVES,
    MATERIAL_WATER, MOBS_PER_CHUNK_SURFACE, MOBS_PER_CHUNK_UNDERGROUND, SKY_DARKEN, SKY_LIGHT_MAX,
    SPAWN_CENSUS_RADIUS_CHUNKS, SPAWN_FULL_TICK_RANGE, SPAWN_GLOBAL_MOB_LIMIT,
    SPAWN_OPEN_MATERIALS, SPAWN_POSITION_OFFSET, SPAWN_SHORT_TICK_RANGE,
    SPAWN_SHORT_TICK_RANGE_DISTANCE, SPAWN_TICK_ROLL, SPAWN_TICK_ROLL_MAX, TICKS_PER_SECOND,
};
use crate::entity::mob::state::Persistent;
use crate::entity::mob::{MobKind, SpawnCause, events, spawn_mob};
use crate::entity::systems::physics::{BlockSource, collides_with_blocks};
use crate::registry::BlockRegistry;
use crate::registry::collision::Aabb;
use crate::server::game::types::{BlockRegistryResource, ServerWorldTemplate};
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::ecs::systems::{world_to_chunk_coords, world_to_local_coords};
use crate::world::ecs::{ChunkData, ChunkLoadConfig, ChunkManager, ChunkStateFlags};

/// One above the highest block (the spawn column search starts here).
const MAX_Y: i32 = MIN_Y + SUBCHUNK_COUNT as i32 * 16;

/// The situation a spawn cluster is checked against.
#[derive(Debug, Clone, Copy)]
struct Conditions {
    surface: bool,
    water: bool,
    lava: bool,
    underground: bool,
    brightness: i32,
    /// The spawn block (the mob stands on it).
    pos: IVec3,
}

/// Per-tick inputs.
struct Tick {
    tick_range: i32,
    difficulty: Difficulty,
    /// World age in seconds.
    age: u64,
    /// Positions of the players that can be spawned around.
    players: Vec<DVec3>,
    /// Spawnable mobs at the start of the tick (the global limit).
    spawnable: u32,
    blocks: Arc<BlockRegistry>,
}

/// Mob counts in the census area, `[surface as usize]`.
#[derive(Default)]
struct Census {
    base: [[i32; 7]; 2],
    types: [HashMap<ActorType, i32>; 2],
}

/// System: the natural spawn cycle (see the module docs).
pub fn natural_spawning(world: &mut World) {
    if !world.contains_resource::<NaturalSpawnRules>() || !do_mob_spawning(world) {
        return;
    }
    world.resource_scope(|world, rules: Mut<NaturalSpawnRules>| run(world, &rules));
}

fn run(world: &mut World, rules: &NaturalSpawnRules) {
    let mut players = world.query_filtered::<(&Position, &GameMode), With<Player>>();
    let players: Vec<DVec3> = players
        .iter(world)
        .filter(|(_, mode)| **mode != GameMode::Spectator)
        .map(|(position, _)| position.0)
        .collect();
    let manager = world.resource::<ChunkManager>();
    let mut chunks: Vec<((i32, i32), Entity, bool)> = manager
        .iter()
        .filter_map(|(coords, entity)| {
            let flags = world.get::<ChunkStateFlags>(entity)?;
            flags
                .is_ticking()
                .then_some((coords, entity, flags.needs_population()))
        })
        .collect();
    if chunks.is_empty() {
        return;
    }
    chunks.sort_unstable_by_key(|(coords, _, _)| *coords);
    let mut mobs = world.query::<&MobKind>();
    let spawnable = mobs
        .iter(world)
        .filter(|kind| {
            ActorType::from_name(&kind.0.identifier).is_some_and(|a| rules.category(a).is_some())
        })
        .count();
    let tick = Tick {
        tick_range: world.resource::<ChunkLoadConfig>().simulation_distance,
        difficulty: world_difficulty(world),
        age: world.resource::<TickCounter>().current / TICKS_PER_SECOND as u64,
        players,
        spawnable: u32::try_from(spawnable).unwrap_or(u32::MAX),
        blocks: Arc::clone(&world.resource::<BlockRegistryResource>().0),
    };
    for (coords, entity, needs_population) in chunks {
        if needs_population {
            if let Some(mut flags) = world.get_mut::<ChunkStateFlags>(entity) {
                flags.clear_needs_population();
            }
            populate(world, rules, &tick, coords);
        }
        if random_int(world, SPAWN_TICK_ROLL) <= SPAWN_TICK_ROLL_MAX {
            spawn_in_chunk(world, rules, &tick, coords);
        }
    }
}

/// The world-generation pass: one surface cluster in a random column.
fn populate(world: &mut World, rules: &NaturalSpawnRules, tick: &Tick, (cx, cz): (i32, i32)) {
    let x = cx * 16 + random_int(world, 16);
    let z = cz * 16 + random_int(world, 16);
    let mut pos = IVec3::new(x, MAX_Y, z);
    if !find_spawn_block(world, &tick.blocks, &mut pos, false) || world.block_at(pos).is_none() {
        return;
    }
    let above = material(world, &tick.blocks, pos + IVec3::Y);
    let conditions = Conditions {
        surface: true,
        water: above == MATERIAL_WATER,
        lava: above == MATERIAL_LAVA,
        underground: false,
        brightness: 0,
        pos,
    };
    spawn_cluster(world, rules, tick, (cx, cz), conditions);
}

/// A spawn attempt: the surface of a random column, then every floor below.
fn spawn_in_chunk(world: &mut World, rules: &NaturalSpawnRules, tick: &Tick, (cx, cz): (i32, i32)) {
    let x = cx * 16 + random_int(world, 16);
    let z = cz * 16 + random_int(world, 16);
    let mut pos = IVec3::new(x, MAX_Y, z);
    let found = find_spawn_block(world, &tick.blocks, &mut pos, true);
    let both = |world: &World, liquid: u8| {
        material(world, &tick.blocks, pos + IVec3::Y) == liquid
            && material(world, &tick.blocks, pos + IVec3::new(0, 2, 0)) == liquid
    };
    let mut conditions = Conditions {
        surface: true,
        water: both(world, MATERIAL_WATER),
        lava: both(world, MATERIAL_LAVA),
        underground: false,
        brightness: 0,
        pos,
    };
    if world.block_at(pos).is_none() {
        return;
    }
    if found {
        spawn_cluster(world, rules, tick, (cx, cz), conditions);
    }
    if material(world, &tick.blocks, pos) != MATERIAL_LEAVES {
        conditions.surface = false;
    }
    conditions.underground = true;
    while find_spawn_block(world, &tick.blocks, &mut pos, false) {
        conditions.pos = pos;
        if material(world, &tick.blocks, pos) != MATERIAL_LEAVES {
            conditions.surface = false;
        }
        let above = material(world, &tick.blocks, pos + IVec3::Y);
        conditions.water = above == MATERIAL_WATER;
        conditions.lava = above == MATERIAL_LAVA;
        spawn_cluster(world, rules, tick, (cx, cz), conditions);
    }
}

/// Move `pos` down to the next spawn block: the first motion-blocking block
/// below open space that is solid, ice or a top slab. On the surface search
/// a solid block above the open space or a bottom slab ends the search.
fn find_spawn_block(world: &World, blocks: &BlockRegistry, pos: &mut IVec3, surface: bool) -> bool {
    loop {
        if world.block_at(*pos).is_none() {
            return false;
        }
        while pos.y >= MIN_Y {
            let data = block_data(world, blocks, *pos);
            if data.is_none_or(|d| SPAWN_OPEN_MATERIALS.contains(&d.material)) {
                break;
            }
            if surface && data.is_some_and(|d| d.solid) {
                return false;
            }
            pos.y -= 1;
        }
        let mut candidate = None;
        while pos.y >= MIN_Y {
            if let Some((id, data)) = block(world, blocks, *pos)
                && data.blocks_motion
            {
                candidate = Some((id, data));
                break;
            }
            pos.y -= 1;
        }
        let Some((id, data)) = candidate else {
            return false;
        };
        match slab_half(blocks, id) {
            Some(true) => return true,
            Some(false) if surface => return false,
            Some(false) => continue,
            None => {}
        }
        if data.material == MATERIAL_ICE || data.solid {
            return true;
        }
    }
}

/// `Some(true)` for a top slab, `Some(false)` for a bottom slab.
fn slab_half(blocks: &BlockRegistry, id: u32) -> Option<bool> {
    match blocks
        .get_by_runtime_id(id)?
        .state_value(id, "minecraft:vertical_half")?
    {
        StateValue::Str(half) => Some(half == "top"),
        _ => None,
    }
}

fn spawn_cluster(
    world: &mut World,
    rules: &NaturalSpawnRules,
    tick: &Tick,
    chunk: (i32, i32),
    mut conditions: Conditions,
) {
    let blocks = &tick.blocks;
    let Some((ground, _)) = block(world, blocks, conditions.pos) else {
        return;
    };
    if blocks
        .get_by_runtime_id(ground)
        .is_none_or(|entry| entry.string_id == "minecraft:air")
    {
        return;
    }
    conditions.brightness = raw_brightness(world, blocks, conditions.pos + IVec3::Y);
    let lookup = if conditions.water || conditions.lava {
        Conditions {
            pos: conditions.pos + IVec3::Y,
            ..conditions
        }
    } else {
        conditions
    };
    // A bottom slab offers no mob.
    if slab_half(blocks, ground) == Some(false) {
        return;
    }
    let Some(entry) = draw_entry(world, rules, tick, &lookup) else {
        return;
    };
    let herd = if entry.herds.is_empty() {
        Herd::SINGLE
    } else {
        let index = random_int(world, entry.herds.len() as i32);
        entry.herds[index as usize].clone()
    };
    if !can_spawn_in_conditions(entry, &conditions, tick) {
        return;
    }
    if entry.rarity >= 1 && random_int(world, entry.rarity as i32) != 0 {
        return;
    }
    let f: f32 = world.resource_mut::<EntityRandom>().0.r#gen();
    let mut count = ((herd.max_size - herd.min_size) as f32 * f * f).round() as i32 + herd.min_size;
    if count <= 0 {
        return;
    }
    let feet = conditions.pos + IVec3::Y;
    if !spawn_position_ok(world, blocks, entry, feet, tick) {
        return;
    }
    if let Some(reason) = &entry.blocked {
        tracing::trace!(source = %entry.source, reason = %reason, "drawn spawn entry is blocked");
        return;
    }
    let global_room = SPAWN_GLOBAL_MOB_LIMIT.saturating_sub(tick.spawnable) as i32;
    if tick.spawnable + count as u32 > SPAWN_GLOBAL_MOB_LIMIT {
        count = global_room;
    }
    let mut census = census(world, rules, chunk);
    let surface = usize::from(conditions.surface);
    let limit = entry.density[usize::from(!conditions.surface)];
    if limit >= 0 {
        let current = entry
            .actor
            .and_then(|actor| census.types[surface].get(&actor).copied())
            .unwrap_or(0);
        if limit <= current + count {
            count = limit - current;
        }
    }
    let position = DVec3::new(
        f64::from(conditions.pos.x) + SPAWN_POSITION_OFFSET,
        f64::from(conditions.pos.y + 1),
        f64::from(conditions.pos.z) + SPAWN_POSITION_OFFSET,
    );
    let mut spawned = Vec::new();
    for permutation in &entry.permutations {
        if count <= 0 {
            break;
        }
        if permutation.entity_type.is_none() {
            continue;
        }
        let guaranteed = (permutation.min_guaranteed as i32).min(count);
        for _ in 0..guaranteed {
            if pop_cap_allows(&census, entry.category, conditions.surface) {
                spawn_one(
                    world,
                    rules,
                    &mut census,
                    permutation.actor,
                    position,
                    conditions.surface,
                    &mut spawned,
                );
            }
        }
        count -= guaranteed;
    }
    while count > 0 {
        if pop_cap_allows(&census, entry.category, conditions.surface) {
            let actor = permuted(world, entry);
            spawn_one(
                world,
                rules,
                &mut census,
                actor,
                position,
                conditions.surface,
                &mut spawned,
            );
        }
        count -= 1;
    }
    send_herd_events(world, &herd, &spawned);
    if entry.persistent {
        for &mob in &spawned {
            world.entity_mut(mob).insert(Persistent);
        }
    }
}

/// The weighted draw among the biome's entries whose conditions hold.
fn draw_entry<'r>(
    world: &mut World,
    rules: &'r NaturalSpawnRules,
    tick: &Tick,
    lookup: &Conditions,
) -> Option<&'r SpawnEntry> {
    let biome = biome_at(world, lookup.pos)?;
    let candidates: Vec<&SpawnEntry> = rules
        .pools
        .get(&biome)?
        .iter()
        .map(|&index| &rules.entries[index])
        .filter(|entry| can_spawn_in_conditions(entry, lookup, tick))
        .collect();
    let total: u32 = candidates.iter().map(|entry| entry.weight).sum();
    let mut roll = if total == 0 {
        0
    } else {
        random_int(world, total as i32)
    };
    candidates.into_iter().find(|entry| {
        roll -= entry.weight as i32;
        roll < 0
    })
}

/// The permutation a non-guaranteed herd member takes.
fn permuted(world: &mut World, entry: &SpawnEntry) -> Option<ActorType> {
    if entry.permutations.is_empty() {
        return entry.actor;
    }
    let total: u32 = entry.permutations.iter().map(|p| p.weight).sum();
    if total == 0 {
        return entry.actor;
    }
    let mut roll = random_int(world, total as i32);
    for permutation in &entry.permutations {
        roll -= permutation.weight as i32;
        if roll < 0 {
            return if permutation.entity_type.is_some() {
                permutation.actor
            } else {
                entry.actor
            };
        }
    }
    entry.actor
}

fn can_spawn_in_conditions(entry: &SpawnEntry, conditions: &Conditions, tick: &Tick) -> bool {
    let y = conditions.pos.y;
    (MIN_Y..MAX_Y).contains(&y)
        && (entry.height.0..=entry.height.1).contains(&y)
        && (!conditions.water || entry.underwater)
        && (!conditions.lava || entry.lava)
        && (!conditions.surface || entry.surface)
        && (!conditions.underground || entry.underground)
        && (entry.difficulty.0..=entry.difficulty.1).contains(&tick.difficulty)
        && (entry.brightness.0..=entry.brightness.1).contains(&conditions.brightness)
        && (entry.world_age.0..=entry.world_age.1).contains(&tick.age)
        // The spawn delay ends at world age 0 (no delay filter is compiled).
        && tick.age > 0
}

/// Player distance, spawn blocks, and room at the feet.
fn spawn_position_ok(
    world: &World,
    blocks: &BlockRegistry,
    entry: &SpawnEntry,
    feet: IVec3,
    tick: &Tick,
) -> bool {
    let max = if tick.tick_range >= SPAWN_FULL_TICK_RANGE {
        if !neighbourhood_ticking(world, feet) {
            return false;
        }
        entry.distance.1
    } else if tick.tick_range == SPAWN_SHORT_TICK_RANGE {
        entry.distance.1.min(SPAWN_SHORT_TICK_RANGE_DISTANCE)
    } else {
        return false;
    };
    let max_sq = if max >= 0 {
        f64::from(max * max)
    } else {
        f64::MAX
    };
    let at = feet.as_dvec3();
    let Some(nearest) = tick
        .players
        .iter()
        .map(|player| player.distance_squared(at))
        .filter(|&distance| distance < max_sq)
        .min_by(f64::total_cmp)
    else {
        return false;
    };
    if nearest < f64::from(entry.distance.0 * entry.distance.0) {
        return false;
    }
    let below = feet - IVec3::Y;
    let Some((ground, _)) = block(world, blocks, below) else {
        return false;
    };
    if !entry.on_blocks.is_empty() && !entry.on_blocks.contains(ground) {
        return false;
    }
    if entry.prevented_blocks.contains(ground) {
        return false;
    }
    if let Some((above, distance)) = &entry.above_blocks {
        let mut found = false;
        for depth in 0..(*distance).max(0) {
            let Some((id, data)) = block(world, blocks, below - IVec3::new(0, depth, 0)) else {
                return false;
            };
            if above.contains(id) {
                found = true;
                break;
            }
            let is_air = blocks
                .get_by_runtime_id(id)
                .is_some_and(|entry| entry.string_id == "minecraft:air");
            if data.material != MATERIAL_WATER && !is_air {
                break;
            }
        }
        if !found {
            return false;
        }
    }
    if solid_blocking(world, blocks, feet + IVec3::Y) {
        return false;
    }
    let feet_material = material(world, blocks, feet);
    if (feet_material == MATERIAL_WATER) != entry.underwater
        || (feet_material == MATERIAL_LAVA) != entry.lava
    {
        return false;
    }
    let ground_name = blocks
        .get_by_runtime_id(ground)
        .map(|entry| entry.string_id.as_str());
    if matches!(
        ground_name,
        Some("minecraft:bedrock" | "minecraft:invisible_bedrock" | "minecraft:barrier")
    ) {
        return false;
    }
    !solid_blocking(world, blocks, feet)
}

/// The chunk of `feet` and the 8 around it are loaded and ticking.
fn neighbourhood_ticking(world: &World, feet: IVec3) -> bool {
    let (cx, cz) = world_to_chunk_coords(feet.x, feet.z);
    let manager = world.resource::<ChunkManager>();
    (-1..=1).all(|dx| {
        (-1..=1).all(|dz| {
            manager
                .get_by_coords(cx + dx, cz + dz)
                .and_then(|entity| world.get::<ChunkStateFlags>(entity))
                .is_some_and(ChunkStateFlags::is_ticking)
        })
    })
}

fn pop_cap_allows(census: &Census, category: Option<Category>, surface: bool) -> bool {
    let Some(category) = category else {
        return false;
    };
    let caps = if surface {
        &MOBS_PER_CHUNK_SURFACE
    } else {
        &MOBS_PER_CHUNK_UNDERGROUND
    };
    let cap = caps[category.index()] as i32;
    cap > 0 && census.base[usize::from(surface)][category.index()] < cap
}

/// Mobs within [`SPAWN_CENSUS_RADIUS_CHUNKS`] of `chunk`, by category and
/// type, split by whether the natural cycle spawned them on the surface.
fn census(world: &mut World, rules: &NaturalSpawnRules, (cx, cz): (i32, i32)) -> Census {
    let mut census = Census::default();
    let mut mobs = world.query::<(&MobKind, &Position, Has<SurfaceMob>)>();
    for (kind, position, surface) in mobs.iter(world) {
        let at = position.0.floor().as_ivec3();
        let (mx, mz) = world_to_chunk_coords(at.x, at.z);
        if (mx - cx).abs() > SPAWN_CENSUS_RADIUS_CHUNKS
            || (mz - cz).abs() > SPAWN_CENSUS_RADIUS_CHUNKS
        {
            continue;
        }
        let Some(actor) = ActorType::from_name(&kind.0.identifier) else {
            continue;
        };
        let pool = usize::from(surface);
        if let Some(category) = rules.category(actor) {
            census.base[pool][category.index()] += 1;
        }
        *census.types[pool].entry(actor).or_default() += 1;
    }
    census
}

/// Spawn one herd member; the natural spawn check rejects a body that
/// overlaps block collision.
fn spawn_one(
    world: &mut World,
    rules: &NaturalSpawnRules,
    census: &mut Census,
    actor: Option<ActorType>,
    position: DVec3,
    surface: bool,
    spawned: &mut Vec<Entity>,
) {
    let Some(actor) = actor else {
        return;
    };
    let mob = match spawn_mob(world, actor.name(), position, 0.0, SpawnCause::Spawned) {
        Ok(mob) => mob,
        Err(error) => {
            tracing::warn!(actor = actor.name(), %error, "natural spawn failed");
            return;
        }
    };
    let obstructed = world.get::<PhysicsBody>(mob).is_some_and(|body| {
        collides_with_blocks(world, Aabb::from_feet(position, body.width, body.height))
    });
    if obstructed {
        world.despawn(mob);
        return;
    }
    if surface {
        world.entity_mut(mob).insert(SurfaceMob);
    }
    let pool = usize::from(surface);
    if let Some(category) = rules.category(actor) {
        census.base[pool][category.index()] += 1;
    }
    *census.types[pool].entry(actor).or_default() += 1;
    spawned.push(mob);
}

/// `initial_event` on the first `initial_event_count` members, `event` on
/// every member from `event_skip_count` on.
fn send_herd_events(world: &mut World, herd: &Herd, spawned: &[Entity]) {
    if herd.event.is_none() && herd.initial_event.is_none() {
        return;
    }
    for (index, &mob) in spawned.iter().enumerate() {
        if index < herd.initial_event_count
            && let Some(name) = &herd.initial_event
        {
            fire(world, mob, name);
        }
        if index >= herd.event_skip_count
            && let Some(name) = &herd.event
        {
            fire(world, mob, name);
        }
    }
    crate::entity::definition::apply_pending_definitions(world);
    for &mob in spawned {
        crate::entity::mob::derive_entity(world, mob);
    }
}

/// Run an event the mob defines (undefined events do nothing).
fn fire(world: &mut World, mob: Entity, name: &str) {
    let Some(handle) = world
        .get::<MobKind>(mob)
        .and_then(|kind| kind.0.events.event(name))
    else {
        return;
    };
    if let Err(error) = events::fire_event(world, mob, &handle, None) {
        tracing::warn!(event = name, %error, "herd event failed");
    }
}

fn block(world: &World, blocks: &BlockRegistry, pos: IVec3) -> Option<(u32, &'static BlockData)> {
    let id = world.block_at(pos)?;
    Some((id, blocks.get_by_runtime_id(id)?.data))
}

fn block_data(world: &World, blocks: &BlockRegistry, pos: IVec3) -> Option<&'static BlockData> {
    block(world, blocks, pos).map(|(_, data)| data)
}

/// Material ordinal at `pos` (air when unloaded or unknown).
fn material(world: &World, blocks: &BlockRegistry, pos: IVec3) -> u8 {
    block_data(world, blocks, pos).map_or(0, |data| data.material)
}

fn solid_blocking(world: &World, blocks: &BlockRegistry, pos: IVec3) -> bool {
    block_data(world, blocks, pos).is_some_and(|data| data.solid && data.blocks_motion)
}

/// Brightness at `pos` derived from its chunk column (module docs).
fn raw_brightness(world: &World, blocks: &BlockRegistry, pos: IVec3) -> i32 {
    let Some(chunk) = chunk_at(world, pos) else {
        return 0;
    };
    let (lx, _, lz) = world_to_local_coords(pos.x, pos.y, pos.z);
    let height = i32::from(chunk.inner.height_map().at(lx, lz));
    let mut sky = SKY_LIGHT_MAX;
    if pos.y < height {
        for y in (pos.y..height).rev() {
            let (x, ly, z) = world_to_local_coords(pos.x, y, pos.z);
            let id = chunk.inner.get_block(x, ly, z);
            sky -= blocks
                .get_by_runtime_id(id)
                .map_or(0, |entry| i32::from(entry.data.filter_light));
            if sky <= 0 {
                return 0;
            }
        }
    }
    (sky - SKY_DARKEN).max(0)
}

fn chunk_at(world: &World, pos: IVec3) -> Option<&ChunkData> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get::<ChunkData>(world.resource::<ChunkManager>().get_by_coords(cx, cz)?)
}

fn biome_at(world: &World, pos: IVec3) -> Option<u32> {
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    chunk_at(world, pos)?.inner.biome_at(x, y, z)
}

fn random_int(world: &mut World, bound: i32) -> i32 {
    world
        .resource_mut::<EntityRandom>()
        .0
        .gen_range(0..bound.max(1))
}

fn world_difficulty(world: &World) -> Difficulty {
    use jolyne::valentine::types::EnumsSharedTypesLegacyDifficulty as Wire;
    match world
        .get_resource::<ServerWorldTemplate>()
        .map(|t| t.0.start_game_template.settings.game_difficulty)
    {
        Some(Wire::Easy) => Difficulty::Easy,
        Some(Wire::Normal) => Difficulty::Normal,
        Some(Wire::Hard) => Difficulty::Hard,
        _ => Difficulty::Peaceful,
    }
}

/// The `domobspawning` game rule the server announces.
fn do_mob_spawning(world: &World) -> bool {
    use jolyne::valentine::types::GameRuleRuleValue;
    world
        .get_resource::<ServerWorldTemplate>()
        .and_then(|template| {
            template
                .0
                .start_game_template
                .settings
                .rule_data
                .rules_list
                .iter()
                .find(|rule| rule.rule_name.eq_ignore_ascii_case(DO_MOB_SPAWNING_RULE))
                .and_then(|rule| match rule.rule_value {
                    GameRuleRuleValue::Bool(value) => Some(value),
                    _ => None,
                })
        })
        .unwrap_or(DO_MOB_SPAWNING_DEFAULT)
}
