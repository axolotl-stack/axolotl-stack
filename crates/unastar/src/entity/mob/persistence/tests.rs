use std::path::{Path, PathBuf};
use std::sync::Arc;

use jolyne::valentine::actor_flags;

use bevy_ecs::prelude::*;
use glam::DVec3;
use zuri_nbt::{NBTTag, encoding::LittleEndian, tag};

use super::save::list;
use super::*;
use crate::entity::EntityRandom;
use crate::entity::components::{Health, Position, Rotation};
use crate::entity::definition::DefinitionState;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue};
use crate::entity::mob::life::ageing::BabyAge;
use crate::entity::mob::state::{BreedCooldown, InLove, Persistent};
use crate::entity::mob::{SpawnCause, spawn_mob};
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
        Self(std::env::temp_dir().join(format!("unastar-mob-persistence-{}", uuid::Uuid::new_v4())))
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
        let cow = spawn_mob(world, "minecraft:cow", COW_FEET, 0.0, SpawnCause::Spawned).unwrap();
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
