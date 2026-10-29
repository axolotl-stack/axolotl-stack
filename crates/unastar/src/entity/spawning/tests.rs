//! Natural spawning on hand-built and generated terrain, driven by
//! `server.tick()`.

use std::collections::{BTreeMap, HashMap};

use bevy_ecs::prelude::*;
use glam::DVec3;
use tokio::sync::mpsc;

use super::{NaturalSpawnRules, SurfaceMob};
use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, HeldSlot, MainInventory, Player, PlayerSession, Position, Rotation, RuntimeEntityId,
};
use crate::entity::mob::{MobKind, SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::storage::ChunkColumn;
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::manager::chunk_entity_components;
use crate::world::ecs::{ChunkManager, ChunkPosition, ChunkStateFlags};
use crate::world::rules::{Difficulty, WorldTime};

/// Noon and midnight on the world clock.
const NOON: u64 = 6_000;
const MIDNIGHT: u64 = 18_000;

/// The ground layer of the test terrain.
const GROUND_Y: i16 = 63;

fn biome_id(identifier: &str) -> u32 {
    unastar_data::biomes::get(identifier)
        .unwrap_or_else(|| panic!("{identifier} is a vanilla biome"))
        .id
}

struct Terrain {
    radius: i32,
    ground: u32,
    biome: u32,
    /// When set, chunk column `x` takes `biome_stripes[x mod len]`.
    biome_stripes: Vec<u32>,
    /// A stone roof 4 blocks above the ground (darkness).
    roof: bool,
    /// Chunks arrive fresh from the generator (world-generation pass).
    generated: bool,
}

impl Terrain {
    fn plains(radius: i32) -> Self {
        Self {
            radius,
            ground: *blocks::GRASS_BLOCK,
            biome: biome_id("minecraft:plains"),
            biome_stripes: Vec::new(),
            roof: false,
            generated: false,
        }
    }
}

/// A server with `terrain` around the origin and one survival player
/// standing at the centre of chunk (0, 0).
fn build_server(
    terrain: &Terrain,
    seed: u64,
) -> (GameServer, mpsc::Receiver<jolyne::valentine::McpePacket>) {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(0),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    // Natural spawns need a world age above 0.
    world.resource_mut::<TickCounter>().current = 200;
    // Daylight unless a test says otherwise.
    world.resource_mut::<WorldTime>().ticks = NOON;
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                8.5,
                f64::from(GROUND_Y + 1) + crate::entity::PLAYER_EYE_HEIGHT,
                8.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            RuntimeEntityId(1),
            PlayerSession::new(1, "watcher".into(), None, None, tx),
        ))
        .id();
    for x in -terrain.radius..=terrain.radius {
        for z in -terrain.radius..=terrain.radius {
            let mut chunk = Chunk::new(x, z);
            let biome = if terrain.biome_stripes.is_empty() {
                terrain.biome
            } else {
                let len = terrain.biome_stripes.len() as i32;
                terrain.biome_stripes[x.rem_euclid(len) as usize]
            };
            chunk.set_biome(biome);
            for lx in 0..16 {
                for lz in 0..16 {
                    chunk.set_block(lx, GROUND_Y - 1, lz, *blocks::STONE);
                    chunk.set_block(lx, GROUND_Y, lz, terrain.ground);
                    if terrain.roof {
                        chunk.set_block(lx, GROUND_Y + 4, lz, *blocks::STONE);
                    }
                }
            }
            let column = ChunkColumn {
                chunk,
                actors: Vec::new(),
                removed_actors: Default::default(),
            };
            let pos = ChunkPosition::new(x, z);
            let (mut components, _) = chunk_entity_components(pos, column, !terrain.generated);
            // Viewed by the player, so the chunks stay loaded.
            components.3.insert(player);
            let entity = world.spawn(components).id();
            world.resource_mut::<ChunkManager>().insert(pos, entity);
        }
    }
    (server, rx)
}

fn mobs(server: &mut GameServer) -> Vec<(Entity, String, DVec3, bool)> {
    let world = server.ecs.world_mut();
    let mut query = world.query::<(Entity, &MobKind, &Position, Has<SurfaceMob>)>();
    query
        .iter(world)
        .map(|(e, kind, pos, surface)| (e, kind.0.identifier.to_string(), pos.0, surface))
        .collect()
}

fn tick(
    server: &mut GameServer,
    rx: &mut mpsc::Receiver<jolyne::valentine::McpePacket>,
    ticks: usize,
) {
    for _ in 0..ticks {
        server.tick();
        while rx.try_recv().is_ok() {}
    }
}

/// Authored herd sizes per spawnable surface identifier.
fn herd_sizes(server: &GameServer) -> HashMap<String, (i32, i32)> {
    let rules = server.ecs.world().resource::<NaturalSpawnRules>();
    let mut sizes: HashMap<String, (i32, i32)> = HashMap::new();
    for entry in rules.spawnable() {
        for herd in &entry.herds {
            let size = sizes
                .entry(entry.identifier.to_string())
                .or_insert((herd.min_size, herd.max_size));
            size.0 = size.0.min(herd.min_size);
            size.1 = size.1.max(herd.max_size);
        }
    }
    sizes
}

#[test]
fn grass_plains_spawn_animal_herds_in_daylight() {
    let terrain = Terrain::plains(6);
    let (mut server, mut rx) = build_server(&terrain, 41);
    let sizes = herd_sizes(&server);
    for animal in [
        "minecraft:cow",
        "minecraft:sheep",
        "minecraft:pig",
        "minecraft:chicken",
    ] {
        assert!(
            sizes.contains_key(animal)
                || server
                    .ecs
                    .world()
                    .resource::<crate::entity::mob::MobRegistry>()
                    .get(animal)
                    .is_none(),
            "{animal} spawns when the mob is supported"
        );
    }
    // Group herds by their spawn tick: a herd spawns together at one spot.
    let mut herds: Vec<(String, DVec3, usize)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..1500 {
        tick(&mut server, &mut rx, 1);
        let mut fresh: BTreeMap<(String, [i64; 3]), usize> = BTreeMap::new();
        for (entity, id, pos, surface) in mobs(&mut server) {
            if seen.insert(entity) {
                assert!(surface, "a surface spawn is a surface mob");
                let key = [pos.x, pos.y, pos.z].map(|v| (v * 100.0).round() as i64);
                *fresh.entry((id, key)).or_default() += 1;
            }
        }
        for ((id, key), count) in fresh {
            herds.push((
                id,
                DVec3::new(key[0] as f64, key[1] as f64, key[2] as f64) / 100.0,
                count,
            ));
        }
    }
    assert!(!herds.is_empty(), "animals spawned on the plains");
    for (id, pos, count) in &herds {
        let (min, max) = sizes[id];
        assert!(
            (1..=max as usize).contains(count),
            "{id} herd of {count} (authored {min}..={max})"
        );
        // Standing on the grass layer, at the spawn block corner + 0.49.
        assert_eq!(pos.y, f64::from(GROUND_Y + 1), "{id} at {pos}");
        assert!((pos.x.rem_euclid(1.0) - 0.49).abs() < 1e-6, "{id} at {pos}");
        assert!((pos.z.rem_euclid(1.0) - 0.49).abs() < 1e-6, "{id} at {pos}");
        // At least the minimum distance from the player's eyes.
        let eye = DVec3::new(
            8.5,
            f64::from(GROUND_Y + 1) + crate::entity::PLAYER_EYE_HEIGHT,
            8.5,
        );
        assert!(pos.distance(eye) >= 24.0 - 1.0, "{id} at {pos}");
    }
    assert!(
        herds
            .iter()
            .any(|(id, _, count)| *count >= sizes[id].0 as usize && sizes[id].0 > 1),
        "a complete multi-mob herd spawned: {herds:?}"
    );
}

#[test]
fn animals_never_spawn_on_stone_in_darkness_or_outside_animal_biomes() {
    let desert = unastar_data::biomes::get("minecraft:desert").expect("desert");
    assert!(!desert.tags.iter().any(|tag| tag == "animal"));
    let cases = [
        Terrain {
            ground: *blocks::STONE,
            ..Terrain::plains(6)
        },
        Terrain {
            roof: true,
            ..Terrain::plains(6)
        },
        Terrain {
            biome: desert.id,
            ..Terrain::plains(6)
        },
    ];
    for (index, terrain) in cases.iter().enumerate() {
        let (mut server, mut rx) = build_server(terrain, 41);
        // Monsters (which do spawn in the dark) are off on Peaceful; animals
        // are not affected by the difficulty.
        *server.ecs.world_mut().resource_mut::<Difficulty>() = Difficulty::Peaceful;
        tick(&mut server, &mut rx, 1500);
        let spawned = mobs(&mut server);
        assert!(spawned.is_empty(), "case {index}: {spawned:?}");
    }
}

#[test]
fn the_animal_cap_stops_spawning() {
    // Radius 2: every census area covers the whole world.
    let terrain = Terrain::plains(2);
    let (mut server, mut rx) = build_server(&terrain, 5);
    let world = server.ecs.world_mut();
    for i in 0..4 {
        let cow = spawn_mob(
            world,
            "minecraft:cow",
            DVec3::new(-20.5 + f64::from(i), f64::from(GROUND_Y + 1), -20.5),
            0.0,
            SpawnCause::Spawned,
        )
        .expect("cow");
        // Persistent so they cannot despawn during the run.
        world
            .entity_mut(cow)
            .insert((SurfaceMob, crate::entity::mob::state::Persistent));
    }
    tick(&mut server, &mut rx, 3000);
    assert_eq!(
        mobs(&mut server).len(),
        4,
        "a full animal cap spawns nothing"
    );

    // The same world without the four cows fills the cap and stops there.
    let (mut server, mut rx) = build_server(&terrain, 5);
    let mut peak = 0;
    for _ in 0..3000 {
        tick(&mut server, &mut rx, 1);
        let surface = mobs(&mut server).iter().filter(|m| m.3).count();
        assert!(surface <= 4, "{surface} surface animals exceed the cap");
        peak = peak.max(surface);
    }
    assert!(peak > 0, "animals spawned up to the cap");
}

#[test]
fn world_generation_places_herds_in_fresh_animal_chunks() {
    let terrain = Terrain {
        generated: true,
        ..Terrain::plains(4)
    };
    let (mut server, mut rx) = build_server(&terrain, 3);
    // Chunks start ticking at the end of the first tick; the second tick is
    // their first ticking tick.
    tick(&mut server, &mut rx, 2);
    let world = server.ecs.world_mut();
    let mut flags = world.query::<&ChunkStateFlags>();
    assert!(
        flags
            .iter(world)
            .all(|f| !f.needs_population() || !f.is_ticking()),
        "ticking chunks were populated"
    );
    let placed = mobs(&mut server);
    assert!(!placed.is_empty(), "the world-generation pass placed herds");
    // Loaded chunks are not populated: the same tick on stored chunks places
    // nothing beyond the regular cycle, which needs many ticks.
    let (mut loaded, mut rx) = build_server(&Terrain::plains(4), 3);
    tick(&mut loaded, &mut rx, 2);
    assert!(mobs(&mut loaded).len() < placed.len());
}

/// The zombie spawn rule's targets.
const ZOMBIES: [&str; 2] = ["minecraft:zombie", "minecraft:zombie_villager_v2"];

/// A zombie herd: where it spawned, how many, and whether on the surface.
#[derive(Debug)]
struct ZombieHerd {
    pos: DVec3,
    count: usize,
    surface: bool,
}

/// Tick `ticks` times and collect the zombie herds that spawned (grouped by
/// spawn tick and spot), checking every tick that the zombies alive stay
/// within the monster caps (the whole radius-2 world is one census area).
fn watch_zombies(
    server: &mut GameServer,
    rx: &mut mpsc::Receiver<jolyne::valentine::McpePacket>,
    ticks: usize,
) -> Vec<ZombieHerd> {
    use crate::entity::mob::constants::{MOBS_PER_CHUNK_SURFACE, MOBS_PER_CHUNK_UNDERGROUND};
    use crate::entity::spawning::Category;
    let surface_cap = MOBS_PER_CHUNK_SURFACE[Category::Monster.index()] as usize;
    let underground_cap = MOBS_PER_CHUNK_UNDERGROUND[Category::Monster.index()] as usize;
    let mut herds = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for _ in 0..ticks {
        tick(server, rx, 1);
        let zombies: Vec<_> = mobs(server)
            .into_iter()
            .filter(|(_, id, _, _)| ZOMBIES.contains(&id.as_str()))
            .collect();
        let on_surface = zombies.iter().filter(|z| z.3).count();
        assert!(on_surface <= surface_cap, "{on_surface} surface zombies");
        let below = zombies.len() - on_surface;
        assert!(below <= underground_cap, "{below} underground zombies");
        let mut fresh: BTreeMap<[i64; 3], (usize, bool)> = BTreeMap::new();
        for (entity, _, pos, surface) in zombies {
            if seen.insert(entity) {
                let key = [pos.x, pos.y, pos.z].map(|v| (v * 100.0).round() as i64);
                let herd = fresh.entry(key).or_insert((0, surface));
                herd.0 += 1;
            }
        }
        herds.extend(fresh.into_iter().map(|(key, (count, surface))| ZombieHerd {
            pos: DVec3::new(key[0] as f64, key[1] as f64, key[2] as f64) / 100.0,
            count,
            surface,
        }));
    }
    herds
}

fn monster_server(terrain: &Terrain, time: u64, difficulty: Difficulty) -> TestServer {
    let (mut server, rx) = build_server(terrain, 7);
    let registry = server
        .ecs
        .world()
        .resource::<crate::entity::mob::MobRegistry>();
    assert!(
        registry.get("minecraft:zombie").is_some(),
        "the zombie archetype compiles: {:?}",
        registry.unsupported_reason("minecraft:zombie")
    );
    let world = server.ecs.world_mut();
    world.resource_mut::<WorldTime>().ticks = time;
    *world.resource_mut::<Difficulty>() = difficulty;
    (server, rx)
}

type TestServer = (GameServer, mpsc::Receiver<jolyne::valentine::McpePacket>);

#[test]
fn zombies_spawn_in_herds_of_two_to_four_at_night_within_the_caps() {
    let (mut server, mut rx) = monster_server(&Terrain::plains(2), MIDNIGHT, Difficulty::Easy);
    let herds = watch_zombies(&mut server, &mut rx, 4000);
    assert!(!herds.is_empty(), "zombies spawned at night");
    for herd in &herds {
        // The cap or a refused member can cut a herd short.
        assert!((1..=4).contains(&herd.count), "{herd:?}");
        assert!(herd.surface, "an open sky column spawns on the surface");
        assert_eq!(herd.pos.y, f64::from(GROUND_Y + 1), "{herd:?}");
    }
    assert!(
        herds.iter().any(|herd| herd.count >= 2),
        "a herd of at least two spawned: {herds:?}"
    );
}

#[test]
fn no_monster_spawns_on_peaceful() {
    let (mut server, mut rx) = monster_server(&Terrain::plains(2), MIDNIGHT, Difficulty::Peaceful);
    let herds = watch_zombies(&mut server, &mut rx, 4000);
    assert!(herds.is_empty(), "{herds:?}");
}

#[test]
fn no_zombie_spawns_on_a_sunlit_surface_at_noon() {
    let (mut server, mut rx) = monster_server(&Terrain::plains(2), NOON, Difficulty::Hard);
    let herds = watch_zombies(&mut server, &mut rx, 4000);
    assert!(herds.is_empty(), "{herds:?}");
}

#[test]
fn zombies_spawn_under_cover_in_daylight() {
    let terrain = Terrain {
        roof: true,
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = monster_server(&terrain, NOON, Difficulty::Easy);
    let herds = watch_zombies(&mut server, &mut rx, 4000);
    assert!(
        !herds.is_empty(),
        "zombies spawned in the dark under the roof"
    );
    for herd in &herds {
        assert!((1..=4).contains(&herd.count), "{herd:?}");
        // On the ground under the roof (raw brightness 0), never on the
        // sunlit roof; a floor below the surface is underground.
        assert_eq!(herd.pos.y, f64::from(GROUND_Y + 1), "{herd:?}");
        assert!(!herd.surface, "{herd:?}");
    }
}

#[test]
fn a_full_monster_cap_spawns_no_zombie() {
    let (mut server, mut rx) = monster_server(&Terrain::plains(2), MIDNIGHT, Difficulty::Easy);
    let world = server.ecs.world_mut();
    for i in 0..8 {
        let zombie = spawn_mob(
            world,
            "minecraft:zombie",
            DVec3::new(-20.5 + f64::from(i), f64::from(GROUND_Y + 1), -20.5),
            0.0,
            SpawnCause::Spawned,
        )
        .expect("zombie");
        world
            .entity_mut(zombie)
            .insert((SurfaceMob, crate::entity::mob::state::Persistent));
    }
    tick(&mut server, &mut rx, 4000);
    let zombies = mobs(&mut server)
        .into_iter()
        .filter(|(_, id, _, _)| ZOMBIES.contains(&id.as_str()))
        .count();
    assert_eq!(zombies, 8, "a full monster cap spawns nothing");
}

/// The mobs among `identifiers` that spawned over `ticks`: identifier, feet
/// and whether they spawned on the surface.
fn watch_spawns(
    server: &mut GameServer,
    rx: &mut mpsc::Receiver<jolyne::valentine::McpePacket>,
    identifiers: &[&str],
    ticks: usize,
) -> Vec<(String, DVec3, bool)> {
    let mut seen = std::collections::HashSet::new();
    let mut spawned = Vec::new();
    for _ in 0..ticks {
        tick(server, rx, 1);
        for (entity, id, pos, surface) in mobs(server) {
            if identifiers.contains(&id.as_str()) && seen.insert(entity) {
                spawned.push((id, pos, surface));
            }
        }
    }
    spawned
}

const SPIDERS: [&str; 2] = ["minecraft:spider", "minecraft:cave_spider"];

#[test]
fn spiders_spawn_at_night_in_the_dark() {
    let (mut server, mut rx) = monster_server(&Terrain::plains(2), MIDNIGHT, Difficulty::Easy);
    let spawned = watch_spawns(&mut server, &mut rx, &SPIDERS, 4000);
    assert!(
        spawned.iter().any(|(id, _, _)| id == "minecraft:spider"),
        "spiders spawned at night: {spawned:?}"
    );
    for (id, pos, surface) in &spawned {
        assert_eq!(id, "minecraft:spider", "cave spiders need sulfur caves");
        assert!(*surface, "{pos:?}");
        assert_eq!(pos.y, f64::from(GROUND_Y + 1), "{pos:?}");
    }

    let (mut server, mut rx) = monster_server(&Terrain::plains(2), NOON, Difficulty::Easy);
    let spawned = watch_spawns(&mut server, &mut rx, &SPIDERS, 4000);
    assert!(
        spawned.is_empty(),
        "no spider on a sunlit surface: {spawned:?}"
    );
}

/// [`monster_server`] on Easy where only the spawn entries of `keep`
/// carry weight, so other monsters do not fill the cap first.
fn weighted_server(terrain: &Terrain, time: u64, keep: &[&str]) -> TestServer {
    let (mut server, rx) = monster_server(terrain, time, Difficulty::Easy);
    let world = server.ecs.world_mut();
    for entry in &mut world.resource_mut::<NaturalSpawnRules>().entries {
        if !keep.contains(&&*entry.identifier) {
            entry.weight = 0;
        }
    }
    (server, rx)
}

/// [`weighted_server`] for the spiders.
fn spider_server(terrain: &Terrain, time: u64) -> TestServer {
    let (server, rx) = weighted_server(terrain, time, &SPIDERS);
    let rules = server.ecs.world().resource::<NaturalSpawnRules>();
    assert!(
        rules
            .spawnable()
            .any(|entry| &*entry.identifier == "minecraft:cave_spider"),
        "the cave spider spawn rule compiles"
    );
    (server, rx)
}

/// Cave spiders spawn by their rule: underground, in the dark, in a biome
/// tagged both `monster` and `sulfur_caves` (where plain spiders do not).
#[test]
fn cave_spiders_spawn_only_underground_in_sulfur_caves() {
    let sulfur = biome_id("minecraft:sulfur_caves");
    let covered = Terrain {
        roof: true,
        biome: sulfur,
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = spider_server(&covered, NOON);
    let spawned = watch_spawns(&mut server, &mut rx, &SPIDERS, 4000);
    assert!(
        spawned
            .iter()
            .any(|(id, _, _)| id == "minecraft:cave_spider"),
        "cave spiders spawned under the roof: {spawned:?}"
    );
    for (id, pos, surface) in &spawned {
        assert_eq!(id, "minecraft:cave_spider", "spiders avoid sulfur caves");
        assert!(!*surface, "{pos:?}");
        assert_eq!(pos.y, f64::from(GROUND_Y + 1), "{pos:?}");
    }

    // The open surface of the same biome at night is not underground.
    let open = Terrain {
        biome: sulfur,
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = spider_server(&open, MIDNIGHT);
    let spawned = watch_spawns(&mut server, &mut rx, &SPIDERS, 4000);
    assert!(spawned.is_empty(), "{spawned:?}");

    // Underground outside sulfur caves: spiders, no cave spider.
    let plains = Terrain {
        roof: true,
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = spider_server(&plains, NOON);
    let spawned = watch_spawns(&mut server, &mut rx, &SPIDERS, 4000);
    assert!(!spawned.is_empty(), "spiders spawned under the roof");
    assert!(
        spawned.iter().all(|(id, _, _)| id == "minecraft:spider"),
        "{spawned:?}"
    );
}

// Creeper lane.
#[test]
fn creepers_spawn_at_night_in_the_dark() {
    // Skeletons and spiders share the monster cap: only creepers draw.
    let creeper = ["minecraft:creeper"];
    let (mut server, mut rx) = weighted_server(&Terrain::plains(2), MIDNIGHT, &creeper);
    let spawned = watch_spawns(&mut server, &mut rx, &["minecraft:creeper"], 4000);
    assert!(!spawned.is_empty(), "creepers spawned at night");
    for (_, pos, surface) in &spawned {
        assert!(*surface, "{pos:?}");
        assert_eq!(pos.y, f64::from(GROUND_Y + 1), "{pos:?}");
    }

    let (mut server, mut rx) = weighted_server(&Terrain::plains(2), NOON, &creeper);
    let spawned = watch_spawns(&mut server, &mut rx, &["minecraft:creeper"], 4000);
    assert!(
        spawned.is_empty(),
        "no creeper on a sunlit surface: {spawned:?}"
    );

    let (mut server, mut rx) = monster_server(&Terrain::plains(2), MIDNIGHT, Difficulty::Peaceful);
    let spawned = watch_spawns(&mut server, &mut rx, &["minecraft:creeper"], 4000);
    assert!(spawned.is_empty(), "no creeper on Peaceful: {spawned:?}");
}

/// Seeded headless run over a flat grass region whose chunk columns cycle
/// through several biomes: spawned counts per biome per identifier.
/// `cargo test -p unastar seeded_spawn_report -- --ignored --nocapture`
#[test]
#[ignore = "long run; prints a report"]
fn seeded_spawn_report() {
    let biomes = [
        "minecraft:plains",
        "minecraft:forest",
        "minecraft:meadow",
        "minecraft:cherry_grove",
        "minecraft:savanna",
        "minecraft:desert",
    ];
    let terrain = Terrain {
        generated: true,
        biome_stripes: biomes.iter().map(|b| biome_id(b)).collect(),
        ..Terrain::plains(8)
    };
    let seed = 2026;
    let ticks = 6000;
    let (mut server, mut rx) = build_server(&terrain, seed);
    let mut seen = std::collections::HashSet::new();
    let mut counts: BTreeMap<(String, String), usize> = BTreeMap::new();
    for _ in 0..ticks {
        tick(&mut server, &mut rx, 1);
        for (entity, id, pos, _) in mobs(&mut server) {
            if seen.insert(entity) {
                let cx = (pos.x.floor() as i32) >> 4;
                let len = biomes.len() as i32;
                let biome = biomes[cx.rem_euclid(len) as usize];
                *counts.entry((biome.to_owned(), id)).or_default() += 1;
            }
        }
    }
    let mut report = format!(
        "seed {seed}, flat grass, {} chunks, {ticks} ticks (world-generation pass + cycle)\n",
        (2 * terrain.radius + 1).pow(2)
    );
    for ((biome, id), count) in &counts {
        report.push_str(&format!("{biome:24} {id:20} {count}\n"));
    }
    report.push_str(&format!(
        "spawned {}, alive at end {}\n",
        seen.len(),
        mobs(&mut server).len()
    ));
    println!("{report}");
}

// Enderman lane.

/// The enderman's end-biome rule: herds of exactly four on the surface, no
/// brightness or difficulty filter of its own (the native monster check
/// still needs darkness).
#[test]
fn endermen_spawn_in_herds_of_up_to_four_on_end_biome_ground_at_night() {
    let terrain = Terrain {
        biome: biome_id("minecraft:the_end"),
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = build_server(&terrain, 11);
    let world = server.ecs.world_mut();
    world.resource_mut::<WorldTime>().ticks = MIDNIGHT;
    *world.resource_mut::<Difficulty>() = Difficulty::Easy;
    let mut seen = std::collections::HashSet::new();
    let mut herds: Vec<usize> = Vec::new();
    for _ in 0..4000 {
        tick(&mut server, &mut rx, 1);
        let fresh = mobs(&mut server)
            .into_iter()
            .filter(|(entity, _, _, _)| seen.insert(*entity))
            .map(|(_, id, _, surface)| {
                assert_eq!(id, "minecraft:enderman", "only endermen spawn here");
                assert!(surface, "an open sky column spawns on the surface");
            })
            .count();
        if fresh > 0 {
            herds.push(fresh);
        }
    }
    assert!(!herds.is_empty(), "endermen spawned at night");
    // The cap or a refused member can cut a herd short.
    assert!(herds.iter().all(|&n| (1..=4).contains(&n)), "{herds:?}");
    assert!(herds.iter().any(|&n| n >= 2), "{herds:?}");
}

// Skeleton lane.

const SKELETONS: [&str; 2] = ["minecraft:skeleton", "minecraft:stray"];

/// The component groups `mob` has added since it spawned.
fn added_groups(server: &mut GameServer, mob: Entity) -> Vec<String> {
    server
        .ecs
        .world()
        .get::<crate::entity::definition::compiled::DefinitionState>(mob)
        .unwrap()
        .group_history()
        .filter(|(_, added)| *added)
        .map(|(group, _)| group.to_owned())
        .collect()
}

/// The spawned mobs among `identifiers` with the groups they added.
fn watch_spawn_groups(
    server: &mut GameServer,
    rx: &mut mpsc::Receiver<jolyne::valentine::McpePacket>,
    identifiers: &[&str],
    ticks: usize,
) -> Vec<(String, bool, Vec<String>)> {
    let mut seen = std::collections::HashSet::new();
    let mut spawned = Vec::new();
    for _ in 0..ticks {
        tick(server, rx, 1);
        for (entity, id, _, surface) in mobs(server) {
            if identifiers.contains(&id.as_str()) && seen.insert(entity) {
                let groups = added_groups(server, entity);
                spawned.push((id, surface, groups));
            }
        }
    }
    spawned
}

#[test]
fn skeletons_spawn_at_night_in_the_dark_outside_frozen_biomes() {
    let (mut server, mut rx) = weighted_server(&Terrain::plains(2), MIDNIGHT, &SKELETONS);
    let spawned = watch_spawn_groups(&mut server, &mut rx, &SKELETONS, 4000);
    assert!(!spawned.is_empty(), "skeletons spawned at night");
    for (id, surface, groups) in &spawned {
        assert_eq!(id, "minecraft:skeleton", "strays need a frozen biome");
        assert!(*surface);
        assert!(
            groups.iter().any(|g| g == "minecraft:ranged_attack"),
            "entity_spawned gave the bow attack: {groups:?}"
        );
    }

    let (mut server, mut rx) = weighted_server(&Terrain::plains(2), NOON, &SKELETONS);
    let spawned = watch_spawns(&mut server, &mut rx, &SKELETONS, 4000);
    assert!(spawned.is_empty(), "no skeleton in sunlight: {spawned:?}");

    // Under a roof the dark ground is underground, where the rule spawns
    // skeletons too.
    let covered = Terrain {
        roof: true,
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = weighted_server(&covered, NOON, &SKELETONS);
    let spawned = watch_spawns(&mut server, &mut rx, &SKELETONS, 4000);
    assert!(!spawned.is_empty(), "skeletons spawned under the roof");
    assert!(
        spawned
            .iter()
            .all(|(id, _, surface)| id == "minecraft:skeleton" && !surface)
    );
}

/// The stray's rules fire `change_to_skeleton` instead of
/// `minecraft:entity_spawned`: the strays never gain the bow groups that
/// event adds, and underground every stray gains `revert_to_skeleton`.
#[test]
fn strays_spawn_in_frozen_biomes_through_their_spawn_event() {
    let entries: Vec<_> = {
        let (server, _) = weighted_server(&Terrain::plains(0), MIDNIGHT, &SKELETONS);
        let rules = server.ecs.world().resource::<NaturalSpawnRules>();
        rules
            .spawnable()
            .filter(|entry| &*entry.identifier == "minecraft:stray")
            .map(|entry| entry.spawn_event.as_deref().map(str::to_owned))
            .collect()
    };
    assert_eq!(
        entries,
        vec![Some("change_to_skeleton".to_owned()); 2],
        "both stray rules compile with their spawn event"
    );

    let snowy = Terrain {
        biome: biome_id("minecraft:ice_plains"),
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = weighted_server(&snowy, MIDNIGHT, &SKELETONS);
    let spawned = watch_spawn_groups(&mut server, &mut rx, &SKELETONS, 4000);
    assert!(!spawned.is_empty(), "strays spawned on frozen ground");
    for (id, surface, groups) in &spawned {
        assert_eq!(id, "minecraft:stray", "skeletons avoid frozen biomes");
        assert!(*surface);
        assert!(
            !groups
                .iter()
                .any(|g| g.starts_with("minecraft:ranged_attack")),
            "entity_spawned did not run: {groups:?}"
        );
    }

    // The frozen ocean rule spawns underground between y 60 and 66, where
    // the event always adds the transformation back to a skeleton.
    let frozen_ocean = Terrain {
        roof: true,
        biome: biome_id("minecraft:frozen_ocean"),
        ..Terrain::plains(2)
    };
    let (mut server, mut rx) = weighted_server(&frozen_ocean, NOON, &SKELETONS);
    let spawned = watch_spawn_groups(&mut server, &mut rx, &SKELETONS, 4000);
    assert!(
        !spawned.is_empty(),
        "strays spawned under the frozen ocean roof"
    );
    for (id, surface, groups) in &spawned {
        assert_eq!(id, "minecraft:stray");
        assert!(!surface);
        assert!(
            groups.iter().any(|g| g == "minecraft:revert_to_skeleton"),
            "{groups:?}"
        );
    }
}
