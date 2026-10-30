//! Replays the player clicks of the BDS 1.26.51 block capture
//! (`testdata/bds_1_26_51_blocks.json`, measured with the bds-sweep
//! `blocks` job) against unastar and compares every watched cell's block
//! and full state map, and the door sounds.
//!
//! The capture's arena floor is at y = -61; the harness floor at
//! [`FLOOR`], so every position is shifted by the difference. Steps that
//! build on each other (a second door beside the first, the toggle and
//! iron-door sequences) share one world.

use glam::{DVec3, IVec3};
use jolyne::valentine::McpePacketData;
use serde_json::Value;

use super::tests::{FLOOR, Harness};
use crate::entity::systems::BlockSource;
use crate::registry::block::StateValue;

const FIXTURE: &str = include_str!("testdata/bds_1_26_51_blocks.json");
/// The capture's floor height.
const BDS_FLOOR: i32 = -61;

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("fixture is JSON")
}

fn shift(pos: &Value) -> IVec3 {
    let at = |i: usize| pos[i].as_i64().unwrap() as i32;
    IVec3::new(at(0), at(1) - BDS_FLOOR + FLOOR, at(2))
}

fn face(name: &str) -> u8 {
    match name {
        "down" => 0,
        "up" => 1,
        "north" => 2,
        "south" => 3,
        "west" => 4,
        "east" => 5,
        other => panic!("face {other}"),
    }
}

/// `name[k=v,...]` in the capture's notation (keys sorted, bytes as
/// booleans, strings quoted), or the bare name without states.
fn notation(state: u32) -> String {
    let registry = crate::registry::BlockRegistry::vanilla();
    let entry = registry.get_by_runtime_id(state).unwrap();
    let mut states: Vec<(String, String)> = entry
        .data
        .properties
        .iter()
        .map(|property| {
            let value = match entry.state_value(state, property.name).unwrap() {
                StateValue::Byte(b) => (b != 0).to_string(),
                StateValue::Int(i) => i.to_string(),
                StateValue::Str(s) => format!("\"{s}\""),
            };
            (property.name.to_string(), value)
        })
        .collect();
    states.sort();
    if states.is_empty() {
        return entry.string_id.clone();
    }
    let joined: Vec<String> = states.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{}[{}]", entry.string_id, joined.join(","))
}

/// The capture's cell (`name` and `states` map) in the same notation.
fn cell_notation(cell: &Value) -> String {
    let name = cell["name"].as_str().unwrap();
    let states = cell["states"].as_object().unwrap();
    if states.is_empty() {
        return name.to_string();
    }
    let mut parts: Vec<(String, String)> = states
        .iter()
        .map(|(k, v)| {
            let value = match v {
                Value::String(s) => format!("\"{s}\""),
                other => other.to_string(),
            };
            (k.clone(), value)
        })
        .collect();
    parts.sort();
    let joined: Vec<String> = parts.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{name}[{}]", joined.join(","))
}

/// The runtime state a capture cell names.
fn cell_state(cell: &Value) -> u32 {
    let registry = crate::registry::BlockRegistry::vanilla();
    let entry = registry
        .get_by_name(cell["name"].as_str().unwrap())
        .unwrap();
    let wanted = cell_notation(cell);
    (entry.min_state_id..=entry.max_state_id)
        .find(|state| notation(*state) == wanted)
        .unwrap_or_else(|| panic!("no state {wanted}"))
}

/// Replay one `use` step in `h` and compare its cells (and, with
/// `sounds`, its door sounds).
fn replay(h: &mut Harness, step: &Value, sounds: bool) {
    let name = step["step"].as_str().unwrap();
    // The clicked block of torch and seed steps stands on its own.
    if let Some(clicked) = step["cells"].get("clicked")
        && clicked["name"] != "minecraft:air"
    {
        h.put(shift(&clicked["pos"]), cell_state(clicked));
    }
    match step["held_after"].as_array() {
        Some(held) => h.hold(held[0].as_str().unwrap(), 32),
        None => {
            h.world
                .get_mut::<crate::entity::components::MainInventory>(h.player)
                .unwrap()
                .0
                .set_item(0, crate::item::ItemStack::empty())
                .unwrap();
        }
    }
    let player = &step["player"];
    let feet = player["feet"].as_array().unwrap();
    let feet = DVec3::new(
        feet[0].as_f64().unwrap(),
        feet[1].as_f64().unwrap() - f64::from(BDS_FLOOR) + f64::from(FLOOR),
        feet[2].as_f64().unwrap(),
    );
    h.stand(
        feet + DVec3::Y * crate::entity::PLAYER_EYE_HEIGHT,
        player["yaw"].as_f64().unwrap() as f32,
        player["pitch"].as_f64().unwrap() as f32,
    );
    let click = step["click"].as_array().unwrap();
    h.packets();
    h.click(
        shift(&step["clicked"]),
        face(step["face"].as_str().unwrap()),
        DVec3::new(
            click[0].as_f64().unwrap(),
            click[1].as_f64().unwrap(),
            click[2].as_f64().unwrap(),
        ),
    );
    for (label, cell) in step["cells"].as_object().unwrap() {
        let ours = notation(h.world.block_at(shift(&cell["pos"])).unwrap());
        assert_eq!(ours, cell_notation(cell), "{name}: cell {label}");
    }
    if sounds {
        let ids = crate::server::game::types::block_network_ids(&h.world);
        let ours: Vec<(String, String)> = h
            .packets()
            .into_iter()
            .filter_map(|packet| match packet {
                McpePacketData::LevelSoundEventPacket(sound)
                    if sound.sound_event.starts_with("door.") =>
                {
                    let state = ids.from_wire(sound.data as u32).unwrap();
                    Some((sound.sound_event.to_string(), notation(state)))
                }
                _ => None,
            })
            .collect();
        let theirs: Vec<(String, String)> = step["packets"]["sounds"]
            .as_array()
            .map(|sounds| {
                sounds
                    .iter()
                    .filter(|s| s["sound"].as_str().unwrap().starts_with("door."))
                    .map(|s| {
                        (
                            s["sound"].as_str().unwrap().to_string(),
                            s["data"].as_str().unwrap().to_string(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        assert_eq!(ours, theirs, "{name}: door sounds");
    }
}

fn steps<'a>(fixture: &'a Value, group: &str) -> impl Iterator<Item = &'a Value> {
    fixture[group]
        .as_array()
        .unwrap()
        .iter()
        .filter(|step| step["action"] == "use")
}

fn step<'a>(fixture: &'a Value, group: &str, name: &str) -> &'a Value {
    steps(fixture, group)
        .find(|step| step["step"] == name)
        .unwrap_or_else(|| panic!("{name} captured"))
}

#[test]
fn door_placement_matches_the_bds_capture() {
    let fixture = fixture();
    let stone = super::tests::state("minecraft:stone", &[]);
    for name in [
        "place_looking_north",
        "place_looking_east",
        "place_looking_north_click_west_half",
        "place_looking_north_click_east_half",
        "place_stone_stacked_west",
        "place_stone_stacked_east",
        "iron_place",
    ] {
        let mut h = Harness::new();
        let side = match name {
            "place_stone_stacked_west" => Some(IVec3::NEG_X),
            "place_stone_stacked_east" => Some(IVec3::X),
            _ => None,
        };
        if let Some(side) = side {
            for y in [1, 2] {
                h.put(IVec3::new(0, FLOOR + y, 0) + side, stone);
            }
        }
        replay(&mut h, step(&fixture, "doors", name), false);
    }
    let mut pair = Harness::new();
    replay(&mut pair, step(&fixture, "doors", "pair_first"), false);
    replay(
        &mut pair,
        step(&fixture, "doors", "pair_second_east"),
        false,
    );
}

#[test]
fn door_use_matches_the_bds_capture() {
    let fixture = fixture();
    let mut h = Harness::new();
    replay(&mut h, step(&fixture, "doors", "toggle_place"), false);
    for name in [
        "toggle_lower_1",
        "toggle_lower_2",
        "toggle_lower_3",
        "toggle_upper_1",
    ] {
        replay(&mut h, step(&fixture, "doors", name), true);
    }
    let mut iron = Harness::new();
    for name in ["iron_place", "iron_click_empty_hand", "iron_click_dirt"] {
        replay(&mut iron, step(&fixture, "doors", name), true);
    }
}

#[test]
fn torch_placement_matches_the_bds_capture() {
    let fixture = fixture();
    for step in steps(&fixture, "torches") {
        replay(&mut Harness::new(), step, false);
    }
}

#[test]
fn seed_planting_matches_the_bds_capture() {
    let fixture = fixture();
    for step in steps(&fixture, "crops") {
        replay(&mut Harness::new(), step, false);
    }
}

/// The capture's crop arena: eight wheat plants on farmland around one
/// water block at (0, floor, 8), farmland starting dry, open sky at noon.
/// After 60 s at `randomtickspeed` 100 BDS had five plants fully grown and
/// a stage total of 46 of 56; at speed 1 none grew. Random ticks are random
/// on both sides, so this checks the rate: over four seeded runs our mean
/// stage total must lie within the range one such BDS run plausibly comes
/// from.
#[test]
fn crop_growth_rate_matches_the_bds_capture() {
    let fixture = fixture();
    let observed = |speed: u64| -> u32 {
        let run = fixture["crops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|step| step["randomtickspeed"].as_u64() == Some(speed))
            .expect("growth run captured");
        run["final_growth"]
            .as_object()
            .unwrap()
            .values()
            .map(|stage| stage.as_u64().unwrap() as u32)
            .sum()
    };
    assert_eq!(observed(100), 46);
    assert_eq!(observed(1), 0);

    let water = super::tests::state("minecraft:water", &[]);
    let farmland = super::tests::state("minecraft:farmland", &[]);
    let wheat = super::tests::state("minecraft:wheat", &[]);
    let run = |seed: u64, speed: u32| -> u32 {
        let mut h = Harness::new();
        h.world.insert_resource(super::BlockRandom::seeded(seed));
        h.world.insert_resource(super::RandomTickSpeed(speed));
        // Only the arena's two chunks (x = -1 and x = 0..1) matter; the
        // others would only add draws.
        for (cx, cz) in [(-1, -1), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)] {
            h.world
                .resource_mut::<crate::world::ecs::ChunkManager>()
                .remove_by_coords(cx, cz);
        }
        let mut cells = Vec::new();
        for x in -1..=1 {
            for z in 7..=9 {
                let soil = IVec3::new(x, FLOOR, z);
                if (x, z) == (0, 8) {
                    h.put(soil, water);
                    continue;
                }
                h.put(soil, farmland);
                h.put(soil + IVec3::Y, wheat);
                cells.push(soil + IVec3::Y);
            }
        }
        h.tick(1200);
        cells
            .iter()
            .map(|cell| match h.value(*cell, "growth") {
                Some(StateValue::Int(stage)) => stage as u32,
                other => panic!("wheat at {cell}: {other:?}"),
            })
            .sum()
    };
    let mean = (0..4).map(|seed| run(seed, 100)).sum::<u32>() as f64 / 4.0;
    assert!((36.0..=56.0).contains(&mean), "mean stage total {mean}");
    let slow = (0..4).map(|seed| run(seed, 1)).sum::<u32>();
    assert!(
        slow <= 4,
        "speed 1 grows next to nothing in a minute: {slow}"
    );
}
