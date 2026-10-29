//! Walking mobs on stepped terrain through a real `GameServer`: a zombie
//! resting half over a block edge, on a ledge beside the player, and
//! walking up and down 1-block steps, and a cow tempted across steps.

use bevy_ecs::prelude::*;
use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacket;
use tokio::sync::mpsc;

use crate::entity::EntityRandom;
use crate::entity::components::{
    Experience, GameMode, Health, HeldSlot, MainInventory, PhysicsBody, Player, PlayerSession,
    Position, Rotation, RuntimeEntityId,
};
use crate::entity::definition::apply_pending_definitions;
use crate::entity::definition::flags::ActorFlags;
use crate::entity::mob::ai::MobBrain;
use crate::entity::mob::state::AttackTarget;
use crate::entity::mob::*;
use crate::entity::navigation::{
    NavigationScratch, WalkNavigation, WalkPathOutcome, WalkPathRequest, find_walk_path,
};
use crate::entity::systems::physics::tests::TestBlocks;
use crate::server::game::{GameServer, ServerConfig, world_stack};
use crate::world::chunk::{Chunk, blocks};
use crate::world::ecs::{ChunkData, ChunkManager, ChunkPosition, ChunkViewers};
use crate::world::rules::Difficulty;
use jolyne::valentine::actor_flags;

const MIDNIGHT: u64 = 18_000;

struct Harness {
    server: GameServer,
    player: Entity,
    _rx: mpsc::Receiver<McpePacket>,
}

/// A grass floor at y = 63 over chunks -1..=1 × -1..=1 at midnight on
/// Normal, viewed by one survival player standing at `player`.
fn harness(seed: u64, player: DVec3) -> Harness {
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
            Position(player + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT),
            Rotation::default(),
            GameMode::Survival,
            MainInventory::default(),
            HeldSlot(0),
            Health::default(),
            Experience::default(),
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
    Harness {
        server,
        player,
        _rx: rx,
    }
}

impl Harness {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// An adult `identifier` with its feet at `feet`.
    fn adult(&mut self, identifier: &str, feet: DVec3) -> Entity {
        let world = self.world();
        for _ in 0..2000 {
            let mob = spawn_mob(world, identifier, feet, 0.0, SpawnCause::Spawned).unwrap();
            apply_pending_definitions(world);
            let baby = world
                .get::<ActorFlags>(mob)
                .unwrap()
                .get(actor_flags::BABY)
                .unwrap();
            if !baby {
                return mob;
            }
            world.despawn(mob);
        }
        panic!("no adult {identifier} in 2000 spawns");
    }

    /// Fill the block box `min..=max` with `name`.
    fn fill(&mut self, min: IVec3, max: IVec3, name: &str) {
        let state = crate::registry::BlockRegistry::vanilla()
            .default_state_id_by_name(name)
            .unwrap();
        let world = self.world();
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let (cx, cz) = crate::world::ecs::world_to_chunk_coords(x, z);
                    let chunk = world
                        .resource::<ChunkManager>()
                        .get_by_coords(cx, cz)
                        .unwrap();
                    let (lx, ly, lz) = crate::world::ecs::world_to_local_coords(x, y, z);
                    world
                        .get_mut::<ChunkData>(chunk)
                        .unwrap()
                        .inner
                        .set_block(lx, ly, lz, state);
                }
            }
        }
    }

    fn hold(&mut self, item: &str) {
        let stack = world_stack(self.world(), item, 1).expect("vanilla item");
        let player = self.player;
        self.world()
            .get_mut::<MainInventory>(player)
            .unwrap()
            .0
            .set_item(0, stack)
            .unwrap();
    }

    fn health(&mut self) -> f32 {
        let player = self.player;
        self.world().get::<Health>(player).unwrap().current
    }

    fn feet(&mut self, mob: Entity) -> DVec3 {
        self.world().get::<Position>(mob).unwrap().0
    }

    /// One line of mob state for failure messages.
    fn describe(&mut self, mob: Entity) -> String {
        let player = self.player;
        let world = self.world();
        let feet = world.get::<Position>(mob).unwrap().0;
        let rotation = world.get::<Rotation>(mob).unwrap();
        let target = world.get::<AttackTarget>(mob).map(|t| t.0 == player);
        let navigating = world.get::<MobBrain>(mob).unwrap().is_navigating();
        format!(
            "feet ({:.2}, {:.2}, {:.2}) yaw {:.0} head {:.0} targets player {target:?} navigating {navigating}",
            feet.x, feet.y, feet.z, rotation.yaw, rotation.head_yaw
        )
    }

    /// Tick until the player loses health or `ticks` run out; the tick of
    /// the first hit, and a trace of `mob` every 20 ticks.
    fn first_hit(&mut self, mob: Entity, ticks: u64) -> (Option<u64>, Vec<String>) {
        let start = self.health();
        let mut trace = Vec::new();
        for tick in 0..ticks {
            if tick % 20 == 0 {
                let line = format!("t{tick}: {}", self.describe(mob));
                trace.push(line);
            }
            self.server.tick();
            if self.health() < start {
                return (Some(tick), trace);
            }
        }
        (None, trace)
    }
}

/// A zombie resting with its box on a ledge but its centre over the air
/// beside it, one block above the player, on a staircase of full blocks:
/// it climbs down and hits the player.
#[test]
fn zombie_half_over_a_ledge_comes_down_and_attacks() {
    let mut h = harness(31, DVec3::new(7.5, 64.0, 0.5));
    // Steps: y = 64 for x 0..=2, y = 65 for x 0..=1 (tops 65 and 66).
    h.fill(
        IVec3::new(-4, 64, -4),
        IVec3::new(2, 64, 4),
        "minecraft:dirt",
    );
    h.fill(
        IVec3::new(-4, 65, -4),
        IVec3::new(1, 65, 4),
        "minecraft:dirt",
    );
    // Centre over column x = 3 (air below), box x 2.9..3.5 resting on x = 2.
    let zombie = h.adult("minecraft:zombie", DVec3::new(3.2, 65.0, 0.5));
    let (hit, trace) = h.first_hit(zombie, 600);
    assert!(hit.is_some(), "never hit the player:\n{}", trace.join("\n"));
}

/// The same edge position, straight on the navigation search: the start is
/// supported by the box, not the centre, and the search still finds the
/// way down.
#[test]
fn a_start_half_over_an_edge_still_finds_a_path() {
    let mut blocks = TestBlocks::floor("minecraft:stone", 0);
    // The test floor spans x, z in -4..=4; the ledge ends at x = 0.
    for x in -4..=-1 {
        for z in -3..=3 {
            blocks.set(IVec3::new(x, 1, z), "minecraft:stone");
        }
    }
    let body = PhysicsBody::living(0.6, 1.9);
    // Box x -0.1..0.5 resting on the ledge, centre over the column x = 0.
    let start = DVec3::new(0.2, 2.0, 0.5);
    let goal = DVec3::new(3.5, 1.0, 0.5);
    let request = WalkPathRequest {
        start,
        goal,
        max_range: 16.0,
        max_expansions: 140,
        node_budget: 1024,
        step_height: body.step_height,
        jump_height: 1.25,
        max_drop: 3.0,
        partial: false,
    };
    let mut output = WalkNavigation::default();
    let mut scratch = NavigationScratch::default();
    let outcome = find_walk_path(
        &blocks,
        &body,
        request,
        &mut scratch,
        &mut output,
        |_, _| true,
    );
    assert_eq!(
        outcome,
        WalkPathOutcome::Found,
        "{:?}",
        output.waypoints.iter().map(|w| w.feet).collect::<Vec<_>>()
    );
    assert_eq!(output.waypoints.first().unwrap().feet, start);
    assert_eq!(output.waypoints.last().unwrap().feet, goal);
    // Wholly past the edge the body rests on nothing: no path.
    let outcome = find_walk_path(
        &blocks,
        &body,
        WalkPathRequest {
            start: DVec3::new(0.5, 2.0, 0.5),
            ..request
        },
        &mut scratch,
        &mut output,
        |_, _| true,
    );
    assert_eq!(outcome, WalkPathOutcome::Unreachable);
}

/// A zombie on a 1-block ledge right beside and above the player hits it.
#[test]
fn zombie_on_a_ledge_beside_the_player_hits_it() {
    let mut h = harness(32, DVec3::new(5.5, 64.0, 0.5));
    h.fill(
        IVec3::new(-4, 64, -4),
        IVec3::new(4, 64, 4),
        "minecraft:dirt",
    );
    let zombie = h.adult("minecraft:zombie", DVec3::new(4.5, 65.0, 0.5));
    let (hit, trace) = h.first_hit(zombie, 200);
    assert!(hit.is_some(), "never hit the player:\n{}", trace.join("\n"));
}

/// A zombie walks up a 1-block step, across a raised strip and down the
/// far step to reach the player, then the reverse from a raised start.
#[test]
fn zombie_walks_up_and_down_steps_to_the_player() {
    // Up, across, down.
    let mut h = harness(33, DVec3::new(10.5, 64.0, 0.5));
    h.fill(
        IVec3::new(3, 64, -6),
        IVec3::new(6, 64, 6),
        "minecraft:dirt",
    );
    let zombie = h.adult("minecraft:zombie", DVec3::new(0.5, 64.0, 0.5));
    let (hit, trace) = h.first_hit(zombie, 600);
    assert!(hit.is_some(), "up/down: never hit:\n{}", trace.join("\n"));

    // Down two steps onto the player's level, diagonally.
    let mut h = harness(34, DVec3::new(8.5, 64.0, 6.5));
    h.fill(
        IVec3::new(-6, 64, -6),
        IVec3::new(3, 64, 3),
        "minecraft:dirt",
    );
    h.fill(
        IVec3::new(-6, 65, -6),
        IVec3::new(1, 65, 1),
        "minecraft:dirt",
    );
    let zombie = h.adult("minecraft:zombie", DVec3::new(-1.5, 66.0, -1.5));
    let (hit, trace) = h.first_hit(zombie, 600);
    assert!(
        hit.is_some(),
        "down steps: never hit:\n{}",
        trace.join("\n")
    );

    // Up two steps onto a player standing above.
    let mut h = harness(35, DVec3::new(-1.5, 66.0, -1.5));
    h.fill(
        IVec3::new(-6, 64, -6),
        IVec3::new(3, 64, 3),
        "minecraft:dirt",
    );
    h.fill(
        IVec3::new(-6, 65, -6),
        IVec3::new(1, 65, 1),
        "minecraft:dirt",
    );
    let zombie = h.adult("minecraft:zombie", DVec3::new(8.5, 64.0, 6.5));
    let (hit, trace) = h.first_hit(zombie, 600);
    assert!(hit.is_some(), "up steps: never hit:\n{}", trace.join("\n"));
}

/// A cow follows a player holding wheat up a 1-block step and down the
/// other side.
#[test]
fn tempted_cow_follows_across_steps() {
    let mut h = harness(36, DVec3::new(10.5, 64.0, 0.5));
    h.fill(
        IVec3::new(3, 64, -6),
        IVec3::new(6, 64, 6),
        "minecraft:dirt",
    );
    h.hold("minecraft:wheat");
    let cow = h.adult("minecraft:cow", DVec3::new(0.5, 64.0, 0.5));
    let mut trace = Vec::new();
    for tick in 0..600 {
        if tick % 20 == 0 {
            let line = format!("t{tick}: {}", h.describe(cow));
            trace.push(line);
        }
        h.server.tick();
    }
    let feet = h.feet(cow);
    assert!(
        feet.x > 7.0 && (feet.y - 64.0).abs() < 1.0e-6,
        "cow ended at {feet:?}:\n{}",
        trace.join("\n")
    );
}
