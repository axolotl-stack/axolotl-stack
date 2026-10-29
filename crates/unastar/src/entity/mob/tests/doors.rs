//! Zombies and doors through a real `GameServer`: a door-breaking zombie
//! chasing a player behind a closed oak door breaks it after the native time
//! on Hard and never on Normal; an iron door is never broken; a zombie
//! without `minecraft:can_break_doors` does not path through the door.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use jolyne::valentine::{McpePacket, McpePacketData};
use tokio::sync::mpsc;

use crate::entity::EntityRandom;
use crate::entity::components::{
    ArmourInventory, DroppedItem, Experience, GameMode, Health, HeldSlot, ItemEntityStack,
    MainInventory, OffhandSlot, Player, PlayerSession, PlayerUi, Position, Rotation,
    RuntimeEntityId,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::{GroupChange, apply_pending_definitions, queue_group_change};
use crate::entity::mob::components::doors::BreakDoorConfig;
use crate::entity::mob::constants::{
    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND, LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH,
    LEVEL_EVENT_START_BLOCK_CRACKING,
};
use crate::entity::mob::*;
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;
use crate::registry::collision::{Aabb, block_collision_boxes};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

/// The lower door half; the upper one is above it.
const DOOR: IVec3 = IVec3::new(3, 64, 0);
/// Native break time of the vanilla zombie's annotation (12 s), plus the
/// tick on which the crack starts.
const BREAK_TICKS: u64 = 240 + 1;

struct Harness {
    server: GameServer,
    player: Entity,
    rx: mpsc::Receiver<McpePacket>,
}

/// A door half of `name` whose closed panel stands across the x axis on the
/// west side of its cell (the side a zombie coming from the west meets).
fn door_state(name: &str, upper: bool) -> u32 {
    let entry = BlockRegistry::vanilla().get_by_name(name).unwrap();
    let set = |state, property, value| entry.with_state_value(state, property, value).unwrap();
    let mut boxes: Vec<Aabb> = Vec::new();
    let facing = ["north", "south", "east", "west"]
        .into_iter()
        .map(|direction| {
            set(
                entry.default_state_id,
                "minecraft:cardinal_direction",
                StateValue::Str(direction),
            )
        })
        .find(|&state| {
            boxes.clear();
            block_collision_boxes(state, &mut boxes);
            boxes[0].max.x <= 0.5
        })
        .unwrap();
    set(facing, "upper_block_bit", StateValue::Byte(u8::from(upper)))
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1, split at x = 3 by a
/// three-high stone wall whose only gap holds a closed door of `door` at
/// [`DOOR`]. One survival player stands east of the wall at (8.5, 64, 0.5).
/// Night, so the zombie does not burn.
fn harness(seed: u64, door: &str, difficulty: Difficulty) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = difficulty;
    crate::world::clock::set_time(world, 18_000);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(
                8.5,
                64.0 + crate::entity::PLAYER_EYE_HEIGHT,
                0.5,
            )),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "villager".into(), None, None, tx),
            // A zombie through the door may kill the player; respawning
            // resends the whole inventory.
            (
                ArmourInventory::default(),
                OffhandSlot::default(),
                PlayerUi::default(),
            ),
        ))
        .id();
    let registry = BlockRegistry::vanilla();
    let stone = registry
        .default_state_id_by_name("minecraft:stone")
        .unwrap();
    let (lower, upper) = (door_state(door, false), door_state(door, true));
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16u8 {
                for z in 0..16u8 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
                    if cx * 16 + i32::from(x) != DOOR.x {
                        continue;
                    }
                    let wz = cz * 16 + i32::from(z);
                    for y in 64..=66 {
                        let state = match (wz == DOOR.z, y) {
                            (true, 64) => lower,
                            (true, 65) => upper,
                            _ => stone,
                        };
                        column.set_block(x, y as i16, z, state);
                    }
                }
            }
            let mut viewers = ChunkViewers::default();
            viewers.insert(player);
            let chunk = world.spawn((ChunkData::new(column), viewers)).id();
            world
                .resource_mut::<ChunkManager>()
                .insert(ChunkPosition::new(cx, cz), chunk);
        }
    }
    Harness { server, player, rx }
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// An adult zombie west of the wall; with `breaker` it also gets the
    /// `minecraft:can_break_doors` group through its definition stack.
    fn zombie(&mut self, breaker: bool) -> Entity {
        let world = self.world();
        let feet = DVec3::new(0.5, 64.0, 0.5);
        let zombie = (0..400)
            .find_map(|_| {
                let mob =
                    spawn_mob(world, "minecraft:zombie", feet, 0.0, SpawnCause::Spawned).unwrap();
                apply_pending_definitions(world);
                let flags = world.get::<ActorFlags>(mob).unwrap();
                let plain = !flags.get(actor_flags::BABY).unwrap()
                    && world.get::<BreakDoorConfig>(mob).is_none();
                if plain {
                    Some(mob)
                } else {
                    world.despawn(mob);
                    None
                }
            })
            .expect("a plain adult zombie in 400 spawns");
        if breaker {
            let group = world
                .get::<MobKind>(zombie)
                .unwrap()
                .0
                .definition
                .group("minecraft:can_break_doors")
                .unwrap();
            queue_group_change(world, zombie, GroupChange::Add(group)).unwrap();
            apply_pending_definitions(world);
            assert!(world.get::<BreakDoorConfig>(zombie).is_some());
            let flags = world.get::<ActorFlags>(zombie).unwrap();
            assert!(flags.get(actor_flags::DOOR_BREAKER).unwrap());
        }
        zombie
    }

    /// The player punches `zombie` (which turns on it) and steps back
    /// behind the wall.
    fn provoke(&mut self, zombie: Entity) {
        let player = self.player;
        let world = self.world();
        let feet = world.get::<Position>(zombie).unwrap().0;
        world.get_mut::<Position>(player).unwrap().0 = DVec3::new(
            feet.x - 2.0,
            feet.y + crate::entity::PLAYER_EYE_HEIGHT,
            feet.z,
        );
        combat::player_attack(world, player, zombie, &Default::default());
        world.get_mut::<Position>(player).unwrap().0 =
            DVec3::new(8.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5);
    }

    fn block(&mut self, pos: IVec3) -> u32 {
        self.world().block_at(pos).unwrap()
    }

    /// `(tick, event id, data)` of every level event at the door cell's
    /// centre sent this tick.
    fn door_events(&mut self, tick: u64, out: &mut Vec<(u64, i32, i32)>) {
        let centre = DOOR.as_vec3() + glam::Vec3::splat(0.5);
        while let Ok(packet) = self.rx.try_recv() {
            if let McpePacketData::LevelEventPacket(event) = packet.data
                && (event.position.x - centre.x).abs() < 1e-4
                && (event.position.y - centre.y).abs() < 1e-4
                && (event.position.z - centre.z).abs() < 1e-4
            {
                out.push((tick, event.event_id, event.data));
            }
        }
    }

    /// Tick `ticks` times collecting door events; also the most eastern x
    /// the zombie reached and the ticks it spent pressed against the door
    /// (within a block west of it, in its row).
    fn run(&mut self, zombie: Entity, ticks: u64) -> (Vec<(u64, i32, i32)>, f64, u64) {
        let mut events = Vec::new();
        let mut east = f64::MIN;
        let mut at_door = 0;
        for tick in 0..ticks {
            self.server.tick();
            self.door_events(tick, &mut events);
            if let Some(position) = self.world().get::<Position>(zombie) {
                let feet = position.0;
                east = east.max(feet.x);
                if feet.x > f64::from(DOOR.x) - 1.0 && (feet.z - 0.5).abs() < 1.0 {
                    at_door += 1;
                }
            }
        }
        (events, east, at_door)
    }

    fn door_items(&mut self) -> usize {
        let world = self.world();
        world
            .query_filtered::<&ItemEntityStack, With<DroppedItem>>()
            .iter(world)
            .filter(|stack| stack.0.item_id().ends_with("door"))
            .count()
    }
}

#[test]
fn breaker_breaks_a_closed_oak_door_after_the_native_time_on_hard() {
    let mut h = harness(7, "minecraft:wooden_door", Difficulty::Hard);
    let air = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:air")
        .unwrap();
    let oak_default = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:wooden_door")
        .unwrap();
    let zombie = h.zombie(true);
    h.provoke(zombie);
    let (events, east, _) = h.run(zombie, 900);

    let start = events
        .iter()
        .find(|(_, id, _)| *id == LEVEL_EVENT_START_BLOCK_CRACKING)
        .expect("the zombie starts cracking the door");
    // (1 / 240) × 65535, truncated.
    assert_eq!(start.2, 273);
    let crash = events
        .iter()
        .find(|(_, id, _)| *id == LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH)
        .unwrap_or_else(|| panic!("the door breaks: {events:?}"));
    let last_start = events
        .iter()
        .rfind(|(tick, id, _)| *id == LEVEL_EVENT_START_BLOCK_CRACKING && *tick <= crash.0)
        .unwrap();
    assert_eq!(crash.0 - last_start.0, BREAK_TICKS, "{events:?}");
    assert!(events.contains(&(
        crash.0,
        LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
        oak_default as i32
    )));
    assert_eq!(h.block(DOOR), air);
    assert_eq!(h.block(DOOR + IVec3::Y), air);
    assert_eq!(h.door_items(), 0, "a broken door drops nothing");
    assert!(east > f64::from(DOOR.x) + 1.0, "walked through: {east}");
    let flags = h.world().get::<ActorFlags>(zombie).unwrap();
    assert!(!flags.get(actor_flags::BREAKING_OBSTRUCTION).unwrap());
}

#[test]
fn breaker_never_breaks_doors_on_normal() {
    let mut h = harness(7, "minecraft:wooden_door", Difficulty::Normal);
    let lower = door_state("minecraft:wooden_door", false);
    let zombie = h.zombie(true);
    h.provoke(zombie);
    let (events, east, at_door) = h.run(zombie, 900);
    assert!(at_door > 450, "it presses against the door: {at_door}");
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(h.block(DOOR), lower);
    assert!(
        east > f64::from(DOOR.x) - 1.5,
        "it paths up to the door: {east}"
    );
    assert!(east < f64::from(DOOR.x), "{east}");
}

#[test]
fn breaker_never_breaks_an_iron_door() {
    let mut h = harness(7, "minecraft:iron_door", Difficulty::Hard);
    let lower = door_state("minecraft:iron_door", false);
    let zombie = h.zombie(true);
    h.provoke(zombie);
    let (events, east, _) = h.run(zombie, 900);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(h.block(DOOR), lower);
    assert!(east < f64::from(DOOR.x), "{east}");
}

#[test]
fn zombie_without_the_annotation_does_not_path_through_the_door() {
    let mut h = harness(7, "minecraft:wooden_door", Difficulty::Hard);
    let lower = door_state("minecraft:wooden_door", false);
    let zombie = h.zombie(false);
    h.provoke(zombie);
    let (events, east, at_door) = h.run(zombie, 900);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(h.block(DOOR), lower);
    // The door cell is a wall to its path search: it never chases the
    // player up to the door.
    assert!(east < f64::from(DOOR.x), "{east}");
    assert!(at_door < 100, "{at_door}");
}
