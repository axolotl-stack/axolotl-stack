//! Headless behaviour tests: a real `GameServer` ticking cows over terrain
//! built in loaded chunks.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::{McpePacket, McpePacketData, actor_flags};
use tokio::sync::mpsc::Receiver;
use unastar_data::actor_components::body::ActorDamageCause;

use super::MobBrain;
use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, HeldSlot, MainInventory, OnGround, PhysicsBody, Player, PlayerSession, Position,
    Rotation, RuntimeEntityId, RuntimeId, Velocity,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::components::{RandomLookAroundGoal, RandomStrollGoal};
use crate::entity::mob::state::{InLove, LastHurt, Persistent};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::entity::systems::physics::collides_with_blocks;
use crate::registry::BlockRegistry;
use crate::registry::collision::Aabb;
use crate::server::game::{GameServer, ServerConfig};
use crate::world::ChunkManager;
use crate::world::chunk::Chunk;
use crate::world::ecs::{ChunkData, ChunkPosition, ChunkViewers};

const SURFACE: f64 = 64.0;

fn block(name: &str) -> u32 {
    BlockRegistry::vanilla()
        .default_state_id_by_name(name)
        .expect(name)
}

/// A server whose chunks -1..=1 × -1..=1 are loaded; `build` sets blocks by
/// world position. The default terrain is a grass floor whose top is y = 64.
struct Terrain {
    server: GameServer,
}

impl Terrain {
    fn new(seed: u64, build: impl Fn(IVec3) -> Option<u32>) -> Self {
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            ..ServerConfig::default()
        });
        let world = server.ecs.world_mut();
        world.insert_resource(EntityRandom::seeded(seed));
        for cx in -1..=1 {
            for cz in -1..=1 {
                let mut chunk = Chunk::new(cx, cz);
                for lx in 0u8..16 {
                    for lz in 0u8..16 {
                        for y in 50i16..80 {
                            let pos = IVec3::new(
                                cx * 16 + i32::from(lx),
                                i32::from(y),
                                cz * 16 + i32::from(lz),
                            );
                            if let Some(state) = build(pos) {
                                chunk.set_block(lx, y, lz, state);
                            }
                        }
                    }
                }
                let entity = world.spawn(ChunkData::new(chunk)).id();
                world
                    .resource_mut::<ChunkManager>()
                    .insert(ChunkPosition::new(cx, cz), entity);
            }
        }
        Self { server }
    }

    fn grass() -> impl Fn(IVec3) -> Option<u32> {
        let grass = block("minecraft:grass_block");
        move |pos| (pos.y == 63).then_some(grass)
    }

    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// An adult cow with its feet at `feet`.
    fn cow(&mut self, feet: DVec3) -> Entity {
        let world = self.world();
        for _ in 0..200 {
            let cow = spawn_mob(world, "minecraft:cow", feet, 0.0, SpawnCause::Spawned).unwrap();
            if !baby(world, cow) {
                return cow;
            }
            world.despawn(cow);
        }
        panic!("no adult cow in 200 spawns");
    }

    fn baby_cow(&mut self, feet: DVec3) -> Entity {
        let world = self.world();
        let cow = spawn_mob(world, "minecraft:cow", feet, 0.0, SpawnCause::Born).unwrap();
        assert!(baby(world, cow));
        cow
    }

    /// A survival player whose feet are at `feet`, holding `item`.
    fn player(&mut self, feet: DVec3, item: &str) -> Entity {
        let mut inventory = MainInventory::default();
        inventory
            .0
            .set_item(0, crate::item::test_support::stack(item, 1))
            .unwrap();
        self.world()
            .spawn((
                Player,
                Position(feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
                Rotation::default(),
                GameMode::Survival,
                inventory,
                HeldSlot(0),
                RuntimeEntityId(1),
            ))
            .id()
    }

    fn feet(&mut self, mob: Entity) -> DVec3 {
        self.world().get::<Position>(mob).unwrap().0
    }

    fn tick(&mut self) {
        self.server.tick();
    }

    /// Drop the goals that would make an isolated behaviour random.
    fn without<T: Component>(&mut self, mob: Entity) {
        self.world().entity_mut(mob).remove::<T>();
    }
}

fn baby(world: &World, mob: Entity) -> bool {
    world
        .get::<ActorFlags>(mob)
        .unwrap()
        .get(actor_flags::BABY)
        .unwrap()
}

fn horizontal(a: DVec3, b: DVec3) -> f64 {
    DVec3::new(a.x - b.x, 0.0, a.z - b.z).length()
}

fn inside_blocks(world: &World, mob: Entity) -> bool {
    let feet = world.get::<Position>(mob).unwrap().0;
    let body = world.get::<PhysicsBody>(mob).unwrap();
    collides_with_blocks(world, Aabb::from_feet(feet, body.width, body.height))
}

#[test]
fn cow_dropped_above_grass_lands_and_stays_on_the_surface() {
    let mut terrain = Terrain::new(1, Terrain::grass());
    let cow = terrain.cow(DVec3::new(8.5, 70.0, 8.5));
    terrain.without::<RandomStrollGoal>(cow);
    for _ in 0..60 {
        terrain.tick();
    }
    let feet = terrain.feet(cow);
    assert!((feet.y - SURFACE).abs() < 1e-9, "landed at {feet}");
    assert!(terrain.world().get::<OnGround>(cow).unwrap().0);
    for _ in 0..100 {
        terrain.tick();
        let feet = terrain.feet(cow);
        assert!((feet.y - SURFACE).abs() < 1e-9, "left the surface: {feet}");
    }
}

#[test]
fn strolling_cow_walks_within_its_range_and_never_enters_blocks() {
    let stone = block("minecraft:stone");
    let grass = Terrain::grass();
    // A few pillars to walk around.
    let mut terrain = Terrain::new(3, move |pos| {
        if (pos.x, pos.z) == (11, 9) || (pos.x, pos.z) == (6, 12) {
            (63..=66).contains(&pos.y).then_some(stone)
        } else {
            grass(pos)
        }
    });
    let cow = terrain.cow(DVec3::new(8.5, SURFACE, 8.5));
    let start = terrain.feet(cow);
    let range = f64::from(
        terrain
            .world()
            .get::<RandomStrollGoal>(cow)
            .unwrap()
            .xz_dist,
    );
    let mut stroll_start = None;
    let mut strolls = 0;
    for _ in 0..1200 {
        terrain.tick();
        let world = terrain.world();
        assert!(!inside_blocks(world, cow), "inside a block");
        let feet = world.get::<Position>(cow).unwrap().0;
        let navigating = world.get::<MobBrain>(cow).unwrap().is_navigating();
        match (stroll_start, navigating) {
            (None, true) => stroll_start = Some(feet),
            (Some(from), false) => {
                let travelled = (feet - from).abs();
                assert!(
                    travelled.x <= range + 1.0 && travelled.z <= range + 1.0,
                    "stroll from {from} ended at {feet}"
                );
                strolls += 1;
                stroll_start = None;
            }
            _ => {}
        }
    }
    assert!(strolls >= 2, "only {strolls} strolls");
    assert!(horizontal(terrain.feet(cow), start) > 1.0 || strolls > 0);
}

#[test]
fn cow_jumps_a_full_block_to_reach_a_stroll_target() {
    let grass = block("minecraft:grass_block");
    // Raised ground (top y = 65) everywhere except the cow's own column.
    let mut terrain = Terrain::new(5, move |pos| {
        let pit = (pos.x, pos.z) == (8, 8);
        (pos.y == 63 || (pos.y == 64 && !pit)).then_some(grass)
    });
    let cow = terrain.cow(DVec3::new(8.5, SURFACE, 8.5));
    let mut climbed = false;
    for _ in 0..1200 {
        terrain.tick();
        assert!(!inside_blocks(terrain.world(), cow));
        if terrain.feet(cow).y >= SURFACE + 1.0 - 1e-9 {
            climbed = true;
            break;
        }
    }
    assert!(climbed, "still in the pit at {}", terrain.feet(cow));
}

#[test]
fn panicking_cow_runs_away_from_its_attacker_faster_than_strolling() {
    fn peak_speed(terrain: &mut Terrain, cow: Entity, ticks: usize) -> f64 {
        let mut peak: f64 = 0.0;
        for _ in 0..ticks {
            terrain.tick();
            let v = terrain.world().get::<Velocity>(cow).unwrap().0;
            peak = peak.max(DVec3::new(v.x, 0.0, v.z).length());
        }
        peak
    }

    let mut strolling = Terrain::new(7, Terrain::grass());
    let cow = strolling.cow(DVec3::new(8.5, SURFACE, 8.5));
    let stroll_speed = peak_speed(&mut strolling, cow, 1200);
    assert!(stroll_speed > 0.0, "the cow never strolled");

    let mut terrain = Terrain::new(7, Terrain::grass());
    let cow = terrain.cow(DVec3::new(8.5, SURFACE, 8.5));
    terrain.without::<RandomStrollGoal>(cow);
    let attacker = terrain.player(DVec3::new(6.5, SURFACE, 8.5), "minecraft:air");
    let start = terrain.feet(cow);
    let tick = terrain
        .world()
        .resource::<crate::ecs::TickCounter>()
        .current;
    terrain.world().entity_mut(cow).insert(LastHurt {
        tick,
        cause: ActorDamageCause::EntityAttack,
        attacker: Some(attacker),
        by_player: true,
    });
    let panic_speed = peak_speed(&mut terrain, cow, 40);
    let feet = terrain.feet(cow);
    let attacker_feet = DVec3::new(6.5, SURFACE, 8.5);
    assert!(
        horizontal(feet, attacker_feet) > horizontal(start, attacker_feet) + 1.0,
        "did not flee: {start} -> {feet}"
    );
    assert!(feet.x > start.x, "fled toward the attacker: {feet}");
    assert!(
        panic_speed > stroll_speed * 1.5,
        "panic {panic_speed} vs stroll {stroll_speed}"
    );
}

#[test]
fn cow_follows_a_player_holding_wheat_and_stops_short_of_them() {
    let mut terrain = Terrain::new(9, Terrain::grass());
    let cow = terrain.cow(DVec3::new(2.5, SURFACE, 8.5));
    let player_feet = DVec3::new(14.5, SURFACE, 8.5);
    terrain.player(player_feet, "minecraft:wheat");
    for _ in 0..300 {
        terrain.tick();
    }
    let distance = horizontal(terrain.feet(cow), player_feet);
    let width = terrain.world().get::<PhysicsBody>(cow).unwrap().width;
    let stop = (0.6 + width) / 2.0 + 1.5;
    assert!(
        distance >= stop - 0.5 && distance < stop + 1.0,
        "stopped {distance} from the player (stop distance {stop})"
    );
    assert!(terrain.world().get::<Persistent>(cow).is_some());
    let flags = terrain.world().get::<ActorFlags>(cow).unwrap();
    assert!(flags.get(actor_flags::TEMPTED).unwrap());
}

#[test]
fn cow_ignores_a_player_holding_dirt() {
    let mut terrain = Terrain::new(9, Terrain::grass());
    let cow = terrain.cow(DVec3::new(2.5, SURFACE, 8.5));
    terrain.without::<RandomStrollGoal>(cow);
    terrain.without::<RandomLookAroundGoal>(cow);
    let player_feet = DVec3::new(14.5, SURFACE, 8.5);
    terrain.player(player_feet, "minecraft:dirt");
    let start = terrain.feet(cow);
    for _ in 0..300 {
        terrain.tick();
    }
    assert!(horizontal(terrain.feet(cow), start) < 1e-6);
    assert!(terrain.world().get::<Persistent>(cow).is_none());
}

#[test]
fn baby_cow_follows_its_parent() {
    let mut terrain = Terrain::new(11, Terrain::grass());
    let adult = terrain.cow(DVec3::new(4.5, SURFACE, 8.5));
    terrain.without::<RandomStrollGoal>(adult);
    let baby = terrain.baby_cow(DVec3::new(11.5, SURFACE, 8.5));
    terrain.without::<RandomStrollGoal>(baby);
    let start = horizontal(terrain.feet(baby), terrain.feet(adult));
    for _ in 0..200 {
        terrain.tick();
    }
    let end = horizontal(terrain.feet(baby), terrain.feet(adult));
    assert!(start > 6.0 && end < 4.0, "baby went from {start} to {end}");
}

#[test]
fn look_at_player_turns_the_head_toward_a_nearby_player() {
    let mut terrain = Terrain::new(13, Terrain::grass());
    let cow = terrain.cow(DVec3::new(8.5, SURFACE, 8.5));
    terrain.without::<RandomStrollGoal>(cow);
    terrain.without::<RandomLookAroundGoal>(cow);
    // East of the cow: yaw -90.
    terrain.player(DVec3::new(12.5, SURFACE, 8.5), "minecraft:air");
    let mut turned = false;
    for _ in 0..600 {
        terrain.tick();
        let rotation = *terrain.world().get::<Rotation>(cow).unwrap();
        if (rotation.head_yaw + 90.0).abs() < 1.0 {
            assert!(rotation.yaw.abs() < 1e-6, "the body stays: {rotation:?}");
            turned = true;
            break;
        }
    }
    assert!(
        turned,
        "{:?}",
        terrain.world().get::<Rotation>(cow).unwrap()
    );
}

#[test]
fn cow_in_water_floats_up() {
    let stone = block("minecraft:stone");
    let water = block("minecraft:water");
    let mut terrain = Terrain::new(15, move |pos| match pos.y {
        52 => Some(stone),
        53..=63 => Some(water),
        _ => None,
    });
    let cow = terrain.cow(DVec3::new(8.5, 56.0, 8.5));
    for _ in 0..80 {
        terrain.tick();
    }
    let feet = terrain.feet(cow);
    assert!(feet.y > 61.5, "still sunk at {feet}");
}

#[test]
fn overlapping_cows_push_each_other_apart() {
    let mut terrain = Terrain::new(17, Terrain::grass());
    let a = terrain.cow(DVec3::new(8.3, SURFACE, 8.5));
    let b = terrain.cow(DVec3::new(8.7, SURFACE, 8.5));
    for cow in [a, b] {
        terrain.without::<RandomStrollGoal>(cow);
        terrain.without::<RandomLookAroundGoal>(cow);
    }
    for _ in 0..40 {
        terrain.tick();
    }
    let gap = terrain.feet(b).x - terrain.feet(a).x;
    assert!(gap >= 0.85, "still overlapping: {gap}");
}

#[test]
fn cows_in_love_walk_together_and_breed() {
    let mut terrain = Terrain::new(21, Terrain::grass());
    let a = terrain.cow(DVec3::new(4.5, SURFACE, 8.5));
    let b = terrain.cow(DVec3::new(10.5, SURFACE, 8.5));
    let tick = terrain
        .world()
        .resource::<crate::ecs::TickCounter>()
        .current;
    for cow in [a, b] {
        terrain.without::<RandomStrollGoal>(cow);
        terrain.world().entity_mut(cow).insert(InLove {
            until_tick: tick + 600,
            player: None,
        });
    }
    let cows = |world: &mut World| {
        world
            .query_filtered::<Entity, With<crate::entity::mob::MobKind>>()
            .iter(world)
            .count()
    };
    assert_eq!(cows(terrain.world()), 2);
    let mut closest = f64::MAX;
    for _ in 0..300 {
        terrain.tick();
        closest = closest.min(horizontal(terrain.feet(a), terrain.feet(b)));
        if cows(terrain.world()) == 3 {
            break;
        }
    }
    assert!(closest < 1.5, "partners stayed {closest} apart");
    assert_eq!(cows(terrain.world()), 3, "no calf was born");
}

#[test]
fn walking_cow_sends_move_actor_delta_to_viewers() {
    let mut terrain = Terrain::new(19, Terrain::grass());
    let (tx, mut rx): (_, Receiver<McpePacket>) = tokio::sync::mpsc::channel(4096);
    let world = terrain.world();
    let viewer = world
        .spawn(PlayerSession::new(2, "viewer".into(), None, None, tx))
        .id();
    let chunks: Vec<Entity> = world
        .query_filtered::<Entity, With<ChunkData>>()
        .iter(world)
        .collect();
    for chunk in chunks {
        let mut viewers = ChunkViewers::default();
        viewers.insert(viewer);
        world.entity_mut(chunk).insert(viewers);
    }
    let cow = terrain.cow(DVec3::new(2.5, SURFACE, 8.5));
    terrain.player(DVec3::new(14.5, SURFACE, 8.5), "minecraft:wheat");
    let runtime_id = terrain.world().get::<RuntimeId>(cow).unwrap().0 as u64;
    for _ in 0..100 {
        terrain.tick();
    }
    let deltas = std::iter::from_fn(|| rx.try_recv().ok().map(|p| p.data))
        .filter(|packet| {
            matches!(
                packet,
                McpePacketData::MoveActorDeltaPacket(delta)
                    if delta.move_data.actor_runtime_id.actor_runtime_id == runtime_id
            )
        })
        .count();
    assert!(deltas >= 10, "only {deltas} move packets");
}
