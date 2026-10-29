//! Saving mobs with their chunk and restoring them when the chunk loads.
//!
//! A mob is stored as Bedrock actor NBT (a rooted little-endian compound) in
//! the chunk its feet are in:
//!
//! | Tag | Type | Meaning |
//! |---|---|---|
//! | `identifier` | String | actor identifier |
//! | `UniqueID` | Long | unique ID, stable across saves ([`MobUniqueId`]) |
//! | `Pos` | List of 3 Float | feet position |
//! | `Rotation` | List of 2 Float | body yaw, pitch (degrees) |
//! | `Motion` | List of 3 Float | velocity, blocks per tick |
//! | `OnGround` | Byte | |
//! | `definitions` | List of String | `+<identifier>`, then every component group added (`+group`) or removed (`-group`) in order of its latest change |
//! | `Attributes` | List of Compound | `minecraft:health` with `Base`, `Current`, `Max`, `Min`, `DefaultMax`, `DefaultMin` |
//! | `properties` | Compound | actor properties by name; enums by value name |
//! | `Persistent` | Byte | never despawns |
//! | `InLove` | Int | remaining love ticks, when in love |
//! | `BreedCooldown` | Int | remaining breed-cooldown ticks, when cooling down |
//! | `Age` | Int | baby age: negative ticks until grown, when growing |
//! | `GrowthPaused` | Byte | baby growth paused |
//! | `entries` | List of Compound | `minecraft:spawn_entity` timers by rule: `SpawnTimer` (Int, ticks until the rule fires, 0 = roll a new wait) and `StopSpawning` (Byte, a fired single-use rule) |
//! | `Mainhand`, `Offhand` | List of 1 item Compound | held items ([`crate::entity::mob::gear::persist`]: `Name`, `Count`, `Damage`, `WasPickedUp`, optional `tag`) |
//! | `Armor` | List of 4 item Compounds | head, chest, legs, feet |
//! | `SlotDropChances` | List of Compound | authored drop chances: `Slot` (String: `mainhand`, `offhand`, `head`, `chest`, `legs`, `feet`) and `DropChance` (Float) |
//! | `canPickupItems`, `hasSetCanPickupItems` | Byte | the `pickup_based_on_chance` roll, once made |
//!
//! Restoring spawns the mob without its lifecycle event, replays the saved
//! groups (so a baby stays a baby and an adult keeps its adult components)
//! and then applies the saved runtime state.
//!
//! Chunk bookkeeping: each chunk's [`ChunkActorStore`] remembers which actor
//! IDs storage lists for it. A save writes the mobs now in the chunk and asks
//! the provider to delete records of listed actors that no longer exist
//! anywhere; actors that walked into another chunk keep their record, which
//! that chunk's save rewrites.

mod load;
mod save;

use std::collections::{HashMap, HashSet};

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;

pub use load::{ActorDecodeError, RestoreError, restore_mob};
use save::encode_mob;

use crate::entity::components::Position;
use crate::entity::mob::MobKind;
use crate::entity::mob::state::Dead;
use crate::storage::{ChunkColumn, StoredActor};
use crate::world::ChunkPos;
use crate::world::ecs::{
    ChunkActorStore, ChunkData, ChunkPosition, ChunkStateFlags, PendingActorRestore,
};

pub(super) const HEALTH_ATTRIBUTE: &str = "minecraft:health";

/// The mob's actor unique ID in storage (`UniqueID`). Assigned when the mob
/// is first saved and kept across saves and restarts. Sparse-set storage, so
/// the late insert does not move the mob's other components.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet", on_insert = unique_id_inserted, on_replace = unique_id_replaced)]
pub struct MobUniqueId(pub i64);

/// The mob is being restored from storage ([`restore_mob`]): component
/// kinds that roll state when they are added (the gear table, the pickup
/// chance) leave it to the saved record. Inserted by `spawn_mob` for
/// [`crate::entity::mob::SpawnCause::Loaded`] and removed when the restore
/// finishes or fails.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet")]
pub struct Restoring;

/// Unique IDs of the mobs currently in the world, so an actor listed by two
/// chunks' stored actor lists is restored once.
#[derive(Resource, Debug, Default)]
pub struct LoadedActorIds(HashSet<i64>);

fn unique_id_inserted(mut world: DeferredWorld<'_>, context: HookContext) {
    let Some(id) = world.get::<MobUniqueId>(context.entity).map(|id| id.0) else {
        return;
    };
    if let Some(mut ids) = world.get_resource_mut::<LoadedActorIds>() {
        ids.0.insert(id);
    }
}

fn unique_id_replaced(mut world: DeferredWorld<'_>, context: HookContext) {
    let Some(id) = world.get::<MobUniqueId>(context.entity).map(|id| id.0) else {
        return;
    };
    if let Some(mut ids) = world.get_resource_mut::<LoadedActorIds>() {
        ids.0.remove(&id);
    }
}

/// The mob's unique ID, assigning a fresh one on first use. IDs are drawn
/// from the operating system's randomness (not the seeded gameplay RNG), so
/// they do not collide with actors stored in other, unloaded chunks.
fn unique_id(world: &mut World, mob: Entity) -> i64 {
    if let Some(id) = world.get::<MobUniqueId>(mob) {
        return id.0;
    }
    world.init_resource::<LoadedActorIds>();
    let id = loop {
        let id: i64 = rand::random();
        // -1 is the invalid actor ID.
        if id != -1 && id != 0 && !world.resource::<LoadedActorIds>().0.contains(&id) {
            break id;
        }
    };
    world.entity_mut(mob).insert(MobUniqueId(id));
    id
}

/// The NBT a save writes for `mob` (tests).
#[cfg(test)]
pub(super) fn encode_for_tests(world: &mut World, mob: Entity) -> Vec<u8> {
    let id = unique_id(world, mob);
    encode_mob(world, mob, id)
}

/// Restore the stored actors waiting on `chunk`. Actors this server does not
/// run are kept in the chunk's [`ChunkActorStore`] and written back unchanged.
pub fn restore_actors_of_chunk(world: &mut World, chunk: Entity) {
    let Some(PendingActorRestore(actors)) = world.entity_mut(chunk).take::<PendingActorRestore>()
    else {
        return;
    };
    let mut foreign = Vec::new();
    for actor in actors {
        match restore_mob(world, &actor.nbt) {
            Ok(_) => {}
            Err(RestoreError::AlreadyLoaded(id)) => {
                tracing::debug!(unique_id = id, "Stored actor is already loaded");
            }
            Err(RestoreError::Decode(error)) => {
                tracing::warn!(
                    unique_id = actor.unique_id,
                    %error,
                    "Keeping a stored actor this server cannot run"
                );
                foreign.push(actor);
            }
        }
    }
    if !foreign.is_empty()
        && let Some(mut store) = world.get_mut::<ChunkActorStore>(chunk)
    {
        store.foreign.extend(foreign);
    }
}

/// System: restore the stored actors of chunks loaded this tick.
pub fn restore_chunk_actors(world: &mut World) {
    let mut pending = world.query_filtered::<Entity, With<PendingActorRestore>>();
    let chunks: Vec<Entity> = pending.iter(world).collect();
    for chunk in chunks {
        restore_actors_of_chunk(world, chunk);
    }
}

/// A chunk prepared for saving.
pub struct ChunkSnapshot {
    pub chunk: Entity,
    pub pos: ChunkPos,
    /// The column to write, or `None` when storage is already up to date
    /// (clean terrain, no mobs, no actors stored before).
    pub column: Option<ChunkColumn>,
    /// Every mob whose feet are in the chunk, dying ones included (those are
    /// not written).
    pub mobs: Vec<Entity>,
}

/// Prepare `chunks` for saving: the mobs standing in each, encoded, plus the
/// chunk's actors this server does not run. With `take_terrain` (unloading)
/// the block data is moved out of the chunk instead of copied.
pub fn snapshot_chunks(
    world: &mut World,
    chunks: &[Entity],
    take_terrain: bool,
) -> Vec<ChunkSnapshot> {
    let mut index = HashMap::with_capacity(chunks.len());
    for (slot, &chunk) in chunks.iter().enumerate() {
        if let Some(pos) = world.get::<ChunkPosition>(chunk) {
            index.insert((pos.x, pos.z), slot);
        }
    }
    let mut mobs_by_chunk = vec![Vec::new(); chunks.len()];
    let mut live = HashSet::new();
    // Projectiles in flight are not stored or unloaded with the chunk:
    // their flight state (owner, ticks in flight) has no saved form here, so
    // they stay in memory, frozen until their chunk loads again.
    let mut mobs = world.query_filtered::<(Entity, &Position, Has<Dead>, Option<&MobUniqueId>), (
        With<MobKind>,
        Without<crate::entity::components::Projectile>,
    )>();
    for (mob, position, dead, id) in mobs.iter(world) {
        if !dead && let Some(id) = id {
            live.insert(id.0);
        }
        let key = (
            (position.0.x.floor() as i32) >> 4,
            (position.0.z.floor() as i32) >> 4,
        );
        if let Some(&slot) = index.get(&key) {
            mobs_by_chunk[slot].push(mob);
        }
    }

    let mut snapshots = Vec::with_capacity(chunks.len());
    for (&chunk, mobs) in chunks.iter().zip(mobs_by_chunk) {
        let Some(pos) = world
            .get::<ChunkPosition>(chunk)
            .map(ChunkPosition::to_chunk_pos)
        else {
            continue;
        };
        let mut actors = Vec::with_capacity(mobs.len());
        for &mob in &mobs {
            if world.get::<Dead>(mob).is_some() {
                continue;
            }
            let unique_id = unique_id(world, mob);
            let nbt = encode_mob(world, mob, unique_id);
            actors.push(StoredActor { unique_id, nbt });
        }
        let (stored, dirty) = {
            let store = world.get::<ChunkActorStore>(chunk);
            if let Some(store) = store {
                actors.extend(store.foreign.iter().cloned());
            }
            (
                store.map(|store| store.stored.clone()).unwrap_or_default(),
                world
                    .get::<ChunkStateFlags>(chunk)
                    .is_some_and(ChunkStateFlags::is_dirty),
            )
        };
        let column = if dirty || !actors.is_empty() || !stored.is_empty() {
            let terrain = if take_terrain {
                world
                    .entity_mut(chunk)
                    .take::<ChunkData>()
                    .map(|data| data.inner)
            } else {
                world.get::<ChunkData>(chunk).map(|data| data.inner.clone())
            };
            terrain.map(|chunk| {
                let removed_actors = stored
                    .into_iter()
                    .filter(|id| {
                        !live.contains(id) && !actors.iter().any(|actor| actor.unique_id == *id)
                    })
                    .collect();
                ChunkColumn {
                    chunk,
                    actors,
                    removed_actors,
                }
            })
        } else {
            None
        };
        snapshots.push(ChunkSnapshot {
            chunk,
            pos,
            column,
            mobs,
        });
    }
    snapshots
}

/// Record that `snapshot` was written: storage now lists exactly its actors
/// and the terrain is clean.
pub fn mark_saved(world: &mut World, snapshot: &ChunkSnapshot) {
    let Some(column) = &snapshot.column else {
        return;
    };
    let Ok(mut chunk) = world.get_entity_mut(snapshot.chunk) else {
        return;
    };
    if let Some(mut store) = chunk.get_mut::<ChunkActorStore>() {
        store.stored.clear();
        store
            .stored
            .extend(column.actors.iter().map(|actor| actor.unique_id));
    }
    if let Some(mut flags) = chunk.get_mut::<ChunkStateFlags>() {
        flags.clear_dirty();
    }
}

#[cfg(test)]
mod tests;
