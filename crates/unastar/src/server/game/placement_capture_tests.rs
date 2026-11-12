//! Survival block clicks compared with BDS 1.26.51
//! (`testdata/bds-placements.json`, the `placements` sweep of
//! `tools/bds-sweep`: a scripted survival player clicks with the stack in
//! hotbar slot 0 and the client records both directions).
//!
//! Each case is rebuilt in a headless server: the same arena (superflat
//! layers and the case's blocks, set with block updates), the player at the
//! same feet position and look direction holding the same stack. The
//! captured click is routed through the server as received, and the result
//! is compared with what BDS stored and sent: the placed blocks, the held
//! stack, the block corrections that follow every click, the last block the
//! client was told for every position, and the place sound.

use std::collections::BTreeMap;

use bevy_ecs::prelude::*;
use bytes::BytesMut;
use glam::{DVec3, IVec3};
use jolyne::valentine::{BorrowedMcpePacket, McpePacket, McpePacketArgs, McpePacketData};
use serde_json::Value;
use tokio::sync::mpsc::Receiver;

use super::inventory::capture::decode;
use crate::entity::PLAYER_EYE_HEIGHT;
use crate::entity::components::{GameMode, HeldSlot, MainInventory, Rotation};
use crate::entity::systems::BlockSource;
use crate::registry::BlockRegistry;
use crate::server::game::block_update::tests::{describe, state};
use crate::{GameServer, PlayerSpawnData, ServerConfig};

const FIXTURE: &str = include_str!("testdata/bds-placements.json");
const SESSION: u64 = 7;

fn cases() -> Vec<Value> {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    fixture["cases"].as_array().unwrap().clone()
}

fn ivec(value: &Value) -> IVec3 {
    IVec3::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

/// What a client was sent about blocks and sounds after one click.
#[derive(Debug, Default, PartialEq)]
struct Answer {
    /// Every `UpdateBlock` in order (position, block, flags).
    updates: Vec<(IVec3, u32, u32)>,
    /// The last block the client was told for each position.
    last: BTreeMap<(i32, i32, i32), u32>,
    /// `LevelSoundEvent`s as (name, position, data, actor unique id).
    sounds: Vec<(String, [i32; 3], i32, i64)>,
    /// Slot 0 of the last main-inventory content (item id, count), if sent.
    hand: Option<(i16, u16)>,
}

fn answer<'a>(packets: impl IntoIterator<Item = &'a McpePacketData>) -> Answer {
    let mut answer = Answer::default();
    let mut updates = Vec::new();
    for packet in packets {
        match packet {
            McpePacketData::UpdateBlockPacket(update) => {
                let pos = IVec3::new(
                    update.block_position.x,
                    update.block_position.y,
                    update.block_position.z,
                );
                updates.push((pos, update.block_runtime_id, update.flags));
                answer
                    .last
                    .insert((pos.x, pos.y, pos.z), update.block_runtime_id);
            }
            McpePacketData::UpdateSubChunkBlocksPacket(update) => {
                for block in &update.blocks_changed.blocks_changed_standards {
                    answer
                        .last
                        .insert((block.pos.x, block.pos.y, block.pos.z), block.runtime_id);
                }
            }
            McpePacketData::LevelSoundEventPacket(sound) => answer.sounds.push((
                sound.sound_event.to_string(),
                [sound.position.x, sound.position.y, sound.position.z].map(|v| (v * 2.0) as i32),
                sound.data,
                sound.actor_unique_id,
            )),
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                answer.hand = content.slots.first().map(|slot| (slot.id, slot.stacksize));
            }
            _ => {}
        }
    }
    answer.updates = updates;
    answer
}

impl Answer {
    /// The six corrections BDS sends after every click (the clicked block and
    /// the target, each with the blocks above and below it): BDS's first six
    /// updates.
    fn corrections(&self) -> &[(IVec3, u32, u32)] {
        &self.updates[..self.updates.len().min(6)]
    }

    /// Whether `run` arrives as consecutive updates. unastar sends the placed
    /// block just before the corrections and BDS just after them; the client
    /// ends up with the same blocks.
    fn has_run(&self, run: &[(IVec3, u32, u32)]) -> bool {
        run.is_empty() || self.updates.windows(run.len()).any(|window| window == run)
    }
}

/// A headless server with the case's arena and player.
struct Arena {
    server: GameServer,
    /// Commits block events as the runtime does when no plugin cancels.
    plugins: crate::plugin::PluginManager,
    player: Entity,
    rx: Receiver<McpePacket>,
}

impl Arena {
    fn world(&mut self) -> &mut World {
        self.server.ecs.world_mut()
    }

    /// One server tick, then the plugin manager's event dispatch, in the
    /// runtime's order.
    fn tick(&mut self) {
        self.server.tick();
        self.plugins.tick(self.server.ecs.world_mut());
    }

    fn new(case: &Value) -> Self {
        let origin = ivec(&case["origin"]);
        let player = &case["player"];
        let feet = origin.as_dvec3()
            + DVec3::new(
                player[0].as_f64().unwrap(),
                player[1].as_f64().unwrap(),
                player[2].as_f64().unwrap(),
            );
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            default_gamemode: GameMode::Survival,
            ..ServerConfig::default()
        });
        let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
        let entity = server
            .spawn_player(PlayerSpawnData {
                session_id: SESSION,
                display_name: "BDSSweep".into(),
                xuid: None,
                uuid: Some("00000000-0000-0000-0000-000000000007".into()),
                runtime_id: SESSION as i64,
                position: feet + DVec3::Y * PLAYER_EYE_HEIGHT,
                outbound_tx,
                saved_data: None,
                chunk_radius: 2,
            })
            .expect("player spawns");
        let mut arena = Self {
            server,
            plugins: crate::plugin::PluginManager::new().expect("plugin host"),
            player: entity,
            rx,
        };
        for _ in 0..40 {
            if arena.world().block_at(origin).is_some() {
                break;
            }
            arena.tick();
        }
        assert!(
            arena.world().block_at(origin).is_some(),
            "arena chunk loads"
        );
        // The superflat world BDS ran in, over the swept box.
        let registry = BlockRegistry::vanilla();
        let layer = |y: i32| {
            let name = match y {
                -64 => "minecraft:bedrock",
                -63 | -62 => "minecraft:dirt",
                -61 => "minecraft:grass_block",
                _ => "minecraft:air",
            };
            registry.default_state_id_by_name(name).unwrap()
        };
        for x in -6..=8 {
            for z in -6..=16 {
                for y in -64..=-50 {
                    let pos = IVec3::new(origin.x + x, y, origin.z + z);
                    super::blocks::write_block(arena.world(), pos, layer(y), false);
                }
            }
        }
        for block in case["setup"].as_array().unwrap() {
            let pos = origin + ivec(block);
            let id = state(block[3].as_str().unwrap(), &block[4]);
            super::blocks::set_block(arena.world(), pos, id, false);
        }
        let hand = &case["hand"];
        let stack = super::world_stack(
            arena.world(),
            hand["item"].as_str().unwrap(),
            hand["count"].as_u64().unwrap() as u8,
        )
        .expect("vanilla item");
        let world = arena.world();
        world
            .get_mut::<MainInventory>(entity)
            .unwrap()
            .0
            .set_item(0, stack.into_slot())
            .unwrap();
        *world.get_mut::<HeldSlot>(entity).unwrap() = HeldSlot(0);
        *world.get_mut::<GameMode>(entity).unwrap() = GameMode::Survival;
        *world.get_mut::<Rotation>(entity).unwrap() = Rotation::new(
            player[3].as_f64().unwrap() as f32,
            player[4].as_f64().unwrap() as f32,
        );
        arena.tick();
        while arena.rx.try_recv().is_ok() {}
        arena
    }

    /// Route a captured client packet as the network task does.
    fn route(&mut self, hex: &str) {
        let packet = McpePacket::from(decode(hex));
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(SESSION, McpePacketArgs, borrowed);
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }
}

/// Every captured click: same blocks, same held stack, same corrections,
/// the same final block per position and the same place sound as BDS.
#[test]
fn survival_clicks_answer_like_bds() {
    // Each case gets its own headless server; the cases run on a few worker
    // threads. The server's plugin host needs a runtime; codecs need a large
    // stack.
    let cases = cases();
    let workers = std::thread::available_parallelism().map_or(1, |n| n.get().min(6));
    let mut failures: Vec<(usize, String)> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|worker| {
                let cases = &cases;
                std::thread::Builder::new()
                    .stack_size(64 * 1024 * 1024)
                    .spawn_scoped(scope, move || {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .unwrap();
                        let _guard = runtime.enter();
                        cases
                            .iter()
                            .enumerate()
                            .skip(worker)
                            .step_by(workers)
                            .filter_map(|(index, case)| {
                                compare_case(case).map(|failure| (index, failure))
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap()
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().unwrap())
            .collect()
    });
    failures.sort();
    let failures: Vec<String> = failures.into_iter().map(|(_, failure)| failure).collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases differ:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

/// What differs between unastar's and BDS's answer to one case's click.
fn compare_case(case: &Value) -> Option<String> {
    {
        let key = case["key"].as_str().unwrap().to_owned();
        // Packets the comparison doesn't read are listed without bytes.
        let packets: Vec<(String, McpePacketData)> = case["packets"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|p| Some((p["dir"].as_str()?.to_owned(), decode(p["hex"].as_str()?))))
            .collect();
        let request = case["packets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["dir"] == "c2s" && p["name"] == "InventoryTransactionPacket")
            .expect("captured click");
        let bds = answer(
            packets
                .iter()
                .filter(|(dir, _)| dir == "s2c")
                .map(|(_, p)| p),
        );

        let mut arena = Arena::new(case);
        arena.route(request["hex"].as_str().unwrap());
        for _ in 0..4 {
            arena.tick();
        }
        let ours_packets = arena.drain();
        let ours = answer(&ours_packets);

        let origin = ivec(&case["origin"]);
        let mut problems = Vec::new();
        for read in case["bds"]["reads"].as_array().unwrap() {
            let pos = origin + ivec(&read[0]);
            let expected = state(read[1].as_str().unwrap(), &read[2]);
            let got = arena.world().block_at(pos).unwrap();
            if got != expected {
                problems.push(format!(
                    "block {pos}: got {} want {}",
                    describe(got),
                    describe(expected)
                ));
            }
        }
        let player = arena.player;
        let held = arena
            .world()
            .get::<MainInventory>(player)
            .unwrap()
            .0
            .item(0)
            .map_or(0, |stack| stack.count);
        let bds_held = case["bds"]["held"][1].as_u64().unwrap_or(0) as u8;
        if held != bds_held {
            problems.push(format!("held {held} want {bds_held}"));
        }
        if !ours.has_run(bds.corrections()) {
            problems.push(format!(
                "updates {:?} lack the corrections {:?}",
                ours.updates,
                bds.corrections()
            ));
        }
        let told: BTreeMap<_, _> = bds
            .last
            .keys()
            .map(|pos| (*pos, ours.last.get(pos).copied()))
            .collect();
        let bds_told: BTreeMap<_, _> = bds.last.iter().map(|(pos, id)| (*pos, Some(*id))).collect();
        if told != bds_told {
            problems.push(format!("client view {told:?} want {bds_told:?}"));
        }
        if ours.sounds != bds.sounds {
            problems.push(format!("sounds {:?} want {:?}", ours.sounds, bds.sounds));
        }
        if ours.hand != bds.hand {
            problems.push(format!("hand resync {:?} want {:?}", ours.hand, bds.hand));
        }
        (!problems.is_empty()).then(|| format!("{key}:\n    {}", problems.join("\n    ")))
    }
}

#[test]
#[ignore = "prints a local capture"]
fn dump_placement_capture() {
    let path = std::env::var("PLACEMENT_CAPTURE").unwrap();
    let text = std::fs::read_to_string(path).unwrap();
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let name = row["name"].as_str().unwrap();
        if matches!(
            name,
            "PlayerAuthInputPacket" | "LevelChunkPacket" | "NetworkStackLatencyPacket"
        ) {
            continue;
        }
        let text = format!("{:?}", decode(row["hex"].as_str().unwrap()));
        println!("  {} {name} {}", row["dir"], &text[..text.len().min(700)]);
    }
}
