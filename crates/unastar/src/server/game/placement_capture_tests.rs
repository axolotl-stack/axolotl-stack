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
fn cases() -> Vec<Value> {
    let dir = format!("{}/src/server/game/testdata", env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("bds-placements") && name.ends_with(".json"))
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
            McpePacketData::LevelSoundEventPacket(sound) => answer.sounds.push((
                sound.sound_event.to_string(),
                [sound.position.x, sound.position.y, sound.position.z].map(|v| (v * 2.0) as i32),
                sound.data,
                sound.actor_unique_id,
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
        let mut server = GameServer::with_config(ServerConfig {
            world: crate::world::WorldConfig::void_spawn_platform(1),
            default_gamemode: GameMode::Survival,
            ..ServerConfig::default()
        });
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
        // Timing samples: a placed coral block dies at a random tick
        // (`placed_coral_blocks_die_within_the_measured_ticks`).
        _ if key.starts_with("coral_block_dies_") => Skip { case: true, ..none },
        // A live coral block may already be dead at the first read.
        _ if key.ends_with("_coral_block_on_top") && !key.starts_with("dead_") => Skip {
            first_read: true,
            ..none
        },
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
        // Eating cake (and a candle cake) is not modelled.
        "candle_onto_bitten_cake"
        | "candle_onto_candle_cake"
        | "candle_cake_empty_hand"
        | "cake_empty_hand" => Skip { case: true, ..none },
        // Lighting with flint and steel answers without the block corrections
        // (item use on blocks, lane ITEMS).
        _ if key.starts_with("flint_and_steel_on_") => Skip { case: true, ..none },
        // Cave vines get a random age, so the corrections differ too.
        "glow_berries_under_stone" => Skip { case: true, ..none },
        // Tripwire states are not modelled.
        "string_on_stone" => Skip { case: true, ..none },
        // Flower pots do not take plants yet.
        _ if key.starts_with("flower_pot_with_") => Skip { case: true, ..none },
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

/// The timing samples of placed coral blocks: BDS had each dead by tick 60
/// and some by tick 10, which unastar's random 1..=60 tick death covers.
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
    assert!(deaths.iter().all(|&(after, _)| after <= 60), "{deaths:?}");
    assert!(deaths.iter().any(|&(_, by)| by <= 10), "{deaths:?}");
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
