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
    MainInventory, OffhandSlot, OnGround, Player, PlayerSession, PlayerUi, Position, Rotation,
    RuntimeEntityId, Velocity,
};
use crate::entity::definition::flags::ActorFlags;
use crate::entity::definition::{GroupChange, apply_pending_definitions, queue_group_change};
use crate::entity::mob::components::doors::BreakDoorConfig;
use crate::entity::mob::constants::{
    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
    LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH, LEVEL_EVENT_SOUND_ZOMBIE_WOODEN_DOOR,
    LEVEL_EVENT_START_BLOCK_CRACKING, LEVEL_EVENT_STOP_BLOCK_CRACKING,
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

pub(super) struct Harness {
    pub(super) server: GameServer,
    pub(super) player: Entity,
    pub(super) rx: mpsc::Receiver<McpePacket>,
    /// Block ids the wire audit checked so far.
    block_ids_checked: usize,
}

/// The side of the door cell a closed panel lies on, seen from the zombie
/// coming from the west.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Panel {
    /// The west side: the zombie meets the panel before the cell.
    Near,
    /// The east side: the zombie walks into the cell up to the panel.
    Far,
}

/// A door half of `name` whose closed panel stands across the x axis on the
/// west side of its cell (the side a zombie coming from the west meets).
fn door_state(name: &str, upper: bool) -> u32 {
    door_state_with_panel(name, Panel::Near, upper)
}

/// A door half of `name` whose closed panel stands across the x axis on the
/// `panel` side of its cell.
fn door_state_with_panel(name: &str, panel: Panel, upper: bool) -> u32 {
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
            match panel {
                Panel::Near => boxes[0].max.x <= 0.5,
                Panel::Far => boxes[0].min.x >= 0.5,
            }
        })
        .unwrap();
    set(facing, "upper_block_bit", StateValue::Byte(u8::from(upper)))
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1, split at x = 3 by a
/// three-high stone wall whose only gap holds a closed door of `door` at
/// [`DOOR`]. One survival player stands east of the wall at (8.5, 64, 0.5).
/// Night, so the zombie does not burn.
fn harness(seed: u64, door: &str, difficulty: Difficulty) -> Harness {
    harness_with_panel(seed, door, Panel::Near, difficulty)
}

/// [`harness`] with the door's closed panel on the `panel` side of its cell.
fn harness_with_panel(seed: u64, door: &str, panel: Panel, difficulty: Difficulty) -> Harness {
    let stone = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:stone")
        .unwrap();
    let (lower, upper) = (
        door_state_with_panel(door, panel, false),
        door_state_with_panel(door, panel, true),
    );
    built(seed, difficulty, move |pos| {
        (pos.x == DOOR.x && (64..=66).contains(&pos.y)).then_some(match (pos.z == DOOR.z, pos.y) {
            (true, 64) => lower,
            (true, 65) => upper,
            _ => stone,
        })
    })
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 with the blocks
/// `layout` gives at y = 63..=67 on top. One survival player stands at
/// (8.5, 64, 0.5). Night, so a zombie does not burn.
pub(super) fn built(
    seed: u64,
    difficulty: Difficulty,
    layout: impl Fn(IVec3) -> Option<u32>,
) -> Harness {
    built_with_radius(seed, difficulty, 1, layout)
}

/// [`built`] with grass columns `radius` chunks around the origin chunk.
pub(super) fn built_with_radius(
    seed: u64,
    difficulty: Difficulty,
    radius: i32,
    layout: impl Fn(IVec3) -> Option<u32>,
) -> Harness {
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
    for cx in -radius..=radius {
        for cz in -radius..=radius {
            let mut column = Chunk::new(cx, cz);
            for x in 0..16u8 {
                for z in 0..16u8 {
                    column.set_block(x, 63, z, *blocks::GRASS_BLOCK);
                    for y in 63..=67 {
                        let pos = IVec3::new(cx * 16 + i32::from(x), y, cz * 16 + i32::from(z));
                        if let Some(state) = layout(pos) {
                            column.set_block(x, y as i16, z, state);
                        }
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
    Harness {
        server,
        player,
        rx,
        block_ids_checked: 0,
    }
}

impl Harness {
    pub(super) fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// An adult zombie west of the wall; with `breaker` it also gets the
    /// `minecraft:can_break_doors` group through its definition stack.
    fn zombie(&mut self, breaker: bool) -> Entity {
        self.zombie_at(DVec3::new(0.5, 64.0, 0.5), breaker)
    }

    /// [`Self::zombie`] with its feet at `feet`.
    pub(super) fn zombie_at(&mut self, feet: DVec3, breaker: bool) -> Entity {
        let world = self.world();
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
    /// centre sent this tick. Every block id sent must be a wire id.
    fn door_events(&mut self, tick: u64, out: &mut Vec<(u64, i32, i32)>) {
        let centre = DOOR.as_vec3() + glam::Vec3::splat(0.5);
        let packets: Vec<_> =
            std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect();
        let ids = crate::server::game::types::block_network_ids(self.world());
        self.block_ids_checked +=
            crate::server::wire_audit::assert_block_ids_on_wire(ids, &packets);
        for packet in packets {
            if let McpePacketData::LevelEventPacket(event) = packet
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

    /// Every door packet sent this tick, in send order: level events at
    /// the centre of either door half and block updates of either half.
    fn door_wire(&mut self, tick: u64, out: &mut Vec<(u64, Wire)>) {
        let packets: Vec<_> =
            std::iter::from_fn(|| self.rx.try_recv().ok().map(|p| p.data)).collect();
        let half = |cell: IVec3| match cell - DOOR {
            IVec3::ZERO => Some(false),
            IVec3::Y => Some(true),
            _ => None,
        };
        for packet in packets {
            match packet {
                McpePacketData::LevelEventPacket(event) => {
                    let p = event.position;
                    let cell =
                        IVec3::new(p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32);
                    let centred = [p.x, p.y, p.z]
                        .into_iter()
                        .all(|v| (v - v.floor() - 0.5).abs() < 1e-4);
                    if let (Some(upper), true) = (half(cell), centred) {
                        let (id, data) = (event.event_id, event.data);
                        out.push((tick, Wire::Event { upper, id, data }));
                    }
                }
                McpePacketData::UpdateBlockPacket(update) => {
                    let p = update.block_position;
                    if let Some(upper) = half(IVec3::new(p.x, p.y, p.z)) {
                        let wire = update.block_runtime_id;
                        out.push((tick, Wire::Block { upper, wire }));
                    }
                }
                _ => {}
            }
        }
    }

    /// Tick `ticks` times recording the door packets, the ticks on which
    /// the zombie stood in the door's cell while cracking it and the ticks
    /// at whose end each door half was first air.
    fn run_wire(&mut self, zombie: Entity, ticks: u64) -> BreakRun {
        let air = air();
        let mut run = BreakRun::default();
        for tick in 0..ticks {
            self.server.tick();
            self.door_wire(tick, &mut run.wire);
            for (half, at) in [
                (DOOR, &mut run.lower_air),
                (DOOR + IVec3::Y, &mut run.upper_air),
            ] {
                if at.is_none() && self.block(half) == air {
                    *at = Some(tick);
                }
            }
            let world = self.world();
            let (Some(position), Some(flags)) = (
                world.get::<Position>(zombie),
                world.get::<ActorFlags>(zombie),
            ) else {
                continue;
            };
            let in_cell = position.0.x + 0.3 > f64::from(DOOR.x);
            if in_cell && flags.get(actor_flags::BREAKING_OBSTRUCTION).unwrap() {
                run.cracking_in_cell += 1;
            }
        }
        run
    }
}

/// What [`Harness::run_wire`] saw.
#[derive(Default)]
struct BreakRun {
    wire: Vec<(u64, Wire)>,
    cracking_in_cell: u64,
    lower_air: Option<u64>,
    upper_air: Option<u64>,
}

/// A door packet on the wire (see [`Harness::door_wire`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wire {
    /// `LevelEvent` `id` with `data` at the centre of the lower or `upper`
    /// door half.
    Event { upper: bool, id: i32, data: i32 },
    /// `UpdateBlock` of the lower or `upper` door half to the `wire` id.
    Block { upper: bool, wire: u32 },
}

/// Wire ids from the BDS 1.26.51 captures `zombie-door-break.jsonl` and
/// `zombie-door-break-inside.jsonl`: air, and the oak door's default state
/// (the destroy particles of a door a zombie broke name it).
const BDS_AIR_WIRE: u32 = 3_690_217_760;
const BDS_OAK_DOOR_DEFAULT_WIRE: i32 = 650_702_320;

/// A zombie breaking a closed oak door as the BDS 1.26.51 captures
/// `zombie-door-break.jsonl` and `zombie-door-break-inside.jsonl` show it:
///
/// 1. the crack starts at the lower half (StartBlockCracking, data 273);
/// 2. only door bashes (1 in 20 per tick: 10 and 14 in the captures) follow;
/// 3. 241 ticks after the start the crash sound and the destroy particles
///    of the door's default state play together at the lower half, and the
///    lower half is removed;
/// 4. on the next tick the upper half, left alone, is destroyed with its
///    own destroy particles.
///
/// The level events must match exactly, and so must the server-side block
/// state: the lower half is air at the end of the crash tick, the upper half
/// one tick later. On the BDS wire each half's UpdateBlock arrives one tick
/// after its removal (lower at crash + 1, upper at crash + 2), because BDS
/// sends block changes once per tick, before the entity tick. Unastar sends
/// them on the tick of the change, and sends each one twice (`set_block`
/// plus `broadcast_block_updates`). Both are pending the block-flush change
/// owned by the blocks lane. Until it lands, an UpdateBlock may arrive one
/// tick before the BDS tick, and same-tick repeats count once.
fn assert_bds_break_order(run: &BreakRun, upper_state: u32) {
    let wire = &run.wire;
    let event = |upper, id, data| Wire::Event { upper, id, data };
    let events: Vec<(u64, Wire)> = wire
        .iter()
        .copied()
        .filter(|(_, w)| matches!(w, Wire::Event { .. }))
        .collect();
    let (start, first) = events.first().copied().expect("the zombie cracks the door");
    assert_eq!(
        first,
        event(false, LEVEL_EVENT_START_BLOCK_CRACKING, 273),
        "{wire:?}"
    );
    let crash = events
        .iter()
        .position(|(_, w)| matches!(w, Wire::Event { id, .. } if *id == LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH))
        .unwrap_or_else(|| panic!("the door breaks: {wire:?}"));
    let bashes = &events[1..crash];
    assert!(
        bashes
            .iter()
            .all(|(_, w)| *w == event(false, LEVEL_EVENT_SOUND_ZOMBIE_WOODEN_DOOR, 0)),
        "only door bashes while cracking: {wire:?}"
    );
    assert!(
        (3..=25).contains(&bashes.len()),
        "1 in 20 per tick over 240 ticks: {}",
        bashes.len()
    );
    let tick = events[crash].0;
    assert_eq!(tick - start, BREAK_TICKS, "{wire:?}");
    let upper_wire = crate::registry::block::BlockNetworkIds::Hashed.to_wire(upper_state) as i32;
    assert_eq!(
        &events[crash..],
        &[
            (tick, event(false, LEVEL_EVENT_SOUND_ZOMBIE_DOOR_CRASH, 0)),
            (
                tick,
                event(
                    false,
                    LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
                    BDS_OAK_DOOR_DEFAULT_WIRE,
                ),
            ),
            (
                tick + 1,
                event(true, LEVEL_EVENT_PARTICLES_DESTROY_BLOCK, upper_wire),
            ),
        ],
        "{wire:?}"
    );
    assert_eq!(
        run.lower_air,
        Some(tick),
        "lower half removed on the crash tick"
    );
    assert_eq!(run.upper_air, Some(tick + 1), "upper half one tick later");
    let mut blocks: Vec<(u64, Wire)> = wire
        .iter()
        .copied()
        .filter(|(_, w)| matches!(w, Wire::Block { .. }))
        .collect();
    // Same-tick repeats count once (see above).
    blocks.dedup();
    let block = |upper| Wire::Block {
        upper,
        wire: BDS_AIR_WIRE,
    };
    // BDS ticks: lower at crash + 1, upper at crash + 2; one tick early is
    // tolerated until the block flush moves (see above).
    assert_eq!(blocks.len(), 2, "{wire:?}");
    for ((at, w), (bds_tick, expected)) in blocks
        .iter()
        .zip([(tick + 1, block(false)), (tick + 2, block(true))])
    {
        assert_eq!(*w, expected, "{wire:?}");
        assert!(
            (bds_tick - 1..=bds_tick).contains(at),
            "UpdateBlock at {at}, BDS at {bds_tick}: {wire:?}"
        );
    }
}

#[test]
fn breaker_breaks_the_door_in_the_bds_wire_order() {
    let mut h = harness(7, "minecraft:wooden_door", Difficulty::Hard);
    let zombie = h.zombie(true);
    h.provoke(zombie);
    let run = h.run_wire(zombie, 900);
    let upper = door_state("minecraft:wooden_door", true);
    // The capture's upper half (`zombie-door-break-inside.jsonl`: panel
    // against the west side, as here) has this wire id.
    assert_eq!(
        crate::registry::block::BlockNetworkIds::Hashed.to_wire(upper) as i32,
        1_706_008_891
    );
    assert_bds_break_order(&run, upper);
}

/// `zombie-door-break-inside.jsonl`: with the panel on the far side the
/// zombie walks into the door's cell up to the panel and cracks the door
/// from there without the crack ever stopping.
#[test]
fn breaker_cracks_from_inside_the_cell_when_the_panel_is_on_the_far_side() {
    let mut h = harness_with_panel(7, "minecraft:wooden_door", Panel::Far, Difficulty::Hard);
    let zombie = h.zombie(true);
    h.provoke(zombie);
    let run = h.run_wire(zombie, 900);
    // As in the capture, the crack starts while the zombie is still outside
    // the cell; it then walks in up to the panel and keeps cracking there.
    assert!(
        run.cracking_in_cell > BREAK_TICKS / 2,
        "it cracks from inside the cell: {} ticks, {:?}",
        run.cracking_in_cell,
        run.wire
    );
    let stops = run
        .wire
        .iter()
        .filter(
            |(_, w)| matches!(w, Wire::Event { id, .. } if *id == LEVEL_EVENT_STOP_BLOCK_CRACKING),
        )
        .count();
    assert_eq!(stops, 0, "the crack never stops: {:?}", run.wire);
    let upper = door_state_with_panel("minecraft:wooden_door", Panel::Far, true);
    // The capture's upper half (`zombie-door-break.jsonl`: panel against
    // the east side, as here) has this wire id.
    assert_eq!(
        crate::registry::block::BlockNetworkIds::Hashed.to_wire(upper) as i32,
        -1_814_932_087
    );
    assert_bds_break_order(&run, upper);
}

/// A jump against the door keeps the crack going (the move still touches
/// the door); a move that touches nothing ends it.
#[test]
fn crack_ends_only_on_a_move_that_touches_nothing() {
    let mut h = harness(7, "minecraft:wooden_door", Difficulty::Hard);
    let zombie = h.zombie(true);
    h.provoke(zombie);
    let mut wire = Vec::new();
    let breaking = |h: &mut Harness| {
        let flags = h.world().get::<ActorFlags>(zombie).unwrap();
        flags.get(actor_flags::BREAKING_OBSTRUCTION).unwrap()
    };
    let mut tick = 0;
    while !breaking(&mut h) {
        assert!(tick < 600, "the zombie starts cracking: {wire:?}");
        h.server.tick();
        h.door_wire(tick, &mut wire);
        tick += 1;
    }
    // Jump: airborne for several ticks while pushing into the door.
    h.world().get_mut::<Velocity>(zombie).unwrap().0.y = 0.42;
    let mut airborne = 0;
    for _ in 0..12 {
        h.server.tick();
        h.door_wire(tick, &mut wire);
        tick += 1;
        if !h.world().get::<OnGround>(zombie).unwrap().0 {
            airborne += 1;
        }
    }
    assert!(airborne >= 5, "it was airborne: {airborne}");
    assert!(breaking(&mut h), "the jump kept the crack: {wire:?}");
    assert!(
        !wire.iter().any(
            |(_, w)| matches!(w, Wire::Event { id, .. } if *id == LEVEL_EVENT_STOP_BLOCK_CRACKING)
        ),
        "{wire:?}"
    );
    // Lift it into the open air west of the door: its next move touches
    // nothing.
    h.world().get_mut::<Position>(zombie).unwrap().0 =
        DVec3::new(f64::from(DOOR.x) - 1.5, 66.0, 0.5);
    for _ in 0..2 {
        h.server.tick();
        h.door_wire(tick, &mut wire);
        tick += 1;
    }
    assert!(!breaking(&mut h), "{wire:?}");
    assert!(
        wire.iter().any(
            |(_, w)| matches!(w, Wire::Event { id, .. } if *id == LEVEL_EVENT_STOP_BLOCK_CRACKING)
        ),
        "{wire:?}"
    );
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
    let oak_wire = crate::registry::block::BlockNetworkIds::Hashed.to_wire(oak_default) as i32;
    assert!(
        events.contains(&(
            crash.0,
            LEVEL_EVENT_PARTICLES_DESTROY_BLOCK_NO_SOUND,
            oak_wire
        )),
        "the broken door's particles name it by its wire id: {events:?}"
    );
    assert!(h.block_ids_checked > 0, "block ids were on the wire");
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
    let (events, east, _) = h.run(zombie, 900);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(h.block(DOOR), lower);
    // The door cell is a wall to its path search: the melee goal follows
    // the partial route to the node closest to the player, which ends in
    // front of the door, and never crosses it.
    assert!(
        east > f64::from(DOOR.x) - 1.5,
        "it paths up to the door: {east}"
    );
    assert!(east < f64::from(DOOR.x), "{east}");
}

/// The player stands just east of the door's cell, close enough that a
/// zombie pressed against the closed panel from the west has the player's
/// box within its reach box; only the door stands between them.
fn stand_behind_the_door(h: &mut Harness) {
    let player = h.player;
    h.world().get_mut::<Position>(player).unwrap().0 = DVec3::new(
        f64::from(DOOR.x) + 1.05,
        64.0 + crate::entity::PLAYER_EYE_HEIGHT,
        0.5,
    );
}

#[test]
fn zombie_behind_a_closed_door_cannot_hurt_the_player_until_the_door_is_gone() {
    let mut h = harness(7, "minecraft:wooden_door", Difficulty::Hard);
    let air = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:air")
        .unwrap();
    let zombie = h.zombie(true);
    h.provoke(zombie);
    stand_behind_the_door(&mut h);
    let player = h.player;
    let mut events = Vec::new();
    let mut broken_at = None;
    let mut hurt_at = None;
    let mut pressed = 0;
    for tick in 0..900u64 {
        h.server.tick();
        h.door_events(tick, &mut events);
        let feet = h.world().get::<Position>(zombie).unwrap().0;
        if feet.x > f64::from(DOOR.x) - 1.0 && (feet.z - 0.5).abs() < 1.0 {
            pressed += 1;
        }
        if broken_at.is_none() && h.block(DOOR) == air {
            broken_at = Some(tick);
        }
        let health = h.world().get::<Health>(player).unwrap().current;
        if health < 20.0 {
            hurt_at = Some(tick);
            break;
        }
    }
    assert!(pressed > 0, "the zombie reaches the door");
    if let Some(hurt_at) = hurt_at {
        assert!(
            broken_at.is_some_and(|broken_at| hurt_at > broken_at),
            "hurt through the closed door at tick {hurt_at} (door broken at {broken_at:?}): \
             {events:?}"
        );
    }
    assert!(broken_at.is_some(), "the door breaks: {events:?}");
    assert!(
        hurt_at.is_some(),
        "the zombie hurts the player once the door is gone"
    );
}

#[test]
fn zombie_never_hurts_the_player_through_an_iron_door() {
    let mut h = harness(7, "minecraft:iron_door", Difficulty::Hard);
    let zombie = h.zombie(true);
    h.provoke(zombie);
    stand_behind_the_door(&mut h);
    let player = h.player;
    let mut pressed = 0;
    let mut events = Vec::new();
    for tick in 0..600u64 {
        h.server.tick();
        h.door_events(tick, &mut events);
        let feet = h.world().get::<Position>(zombie).unwrap().0;
        if feet.x > f64::from(DOOR.x) - 1.0 && (feet.z - 0.5).abs() < 1.0 {
            pressed += 1;
        }
    }
    let health = h.world().get::<Health>(player).unwrap().current;
    assert_eq!(health, 20.0, "hurt through the iron door");
    assert!(pressed > 300, "it presses against the door: {pressed}");
}

/// A door half of `name` facing `direction` (`minecraft:cardinal_direction`),
/// open or closed.
pub(super) fn door_facing(name: &str, direction: &'static str, open: bool, upper: bool) -> u32 {
    let entry = BlockRegistry::vanilla().get_by_name(name).unwrap();
    let set = |state, property, value| entry.with_state_value(state, property, value).unwrap();
    let state = set(
        entry.default_state_id,
        "minecraft:cardinal_direction",
        StateValue::Str(direction),
    );
    let state = set(state, "open_bit", StateValue::Byte(u8::from(open)));
    set(state, "upper_block_bit", StateValue::Byte(u8::from(upper)))
}

/// The `zombie-open-doorway` scenario (client-diff, BDS 1.26.51 capture
/// `zombie-open-doorway.jsonl`): an oak-plank room (walls x -3..=1,
/// z -2..=2, floor y 63, roof y 66) whose east wall holds an open oak door
/// at (1, 64, 0); the player stands inside at (-0.5, 64, 0.5) and a zombie
/// that turned on it starts outside at (6.5, 64, 0.5). On BDS the zombie walks
/// through the doorway and hurts the player about 4 s after it started.
///
/// The door is stored as a player's use leaves it: only the lower half has
/// `open_bit` set and only the upper half the hinge, for both hinges. The
/// upper half used to collide as a closed door, sealing the doorway at head
/// height, so the zombie stopped against it. (On BDS the zombie then follows
/// the player, knocked back west by the hits, to x -0.98; this harness's
/// player is not moved by knockback.)
#[test]
fn zombie_walks_through_an_open_doorway_to_the_player() {
    let planks = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:oak_planks")
        .unwrap();
    let entry = BlockRegistry::vanilla()
        .get_by_name("minecraft:wooden_door")
        .unwrap();
    for hinge in [0u8, 1] {
        let lower = door_facing("minecraft:wooden_door", "north", true, false);
        let upper = entry
            .with_state_value(
                door_facing("minecraft:wooden_door", "north", false, true),
                "door_hinge_bit",
                StateValue::Byte(hinge),
            )
            .unwrap();
        let mut h = built(3, Difficulty::Hard, move |pos| {
            let inside = (-3..=1).contains(&pos.x)
                && (-2..=2).contains(&pos.z)
                && (63..=66).contains(&pos.y);
            if !inside {
                return None;
            }
            if pos == IVec3::new(1, 64, 0) {
                return Some(lower);
            }
            if pos == IVec3::new(1, 65, 0) {
                return Some(upper);
            }
            let shell = pos.x == -3 || pos.x == 1 || pos.z.abs() == 2 || pos.y == 63 || pos.y == 66;
            Some(if shell { planks } else { air() })
        });
        let zombie = h.zombie_at(DVec3::new(6.5, 64.0, 0.5), false);
        h.provoke(zombie);
        let player = h.player;
        h.world().get_mut::<Position>(player).unwrap().0 =
            DVec3::new(-0.5, 64.0 + crate::entity::PLAYER_EYE_HEIGHT, 0.5);
        let mut west = f64::MAX;
        let mut hurt = None;
        let mut events = Vec::new();
        for tick in 0..400u64 {
            h.server.tick();
            h.door_events(tick, &mut events);
            let feet = h.world().get::<Position>(zombie).unwrap().0;
            west = west.min(feet.x);
            if hurt.is_none() && h.world().get::<Health>(player).unwrap().current < 20.0 {
                hurt = Some(tick);
            }
        }
        // Its whole body passed the door cell (x 1..2) into the room.
        assert!(
            west + 0.3 < 1.0,
            "hinge {hinge}: the zombie comes into the room: {west}"
        );
        assert!(hurt.is_some(), "hinge {hinge}: and hurts the player");
    }
}

pub(super) fn air() -> u32 {
    BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:air")
        .unwrap()
}
