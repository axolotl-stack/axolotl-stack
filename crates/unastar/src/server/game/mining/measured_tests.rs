//! Break times and mining wear against the BDS 1.26.51 mining sweep.
//!
//! `unastar-data/data/measured/mining_ticks.json` records, per block state
//! and held stack (empty hand, every tool tier, Efficiency levels, Haste and
//! Mining Fatigue levels, non-tool items), the server ticks BDS took from the
//! click to the break (±1 tick) and, for held stacks, the durability damage
//! each break added. A break that needs `n` ticks of progress completes
//! `n - 1` ticks after the click tick, so a measurement is
//! [`break_ticks`] − 1.
//!
//! `+underwater` cells are not compared: the sweep's water source ran off
//! the player's head between trials (the recorded in-water flag is the
//! feet's), so most trials did not mine with a submerged head.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::*;
use crate::item::enchant::with_enchantments;
use crate::registry::BlockRegistry;

const MEASURED: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../unastar-data/data/measured/mining_ticks.json"
));

#[derive(Deserialize)]
struct Cell {
    ticks: Vec<Option<u32>>,
    median: Option<u32>,
    tool_item: Option<ToolItem>,
    #[serde(default)]
    commands: Vec<String>,
    #[serde(default)]
    wear: Option<Vec<Option<i32>>>,
}

#[derive(Deserialize)]
struct ToolItem {
    item: String,
    #[serde(default)]
    enchants: BTreeMap<String, u8>,
}

/// Block state key (`minecraft:oak_log[pillar_axis="y"]`) to its cells.
fn measured() -> BTreeMap<String, BTreeMap<String, Cell>> {
    serde_json::from_str(MEASURED).expect("measured mining table decodes")
}

fn block_of(state_key: &str) -> &'static BlockEntry {
    let name = state_key.split('[').next().unwrap();
    BlockRegistry::vanilla()
        .get_by_name(name)
        .unwrap_or_else(|| panic!("{name} is not a vanilla block"))
}

fn held(cell: &Cell) -> Option<ItemStack> {
    let tool = cell.tool_item.as_ref()?;
    let stack = crate::item::test_support::stack(&tool.item, 1);
    let enchants: Vec<_> = tool
        .enchants
        .iter()
        .map(|(name, level)| {
            let enchantment = Enchantment::from_name(name)
                .unwrap_or_else(|| panic!("{name} is not a mining enchantment"));
            (enchantment, *level)
        })
        .collect();
    Some(if enchants.is_empty() {
        stack
    } else {
        with_enchantments(&stack, &enchants)
    })
}

/// The effect levels the cell's `/effect` commands gave the player (the
/// command's amplifier is 0-based).
fn conditions(cell: &Cell) -> MiningConditions {
    let mut conditions = MiningConditions {
        on_ground: true,
        ..MiningConditions::default()
    };
    for command in &cell.commands {
        let words: Vec<&str> = command.split_whitespace().collect();
        let [_, _, effect, _, amplifier, ..] = words[..] else {
            panic!("unexpected sweep command {command}");
        };
        let level = amplifier.parse::<u8>().unwrap() + 1;
        match effect {
            "haste" => conditions.haste = level,
            "mining_fatigue" => conditions.mining_fatigue = level,
            _ => panic!("unexpected sweep effect {effect}"),
        }
    }
    conditions
}

#[test]
fn measured_break_times_match_bds_within_a_tick() {
    let mut compared = 0;
    let mut wrong = Vec::new();
    for (state, cells) in measured() {
        let block = block_of(&state);
        for (variant, cell) in &cells {
            if variant.ends_with("+underwater") {
                continue;
            }
            let measured = cell
                .median
                .unwrap_or_else(|| panic!("{state}|{variant} did not break: {:?}", cell.ticks));
            let ours = break_ticks(block, held(cell).as_ref(), conditions(cell)) - 1;
            compared += 1;
            if ours.abs_diff(measured) > 1 {
                wrong.push(format!(
                    "{state}|{variant}: {ours} ticks, BDS {measured} {:?}",
                    cell.ticks
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {compared} cells:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert!(compared > 150, "only {compared} cells compared");
}

#[test]
fn measured_mining_wear_matches_bds() {
    let mut compared = 0;
    let mut wrong = Vec::new();
    for (state, cells) in measured() {
        let block = block_of(&state);
        for (variant, cell) in &cells {
            let (Some(wear), Some(stack)) = (cell.wear.as_ref(), held(cell)) else {
                continue;
            };
            let ours = mining_wear(block, &stack);
            for (trial, measured) in wear.iter().enumerate() {
                compared += 1;
                if *measured != Some(ours) {
                    wrong.push(format!(
                        "{state}|{variant} trial {trial}: wear {ours}, BDS {measured:?}"
                    ));
                }
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "{} of {compared} breaks:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert!(compared > 0, "the table records no wear");
}
