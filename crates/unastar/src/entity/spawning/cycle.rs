//! The natural spawn cycle and the world-generation pass.
//!
//! Every tick each loaded chunk a player ticks (`ticking_area.rs`) rolls for
//! a spawn attempt. An attempt picks a random column of the chunk, finds its
//! top spawn block (the surface) and every cave floor below it
//! (underground), and runs one cluster at each: draw a spawn entry from the
//! biome's pool among the entries whose conditions hold, check the drawn
//! entry's conditions again, its rarity and the spawn position, then spawn a
//! herd within the population caps; any failure ends the cluster without a
//! second draw. A freshly generated chunk additionally runs one surface
//! cluster in a random column the first tick a player ticks it.
//!
//! Brightness is the raw brightness above the block
//! ([`crate::world::light::brightness`]: the sky light after the time of
//! day darkened it, or the block light, whichever is brighter). The server has no
//! weather, so `adjust_for_weather` (the brightness recomputed with the sky
//! darkened by 10 while it rains) never changes the brightness, and no
//! position is in a bubble column, so `disallow_spawns_in_bubble` never
//! rejects. `difficulty_filter` compares with the world [`Difficulty`]; on
//! Peaceful only entries whose filter admits Peaceful spawn (vanilla
//! monster rules start at Easy).
//!
//! Each herd member then passes its native class's spawn check
//! (`spawn_check.rs`), and the room check runs before the mob is created
//! (no mob is created and removed again): the definition's collision box at
//! the spawn position is refused only when a block in the cells it spans is
//! solid-blocking and the box overlaps block collision, so snow layers of
//! any height at the feet allow a spawn. A member fires the entry's
//! `spawn_event`, when it has one, instead of `minecraft:entity_spawned`.
//!
//! The spawn position's player distance counts every player but spectators
//! (dead players count).

use std::collections::HashMap;
use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use rand::Rng;
use unastar_data::actor_identifiers::ActorType;

use super::SurfaceMob;
use super::position::{block, find_spawn_block, has_room, material, slab_half, spawn_position_ok};
use super::rules::{Category, Herd, NaturalSpawnRules, SpawnEntry, SpawnTarget};
use super::ticking_area::TickedChunks;
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{GameMode, Player, Position};
use crate::entity::mob::constants::{
    MATERIAL_LAVA, MATERIAL_LEAVES, MATERIAL_WATER, MOBS_PER_CHUNK_SURFACE,
    MOBS_PER_CHUNK_UNDERGROUND, SPAWN_CENSUS_RADIUS_CHUNKS, SPAWN_GLOBAL_MOB_LIMIT,
    SPAWN_POSITION_OFFSET, SPAWN_TICK_ROLL, SPAWN_TICK_ROLL_MAX, TICKS_PER_SECOND,
};
use crate::entity::mob::state::Persistent;
use crate::entity::mob::{MobKind, SpawnCause, events, spawn_mob, spawn_mob_with_event};
use crate::entity::systems::physics::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::collision::Aabb;
use crate::server::game::types::BlockRegistryResource;
use crate::world::chunk::{MIN_Y, SUBCHUNK_COUNT};
use crate::world::ecs::systems::{world_to_chunk_coords, world_to_local_coords};
use crate::world::ecs::{ChunkData, ChunkLoadConfig, ChunkManager, ChunkStateFlags};
use crate::world::light::brightness;
use crate::world::rules::{Difficulty, GameRules};

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
    /// The chunks players tick.
    ticked: TickedChunks,
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
    if !world.contains_resource::<NaturalSpawnRules>()
        || !world.resource::<GameRules>().do_mob_spawning
    {
        return;
    }
    world.resource_scope(|world, rules: Mut<NaturalSpawnRules>| run(world, &rules));
}

fn run(world: &mut World, rules: &NaturalSpawnRules) {
    let tick_range = world.resource::<ChunkLoadConfig>().simulation_distance;
    let mut query = world.query_filtered::<(&Position, &GameMode), With<Player>>();
    let mut tickers = Vec::new();
    let mut players = Vec::new();
    for (position, mode) in query.iter(world) {
        tickers.push(position.0);
        if *mode != GameMode::Spectator {
            players.push(position.0);
        }
    }
    let manager = world.resource::<ChunkManager>();
    let ticked = TickedChunks::around(tickers, tick_range, |(cx, cz)| {
        manager
            .get_by_coords(cx, cz)
            .and_then(|entity| world.get::<ChunkStateFlags>(entity))
            .is_some_and(ChunkStateFlags::is_ticking)
    });
    let chunks: Vec<((i32, i32), Entity)> = ticked
        .sorted()
        .into_iter()
        .filter_map(|(cx, cz)| Some(((cx, cz), manager.get_by_coords(cx, cz)?)))
        .collect();
    if chunks.is_empty() {
        return;
    }
    let mut mobs = world.query::<&MobKind>();
    let spawnable = mobs
        .iter(world)
        .filter(|kind| {
            ActorType::from_name(&kind.0.identifier).is_some_and(|a| rules.category(a).is_some())
        })
        .count();
    let tick = Tick {
        tick_range,
        difficulty: *world.resource::<Difficulty>(),
        age: world.resource::<TickCounter>().current / TICKS_PER_SECOND as u64,
        players,
        ticked,
        spawnable: u32::try_from(spawnable).unwrap_or(u32::MAX),
        blocks: Arc::clone(&world.resource::<BlockRegistryResource>().0),
    };
    for (coords, entity) in chunks {
        let needs_population = world
            .get::<ChunkStateFlags>(entity)
            .is_some_and(ChunkStateFlags::needs_population);
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
    conditions.brightness = i32::from(brightness(world, conditions.pos + IVec3::Y));
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
    if !can_spawn_in_conditions(world, entry, &conditions, tick) {
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
    if !spawn_position_ok(
        world,
        blocks,
        entry,
        feet,
        tick.tick_range,
        &tick.players,
        &tick.ticked,
    ) {
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
                    entry,
                    &permutation.target,
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
            let target = permuted(world, entry);
            spawn_one(
                world,
                rules,
                &mut census,
                entry,
                target,
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
        .filter(|entry| can_spawn_in_conditions(world, entry, lookup, tick))
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

/// What a non-guaranteed herd member spawns as.
fn permuted<'e>(world: &mut World, entry: &'e SpawnEntry) -> &'e SpawnTarget {
    let total: u32 = entry.permutations.iter().map(|p| p.weight).sum();
    if total == 0 {
        return &entry.own;
    }
    let mut roll = random_int(world, total as i32);
    for permutation in &entry.permutations {
        roll -= permutation.weight as i32;
        if roll < 0 {
            return entry.target(permutation);
        }
    }
    &entry.own
}

/// The entry's conditions at `conditions`. The rule's own placement test
/// ([`SpawnEntry::check`], the slime's) runs after the height and
/// surface/underground conditions and before the difficulty and brightness
/// filters, and may draw from the world's random.
fn can_spawn_in_conditions(
    world: &mut World,
    entry: &SpawnEntry,
    conditions: &Conditions,
    tick: &Tick,
) -> bool {
    let y = conditions.pos.y;
    (MIN_Y..MAX_Y).contains(&y)
        && (entry.height.0..=entry.height.1).contains(&y)
        && (!conditions.water || entry.underwater)
        && (!conditions.lava || entry.lava)
        && (!conditions.surface || entry.surface)
        && (!conditions.underground || entry.underground)
        && entry
            .check
            .as_ref()
            .is_none_or(|check| check.allows(world, conditions.pos, conditions.brightness))
        && (entry.difficulty.0..=entry.difficulty.1).contains(&tick.difficulty)
        && (entry.brightness.0..=entry.brightness.1).contains(&conditions.brightness)
        && (entry.world_age.0..=entry.world_age.1).contains(&tick.age)
        // The spawn delay ends at world age 0 (no delay filter is compiled).
        && tick.age > 0
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

/// Spawn one herd member of `entry` when its body has room at `position`
/// and its native class's spawn check passes; both run before the mob is
/// created.
#[allow(clippy::too_many_arguments)]
fn spawn_one(
    world: &mut World,
    rules: &NaturalSpawnRules,
    census: &mut Census,
    entry: &SpawnEntry,
    target: &SpawnTarget,
    position: DVec3,
    surface: bool,
    spawned: &mut Vec<Entity>,
) {
    let &SpawnTarget::Mob {
        actor,
        class,
        immune_to_wither,
        room,
    } = target
    else {
        return;
    };
    let blocks = &world.resource::<BlockRegistryResource>().0;
    if !has_room(
        world,
        blocks,
        Aabb::from_feet(position, room.width, room.height),
    ) {
        return;
    }
    let feet = position.floor().as_ivec3();
    let passes = world.resource_scope(|world, mut random: Mut<EntityRandom>| {
        rules
            .native_blocks
            .allows(world, class, immune_to_wither, feet, &mut random.0)
    });
    if !passes {
        return;
    }
    let mob = match &entry.spawn_event {
        Some(event) => spawn_mob_with_event(world, actor.name(), position, 0.0, event),
        None => spawn_mob(world, actor.name(), position, 0.0, SpawnCause::Spawned),
    };
    let mob = match mob {
        Ok(mob) => mob,
        Err(error) => {
            tracing::warn!(actor = actor.name(), %error, "natural spawn failed");
            return;
        }
    };
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

fn chunk_at(world: &World, pos: IVec3) -> Option<&ChunkData> {
    let (cx, cz) = world_to_chunk_coords(pos.x, pos.z);
    world.get::<ChunkData>(world.resource::<ChunkManager>().get_by_coords(cx, cz)?)
}

pub(super) fn biome_at(world: &World, pos: IVec3) -> Option<u32> {
    let (x, y, z) = world_to_local_coords(pos.x, pos.y, pos.z);
    chunk_at(world, pos)?.inner.biome_at(x, y, z)
}

fn random_int(world: &mut World, bound: i32) -> i32 {
    world
        .resource_mut::<EntityRandom>()
        .0
        .gen_range(0..bound.max(1))
}
