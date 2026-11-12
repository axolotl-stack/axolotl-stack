//! Survival block clicks compared with BDS 1.26.51
//! (`testdata/bds-placements*.json`, the `placements` sweep of
//! `tools/bds-sweep`: a scripted survival player clicks with the stack in
//! hotbar slot 0 and the client records both directions; a second scripted
//! player standing outside the arena records what other players are sent).
//!
//! Each case is rebuilt in a headless server: the same arena (superflat
//! layers and the case's blocks, set with block updates), the player at the
//! same feet position and look direction holding the same stack, and the
//! watching player. The captured client packets (clicks, sign edits) are
//! routed through the server as received, at the server tick BDS received
//! them, and the result is compared with what BDS stored and sent: the
//! blocks, the held stack, the block corrections that follow a click, the
//! last block each client was told for every position, the sounds and level
//! events, the last block-actor data per position and the sign editors
//! opened.

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

const SESSION: u64 = 7;
const WATCHER: u64 = 8;

/// Every case of every fixture file (`testdata/bds-placements*.json`, one
/// file per case group of `tools/bds-sweep/placements.py`), with the group.
/// The `pistons` and `tnt` groups are not placements: their lever clicks
/// drive pistons and TNT that `redstone/piston_packets.rs` and
/// `explosion/tests.rs` replay in a live world.
fn cases() -> Vec<Value> {
    let dir = format!("{}/src/server/game/testdata", env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    name.starts_with("bds-placements")
                        && name.ends_with(".json")
                        && !matches!(
                            name,
                            "bds-placements-pistons.json" | "bds-placements-tnt.json"
                        )
                })
        })
        .collect();
    files.sort();
    files
        .iter()
        .flat_map(|path| {
            let fixture: Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            fixture["cases"].as_array().unwrap().clone()
        })
        .collect()
}

fn ivec(value: &Value) -> IVec3 {
    IVec3::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

fn dvec(value: &Value) -> DVec3 {
    DVec3::new(
        value[0].as_f64().unwrap(),
        value[1].as_f64().unwrap(),
        value[2].as_f64().unwrap(),
    )
}

/// What a client was sent about blocks, sounds and block actors.
#[derive(Debug, Default, PartialEq)]
struct Answer {
    /// Every `UpdateBlock` in order (position, block, flags).
    updates: Vec<(IVec3, u32, u32)>,
    /// The last block the client was told for each position.
    last: BTreeMap<(i32, i32, i32), u32>,
    /// `LevelSoundEvent`s as (name, position x2, data, actor unique id).
    sounds: Vec<(String, [i32; 3], i32, i64)>,
    /// `LevelEvent`s as (event, position x2, data).
    events: Vec<(i32, [i32; 3], i32)>,
    /// The last block-actor data per position, as sorted `key=value` text
    /// with an editing lock other than none written as `player`.
    actors: BTreeMap<(i32, i32, i32), String>,
    /// `OpenSign`s as (position, front side).
    open_signs: Vec<((i32, i32, i32), bool)>,
    /// Slot 0 of the last main-inventory content (item id, count), if sent.
    hand: Option<(i16, u16)>,
}

fn answer<'a>(packets: impl IntoIterator<Item = &'a McpePacketData>) -> Answer {
    let mut answer = Answer::default();
    for packet in packets {
        match packet {
            // BDS re-sends a block whose actor changed with flags 19; the
            // client already has that block.
            McpePacketData::UpdateBlockPacket(update) if update.flags == 19 => {}
            McpePacketData::UpdateBlockPacket(update) => {
                let pos = IVec3::new(
                    update.block_position.x,
                    update.block_position.y,
                    update.block_position.z,
                );
                answer
                    .updates
                    .push((pos, update.block_runtime_id, update.flags));
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
            // BDS and unastar number actors differently (BDS's clicking
            // player is -4294967295): only whether a sound comes from an
            // actor (a player's burp) is compared.
            McpePacketData::LevelSoundEventPacket(sound) => answer.sounds.push((
                sound.sound_event.to_string(),
                [sound.position.x, sound.position.y, sound.position.z].map(|v| (v * 2.0) as i32),
                sound.data,
                if sound.actor_unique_id == -1 { -1 } else { 0 },
            )),
            McpePacketData::LevelEventPacket(event) => answer.events.push((
                event.event_id,
                [event.position.x, event.position.y, event.position.z].map(|v| (v * 2.0) as i32),
                event.data,
            )),
            McpePacketData::BlockActorDataPacket(data) => {
                let pos = &data.block_position;
                // Which player holds a sign's editor: any id but none (-1).
                let text = nbt_text(&data.actor_data_tags);
                let text = match text.split_once("LockedForEditingBy=Long(Long(") {
                    Some((head, tail)) if !tail.starts_with("-1)") => {
                        let rest = tail.split_once("))").map_or("", |(_, rest)| rest);
                        format!("{head}LockedForEditingBy=player{rest}")
                    }
                    _ => text,
                };
                answer.actors.insert((pos.x, pos.y, pos.z), text);
            }
            McpePacketData::OpenSignPacket(open) => answer
                .open_signs
                .push(((open.pos.x, open.pos.y, open.pos.z), open.is_front_side)),
            McpePacketData::InventoryContentPacket(content) if content.container_id == 0 => {
                answer.hand = content.slots.first().map(|slot| (slot.id, slot.stacksize));
            }
            _ => {}
        }
    }
    answer
}

impl Answer {
    /// The corrections BDS sends after a block click (the clicked block and
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

    /// Where this answer differs from BDS's `bds` for one client: the last
    /// block told per position BDS told, sounds, level events, actor data
    /// and sign editors.
    fn differences(&self, bds: &Answer, who: &str, problems: &mut Vec<String>) {
        let told: BTreeMap<_, _> = bds
            .last
            .keys()
            .map(|pos| (*pos, self.last.get(pos).copied()))
            .collect();
        let bds_told: BTreeMap<_, _> = bds.last.iter().map(|(pos, id)| (*pos, Some(*id))).collect();
        if told != bds_told {
            problems.push(format!("{who} view {told:?} want {bds_told:?}"));
        }
        if self.sounds != bds.sounds {
            problems.push(format!(
                "{who} sounds {:?} want {:?}",
                self.sounds, bds.sounds
            ));
        }
        if self.events != bds.events {
            problems.push(format!(
                "{who} level events {:?} want {:?}",
                self.events, bds.events
            ));
        }
        // Block actors BDS announced (unastar also announces some containers
        // BDS does not, which is not compared).
        let actors: BTreeMap<_, _> = bds
            .actors
            .keys()
            .map(|pos| (*pos, self.actors.get(pos)))
            .collect();
        let bds_actors: BTreeMap<_, _> = bds
            .actors
            .iter()
            .map(|(pos, text)| (*pos, Some(text)))
            .collect();
        if actors != bds_actors {
            problems.push(format!("{who} actors {actors:?} want {bds_actors:?}"));
        }
        if self.open_signs != bds.open_signs {
            problems.push(format!(
                "{who} sign editors {:?} want {:?}",
                self.open_signs, bds.open_signs
            ));
        }
    }
}

/// A headless server with the case's arena, player and watcher.
struct Arena {
    server: GameServer,
    /// Commits block events as the runtime does when no plugin cancels.
    plugins: crate::plugin::PluginManager,
    player: Entity,
    rx: Receiver<McpePacket>,
    watcher: Option<Entity>,
    watcher_rx: Option<Receiver<McpePacket>>,
}

fn spawn(
    server: &mut GameServer,
    session: u64,
    name: &str,
    feet: DVec3,
) -> (Entity, Receiver<McpePacket>) {
    let (outbound_tx, rx) = tokio::sync::mpsc::channel(1 << 16);
    let entity = server
        .spawn_player(PlayerSpawnData {
            session_id: session,
            display_name: name.into(),
            xuid: None,
            uuid: Some(format!("00000000-0000-0000-0000-00000000000{session}")),
            runtime_id: session as i64,
            position: feet + DVec3::Y * PLAYER_EYE_HEIGHT,
            outbound_tx,
            saved_data: None,
            chunk_radius: 2,
        })
        .expect("player spawns");
    (entity, rx)
}

/// The seed of every arena's entity random. Seeded from entropy, the
/// candle a candle cake drops (`candle_onto_candle_cake`,
/// `candle_cake_empty_hand`, `lit_candle_cake_with_candle`) landed within
/// the clicking player's reach as its pickup delay ran out for 5 of 59
/// seeds tried, so a different case failed now and then.
const ARENA_SEED: u64 = 1;

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
        let feet = origin.as_dvec3() + dvec(player);
        // The sweep world was peaceful (`tools/bds-sweep/sweep.py`) unless a
        // case's `before` steps change the difficulty.
        let normal = case["before"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|step| step[1] == "difficulty normal");
        let difficulty = if normal {
            crate::world::rules::Difficulty::Normal
        } else {
            crate::world::rules::Difficulty::Peaceful
        };
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            default_gamemode: GameMode::Survival,
            difficulty,
            ..ServerConfig::default()
        });
        // Item drops pop with a random offset and velocity: a fixed seed
        // keeps a case from depending on where a drop lands.
        server
            .ecs
            .world_mut()
            .insert_resource(crate::entity::EntityRandom::seeded(ARENA_SEED));
        let (entity, rx) = spawn(&mut server, SESSION, "BDSSweep", feet);
        let watcher = case.get("watcher_packets").map(|_| {
            let spot = origin.as_dvec3() + dvec(&case["watcher"]);
            spawn(&mut server, WATCHER, "BDSWatch", spot)
        });
        let mut arena = Self {
            server,
            plugins: crate::plugin::PluginManager::new().expect("plugin host"),
            player: entity,
            rx,
            watcher: None,
            watcher_rx: None,
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
        // The sweep world ran with random ticks off (`tools/bds-sweep/sweep.py`):
        // grass under a case's blocks would otherwise turn to dirt.
        arena
            .world()
            .resource_mut::<crate::world::rules::GameRules>()
            .set_value(
                "randomtickspeed",
                unastar_data::game_rules::GameRuleValue::Int(0),
            )
            .expect("randomtickspeed is a game rule");
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
        arena.give(entity, &case["hand"]);
        if let Some((watcher, watcher_rx)) = watcher {
            arena.give(watcher, &case["watcher_hand"]);
            let spot = &case["watcher"];
            *arena.world().get_mut::<Rotation>(watcher).unwrap() = Rotation::new(
                spot[3].as_f64().unwrap() as f32,
                spot[4].as_f64().unwrap() as f32,
            );
            arena.watcher = Some(watcher);
            arena.watcher_rx = Some(watcher_rx);
        }
        let world = arena.world();
        *world.get_mut::<Rotation>(entity).unwrap() = Rotation::new(
            player[3].as_f64().unwrap() as f32,
            player[4].as_f64().unwrap() as f32,
        );
        arena.tick();
        arena.drain();
        arena.drain_watcher();
        arena
    }

    /// Put `hand` (`{item, count}` or null for an empty hand) into hotbar
    /// slot 0 of `player`, selected, in survival.
    fn give(&mut self, player: Entity, hand: &Value) {
        let world = self.world();
        if let Some(item) = hand.get("item").and_then(Value::as_str) {
            let count = hand["count"].as_u64().unwrap() as u8;
            let stack = super::world_stack(world, item, count).expect("vanilla item");
            world
                .get_mut::<MainInventory>(player)
                .unwrap()
                .0
                .set_item(0, stack.into_slot())
                .unwrap();
        } else {
            world.get_mut::<MainInventory>(player).unwrap().0.clear();
        }
        *world.get_mut::<HeldSlot>(player).unwrap() = HeldSlot(0);
        *world.get_mut::<GameMode>(player).unwrap() = GameMode::Survival;
    }

    /// Route a captured client packet as the network task does.
    fn route(&mut self, session: u64, hex: &str) {
        let packet = McpePacket::from(decode(hex));
        let mut buf = BytesMut::new();
        packet.data.encode_inner(&mut buf, 0, 0).unwrap();
        let borrowed = BorrowedMcpePacket::decode_inner(&mut buf.freeze()).unwrap();
        self.server.route_packet(session, McpePacketArgs, borrowed);
    }

    fn drain(&mut self) -> Vec<McpePacketData> {
        std::iter::from_fn(|| self.rx.try_recv().ok().map(|packet| packet.data)).collect()
    }

    fn drain_watcher(&mut self) -> Vec<McpePacketData> {
        let Some(rx) = self.watcher_rx.as_mut() else {
            return Vec::new();
        };
        std::iter::from_fn(|| rx.try_recv().ok().map(|packet| packet.data)).collect()
    }
}

/// Every captured case: same blocks, same held stack, same corrections and
/// the same client view, sounds, level events, block actors and sign
/// editors as BDS, for the clicking and the watching player.
#[test]
fn survival_clicks_answer_like_bds() {
    let failures = compare_cases(|_| true);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Runs the cases `keep` selects on a few worker threads (one server each;
/// the plugin host needs a runtime, the codecs a large stack) and returns
/// the differences, with a count line first.
fn compare_cases(keep: impl Fn(&Value) -> bool + Sync) -> Vec<String> {
    let cases: Vec<Value> = cases().into_iter().filter(|case| keep(case)).collect();
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
    let mut out: Vec<String> = failures.into_iter().map(|(_, failure)| failure).collect();
    if !out.is_empty() {
        out.insert(0, format!("{} of {} cases differ:", out.len(), cases.len()));
    }
    out
}

/// The client packets a case routes, in capture order: (session, hex).
fn client_packets(packets: &Value, session: u64) -> Vec<(u64, String)> {
    packets
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["dir"] == "c2s"
                && matches!(
                    p["name"].as_str(),
                    Some("InventoryTransactionPacket" | "BlockActorDataPacket")
                )
        })
        .filter_map(|p| Some((session, p["hex"].as_str()?.to_owned())))
        .collect()
}

fn decoded(packets: &Value, direction: &str) -> Vec<McpePacketData> {
    packets
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["dir"] == direction)
        .filter_map(|p| Some(decode(p["hex"].as_str()?)))
        .collect()
}

/// Comparisons left out of a case, and why: outcomes BDS draws at random
/// and behaviour unastar does not model yet (these rows keep gaps).
#[derive(Debug, Default, Clone, Copy)]
struct Skip {
    /// The whole case.
    case: bool,
    /// The first read (the later read is compared).
    first_read: bool,
    sounds: bool,
    actors: bool,
}

fn skipped(key: &str) -> Skip {
    let none = Skip::default();
    match key {
        // Two coral timing samples died to the check the case before them
        // left pending on the same spot; the replay starts each case afresh
        // (`placed_coral_blocks_die_within_the_measured_ticks`).
        "coral_block_dies_20_1" | "coral_block_dies_40_0" => Skip { case: true, ..none },
        // Cave vines get a random age; a campfire crackles at random.
        _ if key.starts_with("look_campfire_") => Skip {
            sounds: true,
            ..none
        },
        // BDS announces a crafter with a partial actor (no id or position).
        _ if key.starts_with("look_crafter_") => Skip {
            actors: true,
            ..none
        },
        // Mushrooms stay where light is low; unastar has no light levels.
        _ if key.starts_with("brown_mushroom_on_") => Skip { case: true, ..none },
        // The banner stack BDS gives carries item user data unastar's stack
        // lacks, so the click does not name the held stack.
        _ if key.starts_with("banner_") => Skip { case: true, ..none },
        // Lighting with flint and steel answers without the block corrections
        // (item use on blocks, lane ITEMS).
        _ if key.starts_with("flint_and_steel_on_") => Skip { case: true, ..none },
        // Cave vines get a random age, so the corrections differ too; the
        // ages are compared by distribution
        // (`placed_cave_vines_get_the_measured_ages`).
        "glow_berries_under_stone" => Skip { case: true, ..none },
        _ if key.starts_with("cave_vines_age_") || key.starts_with("cave_vines_under_vine_") => {
            Skip { case: true, ..none }
        }
        // The hungry player's food before the click is not in the capture
        // (`cake_slices_feed_as_measured`).
        "cake_hungry_normal" => Skip { case: true, ..none },
        // BDS plays no sound for a lily pad on ice.
        "waterlily_on_ice" => Skip {
            sounds: true,
            ..none
        },
        _ => none,
    }
}

/// What differs between unastar's and BDS's answer to one case.
fn compare_case(case: &Value) -> Option<String> {
    let key = case["key"].as_str().unwrap().to_owned();
    let skip = skipped(&key);
    if skip.case {
        return None;
    }
    let bds = &case["bds"];
    let bds_clicker = answer(&decoded(&case["packets"], "s2c"));
    let bds_watcher = case
        .get("watcher_packets")
        .map(|packets| answer(&decoded(packets, "s2c")));

    let mut arena = Arena::new(case);
    // Each recorded step (tick relative to the first click, who, kind) routes
    // that client's next captured packet.
    let mut queues = [
        client_packets(&case["packets"], SESSION).into_iter(),
        client_packets(&case["watcher_packets"], WATCHER).into_iter(),
    ];
    let mut timeline = bds["timeline"]
        .as_array()
        .cloned()
        // Round 1's cases: one click.
        .unwrap_or_else(|| vec![serde_json::json!([0, "player", "use"])])
        .into_iter();
    let steps = std::iter::once(serde_json::json!(["use", "player"]))
        .chain(case["after"].as_array().cloned().unwrap_or_default());
    let mut tick = 0;
    let mut ours = Vec::new();
    let mut ours_watcher = Vec::new();
    for step in steps {
        let who = usize::from(step[1] == "watcher");
        match step[0].as_str().unwrap() {
            "wait" => continue,
            "hand" => {
                // `/replaceitem` between clicks; BDS sends the inventory.
                let player = if who == 0 {
                    arena.player
                } else {
                    arena.watcher.unwrap()
                };
                let hand = if step[2].is_null() {
                    Value::Null
                } else {
                    serde_json::json!({"item": step[2], "count": step[3]})
                };
                arena.give(player, &hand);
                super::inventory::resync_inventory(arena.world(), player);
                continue;
            }
            _ => {}
        }
        let at = timeline.next().expect("timeline step")[0].as_u64().unwrap();
        while tick < at {
            arena.tick();
            tick += 1;
        }
        let Some((session, hex)) = queues[who].next() else {
            return Some(format!("{key}: no captured packet for step at {at}"));
        };
        arena.route(session, &hex);
        // The server handles a click in the tick after it arrives.
        arena.tick();
        tick += 1;
    }
    // Round 1's cases were read 4 ticks after the click.
    let read_tick = bds["read_tick"].as_u64().unwrap_or(4).max(tick);
    while tick < read_tick {
        arena.tick();
        tick += 1;
    }
    let origin = ivec(&case["origin"]);
    let mut problems = Vec::new();
    let compare_reads =
        |arena: &mut Arena, reads: &Value, label: &str, problems: &mut Vec<String>| {
            for read in reads.as_array().into_iter().flatten() {
                let pos = origin + ivec(&read[0]);
                let expected = state(read[1].as_str().unwrap(), &read[2]);
                let got = arena.world().block_at(pos).unwrap();
                if got != expected {
                    problems.push(format!(
                        "{label} block {pos}: got {} want {}",
                        describe(got),
                        describe(expected)
                    ));
                }
            }
        };
    if !skip.first_read {
        compare_reads(&mut arena, &bds["reads"], "", &mut problems);
    }
    let player = arena.player;
    let held = arena
        .world()
        .get::<MainInventory>(player)
        .unwrap()
        .0
        .item(0)
        .map_or(0, |stack| stack.count);
    let bds_held = bds["held"][1].as_u64().unwrap_or(0) as u8;
    if held != bds_held {
        problems.push(format!("held {held} want {bds_held}"));
    }
    if let Some(later) = bds["read_later_tick"].as_u64() {
        // BDS sends a tick's block changes after the chat marker that ends
        // the case's capture: what happens in the last ticks before the late
        // read is not in the capture (a button released 2 ticks before it,
        // the click itself reaching BDS a tick after it was sent).
        while tick + 3 < later {
            arena.tick();
            tick += 1;
        }
        ours.extend(arena.drain());
        ours_watcher.extend(arena.drain_watcher());
        while tick < later {
            arena.tick();
            tick += 1;
        }
        arena.drain();
        arena.drain_watcher();
        compare_reads(&mut arena, &bds["reads_later"], "later", &mut problems);
    }
    ours.extend(arena.drain());
    ours_watcher.extend(arena.drain_watcher());
    let ours = answer(&ours);
    if !ours.has_run(bds_clicker.corrections()) {
        problems.push(format!(
            "updates {:?} lack the corrections {:?}",
            ours.updates,
            bds_clicker.corrections()
        ));
    }
    let (mut bds_clicker, mut ours) = (bds_clicker, ours);
    let mut bds_watcher = bds_watcher;
    let mut ours_watcher = answer(&ours_watcher);
    for answer in [&mut bds_clicker, &mut ours, &mut ours_watcher]
        .into_iter()
        .chain(bds_watcher.as_mut())
    {
        if skip.sounds {
            answer.sounds.clear();
        }
        if skip.actors {
            answer.actors.clear();
        }
    }
    ours.differences(&bds_clicker, "player", &mut problems);
    if bds_clicker.hand.is_some() && ours.hand != bds_clicker.hand {
        problems.push(format!(
            "hand resync {:?} want {:?}",
            ours.hand, bds_clicker.hand
        ));
    }
    if let Some(bds_watcher) = &bds_watcher {
        ours_watcher.differences(bds_watcher, "watcher", &mut problems);
    }
    (!problems.is_empty()).then(|| format!("{key}:\n    {}", problems.join("\n    ")))
}

/// Round 2's player-placed coral blocks (`coral_block_dies_*`, read at tick
/// 10 and again later, cases run back to back on one spot): each was dead by
/// tick 60, as the 45-tick check gives. The two found dead by tick 10 were
/// placed about 45 ticks after the case before them, whose block's check
/// was still pending on the same spot; every other one was alive at its
/// first read.
#[test]
fn placed_coral_blocks_die_within_the_measured_ticks() {
    let mut deaths = Vec::new();
    for case in cases() {
        let key = case["key"].as_str().unwrap();
        if !key.starts_with("coral_block_dies_") {
            continue;
        }
        let bds = &case["bds"];
        let dead = |reads: &Value| reads[0][1].as_str().unwrap().contains("dead_");
        let (first, later) = (
            bds["read_tick"].as_u64().unwrap(),
            bds["read_later_tick"].as_u64().unwrap(),
        );
        // The tick window the death fell in (or after `later` if alive).
        deaths.push(if dead(&bds["reads"]) {
            (0, first)
        } else if dead(&bds["reads_later"]) {
            (first, later)
        } else {
            (later, u64::MAX)
        });
    }
    assert_eq!(deaths.len(), 12);
    let check = super::block_use::CORAL_PLACED_TICKS;
    assert!(deaths.iter().all(|&(after, _)| after <= 60), "{deaths:?}");
    // Alive at a read before the check, dead at a read after it.
    let early = deaths.iter().filter(|&&(_, by)| by <= 10).count();
    assert_eq!(early, 2, "{deaths:?}");
    assert!(
        deaths
            .iter()
            .filter(|&&(_, by)| by > 10)
            .all(|&(after, by)| after < check + 4 && by >= check),
        "{deaths:?}"
    );
}

/// A synthetic case for [`Arena::new`]: the superflat floor at the round 2
/// origin, the player north of it with an empty hand.
fn bare_case() -> Value {
    serde_json::json!({
        "origin": [4, -60, 4],
        "player": [0.5, 0, -1.5, 0, 35],
        "setup": [],
        "hand": null,
    })
}

/// The coral rows BDS 1.26.51 read every tick (`block_updates/coral_timing.json`):
/// [kind, cell, repeat, trace of [tick, block]].
fn coral_timing() -> Vec<Value> {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../unastar-data/data/measured/block_updates/coral_timing.json"
    ))
    .unwrap();
    let file: Value = serde_json::from_str(&text).unwrap();
    file["rows"].as_array().unwrap().clone()
}

/// Ticks from the first read of a trace showing `block` (or tick 0) to the
/// first read showing it dead.
fn death_delay(trace: &[Value]) -> Option<u64> {
    let tick = |entry: &Value| entry[0].as_u64().unwrap();
    let placed = trace
        .iter()
        .find(|entry| {
            let name = entry[1].as_str().unwrap();
            name.contains("coral") && !name.contains("dead_")
        })
        .map(&tick)?;
    let dead = trace
        .iter()
        .find(|entry| entry[1].as_str().unwrap().contains("dead_"))
        .map(&tick)?;
    Some(dead - placed)
}

/// A live coral block placed out of water (set with a block update,
/// `/setblock` or a click) dies at BDS's tick, and one whose water is removed
/// dies within BDS's spread.
#[test]
fn coral_blocks_die_at_the_measured_ticks() {
    let rows = coral_timing();
    let delays = |cell: &str| -> Vec<u64> {
        rows.iter()
            .filter(|row| row[1] == cell)
            .map(|row| death_delay(row[3].as_array().unwrap()).expect("died"))
            .collect()
    };
    for cell in ["block_set", "block_setblock", "block_player"] {
        let found = delays(cell);
        assert!(!found.is_empty(), "{cell}");
        assert!(
            found
                .iter()
                .all(|&delay| delay == super::block_use::CORAL_PLACED_TICKS),
            "{cell}: {found:?}"
        );
    }
    let dry = delays("block_water_goes");
    assert_eq!(dry.len(), 15);
    let range = super::block_use::CORAL_DRY_TICKS;
    assert_eq!(
        (dry.iter().min().copied(), dry.iter().max().copied()),
        (Some(*range.start()), Some(*range.end()))
    );

    // unastar (the plugin host needs a runtime, the codecs a large stack).
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(replay_coral_deaths)
        .unwrap()
        .join()
        .unwrap();
}

fn replay_coral_deaths() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = runtime.enter();
    // A coral block set in air dies 45 ticks later.
    let registry = BlockRegistry::vanilla();
    let coral = registry
        .default_state_id_by_name("minecraft:brain_coral_block")
        .unwrap();
    let dead = registry
        .default_state_id_by_name("minecraft:dead_brain_coral_block")
        .unwrap();
    let mut arena = Arena::new(&bare_case());
    let at = IVec3::new(4, -60, 4);
    super::blocks::set_block(arena.world(), at, coral, false);
    let mut ticks = 0;
    while arena.world().block_at(at) == Some(coral) && ticks < 400 {
        arena.tick();
        ticks += 1;
    }
    assert_eq!(arena.world().block_at(at), Some(dead));
    assert_eq!(ticks, super::block_use::CORAL_PLACED_TICKS);

    // Beside walled-in water it lives; with the water gone it dies within
    // the measured spread.
    let mut arena = Arena::new(&bare_case());
    let stone = registry
        .default_state_id_by_name("minecraft:stone")
        .unwrap();
    let water = registry
        .default_state_id_by_name("minecraft:water")
        .unwrap();
    let air = registry.default_state_id_by_name("minecraft:air").unwrap();
    let west = at - IVec3::X;
    for wall in [west - IVec3::Z, west + IVec3::Z, west - IVec3::X] {
        super::blocks::write_block(arena.world(), wall, stone, false);
    }
    super::blocks::write_block(arena.world(), west, water, false);
    super::blocks::set_block(arena.world(), at, coral, false);
    for _ in 0..100 {
        arena.tick();
    }
    assert_eq!(arena.world().block_at(at), Some(coral));
    super::blocks::set_block(arena.world(), west, air, false);
    let mut ticks = 0;
    while arena.world().block_at(at) == Some(coral) && ticks < 400 {
        arena.tick();
        ticks += 1;
    }
    assert_eq!(arena.world().block_at(at), Some(dead));
    assert!(
        super::block_use::CORAL_DRY_TICKS.contains(&ticks),
        "{ticks}"
    );
}

/// BDS's ages of placed cave vines (100 placements under stone, 10 under a
/// vine and round 2's one) against [`super::placement::cave_vine_age`]:
/// none below 1 or above 25, and the counts in the quarters 1-8, 9-16,
/// 17-24 and 25 (a quarter each under the rule) pass a chi-square test at
/// p = 0.001 (3 degrees of freedom: 16.27).
#[test]
fn placed_cave_vines_get_the_measured_ages() {
    use rand::SeedableRng;
    let mut ages = Vec::new();
    for case in cases() {
        let key = case["key"].as_str().unwrap();
        if key.starts_with("cave_vines_age_")
            || key.starts_with("cave_vines_under_vine_")
            || key == "glow_berries_under_stone"
        {
            let read = &case["bds"]["reads"][0];
            assert_eq!(read[1], "minecraft:cave_vines", "{key}");
            ages.push(read[2]["growing_plant_age"].as_i64().unwrap() as i32);
        }
    }
    assert_eq!(ages.len(), 111);
    assert!(ages.iter().all(|age| (1..=25).contains(age)), "{ages:?}");
    let quarter = |age: i32| ((age - 1) / 8).min(3) as usize;
    let mut bds = [0.0f64; 4];
    for age in &ages {
        bds[quarter(*age)] += 1.0;
    }
    let expected = ages.len() as f64 / 4.0;
    let chi: f64 = bds.iter().map(|o| (o - expected).powi(2) / expected).sum();
    assert!(chi < 16.27, "BDS quarters {bds:?}, chi-square {chi}");
    // unastar's draw gives the same quarters.
    let mut rng = rand::rngs::StdRng::seed_from_u64(7);
    let mut ours = [0.0f64; 4];
    for _ in 0..32_000 {
        let age = super::placement::cave_vine_age(&mut rng);
        assert!((1..=25).contains(&age));
        ours[quarter(age)] += 1.0;
    }
    assert!(
        ours.iter().all(|n| (n / 8_000.0 - 1.0).abs() < 0.05),
        "{ours:?}"
    );
}

/// One cake slice takes a player from BDS's food and saturation after the
/// first slice to BDS's after the second (`cake_hungry_normal`: hunger 12
/// and saturation 0.4, then 14 and 0.8).
#[test]
fn cake_slices_feed_as_measured() {
    let case = cases()
        .into_iter()
        .find(|case| case["key"] == "cake_hungry_normal")
        .unwrap();
    let mut seen = Vec::new();
    for packet in decoded(&case["packets"], "s2c") {
        if let McpePacketData::UpdateAttributesPacket(update) = packet {
            let value = |name: &str| {
                update
                    .attribute_list
                    .iter()
                    .find(|attribute| attribute.name == name)
                    .map(|attribute| attribute.current_value)
            };
            if let (Some(food), Some(saturation)) = (
                value("minecraft:player.hunger"),
                value("minecraft:player.saturation"),
            ) {
                seen.push((food, saturation));
            }
        }
    }
    assert_eq!(seen.len(), 2, "{seen:?}");
    let mut hunger = crate::entity::components::Hunger {
        food_level: seen[0].0 as i32,
        saturation: seen[0].1,
        exhaustion: 0.0,
    };
    let (food, modifier) = super::block_use::CAKE_SLICE_FOOD;
    hunger.eat(food, modifier);
    assert_eq!(hunger.food_level, seen[1].0 as i32);
    assert!(
        (hunger.saturation - seen[1].1).abs() < 1e-4,
        "{hunger:?} {seen:?}"
    );
}

/// One case group at a time (`PLACEMENT_GROUP`, a fixture's group name, or
/// a key prefix in `PLACEMENT_CASE`), for working on a family.
#[test]
#[ignore = "runs the cases PLACEMENT_GROUP / PLACEMENT_CASE select"]
fn selected_clicks_answer_like_bds() {
    let prefix = std::env::var("PLACEMENT_CASE").unwrap_or_default();
    let failures = compare_cases(|case| case["key"].as_str().unwrap().starts_with(&prefix));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Prints the decoded packets of the fixture cases whose key contains
/// `PLACEMENT_CASE`.
#[test]
#[ignore = "prints fixture cases"]
fn dump_fixture_cases() {
    let wanted = std::env::var("PLACEMENT_CASE").unwrap_or_default();
    for case in cases() {
        let key = case["key"].as_str().unwrap();
        if !key.contains(&wanted) {
            continue;
        }
        println!("== {key}");
        for (who, packets) in [
            ("clicker", &case["packets"]),
            ("watcher", &case["watcher_packets"]),
        ] {
            for packet in packets.as_array().into_iter().flatten() {
                let name = packet["name"].as_str().unwrap();
                let Some(hex) = packet["hex"].as_str() else {
                    println!("  {who} {} {name}", packet["dir"]);
                    continue;
                };
                let decoded = decode(hex);
                let text = match &decoded {
                    McpePacketData::BlockActorDataPacket(data) => format!(
                        "{:?} {}",
                        data.block_position,
                        nbt_text(&data.actor_data_tags)
                    ),
                    other => format!("{other:?}"),
                };
                println!(
                    "  {who} {} {name} {}",
                    packet["dir"],
                    &text[..text.len().min(1500)]
                );
            }
        }
    }
}

/// Network NBT as sorted `key=value` text (for the dumps).
fn nbt_text(nbt: &jolyne::valentine::bedrock::codec::Nbt) -> String {
    fn tag(value: &zuri_nbt::NBTTag) -> String {
        match value {
            zuri_nbt::NBTTag::Compound(compound) => {
                let mut entries: Vec<_> = compound.0.iter().collect();
                entries.sort_by(|a, b| a.0.cmp(b.0));
                let inner: Vec<String> = entries
                    .into_iter()
                    .map(|(key, value)| format!("{key}={}", tag(value)))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            zuri_nbt::NBTTag::String(text) => format!("{:?}", text.0),
            other => format!("{other:?}"),
        }
    }
    match crate::world::block_entity::BlockEntity::from_network_nbt(nbt.clone())
        .ok()
        .and_then(|actor| actor.bedrock_compound())
    {
        Some(Ok(compound)) => tag(&zuri_nbt::NBTTag::Compound(compound)),
        _ => format!("{nbt:?}"),
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

/// The block actor BDS 1.26.51 announces right after a player places each
/// block (the first `BlockActorData` for the placed position in a
/// placement capture), keyed by block; signs (locked for their placer) are
/// left out. Written to `data/measured/block_actor_defaults.json` by
/// `write_block_actor_defaults`.
fn measured_block_actor_defaults() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for case in cases() {
        let reads = &case["bds"]["reads"];
        let Some(first) = reads.as_array().and_then(|reads| reads.first()) else {
            continue;
        };
        let block = first[1].as_str().unwrap();
        let key = case["key"].as_str().unwrap();
        if block == "minecraft:air"
            || block.ends_with("_sign")
            || case.get("after").is_some()
            || key.starts_with("flower_pot_with_")
            || key.starts_with("pot_")
        {
            continue;
        }
        let placed = ivec(&case["origin"]) + ivec(&first[0]);
        let actor = case["packets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["dir"] == "s2c" && p["name"] == "BlockActorDataPacket")
            .filter_map(|p| match decode(p["hex"].as_str()?) {
                McpePacketData::BlockActorDataPacket(data)
                    if IVec3::new(
                        data.block_position.x,
                        data.block_position.y,
                        data.block_position.z,
                    ) == placed =>
                {
                    Some(data.actor_data_tags)
                }
                _ => None,
            })
            .next();
        let Some(actor) = actor else { continue };
        // Kept as BDS sent it; the position is rewritten where it is used.
        let hex: String = actor.0.iter().map(|b| format!("{b:02x}")).collect();
        out.entry(block.to_owned()).or_insert(hex);
    }
    out
}

const BLOCK_ACTOR_DEFAULTS: &str = "../unastar-data/data/measured/block_actor_defaults.json";

#[test]
#[ignore = "writes data/measured/block_actor_defaults.json from the placement fixtures"]
fn write_block_actor_defaults() {
    let table = measured_block_actor_defaults();
    let json = serde_json::json!({
        "method": "the first BlockActorData BDS 1.26.51 sent for the placed position after a survival \
                   placement (crates/unastar/src/server/game/testdata/bds-placements*.json), as network \
                   NBT in hex; signs are left out",
        "actors": table,
    });
    let path = format!("{}/{BLOCK_ACTOR_DEFAULTS}", env!("CARGO_MANIFEST_DIR"));
    std::fs::write(path, serde_json::to_string_pretty(&json).unwrap() + "\n").unwrap();
}

/// The committed table is what the fixtures give.
#[test]
fn block_actor_defaults_match_the_placement_captures() {
    let path = format!("{}/{BLOCK_ACTOR_DEFAULTS}", env!("CARGO_MANIFEST_DIR"));
    let committed: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let measured: BTreeMap<String, String> =
        serde_json::from_value(committed["actors"].clone()).unwrap();
    assert_eq!(measured, measured_block_actor_defaults());
}
