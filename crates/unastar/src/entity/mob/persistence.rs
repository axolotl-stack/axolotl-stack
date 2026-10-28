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

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy_ecs::lifecycle::HookContext;
use bevy_ecs::prelude::*;
use bevy_ecs::world::DeferredWorld;
use glam::DVec3;
use zuri_nbt::err::{NBTError, ReadError};
use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};

use super::ageing::BabyAge;
use super::spawn_entity::SpawnEntityTimers;
use super::state::{BreedCooldown, Dead, InLove, Persistent};
use super::{MobKind, SpawnCause, derive_entity, spawn_mob};
use crate::ecs::resources::TickCounter;
use crate::entity::components::{Health, OnGround, Position, Rotation, Velocity};
use crate::entity::definition::{
    DefinitionState, GroupChange, apply_pending_definitions, queue_group_change,
};
use crate::entity::properties::{ActorProperties, NamedPropertyValue, PropertyError};
use crate::storage::{ChunkColumn, StoredActor};
use crate::world::ChunkPos;
use crate::world::ecs::{
    ChunkActorStore, ChunkData, ChunkPosition, ChunkStateFlags, PendingActorRestore,
};

const HEALTH_ATTRIBUTE: &str = "minecraft:health";

/// The mob's actor unique ID in storage (`UniqueID`). Assigned when the mob
/// is first saved and kept across saves and restarts. Sparse-set storage, so
/// the late insert does not move the mob's other components.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
#[component(storage = "SparseSet", on_insert = unique_id_inserted, on_replace = unique_id_replaced)]
pub struct MobUniqueId(pub i64);

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

fn list<T: Into<NBTTag>>(values: impl IntoIterator<Item = T>) -> NBTTag {
    NBTTag::List(tag::List(values.into_iter().map(Into::into).collect()))
}

/// The NBT a save writes for `mob` (tests).
#[cfg(test)]
pub(super) fn encode_for_tests(world: &mut World, mob: Entity) -> Vec<u8> {
    let id = unique_id(world, mob);
    encode_mob(world, mob, id)
}

/// Encode one mob as Bedrock actor NBT. `mob` must be a mob entity.
fn encode_mob(world: &World, mob: Entity, unique_id: i64) -> Vec<u8> {
    let archetype = &world.get::<MobKind>(mob).expect("saved actors are mobs").0;
    let state = world
        .get::<DefinitionState>(mob)
        .expect("every mob has a definition");
    let position = world
        .get::<Position>(mob)
        .expect("every mob has a position")
        .0;
    let rotation = world
        .get::<Rotation>(mob)
        .expect("every mob has a rotation");
    let velocity = world.get::<Velocity>(mob).map_or(DVec3::ZERO, |v| v.0);

    let mut fields = HashMap::new();
    let mut put = |name: &str, value: NBTTag| {
        fields.insert(name.to_owned(), value);
    };
    put(
        "identifier",
        NBTTag::String(archetype.identifier.as_ref().into()),
    );
    put("UniqueID", NBTTag::Long(unique_id.into()));
    put(
        "Pos",
        list(position.to_array().map(|v| tag::Float(v as f32))),
    );
    put(
        "Rotation",
        list([tag::Float(rotation.yaw), tag::Float(rotation.pitch)]),
    );
    put(
        "Motion",
        list(velocity.to_array().map(|v| tag::Float(v as f32))),
    );
    put(
        "OnGround",
        NBTTag::Byte(u8::from(world.get::<OnGround>(mob).is_some_and(|g| g.0)).into()),
    );
    put(
        "definitions",
        list(
            std::iter::once(format!("+{}", archetype.identifier))
                .chain(
                    state
                        .group_history()
                        .map(|(group, added)| format!("{}{group}", if added { '+' } else { '-' })),
                )
                .map(tag::String),
        ),
    );
    if let Some(health) = world.get::<Health>(mob) {
        let attribute = tag::Compound::builder()
            .with_string("Name", HEALTH_ATTRIBUTE)
            .with_float("Base", health.max)
            .with_float("Current", health.current)
            .with_float("Max", health.max)
            .with_float("Min", 0.0)
            .with_float("DefaultMax", health.max)
            .with_float("DefaultMin", 0.0)
            .build();
        put("Attributes", list([attribute]));
    }
    if let Some(properties) = world.get::<ActorProperties>(mob) {
        let mut saved = HashMap::new();
        for (name, value) in properties.named_values() {
            let value = match value {
                NamedPropertyValue::Bool(v) => NBTTag::Byte(u8::from(v).into()),
                NamedPropertyValue::Int(v) => NBTTag::Int(v.into()),
                NamedPropertyValue::Float(v) => NBTTag::Float(v.into()),
                NamedPropertyValue::Enum(v) => NBTTag::String(v.into()),
            };
            saved.insert(name.to_owned(), value);
        }
        put("properties", NBTTag::Compound(saved.into()));
    }
    put(
        "Persistent",
        NBTTag::Byte(u8::from(world.get::<Persistent>(mob).is_some()).into()),
    );
    if let Some(colour) = super::color::color_of(world, mob) {
        put("Color", NBTTag::Byte(colour.into()));
    }
    let now = current_tick(world);
    if let Some(love) = world.get::<InLove>(mob)
        && love.until_tick > now
    {
        put(
            "InLove",
            NBTTag::Int(remaining_ticks(love.until_tick, now).into()),
        );
    }
    if let Some(cooldown) = world.get::<BreedCooldown>(mob)
        && cooldown.until_tick > now
    {
        put(
            "BreedCooldown",
            NBTTag::Int(remaining_ticks(cooldown.until_tick, now).into()),
        );
    }
    // `Never` is recreated by the ageable composition and not stored.
    let growth = match world.get::<BabyAge>(mob) {
        Some(BabyAge::Growing { grow_tick }) => Some((remaining_ticks(*grow_tick, now), false)),
        Some(BabyAge::Paused { remaining }) => Some((remaining_ticks(*remaining, 0), true)),
        Some(BabyAge::Never) | None => None,
    };
    if let Some((remaining, paused)) = growth {
        put("Age", NBTTag::Int((-remaining).into()));
        put("GrowthPaused", NBTTag::Byte(u8::from(paused).into()));
    }
    if let Some(timers) = world.get::<SpawnEntityTimers>(mob) {
        put("entries", timers.to_nbt(now));
    }

    let mut out = Vec::new();
    NBTTag::Compound(fields.into())
        .write(&mut out, LittleEndian)
        .expect("actor NBT lists are homogeneous and write into a Vec");
    out
}

fn current_tick(world: &World) -> u64 {
    world
        .get_resource::<TickCounter>()
        .map_or(0, |tick| tick.current)
}

/// Ticks from `now` until the deadline `until_tick`, as BDS stores timers.
fn remaining_ticks(until_tick: u64, now: u64) -> i32 {
    i32::try_from(until_tick.saturating_sub(now)).unwrap_or(i32::MAX)
}

/// Why stored actor NBT could not be turned back into a mob.
#[derive(Debug, thiserror::Error)]
pub enum ActorDecodeError {
    #[error("actor NBT: {0}")]
    Nbt(#[from] NBTError<ReadError>),
    #[error("trailing bytes after the actor compound")]
    TrailingBytes,
    #[error("actor root is not a compound")]
    NotACompound,
    #[error("actor needs {kind} {name}")]
    MissingField {
        name: &'static str,
        kind: &'static str,
    },
    #[error("actor {name} is not {kind}")]
    WrongType {
        name: &'static str,
        kind: &'static str,
    },
    #[error("actor {name} is not a list of {len} Floats")]
    WrongFloatList { name: &'static str, len: usize },
    #[error("definition entry {0:?} has no +/- prefix")]
    UnprefixedDefinition(String),
    #[error("{identifier} has no component group {group}")]
    UnknownGroup { identifier: Arc<str>, group: String },
    #[error("property {name} has an unsupported tag type")]
    PropertyTag { name: String },
    #[error(transparent)]
    Property(#[from] PropertyError),
    #[error("cannot spawn the actor: {0}")]
    Spawn(String),
    #[error("cannot apply a saved component group: {0}")]
    Group(String),
}

/// Read access to a decoded actor compound.
struct Fields<'a>(&'a HashMap<String, NBTTag>);

impl Fields<'_> {
    fn string(&self, name: &'static str) -> Result<&str, ActorDecodeError> {
        match self.0.get(name) {
            Some(NBTTag::String(value)) => Ok(&value.0),
            None => Err(ActorDecodeError::MissingField {
                name,
                kind: "String",
            }),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "a String",
            }),
        }
    }

    fn long(&self, name: &'static str) -> Result<i64, ActorDecodeError> {
        match self.0.get(name) {
            Some(NBTTag::Long(value)) => Ok(value.0),
            None => Err(ActorDecodeError::MissingField { name, kind: "Long" }),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "a Long",
            }),
        }
    }

    fn int(&self, name: &'static str) -> Result<Option<i32>, ActorDecodeError> {
        match self.0.get(name) {
            None => Ok(None),
            Some(NBTTag::Int(value)) => Ok(Some(value.0)),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "an Int",
            }),
        }
    }

    fn flag(&self, name: &'static str) -> Result<bool, ActorDecodeError> {
        match self.0.get(name) {
            None => Ok(false),
            Some(NBTTag::Byte(value)) => Ok(value.0 != 0),
            Some(_) => Err(ActorDecodeError::WrongType {
                name,
                kind: "a Byte",
            }),
        }
    }

    fn floats<const N: usize>(
        &self,
        name: &'static str,
    ) -> Result<Option<[f32; N]>, ActorDecodeError> {
        let Some(tag) = self.0.get(name) else {
            return Ok(None);
        };
        let invalid = || ActorDecodeError::WrongFloatList { name, len: N };
        let NBTTag::List(values) = tag else {
            return Err(invalid());
        };
        if values.0.len() != N {
            return Err(invalid());
        }
        let mut out = [0.0; N];
        for (slot, value) in out.iter_mut().zip(&values.0) {
            let NBTTag::Float(value) = value else {
                return Err(invalid());
            };
            *slot = value.0;
        }
        Ok(Some(out))
    }
}

/// Why a stored actor was not restored.
#[derive(Debug, thiserror::Error)]
pub enum RestoreError {
    /// A mob with this unique ID is already in the world.
    #[error("actor {0} is already loaded")]
    AlreadyLoaded(i64),
    /// The record is malformed or names an actor this server does not run.
    #[error(transparent)]
    Decode(#[from] ActorDecodeError),
}

/// Restore one mob from Bedrock actor NBT.
pub fn restore_mob(world: &mut World, nbt: &[u8]) -> Result<Entity, RestoreError> {
    let mut input = nbt;
    let root = NBTTag::read(&mut input, LittleEndian).map_err(ActorDecodeError::from)?;
    if !input.is_empty() {
        return Err(ActorDecodeError::TrailingBytes.into());
    }
    let NBTTag::Compound(root) = root else {
        return Err(ActorDecodeError::NotACompound.into());
    };
    let fields = Fields(&root.0);
    let identifier = fields.string("identifier")?;
    let unique_id = fields.long("UniqueID")?;
    world.init_resource::<LoadedActorIds>();
    if world.resource::<LoadedActorIds>().0.contains(&unique_id) {
        return Err(RestoreError::AlreadyLoaded(unique_id));
    }
    let [x, y, z] = fields
        .floats::<3>("Pos")?
        .ok_or(ActorDecodeError::MissingField {
            name: "Pos",
            kind: "list of 3 Floats",
        })?;
    let [yaw, pitch] = fields.floats::<2>("Rotation")?.unwrap_or_default();
    let motion = fields.floats::<3>("Motion")?.unwrap_or_default();
    let position = DVec3::new(f64::from(x), f64::from(y), f64::from(z));

    let mob = spawn_mob(world, identifier, position, yaw, SpawnCause::Loaded)
        .map_err(ActorDecodeError::Spawn)?;
    match restore_state(world, mob, &fields, pitch, motion) {
        Ok(()) => {
            world.entity_mut(mob).insert(MobUniqueId(unique_id));
            Ok(mob)
        }
        Err(error) => {
            world.despawn(mob);
            Err(error.into())
        }
    }
}

/// Re-apply the saved composition and runtime state to a freshly spawned mob.
fn restore_state(
    world: &mut World,
    mob: Entity,
    fields: &Fields<'_>,
    pitch: f32,
    motion: [f32; 3],
) -> Result<(), ActorDecodeError> {
    let archetype = Arc::clone(&world.get::<MobKind>(mob).expect("spawn_mob makes mobs").0);
    match fields.0.get("definitions") {
        None => {}
        Some(NBTTag::List(entries)) => {
            for entry in &entries.0 {
                let NBTTag::String(entry) = entry else {
                    return Err(ActorDecodeError::WrongType {
                        name: "definitions",
                        kind: "a list of Strings",
                    });
                };
                let (added, name) = match entry.0.split_at_checked(1) {
                    Some(("+", name)) => (true, name),
                    Some(("-", name)) => (false, name),
                    _ => return Err(ActorDecodeError::UnprefixedDefinition(entry.0.clone())),
                };
                if name == &*archetype.identifier {
                    continue;
                }
                let group = archetype.definition.group(name).ok_or_else(|| {
                    ActorDecodeError::UnknownGroup {
                        identifier: Arc::clone(&archetype.identifier),
                        group: name.to_owned(),
                    }
                })?;
                let change = if added {
                    GroupChange::Add(group)
                } else {
                    GroupChange::Remove(group)
                };
                queue_group_change(world, mob, change).map_err(ActorDecodeError::Group)?;
            }
        }
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "definitions",
                kind: "a list",
            });
        }
    }
    apply_pending_definitions(world);

    let health = match fields.0.get("Attributes") {
        None => None,
        Some(NBTTag::List(attributes)) => attributes.0.iter().find_map(|attribute| {
            let NBTTag::Compound(attribute) = attribute else {
                return None;
            };
            let attribute = Fields(&attribute.0);
            if attribute.string("Name").ok()? != HEALTH_ATTRIBUTE {
                return None;
            }
            let float = |name| match attribute.0.get(name) {
                Some(NBTTag::Float(value)) => Some(value.0),
                _ => None,
            };
            Some((float("Current"), float("Max")))
        }),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "Attributes",
                kind: "a list",
            });
        }
    };
    let properties = match fields.0.get("properties") {
        None => None,
        Some(NBTTag::Compound(properties)) => Some(&properties.0),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "properties",
                kind: "a compound",
            });
        }
    };
    let persistent = fields.flag("Persistent")?;
    let in_love = fields.int("InLove")?;
    let cooldown = fields.int("BreedCooldown")?;
    let age = fields.int("Age")?;
    let growth_paused = fields.flag("GrowthPaused")?;
    let on_ground = fields.flag("OnGround")?;
    let color = match fields.0.get("Color") {
        None => None,
        Some(NBTTag::Byte(value)) => Some((i32::from(value.0) & 0xf) as u8),
        Some(_) => {
            return Err(ActorDecodeError::WrongType {
                name: "Color",
                kind: "a Byte",
            });
        }
    };

    let now = current_tick(world);
    let mut entity = world.entity_mut(mob);
    if let Some((current, max)) = health
        && let Some(mut component) = entity.get_mut::<Health>()
    {
        if let Some(max) = max {
            component.max = max;
        }
        if let Some(current) = current {
            component.current = current;
        }
    }
    if let Some(properties) = properties {
        let mut component = entity
            .get_mut::<ActorProperties>()
            .expect("every mob has properties");
        for (name, value) in properties {
            let value = match value {
                NBTTag::Byte(v) => NamedPropertyValue::Bool(v.0 != 0),
                NBTTag::Int(v) => NamedPropertyValue::Int(v.0),
                NBTTag::Float(v) => NamedPropertyValue::Float(v.0),
                NBTTag::String(v) => NamedPropertyValue::Enum(&v.0),
                _ => return Err(ActorDecodeError::PropertyTag { name: name.clone() }),
            };
            component.set_named(name, value)?;
        }
    }
    if let Some(mut rotation) = entity.get_mut::<Rotation>() {
        rotation.pitch = pitch;
    }
    if let Some(mut velocity) = entity.get_mut::<Velocity>() {
        velocity.0 = DVec3::from_array(motion.map(f64::from));
    }
    if let Some(mut ground) = entity.get_mut::<OnGround>() {
        ground.0 = on_ground;
    }
    // The composition re-applied its colour group; a dyed colour overrides it.
    if let Some(colour) = color {
        super::color::set_color(&mut entity, super::color::ActorColor(colour));
    }
    if persistent {
        entity.insert(Persistent);
    }
    if let Some(ticks) = in_love.filter(|&ticks| ticks > 0) {
        entity.insert(InLove {
            until_tick: now + ticks as u64,
            player: None,
        });
    }
    if let Some(ticks) = cooldown.filter(|&ticks| ticks > 0) {
        entity.insert(BreedCooldown {
            until_tick: now + ticks as u64,
        });
    }
    // The ageable composition inserted a fresh growth state; overwrite it
    // with the saved one.
    if let Some(age) = age.filter(|&age| age <= 0)
        && entity.contains::<BabyAge>()
    {
        let remaining = u64::from(age.unsigned_abs());
        entity.insert(if growth_paused {
            BabyAge::Paused { remaining }
        } else {
            BabyAge::Growing {
                grow_tick: now + remaining,
            }
        });
    }
    if let Some(mut timers) = world.get_mut::<SpawnEntityTimers>(mob) {
        let legacy = match fields.0.get("SpawnTimer") {
            Some(NBTTag::Int(value)) => Some(value.0),
            _ => None,
        };
        timers.restore(fields.0.get("entries"), legacy, now);
    }
    derive_entity(world, mob);
    Ok(())
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
mod tests {
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use jolyne::valentine::actor_flags;

    use super::*;
    use crate::entity::EntityRandom;
    use crate::entity::flags::ActorFlags;
    use crate::server::game::{GameServer, ServerConfig};
    use crate::storage::LevelDBWorldProvider;
    use crate::world::ChunkManager;
    use crate::world::ecs::{ChunkLoadConfig, ChunkManagerWorldExt};

    /// Feet position in the middle of chunk (0, 0), on the void world's
    /// stone platform (top at y = 16).
    const COW_FEET: DVec3 = DVec3::new(8.5, 16.0, 8.5);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            Self(
                std::env::temp_dir()
                    .join(format!("unastar-mob-persistence-{}", uuid::Uuid::new_v4())),
            )
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A server whose world is stored by the LevelDB provider in `dir`.
    fn stored_server(dir: &Path) -> GameServer {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(EntityRandom::seeded(7));
        let provider = LevelDBWorldProvider::open(dir, 0).expect("open leveldb");
        world
            .resource_mut::<ChunkManager>()
            .set_provider(Arc::new(provider));
        server
    }

    fn is_baby(world: &World, mob: Entity) -> bool {
        world
            .get::<ActorFlags>(mob)
            .unwrap()
            .get(actor_flags::BABY)
            .unwrap()
    }

    /// Spawn cows until the spawn event picks the given age.
    fn spawn_cow(world: &mut World, baby: bool) -> Entity {
        for _ in 0..200 {
            let cow =
                spawn_mob(world, "minecraft:cow", COW_FEET, 0.0, SpawnCause::Spawned).unwrap();
            if is_baby(world, cow) == baby {
                return cow;
            }
            world.despawn(cow);
        }
        panic!("no cow of the requested age in 200 spawns");
    }

    fn cows(world: &mut World) -> Vec<Entity> {
        let mut query = world.query_filtered::<Entity, With<MobKind>>();
        query.iter(world).collect()
    }

    /// Everything persistence must carry, comparable across worlds.
    #[derive(Debug, PartialEq)]
    struct Saved {
        identifier: String,
        position: DVec3,
        yaw: f32,
        health: (f32, f32),
        baby: bool,
        groups: Vec<(String, bool)>,
        properties: Vec<(String, String)>,
        persistent: bool,
        growth: Option<BabyAge>,
        love: Option<u64>,
        cooldown: Option<u64>,
    }

    fn saved(world: &World, mob: Entity) -> Saved {
        let health = world.get::<Health>(mob).unwrap();
        Saved {
            identifier: world.get::<MobKind>(mob).unwrap().0.identifier.to_string(),
            position: world.get::<Position>(mob).unwrap().0,
            yaw: world.get::<Rotation>(mob).unwrap().yaw,
            health: (health.current, health.max),
            baby: is_baby(world, mob),
            groups: world
                .get::<DefinitionState>(mob)
                .unwrap()
                .group_history()
                .map(|(group, added)| (group.to_owned(), added))
                .collect(),
            properties: world
                .get::<ActorProperties>(mob)
                .unwrap()
                .named_values()
                .map(|(name, value)| (name.to_owned(), format!("{value:?}")))
                .collect(),
            persistent: world.get::<Persistent>(mob).is_some(),
            growth: world.get::<BabyAge>(mob).copied(),
            love: world.get::<InLove>(mob).map(|love| love.until_tick),
            cooldown: world.get::<BreedCooldown>(mob).map(|c| c.until_tick),
        }
    }

    fn save_all_chunks(server: &mut GameServer) -> usize {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(server.save_all_chunks())
    }

    #[test]
    fn cows_round_trip_through_leveldb_across_a_restart() {
        let dir = TempDir::new();
        let (adult, baby) = {
            let mut server = stored_server(&dir.0);
            let world = server.ecs.world_mut();
            world.get_or_create_chunk(0, 0);
            let adult = spawn_cow(world, false);
            let baby = spawn_cow(world, true);
            world.get_mut::<Health>(adult).unwrap().current = 7.0;
            world.get_mut::<Rotation>(adult).unwrap().yaw = 33.0;
            world
                .get_mut::<ActorProperties>(adult)
                .unwrap()
                .set_named(
                    "minecraft:climate_variant",
                    NamedPropertyValue::Enum("cold"),
                )
                .unwrap();
            assert!(
                matches!(world.get::<BabyAge>(baby), Some(BabyAge::Growing { .. })),
                "a spawned baby cow is growing"
            );
            world
                .entity_mut(baby)
                .insert((Persistent, BabyAge::Paused { remaining: 1234 }));
            world.entity_mut(adult).insert((
                InLove {
                    until_tick: 300,
                    player: None,
                },
                BreedCooldown { until_tick: 900 },
            ));
            let saved = (saved(world, adult), saved(world, baby));
            assert_eq!(save_all_chunks(&mut server), 1);
            saved
        };
        assert!(
            adult
                .groups
                .contains(&("minecraft:cow_adult".to_owned(), true)),
            "{adult:?}"
        );
        assert!(
            baby.groups
                .contains(&("minecraft:cow_baby".to_owned(), true)),
            "{baby:?}"
        );

        // A new server over the same storage.
        let mut server = stored_server(&dir.0);
        let world = server.ecs.world_mut();
        world.get_or_create_chunk(0, 0);
        let mut restored: Vec<Saved> = cows(world)
            .into_iter()
            .map(|cow| saved(world, cow))
            .collect();
        restored.sort_by_key(|cow| cow.baby);
        assert_eq!(restored, [adult, baby]);
    }

    #[test]
    fn unloading_saves_and_removes_the_cow_and_reloading_restores_it() {
        let dir = TempDir::new();
        let mut server = stored_server(&dir.0);
        server
            .ecs
            .world_mut()
            .resource_mut::<ChunkLoadConfig>()
            .unload_grace_ticks = 1;
        let world = server.ecs.world_mut();
        world.get_or_create_chunk(0, 0);
        let cow = spawn_cow(world, false);
        let unique_id = unique_id(world, cow);

        let mut last_position = COW_FEET;
        for _ in 0..5 {
            if let Some(position) = server.ecs.world().get::<Position>(cow) {
                last_position = position.0;
            }
            server.tick();
            if !server
                .ecs
                .world()
                .resource::<ChunkManager>()
                .contains_coords(0, 0)
            {
                break;
            }
        }
        let world = server.ecs.world_mut();
        assert!(
            !world.resource::<ChunkManager>().contains_coords(0, 0),
            "chunk without viewers unloads after its grace period"
        );
        assert!(world.get_entity(cow).is_err());
        assert!(cows(world).is_empty());

        let chunk = world.get_or_create_chunk(0, 0);
        let [restored] = cows(world)[..] else {
            panic!("exactly one cow restored");
        };
        assert_eq!(
            &*world.get::<MobKind>(restored).unwrap().0.identifier,
            "minecraft:cow"
        );
        assert_eq!(
            world.get::<MobUniqueId>(restored),
            Some(&MobUniqueId(unique_id))
        );
        let position = world.get::<Position>(restored).unwrap().0;
        assert!(
            position.distance(last_position) < 0.5,
            "{position} vs {last_position}"
        );
        assert_eq!(
            world.get::<ChunkActorStore>(chunk).unwrap().stored,
            [unique_id]
        );
    }

    #[test]
    fn a_dead_cow_is_deleted_from_storage_on_the_next_save() {
        let dir = TempDir::new();
        let mut server = stored_server(&dir.0);
        let world = server.ecs.world_mut();
        world.get_or_create_chunk(0, 0);
        let cow = spawn_cow(world, false);
        save_all_chunks(&mut server);
        server.ecs.world_mut().despawn(cow);
        save_all_chunks(&mut server);
        drop(server);

        let mut server = stored_server(&dir.0);
        let world = server.ecs.world_mut();
        world.get_or_create_chunk(0, 0);
        assert!(cows(world).is_empty());
    }

    #[test]
    fn unknown_stored_actors_are_kept_and_written_back() {
        let dir = TempDir::new();
        let mut server = stored_server(&dir.0);
        let world = server.ecs.world_mut();
        let chunk = world.get_or_create_chunk(0, 0);
        let mut nbt = Vec::new();
        NBTTag::Compound(
            tag::Compound::builder()
                .with_string("identifier", "minecraft:unknown_mob")
                .with_long("UniqueID", 42i64)
                .with("Pos", list([1.0f32, 16.0, 1.0].map(tag::Float)))
                .build(),
        )
        .write(&mut nbt, LittleEndian)
        .unwrap();
        world
            .entity_mut(chunk)
            .insert(PendingActorRestore(vec![StoredActor {
                unique_id: 42,
                nbt: nbt.clone(),
            }]));
        restore_actors_of_chunk(world, chunk);
        assert!(cows(world).is_empty());
        save_all_chunks(&mut server);
        drop(server);

        let mut server = stored_server(&dir.0);
        let world = server.ecs.world_mut();
        let chunk = world.get_or_create_chunk(0, 0);
        assert_eq!(
            world.get::<ChunkActorStore>(chunk).unwrap().foreign,
            [StoredActor { unique_id: 42, nbt }]
        );
    }
}
