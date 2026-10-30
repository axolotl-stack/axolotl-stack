//! Replays the BDS 1.26.51 redstone capture
//! (`testdata/bds_1_26_51_redstone.json`, measured with the bds-sweep
//! `blocks` job) against unastar: every watched cell's states in order,
//! their timing on the circuit grid, the final states, the sounds and the
//! block events of each step.
//!
//! The capture's arena floor is at y = -61, the harness floor at
//! [`FLOOR`]; positions are shifted by the difference. Each scenario's
//! world is built from the blocks its placement steps reported (the
//! clicked block and the placed one) and the watched cells as they were
//! before its first watched step, plus the unwatched dust of the
//! comparator lines. Steps of one scenario share a world and keep the
//! capture's spacing in ticks.
//!
//! Timing: the capture reads the cells once per server tick, tick 0 being
//! the tick the trigger (lever, button, comparator, plate) changed or the
//! command ran. Its circuit ticks fall on odd counts; the replay maps them
//! onto the harness' even circuit ticks. A click or command reaches BDS
//! 1–3 ticks before the trigger shows, so the first circuit tick after it
//! can come one circuit tick later than the replay's: every change must
//! land on the same circuit grid at the capture's tick or one circuit tick
//! (2 game ticks) before it, in the same order per cell.

use std::collections::BTreeMap;

use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacketData;
use serde_json::Value;

use super::super::tests::{FLOOR, Harness};
use crate::entity::components::{DroppedItem, PhysicsBody, Position};
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;

const FIXTURE: &str = include_str!("../testdata/bds_1_26_51_redstone.json");
/// The capture's floor height.
const BDS_FLOOR: i32 = -61;
/// Game ticks the capture's circuit timing may run behind the replay's.
const ARRIVAL_JITTER: i64 = 2;

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture is JSON")
}

fn shift(pos: &Value) -> IVec3 {
    let at = |i: usize| pos[i].as_i64().expect("integer coordinate") as i32;
    IVec3::new(at(0), at(1) - BDS_FLOOR + FLOOR, at(2))
}

fn shift_f(x: f64, y: f64, z: f64) -> DVec3 {
    DVec3::new(x, y - f64::from(BDS_FLOOR) + f64::from(FLOOR), z)
}

/// `name[k=v,...]` in the capture's notation (keys sorted, bytes as
/// booleans, strings quoted), or the bare name without states.
fn notation(state: u32) -> String {
    let registry = BlockRegistry::vanilla();
    let entry = registry.get_by_runtime_id(state).expect("known state");
    let mut states: Vec<(String, String)> = entry
        .data
        .properties
        .iter()
        .map(|property| {
            let value = match entry.state_value(state, property.name).expect("property") {
                StateValue::Byte(b) => (b != 0).to_string(),
                StateValue::Int(i) => i.to_string(),
                StateValue::Str(s) => format!("\"{s}\""),
            };
            (property.name.to_string(), value)
        })
        .collect();
    if states.is_empty() {
        return entry.string_id.clone();
    }
    states.sort();
    let joined: Vec<String> = states.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{}[{}]", entry.string_id, joined.join(","))
}

/// The runtime state a capture notation names.
fn parse_state(text: &str) -> u32 {
    let name = text.split('[').next().expect("name");
    let entry = BlockRegistry::vanilla()
        .get_by_name(name)
        .unwrap_or_else(|| panic!("{name} is a block"));
    (entry.min_state_id..=entry.max_state_id)
        .find(|state| notation(*state) == text)
        .unwrap_or_else(|| panic!("no state {text}"))
}

/// One scenario's world on the capture's clock.
struct Replay {
    h: Harness,
    /// Harness tick minus capture tick (odd: the capture's circuit ticks
    /// are odd counts, the harness' even ones).
    offset: i64,
}

impl Replay {
    fn new(first_tick: u64) -> Self {
        let mut h = Harness::new();
        h.stand(DVec3::new(-10.5, f64::from(FLOOR) + 2.62, -10.5), 0.0, 0.0);
        let start = 4 + (first_tick as i64 + 1) % 2;
        Self {
            h,
            offset: start - first_tick as i64,
        }
    }

    fn now(&self) -> i64 {
        crate::server::game::block_behaviour::redstone::circuit::now(&self.h.world) as i64
    }

    fn tick(&mut self) {
        self.h.tick(1);
        super::press_plates(&mut self.h.world);
    }

    /// Run the harness up to the capture tick `tick`.
    fn advance_to(&mut self, tick: u64) {
        let target = tick as i64 + self.offset;
        assert!(self.now() <= target, "steps overlap");
        while self.now() < target {
            self.tick();
        }
    }

    fn put(&mut self, pos: IVec3, text: &str) {
        self.h.put(pos, parse_state(text));
    }

    fn cell(&self, pos: IVec3) -> String {
        notation(self.h.state(pos))
    }

    /// Put the blocks the placement steps reported.
    fn place(&mut self, step: &Value) {
        let Some(cells) = step["cells"].as_object() else {
            return;
        };
        for cell in cells.values() {
            let pos = shift(&cell["pos"]);
            let block = cell["block"].as_str().expect("block");
            self.put(pos, block);
        }
    }

    /// Run one command of a command step.
    fn command(&mut self, command: &str) {
        let words: Vec<&str> = command.split_whitespace().collect();
        let number = |i: usize| words[i].parse::<f64>().expect("coordinate");
        match words.as_slice() {
            ["setblock", _, _, _, block] => {
                let pos = shift(&serde_json::json!([
                    number(1) as i64,
                    number(2) as i64,
                    number(3) as i64
                ]));
                let state = BlockRegistry::vanilla()
                    .default_state_id_by_name(&format!("minecraft:{block}"))
                    .unwrap_or_else(|| panic!("{block} is a block"));
                self.h.set(pos, state);
            }
            ["tp", "@e[type=item]", ..] => {
                let to = shift_f(number(2), number(3), number(4));
                let mut items = self
                    .h
                    .world
                    .query_filtered::<&mut Position, bevy_ecs::query::With<DroppedItem>>();
                let mut moved = 0;
                for mut position in items.iter_mut(&mut self.h.world) {
                    position.0 = to;
                    moved += 1;
                }
                if moved == 0 {
                    self.h
                        .world
                        .spawn((DroppedItem, Position(to), PhysicsBody::ITEM));
                }
            }
            ["kill", "@e[type=item]"] => {
                let items: Vec<_> = self
                    .h
                    .world
                    .query_filtered::<bevy_ecs::entity::Entity, bevy_ecs::query::With<DroppedItem>>(
                    )
                    .iter(&self.h.world)
                    .collect();
                for item in items {
                    self.h.world.despawn(item);
                }
            }
            ["tp", _player, ..] => {
                let feet = shift_f(number(2), number(3), number(4));
                let eye = feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT;
                self.h.stand(eye, 0.0, 0.0);
            }
            other => panic!("command {other:?}"),
        }
    }

    /// Run a watched step and compare it with the capture.
    fn watch(&mut self, group: &str, step: &Value, first: bool) {
        let name = format!("{group}/{}", step["step"].as_str().expect("step name"));
        let positions: BTreeMap<String, IVec3> = step["positions"]
            .as_object()
            .expect("positions")
            .iter()
            .map(|(cell, pos)| (cell.clone(), shift(pos)))
            .collect();
        self.advance_to(step["tick_0_server_tick"].as_u64().expect("tick 0"));
        for (cell, text) in step["before"].as_object().expect("before") {
            let text = text.as_str().expect("state");
            if first {
                self.put(positions[cell], text);
            } else {
                assert_eq!(self.cell(positions[cell]), text, "{name}: {cell} before");
            }
        }
        self.h.packets();

        let mut seen: BTreeMap<String, String> = positions
            .iter()
            .map(|(cell, pos)| (cell.clone(), self.cell(*pos)))
            .collect();
        match step["action"].as_str() {
            Some("use_watch") => {
                let clicked = shift(&step["clicked"]);
                assert!(
                    super::super::use_block(&mut self.h.world, self.h.player, clicked),
                    "{name}: the click is used"
                );
            }
            Some("command_watch") => {
                for command in step["commands"].as_array().expect("commands") {
                    self.command(command.as_str().expect("command"));
                }
            }
            other => panic!("{name}: action {other:?}"),
        }
        super::press_plates(&mut self.h.world);

        let mut ours: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
        let mut packets = Vec::new();
        let watched = step["watched_ticks"].as_i64().expect("watched ticks");
        for tick in 0..=watched {
            if tick > 0 {
                self.tick();
            }
            packets.extend(self.h.packets());
            for (cell, pos) in &positions {
                let now = self.cell(*pos);
                if seen[cell] != now {
                    ours.entry(cell.clone())
                        .or_default()
                        .push((tick, now.clone()));
                    seen.insert(cell.clone(), now);
                }
            }
        }

        let mut theirs: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
        for change in step["changes"].as_array().expect("changes") {
            let tick = change["tick"].as_i64().expect("tick");
            for (cell, text) in change["cells"].as_object().expect("cells") {
                theirs
                    .entry(cell.clone())
                    .or_default()
                    .push((tick, text.as_str().expect("state").to_string()));
            }
        }
        for cell in positions.keys() {
            let ours = ours.get(cell).cloned().unwrap_or_default();
            let theirs = theirs.get(cell).cloned().unwrap_or_default();
            let states = |list: &[(i64, String)]| -> Vec<String> {
                list.iter().map(|(_, s)| s.clone()).collect()
            };
            assert_eq!(
                states(&ours),
                states(&theirs),
                "{name}: {cell} changes (ours {ours:?}, BDS {theirs:?})"
            );
            for ((our_tick, _), (their_tick, _)) in ours.iter().zip(&theirs) {
                let late = their_tick - our_tick;
                assert!(
                    late == 0 || (*our_tick > 0 && late == ARRIVAL_JITTER),
                    "{name}: {cell} changed at tick {our_tick}, BDS at {their_tick} \
                     (ours {ours:?}, BDS {theirs:?})"
                );
            }
        }
        for (cell, text) in step["after"].as_object().expect("after") {
            assert_eq!(
                self.cell(positions[cell]),
                text.as_str().expect("state"),
                "{name}: {cell} after"
            );
        }

        if let Some(expected) = step["sounds"].as_array() {
            let mut expected: Vec<String> = expected
                .iter()
                .flat_map(|sound| {
                    let position = &sound["position"];
                    let at = shift_f(
                        position[0].as_f64().unwrap(),
                        position[1].as_f64().unwrap(),
                        position[2].as_f64().unwrap(),
                    );
                    let data = match &sound["data"] {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    let times = sound["times"].as_u64().unwrap_or(1) as usize;
                    std::iter::repeat_n(
                        sound_key(sound["sound"].as_str().unwrap(), at, &data),
                        times,
                    )
                })
                .collect();
            let mut actual: Vec<String> = packets
                .iter()
                .filter_map(|packet| match packet {
                    McpePacketData::LevelSoundEventPacket(sound) => {
                        let name = sound.sound_event.to_string();
                        let at = DVec3::new(
                            f64::from(sound.position.x),
                            f64::from(sound.position.y),
                            f64::from(sound.position.z),
                        );
                        // Block sounds carry a wire id (hashes may read as
                        // negative); the note sound and -1 are plain data.
                        let data = if name == "note" || sound.data == -1 {
                            sound.data.to_string()
                        } else {
                            crate::server::game::types::block_network_ids(&self.h.world)
                                .from_wire(sound.data as u32)
                                .map_or_else(|| sound.data.to_string(), notation)
                        };
                        Some(sound_key(&name, at, &data))
                    }
                    _ => None,
                })
                .collect();
            expected.sort();
            actual.sort();
            assert_eq!(actual, expected, "{name}: sounds");
        }

        let expected_events: Vec<(IVec3, i32, i32)> = step["block_events"]
            .as_array()
            .map(|events| {
                events
                    .iter()
                    .map(|event| {
                        (
                            shift(&event["position"]),
                            event["event_type"].as_i64().unwrap() as i32,
                            event["event_value"].as_i64().unwrap() as i32,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let events: Vec<(IVec3, i32, i32)> = packets
            .iter()
            .filter_map(|packet| match packet {
                McpePacketData::BlockEventPacket(event) => Some((
                    IVec3::new(
                        event.block_position.x,
                        event.block_position.y,
                        event.block_position.z,
                    ),
                    event.event_type,
                    event.event_value,
                )),
                _ => None,
            })
            .collect();
        assert_eq!(events, expected_events, "{name}: block events");
    }
}

/// A sound as `name @ (x, y, z) data`, positions to 0.01.
fn sound_key(name: &str, at: DVec3, data: &str) -> String {
    format!("{name} @ ({:.2}, {:.2}, {:.2}) {data}", at.x, at.y, at.z)
}

/// Dust the comparator scenarios lay between a command's redstone block
/// and the watched dust next to the comparator at x = 8, z = 0: the rear
/// line along z = 0 up to x = 6, the side line along x = 8 from z = 2.
fn comparator_lines(replay: &mut Replay, step: &Value) {
    for command in step["commands"].as_array().into_iter().flatten() {
        let words: Vec<i64> = command
            .as_str()
            .unwrap()
            .split_whitespace()
            .skip(1)
            .take(3)
            .map(|w| w.parse().unwrap())
            .collect();
        let (x, y, z) = (words[0], words[1], words[2]);
        let cells: Vec<(i64, i64)> = if z == 0 {
            (x + 1..=6).map(|x| (x, 0)).collect()
        } else {
            (2..z).map(|z| (8, z)).collect()
        };
        for (x, z) in cells {
            let pos = shift(&serde_json::json!([x, y, z]));
            replay.put(pos, "minecraft:redstone_wire[redstone_signal=0]");
        }
    }
}

/// Replay a fixture group: a new world starts at every placement step
/// that follows a watched step.
fn replay_group(group: &str) {
    let fixture = fixture();
    let steps = fixture[group].as_array().expect("group");
    let mut replay: Option<Replay> = None;
    let mut placements: Vec<&Value> = Vec::new();
    let mut first = true;
    for step in steps {
        let watched = step.get("changes").is_some();
        if !watched {
            if replay.is_some() {
                replay = None;
                placements.clear();
            }
            placements.push(step);
            continue;
        }
        let replay = replay.get_or_insert_with(|| {
            first = true;
            let mut world = Replay::new(step["tick_0_server_tick"].as_u64().unwrap());
            for placement in &placements {
                world.place(placement);
            }
            world
        });
        if first && group == "comparators" {
            comparator_lines(replay, step);
        }
        replay.watch(group, step, first);
        first = false;
    }
}

#[test]
fn dust_lines_and_their_lamps_match_bds() {
    replay_group("redstone_dust");
}

#[test]
fn repeater_delays_and_locks_match_bds() {
    replay_group("repeaters");
}

#[test]
fn redstone_torch_inversion_and_burnout_match_bds() {
    replay_group("redstone_torch");
}

#[test]
fn comparator_modes_match_bds() {
    replay_group("comparators");
}

#[test]
fn button_lengths_match_bds() {
    replay_group("buttons");
}

#[test]
fn pressure_plates_match_bds() {
    replay_group("pressure_plates");
}

#[test]
fn iron_door_by_lever_matches_bds() {
    replay_group("iron_door_lever");
}

#[test]
fn note_blocks_match_bds() {
    replay_group("note_blocks");
}
