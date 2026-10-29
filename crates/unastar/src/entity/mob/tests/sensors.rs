//! Environment sensors and timers through a real `GameServer`: the vanilla
//! zombie's underwater transformation (`is_underwater` sensor, the 30 s
//! timer, `is_shaking`), and looping / one-shot timers.

use std::sync::Arc;

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::actor_flags;
use tokio::sync::mpsc;

use crate::ecs::resources::TickCounter;
use crate::entity::EntityRandom;
use crate::entity::components::{
    GameMode, Health, HeldSlot, MainInventory, Player, PlayerSession, Position, Rotation,
    RuntimeEntityId, Velocity,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::properties::{ActorProperties, NamedPropertyValue, PropertyValue};
use crate::entity::mob::components::sensors::{TimerConfig, TimerSpan};
use crate::entity::mob::components::{EnvironmentSensorConfig, SelfEvent, Timer};
use crate::entity::mob::{SpawnCause, spawn_mob};
use crate::server::game::{GameServer, ServerConfig};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;

const MIDNIGHT: u64 = 18_000;
const RIDING: &str = "minecraft:is_riding_zombie_horse";
/// Where the zombie stands: inside the water column at (4, 64..=66, 4).
const FEET: DVec3 = DVec3::new(4.5, 64.0, 4.5);

struct Harness {
    server: GameServer,
    _rx: mpsc::Receiver<jolyne::valentine::McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at midnight on
/// normal, viewed by one creative player (so the zombie neither burns nor
/// hunts).
fn harness(seed: u64) -> Harness {
    let mut server = GameServer::with_config(ServerConfig {
        world: crate::world::WorldConfig::void_spawn_platform(1),
        ..ServerConfig::default()
    });
    let world = server.ecs.world_mut();
    world.insert_resource(EntityRandom::seeded(seed));
    *world.resource_mut::<Difficulty>() = Difficulty::Normal;
    crate::world::clock::set_time(world, MIDNIGHT);
    let (tx, rx) = mpsc::channel(1 << 16);
    let player = world
        .spawn((
            Player,
            Position(DVec3::new(12.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 12.5)),
            Rotation::default(),
            GameMode::Creative,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            RuntimeEntityId(1),
            PlayerSession::new(1, "steve".into(), None, None, tx),
        ))
        .id();
    for cx in -1..=1 {
        for cz in -1..=1 {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16 {
                for z in 0..16 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
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
    Harness { server, _rx: rx }
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    fn now(&mut self) -> u64 {
        self.world().resource::<TickCounter>().current
    }

    /// An adult zombie at [`FEET`].
    fn zombie(&mut self) -> Entity {
        let world = self.world();
        for _ in 0..2000 {
            let mob = spawn_mob(world, "minecraft:zombie", FEET, 0.0, SpawnCause::Spawned).unwrap();
            apply_pending_definitions(world);
            if !flag(world, mob, actor_flags::BABY) {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no adult zombie in 2000 spawns");
    }

    fn set_block(&mut self, pos: IVec3, name: &str) {
        let state = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
        let world = self.world();
        let (cx, cz) = crate::world::ecs::world_to_chunk_coords(pos.x, pos.z);
        let chunk = world
            .resource::<ChunkManager>()
            .get_by_coords(cx, cz)
            .unwrap();
        let (x, y, z) = crate::world::ecs::world_to_local_coords(pos.x, pos.y, pos.z);
        world
            .get_mut::<ChunkData>(chunk)
            .unwrap()
            .inner
            .set_block(x, y, z, state);
    }

    /// Water source blocks at (4, 64..64 + depth, 4).
    fn water_column(&mut self, depth: i32, name: &str) {
        for y in 64..64 + depth {
            self.set_block(IVec3::new(4, y, 4), name);
        }
    }

    /// One server tick with `mob` held at [`FEET`].
    fn tick_held(&mut self, mob: Entity) {
        let world = self.world();
        world.get_mut::<Position>(mob).unwrap().0 = FEET;
        if let Some(mut velocity) = world.get_mut::<Velocity>(mob) {
            velocity.0 = DVec3::ZERO;
        }
        self.server.tick();
    }

    fn ticks_held(&mut self, mob: Entity, n: u64) {
        for _ in 0..n {
            self.tick_held(mob);
        }
    }

    fn riding(&mut self, mob: Entity) -> bool {
        let properties = self.world().get::<ActorProperties>(mob).unwrap();
        let slot = properties.layout().slot(RIDING).unwrap();
        properties.value(&slot).unwrap() == PropertyValue::Bool(true)
    }

    fn set_riding(&mut self, mob: Entity, value: bool) {
        self.world()
            .get_mut::<ActorProperties>(mob)
            .unwrap()
            .set_named(RIDING, NamedPropertyValue::Bool(value))
            .unwrap();
    }

    /// Replace `mob`'s timer by one firing `on_stop_riding_zombie_horse`
    /// (which clears [`RIDING`] at once) every `span` ticks, and drop its
    /// environment sensor so nothing else touches the property.
    fn riding_timer(&mut self, mob: Entity, looping: bool, span: i32) -> u64 {
        let deadline = self.now() + span as u64;
        let config = Arc::new(TimerConfig {
            looping,
            random_interval: false,
            span: TimerSpan::Range {
                min: span,
                max: span,
            },
            event: SelfEvent(Arc::from("minecraft:on_stop_riding_zombie_horse")),
        });
        self.world()
            .entity_mut(mob)
            .remove::<EnvironmentSensorConfig>()
            .insert(Timer {
                config,
                span_ticks: span,
                deadline,
                fired: false,
            });
        deadline
    }

    /// Tick until the tick about to run is `tick`.
    fn run_until(&mut self, mob: Entity, tick: u64) {
        while self.now() < tick {
            self.tick_held(mob);
        }
    }
}

fn flag(world: &World, mob: Entity, index: u32) -> bool {
    world
        .get::<ActorFlags>(mob)
        .is_some_and(|flags| flags.get(index).unwrap())
}

#[test]
fn an_underwater_zombie_starts_transforming_and_shakes_when_the_timer_fires() {
    let mut h = harness(1);
    let zombie = h.zombie();
    h.water_column(3, "minecraft:water");
    // Both base sensor triggers pass this tick: underwater, and riding set
    // without a zombie-horse vehicle. Every passing trigger fires.
    h.set_riding(zombie, true);
    h.tick_held(zombie);
    assert!(!h.riding(zombie), "on_stop_riding_zombie_horse fired");
    assert!(h.world().get::<Timer>(zombie).is_none(), "group change waits for the barrier");
    // The next tick's barrier adds start_drowned_transformation and its timer.
    h.tick_held(zombie);
    let timer = h.world().get::<Timer>(zombie).unwrap().clone();
    assert_eq!(timer.span_ticks, 600, "time 30 s");
    assert!(!timer.config.looping && !timer.fired);
    assert!(!flag(h.world(), zombie, actor_flags::SHAKING));

    h.run_until(zombie, timer.deadline);
    let waiting = h.world().get::<Timer>(zombie).unwrap();
    assert!(!waiting.fired, "nothing fires before the deadline");
    assert_eq!(waiting.deadline, timer.deadline, "nothing is written while waiting");
    assert!(!flag(h.world(), zombie, actor_flags::SHAKING));

    // The deadline tick fires convert_to_drowned; its groups apply next tick.
    h.tick_held(zombie);
    assert!(h.world().get::<Timer>(zombie).unwrap().fired);
    h.tick_held(zombie);
    assert!(flag(h.world(), zombie, actor_flags::SHAKING));
}

#[test]
fn a_zombie_that_leaves_the_water_stops_transforming() {
    let mut h = harness(2);
    let zombie = h.zombie();
    h.water_column(3, "minecraft:water");
    h.ticks_held(zombie, 2);
    assert!(h.world().get::<Timer>(zombie).is_some());
    h.water_column(3, "minecraft:air");
    // stop_transforming fires, then the barrier removes the timer group.
    h.ticks_held(zombie, 2);
    assert!(h.world().get::<Timer>(zombie).is_none());
    h.ticks_held(zombie, 700);
    assert!(!flag(h.world(), zombie, actor_flags::SHAKING));
}

#[test]
fn a_zombie_with_its_head_above_the_water_does_not_transform() {
    let mut h = harness(3);
    let zombie = h.zombie();
    // The head (64 + 1.9 × 0.85) is in the block above the single water
    // block.
    h.water_column(1, "minecraft:water");
    h.ticks_held(zombie, 5);
    assert!(h.world().get::<Timer>(zombie).is_none());
    // Two blocks: the head is inside the top source block, below its surface.
    h.water_column(2, "minecraft:water");
    h.ticks_held(zombie, 2);
    assert!(h.world().get::<Timer>(zombie).is_some());
}

#[test]
fn a_looping_timer_fires_at_every_deadline() {
    let mut h = harness(4);
    let zombie = h.zombie();
    let deadline = h.riding_timer(zombie, true, 5);
    h.set_riding(zombie, true);
    h.run_until(zombie, deadline);
    assert!(h.riding(zombie), "nothing fires before the deadline");
    h.tick_held(zombie);
    assert!(!h.riding(zombie), "fired at the deadline");
    let timer = h.world().get::<Timer>(zombie).unwrap().clone();
    assert!(timer.fired);
    assert_eq!(timer.deadline, deadline + 5, "rescheduled one span later");

    h.set_riding(zombie, true);
    h.run_until(zombie, timer.deadline);
    assert!(h.riding(zombie));
    h.tick_held(zombie);
    assert!(!h.riding(zombie), "fired again");
}

#[test]
fn a_one_shot_timer_fires_once() {
    let mut h = harness(5);
    let zombie = h.zombie();
    let deadline = h.riding_timer(zombie, false, 5);
    h.set_riding(zombie, true);
    h.run_until(zombie, deadline + 1);
    assert!(!h.riding(zombie), "fired at the deadline");
    assert!(h.world().get::<Timer>(zombie).unwrap().fired);
    h.set_riding(zombie, true);
    h.ticks_held(zombie, 30);
    assert!(h.riding(zombie), "never fires again");
}
