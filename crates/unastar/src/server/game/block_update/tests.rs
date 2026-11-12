//! Replays of the BDS 1.26.51 block scenarios
//! (`crates/unastar-data/data/measured/block_updates/`): every scenario is
//! rebuilt in an in-memory world with the same placements, with or without
//! block updates, and the settled states are compared with what BDS stored.

use std::collections::HashMap;

use glam::IVec3;
use serde_json::Value;

use super::{UpdateTarget, changed};
use crate::entity::systems::BlockSource;
use crate::entity::systems::physics::tests::TestBlocks;
use crate::registry::BlockRegistry;
use crate::registry::block::StateValue;

/// An in-memory world on the sweep's superflat floor (grass at y = -1,
/// dirt below) that records what block updates pop.
struct Cell {
    blocks: TestBlocks,
    popped: Vec<(IVec3, u32)>,
}

impl Cell {
    fn new() -> Self {
        let mut blocks = TestBlocks::floor("minecraft:grass_block", -1);
        for y in [-2, -3] {
            for x in -4..=4 {
                for z in -4..=4 {
                    blocks.set(IVec3::new(x, y, z), "minecraft:dirt");
                }
            }
        }
        Self {
            blocks,
            popped: Vec::new(),
        }
    }

    /// Set `state` at `pos`, with the block update a placement triggers when
    /// `physics`.
    fn set(&mut self, pos: IVec3, state: u32, physics: bool) {
        self.write(pos, state);
        if physics {
            changed(self, pos, true);
        }
    }

    fn get(&self, pos: IVec3) -> u32 {
        self.block_at(pos).unwrap()
    }
}

impl BlockSource for Cell {
    fn block_at(&self, pos: IVec3) -> Option<u32> {
        self.blocks.block_at(pos)
    }

    fn registry(&self) -> &BlockRegistry {
        BlockRegistry::vanilla()
    }
}

impl UpdateTarget for Cell {
    fn write(&mut self, pos: IVec3, state: u32) {
        self.blocks.set_state(pos, state);
    }

    fn pop(&mut self, pos: IVec3, state: u32) {
        self.popped.push((pos, state));
        self.blocks.set_state(pos, air());
    }
}

fn air() -> u32 {
    BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:air")
        .unwrap()
}

/// Rows of a measured scenario file.
pub(crate) fn rows(family: &str) -> Vec<Value> {
    let path = format!(
        "{}/../unastar-data/data/measured/block_updates/{family}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    let mut value: Value = serde_json::from_str(&text).expect(&path);
    match value["rows"].take() {
        Value::Array(rows) => rows,
        other => panic!("{path}: rows {other:?}"),
    }
}

/// The runtime id of `block` with the measured `states` (booleans are byte
/// properties).
pub(crate) fn state(block: &str, states: &Value) -> u32 {
    let entry = BlockRegistry::vanilla().get_by_name(block).expect(block);
    let mut id = entry.default_state_id;
    for (property, value) in states.as_object().expect("states object") {
        let value = match value {
            Value::Bool(b) => StateValue::Byte(u8::from(*b)),
            Value::Number(n) => StateValue::Int(n.as_i64().unwrap() as i32),
            Value::String(s) => StateValue::Str(Box::leak(s.clone().into_boxed_str())),
            other => panic!("{block} {property} {other:?}"),
        };
        id = entry
            .with_state_value(id, property, value)
            .unwrap_or_else(|| panic!("{block} has no {property}={value:?}"));
    }
    id
}

/// `block[property=value,...]` of a runtime id, for assertion messages.
pub(crate) fn describe(state: u32) -> String {
    let entry = BlockRegistry::vanilla().get_by_runtime_id(state).unwrap();
    let properties: Vec<String> = entry
        .data
        .properties
        .iter()
        .map(|p| format!("{}={}", p.name, value_str(state, p.name)))
        .collect();
    format!("{}[{}]", entry.string_id, properties.join(","))
}

fn name(state: u32) -> &'static str {
    &BlockRegistry::vanilla()
        .get_by_runtime_id(state)
        .unwrap()
        .string_id
}

fn side(name: &str) -> IVec3 {
    match name {
        "east" => IVec3::X,
        "west" => IVec3::NEG_X,
        "south" => IVec3::Z,
        "north" => IVec3::NEG_Z,
        "up" => IVec3::Y,
        "down" => IVec3::NEG_Y,
        other => panic!("side {other}"),
    }
}

fn stairs(block: &str, weirdo: &Value, upside_down: &Value) -> u32 {
    state(
        block,
        &serde_json::json!({
            "weirdo_direction": weirdo,
            "upside_down_bit": upside_down,
            "minecraft:corner": "none",
        }),
    )
}

fn value_str(state: u32, property: &str) -> String {
    match BlockRegistry::vanilla()
        .get_by_runtime_id(state)
        .unwrap()
        .state_value(state, property)
    {
        Some(StateValue::Str(s)) => s.to_owned(),
        Some(StateValue::Byte(b)) => (b != 0).to_string(),
        Some(StateValue::Int(i)) => i.to_string(),
        None => "-".to_owned(),
    }
}

/// Every measured stair scenario: one or two neighbouring stairs, both
/// placement orders, mixed materials.
#[test]
fn stair_corners_match_the_bds_scenarios() {
    let rows = rows("stairs");
    assert_eq!(rows.len(), 3840);
    for row in &rows {
        let mut cell = Cell::new();
        let centre = stairs("minecraft:oak_stairs", &row[0], &row[1]);
        let neighbours: Vec<(IVec3, u32)> = row[2]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| {
                (
                    side(n[0].as_str().unwrap()),
                    stairs(n[1].as_str().unwrap(), &n[2], &n[3]),
                )
            })
            .collect();
        if row[3] == "centre_first" {
            cell.set(IVec3::ZERO, centre, true);
        }
        for &(pos, state) in &neighbours {
            cell.set(pos, state, true);
        }
        if row[3] == "neighbour_first" {
            cell.set(IVec3::ZERO, centre, true);
        }
        let got: Vec<String> = std::iter::once(IVec3::ZERO)
            .chain(neighbours.iter().map(|(pos, _)| *pos))
            .map(|pos| value_str(cell.get(pos), "minecraft:corner"))
            .collect();
        let expected: Vec<String> = row[4]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(got, expected, "scenario {row}");
    }
}

/// Every measured neighbour (each block's default state and every further
/// collision shape) east of each fence, pane, bars and wall centre.
#[test]
fn connections_match_the_bds_scenarios() {
    let rows = rows("connections");
    let cells: usize = rows
        .iter()
        .map(|row| row[2].as_object().unwrap().len())
        .sum();
    assert!(cells > 11_000, "{cells}");
    let mut checked = 0;
    for row in &rows {
        let neighbour = state(row[0].as_str().unwrap(), &row[1]);
        for (centre, measured) in row[2].as_object().unwrap() {
            let mut cell = Cell::new();
            cell.set(IVec3::X, neighbour, true);
            let centre_state = BlockRegistry::vanilla()
                .default_state_id_by_name(centre)
                .unwrap();
            cell.set(IVec3::ZERO, centre_state, true);
            let expected = state(centre, measured);
            assert_eq!(
                cell.get(IVec3::ZERO),
                expected,
                "{centre} next to {} {}: got {}",
                row[0],
                row[1],
                describe(cell.get(IVec3::ZERO))
            );
            checked += 1;
        }
    }
    assert_eq!(checked, cells);
}

/// Blocks whose outline BDS offsets per position (coral plants, mangrove
/// propagules, dripstone), the technical `unknown` torch state, and the
/// lone lower door half, whose stored shape BDS does not use without its
/// upper half: their wall heights depend on things the shape table does
/// not have.
fn unmodelled_above(block: &str, states: &Value) -> bool {
    let coral_plant = block.ends_with("_coral") && !block.contains("block");
    let offset = matches!(
        block,
        "minecraft:mangrove_propagule" | "minecraft:pointed_dripstone" | "minecraft:sulfur_spike"
    );
    coral_plant
        || offset
        || block.ends_with("_door")
        || states["torch_facing_direction"] == "unknown"
}

/// Wall heights and posts under every measured block (straight, end and
/// lone walls).
#[test]
fn wall_heights_and_posts_match_the_bds_scenarios() {
    let rows = rows("wall_above");
    let wall = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:cobblestone_wall")
        .unwrap();
    let mut checked = 0;
    let mut skipped = HashMap::<String, usize>::new();
    for row in &rows {
        let block = row[0].as_str().unwrap();
        if unmodelled_above(block, &row[1]) {
            *skipped.entry(block.to_owned()).or_default() += 1;
            continue;
        }
        let above = state(block, &row[1]);
        for (shape, measured) in row[2].as_object().unwrap() {
            let sides: &[IVec3] = match shape.as_str() {
                "straight" => &[IVec3::X, IVec3::NEG_X],
                "end" => &[IVec3::X],
                "lone" => &[],
                other => panic!("shape {other}"),
            };
            let mut cell = Cell::new();
            for &pos in sides {
                cell.set(pos, wall, true);
            }
            cell.set(IVec3::ZERO, wall, true);
            cell.set(IVec3::Y, above, true);
            assert_eq!(
                cell.get(IVec3::ZERO),
                state("minecraft:cobblestone_wall", measured),
                "{shape} wall under {block} {}: got {}",
                row[1],
                describe(cell.get(IVec3::ZERO))
            );
            checked += 1;
        }
    }
    assert!(checked > 4_000, "{checked}");
    assert!(skipped.len() < 30, "{skipped:?}");
}

#[test]
fn gate_in_wall_matches_the_bds_scenarios() {
    let rows = rows("gate_in_wall");
    assert_eq!(rows.len(), 192);
    for row in &rows {
        let mut cell = Cell::new();
        let side_block = BlockRegistry::vanilla()
            .default_state_id_by_name(row[2].as_str().unwrap())
            .unwrap();
        for s in row[3].as_array().unwrap() {
            cell.set(side(s.as_str().unwrap()), side_block, true);
        }
        let gate = state(
            "minecraft:fence_gate",
            &serde_json::json!({
                "minecraft:cardinal_direction": row[0],
                "open_bit": row[1],
                "in_wall_bit": false,
            }),
        );
        cell.set(IVec3::ZERO, gate, true);
        assert_eq!(
            value_str(cell.get(IVec3::ZERO), "in_wall_bit"),
            row[4].as_bool().unwrap().to_string(),
            "{row}"
        );
    }
}

#[test]
fn a_placed_block_is_not_removed_by_its_own_placement() {
    // BDS keeps a dandelion set on stone until a neighbour changes.
    let mut cell = Cell::new();
    cell.set(
        IVec3::NEG_Y,
        BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:stone")
            .unwrap(),
        false,
    );
    let dandelion = BlockRegistry::vanilla()
        .default_state_id_by_name("minecraft:dandelion")
        .unwrap();
    cell.set(IVec3::ZERO, dandelion, true);
    assert_eq!(name(cell.get(IVec3::ZERO)), "minecraft:dandelion");
    assert!(cell.popped.is_empty());
}

fn default_state(name: &str) -> u32 {
    BlockRegistry::vanilla()
        .default_state_id_by_name(name)
        .unwrap_or_else(|| panic!("{name}"))
}

/// Wall posts and arm heights at crosses and T junctions under every
/// measured block.
#[test]
fn wall_junctions_match_the_bds_scenarios() {
    let rows = rows("junctions");
    let wall = default_state("minecraft:cobblestone_wall");
    let mut checked = 0;
    let mut skipped = 0;
    for row in &rows {
        let block = row[0].as_str().unwrap();
        if unmodelled_above(block, &row[1]) {
            skipped += row[2].as_object().unwrap().len();
            continue;
        }
        let above = state(block, &row[1]);
        for (shape, measured) in row[2].as_object().unwrap() {
            let sides: &[IVec3] = match shape.as_str() {
                "cross" => &[IVec3::NEG_Z, IVec3::X, IVec3::Z, IVec3::NEG_X],
                "t" => &[IVec3::NEG_Z, IVec3::X, IVec3::Z],
                other => panic!("shape {other}"),
            };
            let mut cell = Cell::new();
            for &pos in sides {
                cell.set(pos, wall, true);
            }
            cell.set(IVec3::ZERO, wall, true);
            cell.set(IVec3::Y, above, true);
            assert_eq!(
                cell.get(IVec3::ZERO),
                state("minecraft:cobblestone_wall", measured),
                "{shape} under {block} {}: got {}",
                row[1],
                describe(cell.get(IVec3::ZERO))
            );
            checked += 1;
        }
    }
    // Two junctions (cross and T) under each of 1407 measured blocks.
    assert_eq!(checked + skipped, 2 * rows.len());
    assert!(
        checked >= 2_700,
        "{checked} of {} checked",
        checked + skipped
    );
}

fn falls(state: u32) -> bool {
    BlockRegistry::vanilla()
        .get_by_runtime_id(state)
        .is_some_and(|entry| entry.data.falling)
}

/// The poke side of each support side in the `surfaces` family.
fn poke(side_name: &str) -> IVec3 {
    match side_name {
        "down" => IVec3::Y,
        "east" => IVec3::NEG_X,
        "up" => IVec3::NEG_Y,
        other => panic!("side {other}"),
    }
}

/// Every measured holder of every swept kind of attached block: the holder
/// on the support side, the attached block next to it, a neighbour update.
#[test]
fn attached_blocks_stay_on_the_holders_bds_keeps_them_on() {
    let rows = rows("surfaces");
    assert_eq!(rows.len(), 34);
    let stone = default_state("minecraft:stone");
    let mut checked = 0;
    let mut skipped = 0;
    for row in &rows {
        let block = row[0].as_str().unwrap();
        let attached = state(block, &row[1]);
        let side_name = row[2].as_str().unwrap();
        // Snow layers are gravity blocks: they fall rather than pop.
        if block == "minecraft:snow_layer" {
            skipped += row[3].as_array().unwrap().len();
            continue;
        }
        for case in row[3].as_array().unwrap() {
            let holder = state(case[0].as_str().unwrap(), &case[1]);
            // A gravity block as holder varies with its own fall.
            if falls(holder) {
                skipped += 1;
                continue;
            }
            let mut cell = Cell::new();
            cell.write(side(side_name), holder);
            cell.set(IVec3::ZERO, attached, true);
            cell.set(poke(side_name), stone, true);
            let stayed = name(cell.get(IVec3::ZERO)) == block;
            assert_eq!(
                stayed,
                case[2].as_bool().unwrap(),
                "{block} {} on {side_name} side: holder {} {}",
                row[1],
                case[0],
                case[1]
            );
            checked += 1;
        }
    }
    let total: usize = rows
        .iter()
        .map(|row| row[3].as_array().unwrap().len())
        .sum();
    assert_eq!(checked + skipped, total);
    assert!(checked >= 45_000, "{checked} of {total} checked");
}

/// Every measured plant on every block's default state.
#[test]
fn plants_stay_on_the_soils_bds_keeps_them_on() {
    let rows = rows("soils");
    let stone = default_state("minecraft:stone");
    let mut checked = 0;
    for row in &rows {
        let plant = row[0].as_str().unwrap();
        let state_id = state(plant, &row[1]);
        // Cactus also falls next to a block, which no sweep isolated.
        if plant == "minecraft:cactus" {
            continue;
        }
        for case in row[2].as_array().unwrap() {
            let soil = state(case[0].as_str().unwrap(), &case[1]);
            let mut cell = Cell::new();
            cell.write(IVec3::NEG_Y, soil);
            cell.set(IVec3::ZERO, state_id, true);
            cell.set(IVec3::Y, stone, true);
            let stayed = name(cell.get(IVec3::ZERO)) == plant;
            assert_eq!(
                stayed,
                case[2].as_bool().unwrap(),
                "{plant} on {} {}",
                case[0],
                case[1]
            );
            checked += 1;
        }
    }
    assert!(checked > 25_000, "{checked}");
}

/// Doors and tall plants: removing either half or the floor removes both.
#[test]
fn two_block_structures_come_apart_like_bds() {
    let rows = rows("halves");
    assert_eq!(rows.len(), 30);
    for row in &rows {
        let block = row[0].as_str().unwrap();
        let entry = BlockRegistry::vanilla().get_by_name(block).unwrap();
        let half = |upper: u8| {
            entry
                .with_state_value(
                    entry.default_state_id,
                    "upper_block_bit",
                    StateValue::Byte(upper),
                )
                .unwrap()
        };
        let mut cell = Cell::new();
        cell.write(IVec3::NEG_Y, default_state(row[1].as_str().unwrap()));
        cell.set(IVec3::ZERO, half(0), true);
        cell.set(IVec3::Y, half(1), true);
        let reads = |cell: &Cell| -> Vec<&'static str> {
            [IVec3::ZERO, IVec3::Y]
                .map(|pos| name(cell.get(pos)))
                .to_vec()
        };
        let expected = |value: &Value| -> Vec<String> {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(|read| read[0].as_str().unwrap().to_owned())
                .collect()
        };
        assert_eq!(reads(&cell), expected(&row[3]), "{row}");
        let target = match row[2].as_str().unwrap() {
            "lower" => IVec3::ZERO,
            "upper" => IVec3::Y,
            _ => IVec3::NEG_Y,
        };
        cell.set(target, air(), true);
        assert_eq!(reads(&cell), expected(&row[4]), "{row}");
    }
}

/// Blocks whose behaviour in the stone-shell scenarios is not (only) their
/// support, with the reason; their rows keep these behaviours as gaps.
fn unmodelled_support(block: &str) -> Option<&'static str> {
    let entry = BlockRegistry::vanilla().get_by_name(block)?;
    if entry.data.falling {
        return Some("falls as a gravity block");
    }
    let name = block.strip_prefix("minecraft:").unwrap();
    let dead_coral_attachment = name.starts_with("dead_")
        && (name.ends_with("_coral_fan") || name.ends_with("_coral_wall_fan"));
    Some(match name {
        "unlit_redstone_torch" | "lit_redstone_lamp" | "powered_repeater" => "redstone",
        "farmland" | "grass_path" => "turns to dirt under a block",
        "brown_mushroom" | "red_mushroom" => "light level",
        "vine" | "glow_lichen" | "sculk_vein" | "resin_clump" => "multi-face attachment",
        "big_dripleaf" | "chorus_flower" | "chorus_plant" | "cactus" | "reeds" | "frog_spawn"
        | "seagrass" | "kelp" | "twisting_vines" | "cocoa" | "pitcher_crop"
        | "pale_moss_carpet" | "bed" | "straw_bed" | "grindstone" | "shelf_mushroom"
        | "mangrove_propagule" | "wall_banner" | "chalkboard" => "not modelled",
        // Plants on stone: BDS removes them at the first neighbour change;
        // their soils (water for a lily pad) were not swept.
        "red_shrub" | "wither_rose" | "waterlily" => "soils not measured",
        _ if dead_coral_attachment => return None,
        _ if name.contains("coral") && !name.ends_with("_block") => "coral death",
        _ if name.ends_with("_hanging_sign") => "not modelled",
        _ => return None,
    })
}

/// Every block's default state and single-property variations in a stone
/// shell whose sides are removed one at a time: the block goes at the same
/// removal as in BDS.
#[test]
fn blocks_fall_off_when_their_support_goes_like_bds() {
    let rows = rows("support");
    let stone = default_state("minecraft:stone");
    let orders = [
        ["east", "west", "south", "north", "up", "down"],
        ["down", "up", "north", "south", "west", "east"],
    ];
    let mut checked = 0;
    // Cases where the block did not survive its own placement in BDS.
    let mut placement = 0;
    let mut wrong = Vec::new();
    let mut skipped = HashMap::<&'static str, usize>::new();
    for row in &rows {
        let block = row[0].as_str().unwrap();
        if let Some(reason) = unmodelled_support(block) {
            *skipped.entry(reason).or_default() += 1;
            continue;
        }
        // Bells fixed to one or two walls and the technical `unknown` torch
        // are not modelled.
        let attachment = row[1]["attachment"].as_str();
        if matches!(attachment, Some("side" | "multiple"))
            || row[1]["torch_facing_direction"] == "unknown"
        {
            *skipped.entry("bell on a wall, unknown torch").or_default() += 1;
            continue;
        }
        // Sloped rails also need the block they climb against (not modelled).
        if row[1]["rail_direction"]
            .as_i64()
            .is_some_and(|d| (2..=5).contains(&d))
        {
            *skipped.entry("sloped rail").or_default() += 1;
            continue;
        }
        let placed = state(block, &row[1]);
        for (order, measured) in orders.iter().zip([&row[3], &row[4]]) {
            let expected = measured[0].as_str();
            if expected == Some("placement") {
                placement += 1;
                continue;
            }
            let mut cell = Cell::new();
            for side_name in order {
                cell.write(side(side_name), stone);
            }
            cell.set(IVec3::ZERO, placed, true);
            let mut gone = None;
            for side_name in order {
                cell.set(side(side_name), air(), true);
                if name(cell.get(IVec3::ZERO)) != block {
                    gone = Some(*side_name);
                    break;
                }
            }
            if gone != expected {
                wrong.push(format!(
                    "{block} {} removing {order:?}: gone at {gone:?}, BDS {expected:?}",
                    row[1]
                ));
            }
            checked += 1;
        }
    }
    assert!(
        wrong.is_empty(),
        "{} differ:
{}",
        wrong.len(),
        wrong.join(
            "
"
        )
    );
    // Every row has two removal orders.
    let skipped_rows: usize = skipped.values().sum();
    assert_eq!(checked + placement + 2 * skipped_rows, 2 * rows.len());
    assert!(checked >= 10_000, "{checked} {skipped:?}");
}
